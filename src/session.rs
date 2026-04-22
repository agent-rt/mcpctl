use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, GetPromptRequestParams,
    GetPromptResult, Implementation, Prompt, ReadResourceRequestParams, ReadResourceResult,
    Resource, Tool,
};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::config::{McpServerConfig, McpTransport};
use crate::error::{CmcpError, Result};

pub struct Session {
    inner: RunningService<RoleClient, ClientInfo>,
    /// Set for stdio children (Unix): process-group id we can killpg on close.
    pgid: Option<u32>,
}

impl Session {
    fn new(inner: RunningService<RoleClient, ClientInfo>, pgid: Option<u32>) -> Self {
        Self { inner, pgid }
    }
}

/// Raw pieces of a spawned stdio MCP server, before the client handshake. Used
/// by both the generic [`connect`] path and specialised flows like `tail`
/// which need a non-standard [`rmcp::ClientHandler`].
pub struct SpawnedChild {
    pub child: TokioChildProcess,
    /// Unix only: process-group id (equal to the direct child pid because we
    /// spawn with `process_group(0)`). Pass this to [`kill_process_group`] /
    /// [`kill_process_group_now`] to signal the whole subtree.
    pub pgid: Option<u32>,
    pub stderr_ring: Arc<Mutex<VecDeque<String>>>,
}

pub const STDERR_RING_CAP: usize = 50;

pub fn client_info() -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("mcpctl", env!("CARGO_PKG_VERSION")),
    )
}

pub async fn connect(
    cfg: &McpServerConfig,
    verbose: bool,
    handshake_timeout: Duration,
) -> Result<Session> {
    connect_with_retry(cfg, verbose, handshake_timeout, 0).await
}

/// Connect, retrying `retries` extra times on `Transport` errors (spawn
/// failure, child exited before handshake). We do NOT retry on
/// `HandshakeTimeout` — the user already waited the full budget and a repeat
/// would just double the wait. Typical use: `retries=1` to absorb one cold-
/// start flake (Python import, Docker first-run).
pub async fn connect_with_retry(
    cfg: &McpServerConfig,
    verbose: bool,
    handshake_timeout: Duration,
    retries: u32,
) -> Result<Session> {
    let mut attempt: u32 = 0;
    loop {
        let res = match &cfg.transport {
            McpTransport::Stdio { command, args, env } => {
                connect_stdio(command, args, env, verbose, handshake_timeout).await
            }
            McpTransport::Http { url, headers } => {
                match tokio::time::timeout(handshake_timeout, connect_http(url, headers)).await {
                    Ok(r) => r,
                    Err(_) => Err(CmcpError::HandshakeTimeout {
                        secs: handshake_timeout.as_secs(),
                        tail: None,
                    }),
                }
            }
        };
        match res {
            Ok(s) => return Ok(s),
            Err(CmcpError::Transport(_)) if attempt < retries => {
                attempt += 1;
                if verbose {
                    eprintln!("[mcpctl] retry {attempt}/{retries} after transport error");
                }
                // Brief pause so we don't hammer an initializing server.
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Spawn an MCP stdio server and attach the stderr ring buffer. Does NOT
/// perform the MCP handshake — caller supplies any [`rmcp::ClientHandler`]
/// they like and calls [`SpawnedChild::child`]`.serve(handler)` themselves.
pub fn spawn_stdio_child(
    command: &str,
    args: &[String],
    env: &std::collections::BTreeMap<String, String>,
    verbose: bool,
) -> Result<SpawnedChild> {
    let args = args.to_vec();
    let env = env.clone();

    let (child, stderr) = TokioChildProcess::builder(Command::new(command).configure(move |cmd| {
        for a in &args {
            cmd.arg(a);
        }
        for (k, v) in &env {
            cmd.env(k, v);
        }
        cmd.kill_on_drop(true);
        // On Unix, put the child in its own process group so we can signal the
        // entire subtree (including any workers it forks) rather than just the
        // direct child. kill_on_drop only reaches the direct child.
        #[cfg(unix)]
        cmd.process_group(0);
    }))
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| {
        CmcpError::Transport(format!(
            "stdio spawn failed: {e}\nhint: rerun with -v/--verbose to stream server stderr"
        ))
    })?;

    #[cfg(unix)]
    let pgid = child.id();
    #[cfg(not(unix))]
    let pgid: Option<u32> = None;

    let ring: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_RING_CAP)));

    if let Some(stderr) = stderr {
        let ring_bg = Arc::clone(&ring);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if verbose {
                    eprintln!("[mcp-stderr] {line}");
                }
                if let Ok(mut guard) = ring_bg.lock() {
                    if guard.len() == STDERR_RING_CAP {
                        guard.pop_front();
                    }
                    guard.push_back(line);
                }
            }
        });
    }

    Ok(SpawnedChild {
        child,
        pgid,
        stderr_ring: ring,
    })
}

