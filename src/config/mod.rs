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
    /// mcpctl's own writable config at `~/.config/mcpctl/mcp.json`.
    Cmcp,
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    Windsurf,
    Gemini,
    Zed,
}

impl ConfigSource {
    pub fn priority(self) -> u8 {
        match self {
            ConfigSource::Cmcp => 0,
            ConfigSource::ClaudeCode => 1,
            ConfigSource::ClaudeDesktop => 2,
            ConfigSource::Cursor => 3,
            ConfigSource::Windsurf => 4,
            ConfigSource::Gemini => 5,
            ConfigSource::Zed => 6,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConfigSource::Cmcp => "mcpctl",
            ConfigSource::ClaudeCode => "claude-code",
            ConfigSource::ClaudeDesktop => "claude-desktop",
            ConfigSource::Cursor => "cursor",
            ConfigSource::Windsurf => "windsurf",
            ConfigSource::Gemini => "gemini",
            ConfigSource::Zed => "zed",
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

/// Zed uses `context_servers` with a different shape.
#[derive(Deserialize, Default)]
struct ZedSettings {
    #[serde(rename = "context_servers", default)]
    context_servers: BTreeMap<String, ZedContextServer>,
}

#[derive(Deserialize)]
struct ZedContextServer {
    #[serde(default)]
    settings: Option<ZedContextServerSettings>,
}

#[derive(Deserialize)]
struct ZedContextServerSettings {
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

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
    // Test / CI override: when MCPCTL_CONFIG_DIR is set, look in that directory
    // for both cmcp's own `mcp.json` and Claude Code's `.claude.json`.
    // Desktop and Cursor are skipped in this mode.
    if let Some(dir) = std::env::var_os("MCPCTL_CONFIG_DIR") {
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
        (ConfigSource::Cmcp, migrate_own_config_path(&home)),
        (ConfigSource::ClaudeCode, home.join(".claude.json")),
        (ConfigSource::ClaudeDesktop, claude_desktop_path(&home)),
        (ConfigSource::Cursor, home.join(".cursor/mcp.json")),
        (ConfigSource::Windsurf, home.join(".codeium/windsurf/mcp_config.json")),
        (ConfigSource::Gemini, home.join(".gemini/settings.json")),
        (ConfigSource::Zed, zed_settings_path(&home)),
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

/// Returns the mcpctl config path, auto-migrating from the old cmcp path if needed.
fn migrate_own_config_path(home: &Path) -> PathBuf {
    let new = own_config_path(home);
    if !new.exists() {
        let old = home.join(".config/cmcp/mcp.json");
        if old.exists() {
            if let Some(parent) = new.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(&old, &new);
        }
    }
    new
}

/// Path to mcpctl's own writable config (`$XDG_CONFIG_HOME/mcpctl/mcp.json`,
/// falling back to `$HOME/.config/mcpctl/mcp.json`).
pub fn own_config_path(home: &Path) -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let x = PathBuf::from(xdg);
        if !x.as_os_str().is_empty() {
            return x.join("mcpctl/mcp.json");
        }
    }
    home.join(".config/mcpctl/mcp.json")
}

/// Resolve mcpctl's writable config path using the current environment
/// (honors `MCPCTL_CONFIG_DIR` for tests, then `XDG_CONFIG_HOME`, then `$HOME`).
pub fn resolve_own_config_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("MCPCTL_CONFIG_DIR") {
        return Ok(PathBuf::from(dir).join("mcp.json"));
    }
    let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
    Ok(migrate_own_config_path(&home))
}

fn zed_settings_path(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/Zed/settings.json")
    }
    #[cfg(not(target_os = "macos"))]
    {
        home.join(".config/zed/settings.json")
    }
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

/// A server resolution including any same-named entries from lower-priority
/// sources that were shadowed by the active one. `active` is always the
/// effective entry; `shadows` is empty when nothing else defined the name.
#[derive(Debug, Clone)]
pub struct ResolvedServer {
    pub active: McpServerConfig,
    pub shadows: Vec<McpServerConfig>,
}

/// Load all MCP server configs across discovered sources, resolving same-name
/// collisions by `ConfigSource::priority()` (lower wins). Shadowed entries are
/// preserved on each `ResolvedServer` so callers that need them (e.g. `server
/// show`) can report them; callers that don't just access `.active`.
pub fn load_all() -> Result<BTreeMap<ServerId, ResolvedServer>> {
    let mut merged: BTreeMap<ServerId, ResolvedServer> = BTreeMap::new();

    for src in discovered_sources()? {
        if !src.exists {
            continue;
        }
        let servers = parse_source(src.source, &src.path)?;
        for server in servers {
            match merged.get_mut(&server.id) {
                None => {
                    merged.insert(
                        server.id.clone(),
                        ResolvedServer {
                            active: server,
                            shadows: Vec::new(),
                        },
                    );
                }
                Some(existing) => {
                    if server.source.priority() < existing.active.source.priority() {
                        let demoted = std::mem::replace(&mut existing.active, server);
                        existing.shadows.push(demoted);
                    } else {
                        existing.shadows.push(server);
                    }
                }
            }
        }
    }

    // Keep shadow lists deterministically ordered by priority.
    for r in merged.values_mut() {
        r.shadows.sort_by_key(|s| s.source.priority());
    }

    Ok(merged)
}

fn parse_source(source: ConfigSource, path: &Path) -> Result<Vec<McpServerConfig>> {
    let bytes = std::fs::read(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })?;

    if source == ConfigSource::Zed {
        return parse_zed_source(path, &bytes);
    }

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

fn parse_zed_source(path: &Path, bytes: &[u8]) -> Result<Vec<McpServerConfig>> {
    let settings: ZedSettings = serde_json::from_slice(bytes).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;

    Ok(settings
        .context_servers
        .into_iter()
        .filter_map(|(name, srv)| {
            let s = srv.settings?;
            let command = s.command?;
            Some(McpServerConfig {
                id: ServerId(name),
                source: ConfigSource::Zed,
                source_path: path.to_path_buf(),
                transport: McpTransport::Stdio {
                    command,
                    args: s.args,
                    env: s.env,
                },
            })
        })
        .collect())
}

/// Apply a CLI `--override-cmd` to a resolved server config. Returns the config
/// unchanged when `override_cmd` is `None`; errors when the target uses an
/// http transport (override-cmd is stdio-only).
pub fn apply_override(
    cfg: &McpServerConfig,
    override_cmd: Option<&str>,
) -> Result<McpServerConfig> {
    let Some(new_command) = override_cmd else {
        return Ok(cfg.clone());
    };
    let McpTransport::Stdio { args, env, .. } = &cfg.transport else {
        return Err(crate::error::CmcpError::InvalidArg {
            arg: "--override-cmd".into(),
            reason: format!(
                "server '{}' uses http transport; --override-cmd only applies to stdio",
                cfg.id
            ),
        });
    };
    let mut out = cfg.clone();
    out.transport = McpTransport::Stdio {
        command: new_command.to_string(),
        args: args.clone(),
        env: env.clone(),
    };
    Ok(out)
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
    fn apply_override_swaps_command_keeps_args_env() {
        let cfg = McpServerConfig {
            id: ServerId("s".into()),
            source: ConfigSource::Cmcp,
            source_path: PathBuf::from("/tmp/x"),
            transport: McpTransport::Stdio {
                command: "orig".into(),
                args: vec!["a".into(), "b".into()],
                env: BTreeMap::from([("K".into(), "V".into())]),
            },
        };
        let out = apply_override(&cfg, Some("/new/bin")).unwrap();
        let McpTransport::Stdio { command, args, env } = &out.transport else {
            panic!("expected stdio");
        };
        assert_eq!(command, "/new/bin");
        assert_eq!(args, &vec!["a".to_string(), "b".into()]);
        assert_eq!(env.get("K").map(String::as_str), Some("V"));
    }

    #[test]
    fn apply_override_none_is_noop_clone() {
        let cfg = McpServerConfig {
            id: ServerId("s".into()),
            source: ConfigSource::Cmcp,
            source_path: PathBuf::from("/tmp/x"),
            transport: McpTransport::Stdio {
                command: "orig".into(),
                args: vec![],
                env: BTreeMap::new(),
            },
        };
        let out = apply_override(&cfg, None).unwrap();
        assert!(matches!(
            out.transport,
            McpTransport::Stdio { ref command, .. } if command == "orig"
        ));
    }

    #[test]
    fn apply_override_rejects_http() {
        let cfg = McpServerConfig {
            id: ServerId("s".into()),
            source: ConfigSource::Cmcp,
            source_path: PathBuf::from("/tmp/x"),
            transport: McpTransport::Http {
                url: "https://x".into(),
                headers: BTreeMap::new(),
            },
        };
        let err = apply_override(&cfg, Some("/new/bin")).unwrap_err();
        assert!(err.to_string().contains("http"));
    }

    #[test]
    fn shadow_ordering_sorted_by_priority() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("MCPCTL_CONFIG_DIR", tmp.path());
        std::fs::write(
            tmp.path().join("mcp.json"),
            r#"{"mcpServers":{"s":{"command":"cmcp-one"}}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".claude.json"),
            r#"{"mcpServers":{"s":{"command":"claude-one"}}}"#,
        )
        .unwrap();
        let all = load_all().unwrap();
        let r = all.get(&ServerId("s".into())).unwrap();
        assert!(matches!(
            r.active.transport,
            McpTransport::Stdio { ref command, .. } if command == "cmcp-one"
        ));
        assert_eq!(r.shadows.len(), 1);
        assert_eq!(r.shadows[0].source, ConfigSource::ClaudeCode);
        std::env::remove_var("MCPCTL_CONFIG_DIR");
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
