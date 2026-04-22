use std::time::Duration;

use rmcp::model::ReadResourceRequestParams;

use crate::cli::ResourceCmd;
use crate::config::{apply_override, load_all, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::{print_resource_list, print_resource_read};
use crate::session::{self, Session};

pub async fn run(
    cmd: ResourceCmd,
    json: bool,
    verbose: bool,
    override_cmd: Option<&str>,
) -> Result<()> {
    match cmd {
        ResourceCmd::List { server, timeout } => {
            let session =
                open(&server, Duration::from_secs(timeout), verbose, override_cmd).await?;
            let resources = session::list_resources(&session).await?;
            session::close(session).await;
            print_resource_list(&server, &resources, json);
        }
        ResourceCmd::Read {
            server,
            uri,
            timeout,
        } => {
            let session =
                open(&server, Duration::from_secs(timeout), verbose, override_cmd).await?;
            let params = ReadResourceRequestParams::new(uri.clone());
            let result = session::read_resource(&session, params).await?;
            session::close(session).await;
            print_resource_read(&server, &uri, &result, json);
        }
    }
    Ok(())
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
