use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, Implementation, Tool,
};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::config::{McpServerConfig, McpTransport};
use crate::error::{CmcpError, Result};

pub type Session = RunningService<RoleClient, ClientInfo>;

const STDERR_RING_CAP: usize = 50;

fn client_info() -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("cmcp", env!("CARGO_PKG_VERSION")),
    )
}

pub async fn connect(cfg: &McpServerConfig, verbose: bool) -> Result<Session> {
    match &cfg.transport {
        McpTransport::Stdio { command, args, env } => {
            connect_stdio(command, args, env, verbose).await
        }
        McpTransport::Http { url, headers } => connect_http(url, headers).await,
    }
}

async fn connect_stdio(
    command: &str,
    args: &[String],
    env: &std::collections::BTreeMap<String, String>,
    verbose: bool,
) -> Result<Session> {
    let args = args.to_vec();
    let env = env.clone();

    let (child, stderr) = TokioChildProcess::builder(Command::new(command).configure(move |cmd| {
        for a in &args {
            cmd.arg(a);
        }
        for (k, v) in &env {
            cmd.env(k, v);
        }
    }))
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| CmcpError::Transport(format!("stdio spawn failed: {e}")))?;

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

    match client_info().serve(child).await {
        Ok(session) => Ok(session),
        Err(e) => {
            // Give the bg task a moment to collect exit-time stderr.
            tokio::time::sleep(Duration::from_millis(150)).await;
            let tail = ring
                .lock()
                .map(|g| g.iter().cloned().collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            let msg = if tail.is_empty() {
                format!("stdio handshake failed: {e}")
            } else {
                format!("stdio handshake failed: {e}\n--- last stderr lines ---\n{tail}")
            };
            Err(CmcpError::Transport(msg))
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
        .map_err(|e| CmcpError::Transport(format!("http handshake failed: {e}")))
}

pub async fn list_tools(session: &Session) -> Result<Vec<Tool>> {
    session
        .list_all_tools()
        .await
        .map_err(|e| CmcpError::Service(e.to_string()))
}

pub async fn call_tool(
    session: &Session,
    params: CallToolRequestParams,
    timeout: Duration,
) -> Result<CallToolResult> {
    let fut = session.call_tool(params);
    match tokio::time::timeout(timeout, fut).await {
        Ok(res) => res.map_err(|e| CmcpError::Service(e.to_string())),
        Err(_) => Err(CmcpError::Timeout(timeout.as_secs())),
    }
}

pub async fn close(session: Session) {
    let _ = session.cancel().await;
}
