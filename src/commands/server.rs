use std::collections::BTreeMap;
use std::process::Command;

use crate::cli::{ServerAddArgs, ServerCmd};
use crate::config::{
    self, load_all, own, ConfigSource, McpConfigRoot, RawServer, ServerId,
};
use crate::error::{CmcpError, Result};
use crate::output::{print_check, print_server_list, print_server_show, CheckRow, CheckStatus};

pub async fn run(cmd: ServerCmd, json: bool) -> Result<()> {
    match cmd {
        ServerCmd::List { probe, probe_timeout } => {
            if probe {
                list_probe(json, probe_timeout).await
            } else {
                list(json)
            }
        }
        ServerCmd::Show { name, reveal } => show(name, reveal, json),
        ServerCmd::Add(args) => add(args),
        ServerCmd::Remove { name } => remove(name),
        ServerCmd::Edit => edit(),
        ServerCmd::Check { name, timeout } => check(name, json, timeout).await,
    }
}

async fn check(name: String, json: bool, timeout: u64) -> Result<()> {
    use std::time::{Duration, Instant};
    // `check` owns user-facing output for all outcomes: the report IS the
    // error surface. We return `Silent(exit_code)` on failure so main won't
    // re-print a one-liner — exit code is the only signal main passes through.
    let servers = load_all()?;
    let id = ServerId(name.clone());
    let Some(resolved) = servers.get(&id) else {
        let rows = [CheckRow {
            stage: "resolve",
            ms: None,
            status: CheckStatus::Error,
            detail: format!("server '{id}' not found"),
        }];
        print_check(&name, &rows, json);
        return Err(CmcpError::Silent(CmcpError::ServerNotFound(id).exit_code()));
    };
    let cfg = &resolved.active;
    let transport = transport_kind(cfg);

    let timeout = Duration::from_secs(timeout);
    let t0 = Instant::now();
    let session = match crate::session::connect(cfg, false, timeout).await {
        Ok(s) => s,
        Err(e) => {
            let code = e.exit_code();
            let rows = [
                CheckRow {
                    stage: "handshake",
                    ms: Some(t0.elapsed().as_millis() as u64),
                    status: CheckStatus::Error,
                    detail: e.to_string(),
                },
                CheckRow {
                    stage: "list_tools",
                    ms: None,
                    status: CheckStatus::Skipped,
                    detail: String::new(),
                },
            ];
            print_check(&name, &rows, json);
            return Err(CmcpError::Silent(code));
        }
    };
    let handshake_ms = t0.elapsed().as_millis() as u64;

    let t1 = Instant::now();
    let tools_result = crate::session::list_tools(&session).await;
    let list_ms = t1.elapsed().as_millis() as u64;
    crate::session::close(session).await;

    match tools_result {
        Ok(tools) => {
            let rows = [
                CheckRow {
                    stage: "handshake",
                    ms: Some(handshake_ms),
                    status: CheckStatus::Ok,
                    detail: transport.to_string(),
                },
                CheckRow {
                    stage: "list_tools",
                    ms: Some(list_ms),
                    status: CheckStatus::Ok,
                    detail: format!("{} tools", tools.len()),
                },
            ];
            print_check(&name, &rows, json);
            Ok(())
        }
        Err(e) => {
            let code = e.exit_code();
            let rows = [
                CheckRow {
                    stage: "handshake",
                    ms: Some(handshake_ms),
                    status: CheckStatus::Ok,
                    detail: transport.to_string(),
                },
                CheckRow {
                    stage: "list_tools",
                    ms: Some(list_ms),
                    status: CheckStatus::Error,
                    detail: e.to_string(),
                },
            ];
            print_check(&name, &rows, json);
            Err(CmcpError::Silent(code))
        }
    }
}

fn transport_kind(cfg: &crate::config::McpServerConfig) -> &'static str {
    match cfg.transport {
        crate::config::McpTransport::Stdio { .. } => "stdio",
        crate::config::McpTransport::Http { .. } => "http",
    }
}

fn list(json: bool) -> Result<()> {
    let servers = load_all()?;
    let refs: Vec<_> = servers.values().map(|r| &r.active).collect();
    print_server_list(&refs, json);
    Ok(())
}

async fn list_probe(json: bool, probe_timeout: u64) -> Result<()> {
    use std::time::{Duration, Instant};

    let servers = load_all()?;
    let timeout = Duration::from_secs(probe_timeout);

    let futs = servers.values().map(|r| {
        let cfg = r.active.clone();
        async move {
            let t0 = Instant::now();
            let res = crate::session::connect(&cfg, false, timeout).await;
            let (status, detail) = match res {
                Ok(session) => {
                    let tools = crate::session::list_tools(&session).await;
                    crate::session::close(session).await;
                    match tools {
                        Ok(ts) => (
                            crate::output::ProbeStatus::Ok,
                            format!("{} tools", ts.len()),
                        ),
                        Err(e) => (crate::output::ProbeStatus::Error, e.to_string()),
                    }
                }
                Err(CmcpError::HandshakeTimeout { .. }) => (
                    crate::output::ProbeStatus::Timeout,
                    format!("handshake > {probe_timeout}s"),
                ),
                Err(e) => (crate::output::ProbeStatus::Error, e.to_string()),
            };
            crate::output::ProbeRow {
                cfg,
                ms: t0.elapsed().as_millis() as u64,
                status,
                detail,
            }
        }
    });

    let results = futures_like_join_all(futs).await;
    crate::output::print_server_list_probed(&results, json);
    Ok(())
}

