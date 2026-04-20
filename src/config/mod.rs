use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};

pub mod own;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerId(pub String);

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ServerId {
    fn from(s: &str) -> Self {
        ServerId(s.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum ConfigSource {
    /// cmcp's own writable config at `~/.config/cmcp/mcp.json`.
    Cmcp,
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
}

impl ConfigSource {
    pub fn priority(self) -> u8 {
        match self {
            ConfigSource::Cmcp => 0,
            ConfigSource::ClaudeCode => 1,
            ConfigSource::ClaudeDesktop => 2,
            ConfigSource::Cursor => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConfigSource::Cmcp => "cmcp",
            ConfigSource::ClaudeCode => "claude-code",
            ConfigSource::ClaudeDesktop => "claude-desktop",
            ConfigSource::Cursor => "cursor",
        }
    }

    pub fn writable(self) -> bool {
        matches!(self, ConfigSource::Cmcp)
    }
}

impl fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerConfig {
    pub id: ServerId,
    pub source: ConfigSource,
    pub source_path: PathBuf,
    pub transport: McpTransport,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredSource {
    pub source: ConfigSource,
    pub path: PathBuf,
    pub exists: bool,
}

/// ==== Raw deserialization ========================================================
///
/// Agents use slightly different JSON shapes; we parse each and normalize into
/// [`McpServerConfig`]. Unknown fields are ignored to stay forward compatible.

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct McpConfigRoot {
    #[serde(rename = "mcpServers", default)]
    pub(crate) mcp_servers: BTreeMap<String, RawServer>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub(crate) enum RawServer {
    Http {
        url: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, String>,
    },
    Stdio {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
    },
}

impl RawServer {
    fn into_transport(self) -> McpTransport {
        match self {
            RawServer::Http { url, headers } => McpTransport::Http { url, headers },
            RawServer::Stdio { command, args, env } => McpTransport::Stdio { command, args, env },
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_transport(t: &McpTransport) -> Self {
        match t {
            McpTransport::Http { url, headers } => RawServer::Http {
                url: url.clone(),
                headers: headers.clone(),
            },
            McpTransport::Stdio { command, args, env } => RawServer::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
            },
        }
    }
}

// ==== Path discovery ============================================================

pub fn discovered_sources() -> Result<Vec<DiscoveredSource>> {
    // Test / CI override: when CMCP_CONFIG_DIR is set, look in that directory
    // for both cmcp's own `mcp.json` and Claude Code's `.claude.json`.
    // Desktop and Cursor are skipped in this mode.
    if let Some(dir) = std::env::var_os("CMCP_CONFIG_DIR") {
        let dir = PathBuf::from(dir);
        let entries = [
            (ConfigSource::Cmcp, dir.join("mcp.json")),
            (ConfigSource::ClaudeCode, dir.join(".claude.json")),
        ];
        return Ok(entries
            .into_iter()
            .map(|(source, path)| DiscoveredSource {
                source,
                exists: path.is_file(),
                path,
            })
            .collect());
    }

    let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
    let entries = [
        (ConfigSource::Cmcp, own_config_path(&home)),
        (ConfigSource::ClaudeCode, home.join(".claude.json")),
        (ConfigSource::ClaudeDesktop, claude_desktop_path(&home)),
        (ConfigSource::Cursor, home.join(".cursor/mcp.json")),
    ];
    Ok(entries
        .into_iter()
        .map(|(source, path)| DiscoveredSource {
            source,
            exists: path.is_file(),
            path,
        })
        .collect())
}

/// Path to cmcp's own writable config (`$XDG_CONFIG_HOME/cmcp/mcp.json`,
/// falling back to `$HOME/.config/cmcp/mcp.json`).
pub fn own_config_path(home: &Path) -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let x = PathBuf::from(xdg);
        if !x.as_os_str().is_empty() {
            return x.join("cmcp/mcp.json");
        }
    }
    home.join(".config/cmcp/mcp.json")
}

/// Resolve cmcp's writable config path using the current environment
/// (honors `CMCP_CONFIG_DIR` for tests, then `XDG_CONFIG_HOME`, then `$HOME`).
pub fn resolve_own_config_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CMCP_CONFIG_DIR") {
        return Ok(PathBuf::from(dir).join("mcp.json"));
    }
    let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
    Ok(own_config_path(&home))
}

fn claude_desktop_path(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/Claude/claude_desktop_config.json")
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux fallback (XDG config). Not officially supported by Claude Desktop yet,
        // but harmless: if the file does not exist we simply skip it.
        home.join(".config/Claude/claude_desktop_config.json")
    }
}

// ==== Loader ====================================================================

pub fn load_all() -> Result<BTreeMap<ServerId, McpServerConfig>> {
    let mut merged: BTreeMap<ServerId, McpServerConfig> = BTreeMap::new();

    for src in discovered_sources()? {
        if !src.exists {
            continue;
        }
        let servers = parse_source(src.source, &src.path)?;
        for server in servers {
            merged
                .entry(server.id.clone())
                .and_modify(|existing| {
                    if server.source.priority() < existing.source.priority() {
                        *existing = server.clone();
                    }
                })
                .or_insert(server);
        }
    }

    Ok(merged)
}

fn parse_source(source: ConfigSource, path: &Path) -> Result<Vec<McpServerConfig>> {
    let bytes = std::fs::read(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })?;

    let raw: BTreeMap<String, RawServer> = serde_json::from_slice::<McpConfigRoot>(&bytes)
        .map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            source: e,
        })?
        .mcp_servers;

    Ok(raw
        .into_iter()
        .map(|(name, raw)| McpServerConfig {
            id: ServerId(name),
            source,
            source_path: path.to_path_buf(),
            transport: raw.into_transport(),
        })
        .collect())
}

