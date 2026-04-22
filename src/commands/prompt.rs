use std::time::Duration;

use rmcp::model::GetPromptRequestParams;
use serde_json::{Map, Value};

use crate::cli::PromptCmd;
use crate::config::{apply_override, load_all, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::{print_prompt_get, print_prompt_list};
use crate::session::{self, Session};

pub async fn run(
    cmd: PromptCmd,
    json: bool,
    verbose: bool,
    override_cmd: Option<&str>,
) -> Result<()> {
    match cmd {
        PromptCmd::List { server, timeout } => {
            let session =
                open(&server, Duration::from_secs(timeout), verbose, override_cmd).await?;
            let prompts = session::list_prompts(&session).await?;
            session::close(session).await;
            print_prompt_list(&server, &prompts, json);
        }
        PromptCmd::Get {
            server,
            prompt,
            args,
            timeout,
        } => {
            let session =
                open(&server, Duration::from_secs(timeout), verbose, override_cmd).await?;
            let arguments = parse_args(&args)?;
            let params = if arguments.is_empty() {
                GetPromptRequestParams::new(prompt.clone())
            } else {
                GetPromptRequestParams::new(prompt.clone()).with_arguments(arguments)
            };
            let result = session::get_prompt(&session, params).await?;
            session::close(session).await;
            print_prompt_get(&server, &prompt, &result, json);
        }
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for kv in args {
        let (k, v) = kv.split_once('=').ok_or_else(|| CmcpError::InvalidArg {
            arg: kv.clone(),
            reason: "expected key=value".into(),
        })?;
        out.insert(k.to_string(), Value::String(v.to_string()));
    }
    Ok(out)
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