async fn futures_like_join_all<F>(iter: impl IntoIterator<Item = F>) -> Vec<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handles: Vec<_> = iter.into_iter().map(tokio::spawn).collect();
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(v) = h.await {
            out.push(v);
        }
    }
    out
}

fn show(name: String, reveal: bool, json: bool) -> Result<()> {
    let servers = load_all()?;
    let id = ServerId(name);
    let resolved = servers
        .get(&id)
        .ok_or_else(|| CmcpError::ServerNotFound(id.clone()))?;
    print_server_show(&resolved.active, &resolved.shadows, reveal, json);
    Ok(())
}

fn add(args: ServerAddArgs) -> Result<()> {
    let raw = build_raw_server(&args)?;
    let path = own::upsert(&args.name, raw, args.force)?;
    println!("wrote {} → {}", args.name, path.display());
    Ok(())
}

fn remove(name: String) -> Result<()> {
    let all = load_all()?;
    if let Some(resolved) = all.get(&ServerId(name.clone())) {
        let cfg = &resolved.active;
        if !cfg.source.writable() && !own::entries()?.contains_key(&name) {
            return Err(CmcpError::InvalidArg {
                arg: name,
                reason: format!(
                    "server comes from read-only source '{}' ({}); edit it there",
                    cfg.source.label(),
                    cfg.source_path.display()
                ),
            });
        }
    }
    if !own::remove(&name)? {
        return Err(CmcpError::ServerNotFound(ServerId(name)));
    }
    println!("removed {name} from mcpctl config");
    Ok(())
}

fn edit() -> Result<()> {
    let path = config::resolve_own_config_path()?;
    if !path.is_file() {
        own::save(&McpConfigRoot::default())?;
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| CmcpError::InvalidArg {
            arg: editor.clone(),
            reason: format!("failed to launch editor: {e}"),
        })?;
    if !status.success() {
        return Err(CmcpError::InvalidArg {
            arg: editor,
            reason: format!("editor exited with {status}"),
        });
    }
    Ok(())
}

fn build_raw_server(args: &ServerAddArgs) -> Result<RawServer> {
    match (&args.command, &args.url) {
        (Some(command), None) => {
            let env = parse_kv_pairs(&args.env, "--env")?;
            Ok(RawServer::Stdio {
                command: command.clone(),
                args: args.args.clone(),
                env,
            })
        }
        (None, Some(url)) => {
            let headers = parse_header_pairs(&args.headers)?;
            Ok(RawServer::Http {
                url: url.clone(),
                headers,
            })
        }
        (Some(_), Some(_)) => Err(CmcpError::InvalidArg {
            arg: "transport".into(),
            reason: "--command and --url are mutually exclusive".into(),
        }),
        (None, None) => Err(CmcpError::InvalidArg {
            arg: "transport".into(),
            reason: "specify either --command <...> or --url <...>".into(),
        }),
    }
}

fn parse_kv_pairs(items: &[String], flag: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for it in items {
        let (k, v) = it.split_once('=').ok_or_else(|| CmcpError::InvalidArg {
            arg: format!("{flag} {it}"),
            reason: "expected KEY=VALUE".into(),
        })?;
        if k.is_empty() {
            return Err(CmcpError::InvalidArg {
                arg: format!("{flag} {it}"),
                reason: "key must be non-empty".into(),
            });
        }
        out.insert(k.to_string(), v.to_string());
    }
    Ok(out)
}

fn parse_header_pairs(items: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for it in items {
        let (k, v) = if let Some(p) = it.split_once(':') {
            (p.0.trim().to_string(), p.1.trim().to_string())
        } else if let Some(p) = it.split_once('=') {
            (p.0.trim().to_string(), p.1.trim().to_string())
        } else {
            return Err(CmcpError::InvalidArg {
                arg: format!("--header {it}"),
                reason: "expected 'Name: Value' or 'Name=Value'".into(),
            });
        };
        if k.is_empty() {
            return Err(CmcpError::InvalidArg {
                arg: format!("--header {it}"),
                reason: "header name must be non-empty".into(),
            });
        }
        out.insert(k, v);
    }
    Ok(out)
}

#[allow(dead_code)]
fn _ref_writable(s: ConfigSource) -> bool {
    s.writable()
}