/// Redact secret-looking values in env/headers for display.
pub fn redact_map<'a>(
    map: &'a BTreeMap<String, String>,
    reveal: bool,
) -> impl Iterator<Item = (&'a String, String)> + 'a {
    map.iter().map(move |(k, v)| {
        let shown = if reveal || !looks_sensitive(k) {
            v.clone()
        } else {
            mask(v)
        };
        (k, shown)
    })
}

fn looks_sensitive(key: &str) -> bool {
    let up = key.to_ascii_uppercase();
    const NEEDLES: &[&str] = &["TOKEN", "SECRET", "KEY", "PASSWORD", "AUTH", "BEARER"];
    NEEDLES.iter().any(|n| up.contains(n))
}

fn mask(v: &str) -> String {
    if v.len() <= 4 {
        "****".into()
    } else {
        format!("{}…(redacted, {}b)", &v[..2], v.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_code_stdio_and_http() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".claude.json");
        std::fs::write(
            &path,
            r#"{
                "mcpServers": {
                    "fs": {"command": "node", "args": ["fs.js"], "env": {"API_TOKEN": "sk-abc"}},
                    "remote": {"url": "http://x/mcp", "headers": {"Authorization": "Bearer y"}}
                }
            }"#,
        )
        .unwrap();

        let servers = parse_source(ConfigSource::ClaudeCode, &path).unwrap();
        assert_eq!(servers.len(), 2);
        let by_id: BTreeMap<_, _> = servers.into_iter().map(|s| (s.id.0.clone(), s)).collect();
        assert!(matches!(by_id["fs"].transport, McpTransport::Stdio { .. }));
        assert!(matches!(
            by_id["remote"].transport,
            McpTransport::Http { .. }
        ));
    }

    #[test]
    fn redacts_sensitive_env() {
        let mut m = BTreeMap::new();
        m.insert("API_TOKEN".into(), "super-long-secret".into());
        m.insert("DEBUG".into(), "1".into());
        let v: Vec<_> = redact_map(&m, false).collect();
        let api = v.iter().find(|(k, _)| k.as_str() == "API_TOKEN").unwrap();
        assert!(api.1.contains("redacted"));
        let dbg = v.iter().find(|(k, _)| k.as_str() == "DEBUG").unwrap();
        assert_eq!(dbg.1, "1");
    }
}
