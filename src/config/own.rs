//! Read/write helpers for mcpctl's own config at `~/.config/mcpctl/mcp.json`.
//!
//! The file is a `{ "mcpServers": { name: {...} } }` JSON document, matching
//! Claude Code's shape so users can hand-edit or copy entries around.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{McpConfigRoot, RawServer};
use crate::error::{ConfigError, Result};

pub use crate::config::resolve_own_config_path as path;

/// Load the own-config file. Missing file → empty root (not an error).
pub fn load() -> Result<McpConfigRoot> {
    let p = path()?;
    if !p.is_file() {
        return Ok(McpConfigRoot::default());
    }
    let bytes = fs::read(&p).map_err(|e| ConfigError::Read {
        path: p.clone(),
        source: e,
    })?;
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(McpConfigRoot::default());
    }
    let root: McpConfigRoot =
        serde_json::from_slice(&bytes).map_err(|e| ConfigError::Parse { path: p, source: e })?;
    Ok(root)
}

/// Atomically write `root` to the own-config path, creating parent dirs as
/// needed. Writes a sibling `.tmp` file then renames.
pub fn save(root: &McpConfigRoot) -> Result<PathBuf> {
    let p = path()?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_sibling(&p);
    {
        let mut f = fs::File::create(&tmp)?;
        let bytes = serde_json::to_vec_pretty(root)?;
        f.write_all(&bytes)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &p)?;
    Ok(p)
}

fn tmp_sibling(p: &Path) -> PathBuf {
    let mut name = p.file_name().map(|s| s.to_owned()).unwrap_or_default();
    name.push(".tmp");
    p.with_file_name(name)
}

/// Upsert a server entry. Returns the absolute path written.
/// If `force` is false and the name already exists in the *own* config,
/// returns an error — the caller should suggest `--force`.
pub fn upsert(name: &str, server: RawServer, force: bool) -> Result<PathBuf> {
    let mut root = load()?;
    if root.mcp_servers.contains_key(name) && !force {
        return Err(crate::error::CmcpError::InvalidArg {
            arg: name.to_string(),
            reason: "already defined in mcpctl config; pass --force to overwrite".into(),
        });
    }
    root.mcp_servers.insert(name.to_string(), server);
    save(&root)
}

/// Remove a server entry. Returns `true` if it existed, `false` otherwise.
pub fn remove(name: &str) -> Result<bool> {
    let mut root = load()?;
    let existed = root.mcp_servers.remove(name).is_some();
    if existed {
        save(&root)?;
    }
    Ok(existed)
}

/// List entries currently in the own config (helper for tests / `server list`).
#[allow(dead_code)]
pub fn entries() -> Result<BTreeMap<String, RawServer>> {
    Ok(load()?.mcp_servers)
}
