use std::collections::BTreeMap;
use std::process::Command;

use crate::cli::{ServerAddArgs, ServerCmd};
use crate::config::{self, load_all, own, ConfigSource, McpConfigRoot, RawServer, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::{print_server_list, print_server_show};

pub fn run(cmd: ServerCmd) -> Result<()> {
    match cmd {
        ServerCmd::List { json } => list(json),
        ServerCmd::Show { name, reveal, json } => show(name, reveal, json),
        ServerCmd::Add(args) => add(args),
        ServerCmd::Remove { name } => remove(name),
        ServerCmd::Edit => edit(),
    }
}

fn list(json: bool) -> Result<()> {
    let servers = load_all()?;
    let refs: Vec<_> = servers.values().collect();
    print_server_list(&refs, json);
    Ok(())
}

fn show(name: String, reveal: bool, json: bool) -> Result<()> {
    let servers = load_all()?;
    let id = ServerId(name);
    let server = servers
        .get(&id)
        .ok_or_else(|| CmcpError::ServerNotFound(id.clone()))?;
    print_server_show(server, reveal, json);
    Ok(())
}

fn add(args: ServerAddArgs) -> Result<()> {
    let raw = build_raw_server(&args)?;
    let path = own::upsert(&args.name, raw, args.force)?;
    println!("wrote {} → {}", args.name, path.display());
    Ok(())
}

fn remove(name: String) -> Result<()> {
    // If name exists but only in a read-only source, give a helpful error.
    let all = load_all()?;
    if let Some(cfg) = all.get(&ServerId(name.clone())) {
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
    println!("removed {name} from cmcp config");
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
    // Accept either `Name: Value` (familiar to curl users) or `Name=Value`.
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

/// Tie the unused helpers into the module so dead-code warnings silence
/// until the test harness / future commands call them. Keeps compiler happy
/// without decorating individual items.
#[allow(dead_code)]
fn _ref_writable(s: ConfigSource) -> bool {
    s.writable()
}
