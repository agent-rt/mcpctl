use std::time::Duration;

use crate::cli::ToolCmd;
use crate::commands::call::parse_uri;
use crate::config::{apply_override, load_all, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::{print_tool_list, print_tool_show};
use crate::session::{self, Session};

pub async fn run(cmd: ToolCmd, json: bool, verbose: bool, override_cmd: Option<&str>) -> Result<()> {
    match cmd {
        ToolCmd::List { server, timeout, desc_chars } => {
            let session =
                open(&server, Duration::from_secs(timeout), verbose, override_cmd).await?;
            let tools = session::list_tools(&session).await?;
            print_tool_list(&server, &tools, desc_chars, json);
            session::close(session).await;
        }
        ToolCmd::Show { target, tool, signature, timeout } => {
            let (server, tool_name) = resolve_show_target(target, tool)?;
            let session =
                open(&server, Duration::from_secs(timeout), verbose, override_cmd).await?;
            let tools = session::list_tools(&session).await?;
            let found = tools
                .iter()
                .find(|t| t.name.as_ref() == tool_name)
                .ok_or_else(|| CmcpError::ToolNotFound {
                    server: ServerId(server.clone()),
                    tool: tool_name.clone(),
                })?;
            print_tool_show(found, signature, json);
            session::close(session).await;
        }
    }
    Ok(())
}

fn resolve_show_target(target: String, tool: Option<String>) -> Result<(String, String)> {
    match tool {
        Some(t) => Ok((target, t)),
        None => {
            let inv = parse_uri(&target)?;
            Ok((inv.server.0, inv.tool))
        }
    }
}

async fn open(
    server: &str,
    timeout: Duration,
    verbose: bool,
    override_cmd: Option<&str>,
) -> Result<Session> {
    let servers = load_all()?;
    let id = ServerId(server.to_string());
    let cfg = servers
        .get(&id)
        .map(|r| &r.active)
        .ok_or_else(|| CmcpError::ServerNotFound(id.clone()))?;
    let cfg = apply_override(cfg, override_cmd)?;
    session::connect(&cfg, verbose, timeout).await
}
