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
    let exit = match run().await {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("cmcp: {err}");
            err.exit_code()
        }
    };
    std::process::exit(exit);
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    let verbose = cli.verbose;
    match cli.command {
        Some(Cmd::Server { cmd }) => commands::server::run(cmd),
        Some(Cmd::Tool { cmd }) => commands::tool::run(cmd, verbose).await,
        Some(Cmd::Config { cmd }) => commands::config_cmd::run(cmd),
        Some(Cmd::Call { uri, call }) => commands::call::run(&uri, call, verbose).await,
        None => match cli.uri {
            Some(uri) => commands::call::run(&uri, cli.call, verbose).await,
            None => Err(CmcpError::InvalidUri(
                "no subcommand or mcp:// URI provided (try `cmcp --help`)".into(),
            )),
        },
    }
}