async fn connect_stdio(
    command: &str,
    args: &[String],
    env: &std::collections::BTreeMap<String, String>,
    verbose: bool,
    handshake_timeout: Duration,
) -> Result<Session> {
    let spawned = spawn_stdio_child(command, args, env, verbose)?;
    let SpawnedChild {
        child,
        pgid,
        stderr_ring,
    } = spawned;

    let serve = client_info().serve(child);
    match tokio::time::timeout(handshake_timeout, serve).await {
        Ok(Ok(session)) => Ok(Session::new(session, pgid)),
        Ok(Err(e)) => {
            kill_process_group_now(pgid);
            // Give the bg task a moment to collect exit-time stderr.
            tokio::time::sleep(Duration::from_millis(150)).await;
            let tail = drain_tail(&stderr_ring);
            let hint = "hint: rerun with -v/--verbose to stream server stderr";
            let msg = if tail.is_empty() {
                format!("stdio handshake failed: {e}\n{hint}")
            } else {
                format!("stdio handshake failed: {e}\n--- last stderr lines ---\n{tail}\n{hint}")
            };
            Err(CmcpError::Transport(msg))
        }
        Err(_) => {
            kill_process_group_now(pgid);
            tokio::time::sleep(Duration::from_millis(150)).await;
            let tail = drain_tail(&stderr_ring);
            Err(CmcpError::HandshakeTimeout {
                secs: handshake_timeout.as_secs(),
                tail: if tail.is_empty() { None } else { Some(tail) },
            })
        }
    }
}

fn drain_tail(ring: &Arc<Mutex<VecDeque<String>>>) -> String {
    ring.lock()
        .map(|g| g.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// Escalated kill: SIGTERM → 300 ms grace → SIGKILL. Gives servers a chance
/// to flush state (caches, locks, DB handles) before we pull the floor out.
pub async fn kill_process_group(_pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = _pid {
        // pgid == pid because we spawned with process_group(0).
        // SAFETY: killpg is async-signal-safe; invalid pgids just return ESRCH.
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGTERM);
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

/// Sync fallback for contexts where we can't await (e.g. inside a `Drop`).
/// Sends SIGKILL only — no grace. Prefer the async version.
pub fn kill_process_group_now(_pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = _pid {
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

async fn connect_http(
    url: &str,
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<Session> {
    let auth_header = headers
        .iter()
        .find_map(|(k, v)| (k.eq_ignore_ascii_case("authorization")).then(|| v.clone()));
    let mut config = StreamableHttpClientTransportConfig::default();
    config.uri = url.to_string().into();
    config.auth_header = auth_header;
    let transport = StreamableHttpClientTransport::from_config(config);
    client_info()
        .serve(transport)
        .await
        .map(|s| Session::new(s, None))
        .map_err(|e| {
            CmcpError::Transport(format!(
                "http handshake failed: {e}\nhint: rerun with -v/--verbose for transport detail"
            ))
        })
}

pub async fn list_tools(session: &Session) -> Result<Vec<Tool>> {
    session
        .inner
        .list_all_tools()
        .await
        .map_err(|e| CmcpError::Service(e.to_string()))
}

pub async fn call_tool(
    session: &Session,
    params: CallToolRequestParams,
    timeout: Duration,
) -> Result<CallToolResult> {
    let fut = session.inner.call_tool(params);
    match tokio::time::timeout(timeout, fut).await {
        Ok(res) => res.map_err(|e| CmcpError::Service(e.to_string())),
        Err(_) => Err(CmcpError::Timeout(timeout.as_secs())),
    }
}

pub async fn list_prompts(session: &Session) -> Result<Vec<Prompt>> {
    session
        .inner
        .list_all_prompts()
        .await
        .map_err(|e| CmcpError::Service(e.to_string()))
}

pub async fn get_prompt(
    session: &Session,
    params: GetPromptRequestParams,
) -> Result<GetPromptResult> {
    session
        .inner
        .get_prompt(params)
        .await
        .map_err(|e| CmcpError::Service(e.to_string()))
}

pub async fn list_resources(session: &Session) -> Result<Vec<Resource>> {
    session
        .inner
        .list_all_resources()
        .await
        .map_err(|e| CmcpError::Service(e.to_string()))
}

pub async fn read_resource(
    session: &Session,
    params: ReadResourceRequestParams,
) -> Result<ReadResourceResult> {
    session
        .inner
        .read_resource(params)
        .await
        .map_err(|e| CmcpError::Service(e.to_string()))
}

pub async fn close(session: Session) {
    let Session { inner, pgid } = session;
    let _ = inner.cancel().await;
    // Catch any worker processes the server forked that would otherwise
    // reparent to init and leak. Graceful: SIGTERM → wait → SIGKILL.
    kill_process_group(pgid).await;
}
