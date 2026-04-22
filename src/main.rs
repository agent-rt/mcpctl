mod cli;
mod commands;
mod config;
mod error;
mod output;
mod session;

use clap::Parser;

use crate::cli::{Cli, Cmd};
use crate::error::{CmcpError, Result};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json = cli.json;

    let exit = match run(cli).await {
        Ok(()) => 0,
        Err(CmcpError::Silent(code)) => code,
        Err(err) => {
            if json {
                eprintln!(
                    "{}",
                    serde_json::json!({"error": err.to_string(), "code": err.exit_code()})
                );
            } else {
                eprintln!("mcpctl: {err}");
            }
            err.exit_code()
        }
    };
    std::process::exit(exit);
}

async fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    let verbose = cli.verbose;
    let override_cmd = cli.override_cmd.as_deref();
    match cli.command {
        Some(Cmd::Server { cmd }) => commands::server::run(cmd, json).await,
        Some(Cmd::Tool { cmd }) => commands::tool::run(cmd, json, verbose, override_cmd).await,
        Some(Cmd::Prompt { cmd }) => commands::prompt::run(cmd, json, verbose, override_cmd).await,
        Some(Cmd::Resource { cmd }) => commands::resource::run(cmd, json, verbose, override_cmd).await,
        Some(Cmd::Config { cmd }) => commands::config_cmd::run(cmd, json),
        Some(Cmd::Call { uri, call }) => {
            commands::call::run(&uri, call, json, verbose, override_cmd).await
        }
        Some(Cmd::Tail {
            server,
            filters,
            count,
            timeout,
        }) => {
            commands::tail::run(&server, filters, count, timeout, json, verbose, override_cmd)
                .await
        }
        Some(Cmd::Introspect {
            server,
            signature,
            desc_chars,
            timeout,
        }) => {
            commands::introspect::run(&server, json, signature, desc_chars, timeout, verbose, override_cmd)
                .await
        }
        None => match cli.uri {
            Some(uri) => commands::call::run(&uri, cli.call, json, verbose, override_cmd).await,
            None => Err(CmcpError::InvalidUri(
                "no subcommand or mcp:// URI provided (try `mcpctl --help`)".into(),
            )),
        },
    }
}
