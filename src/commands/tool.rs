use std::time::Duration;

use crate::cli::ToolCmd;
use crate::config::{load_all, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::{print_tool_list, print_tool_show};
use crate::session::{self, Session};

pub async fn run(cmd: ToolCmd, verbose: bool) -> Result<()> {
    match cmd {
        ToolCmd::List {
            server,
            json,
            timeout,
        } => {
            let session = open(&server, Duration::from_secs(timeout), verbose).await?;
            let tools = session::list_tools(&session).await?;
            print_tool_list(&server, &tools, json);
            session::close(session).await;
        }
        ToolCmd::Show {
            server,
            tool,
            json,
            timeout,
        } => {
            let session = open(&server, Duration::from_secs(timeout), verbose).await?;
            let tools = session::list_tools(&session).await?;
            let found = tools
                .iter()
                .find(|t| t.name.as_ref() == tool)
                .ok_or_else(|| CmcpError::ToolNotFound {
                    server: ServerId(server.clone()),
                    tool: tool.clone(),
                })?;
            print_tool_show(found, json);
            session::close(session).await;
        }
    }
    Ok(())
}

async fn open(server: &str, timeout: Duration, verbose: bool) -> Result<Session> {
    let servers = load_all()?;
    let id = ServerId(server.to_string());
    let cfg = servers
        .get(&id)
        .ok_or_else(|| CmcpError::ServerNotFound(id.clone()))?;
    match tokio::time::timeout(timeout, session::connect(cfg, verbose)).await {
        Ok(s) => s,
        Err(_) => Err(CmcpError::Timeout(timeout.as_secs())),
    }
}
