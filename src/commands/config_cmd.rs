use crate::cli::ConfigCmd;
use crate::config::discovered_sources;
use crate::error::Result;
use crate::output::print_config_sources;

pub fn run(cmd: ConfigCmd, json: bool) -> Result<()> {
    let ConfigCmd::Sources = cmd;
    let sources = discovered_sources()?;
    print_config_sources(&sources, json);
    Ok(())
}
