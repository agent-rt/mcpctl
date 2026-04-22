use std::time::Duration;

use crate::config::{apply_override, load_all, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::print_introspect;
use crate::session;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    server: &str,
    json: bool,
    signature: bool,
    desc_chars: usize,
    timeout: u64,
    verbose: bool,
    override_cmd: Option<&str>,
) -> Result<()> {
    let servers = load_all()?;
    let id = ServerId(server.to_string());
    let cfg = servers
        .get(&id)
        .map(|r| &r.active)
        .ok_or_else(|| CmcpError::ServerNotFound(id.clone()))?;
    let cfg = apply_override(cfg, override_cmd)?;

    let timeout = Duration::from_secs(timeout);
    let session = session::connect(&cfg, verbose, timeout).await?;

    let tools = session::list_tools(&session).await?;

    // Attempt prompts and resources; if server doesn't support them, return None.
    let prompts = session::list_prompts(&session).await.ok();
    let resources = session::list_resources(&session).await.ok();

    session::close(session).await;

    print_introspect(server, &cfg, &tools, prompts.as_deref(), resources.as_deref(), signature, desc_chars, json);
    Ok(())
}
