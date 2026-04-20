use serde_json::json;

use crate::cli::ConfigCmd;
use crate::config::discovered_sources;
use crate::error::Result;

pub fn run(cmd: ConfigCmd) -> Result<()> {
    let ConfigCmd::Sources { json } = cmd;
    let sources = discovered_sources()?;
    if json {
        let arr: Vec<_> = sources
            .iter()
            .map(|s| {
                json!({
                    "source": s.source.label(),
                    "path": s.path,
                    "exists": s.exists,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        return Ok(());
    }
    println!("{:<16} {:<8} PATH", "SOURCE", "EXISTS");
    for s in &sources {
        println!(
            "{:<16} {:<8} {}",
            s.source.label(),
            if s.exists { "yes" } else { "no" },
            s.path.display()
        );
    }
    Ok(())
}
