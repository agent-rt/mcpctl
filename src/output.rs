use std::io::IsTerminal;

use rmcp::model::{
    CallToolResult, GetPromptResult, Prompt, PromptMessageContent, ReadResourceResult, Resource,
    ResourceContents, Tool,
};
use serde_json::json;

use crate::config::{redact_map, McpServerConfig, McpTransport};

/// Output format chosen per invocation. Defaults:
/// - `--json` → [`Format::Json`] (explicit opt-in for structured JSON)
/// - stdout is a TTY → [`Format::Pretty`] (human-friendly columns)
/// - stdout is a pipe / file → [`Format::Tsv`] (agent-friendly, one record per
///   line, fields tab-separated, no header)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Format {
    Pretty,
    Tsv,
    Json,
}

pub fn list_format(as_json: bool) -> Format {
    if as_json {
        Format::Json
    } else if std::io::stdout().is_terminal() {
        Format::Pretty
    } else {
        Format::Tsv
    }
}

/// Squeeze TSV-hostile characters out of a field so each record stays on one
/// line. Deliberately lossy: agents that need the original multi-line content
/// should use `--json`.
fn tsv_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' | '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

// ==== server list ==============================================================

pub fn print_server_list(servers: &[&McpServerConfig], as_json: bool) {
    match list_format(as_json) {
        Format::Json => {
            let arr: Vec<_> = servers
                .iter()
                .map(|s| {
                    json!({
                        "id": s.id,
                        "source": s.source.label(),
                        "source_path": s.source_path,
                        "transport": match &s.transport {
                            McpTransport::Stdio { command, .. } => json!({"type": "stdio", "command": command}),
                            McpTransport::Http  { url, .. }     => json!({"type": "http",  "url": url}),
                        }
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        }
        Format::Tsv => {
            // columns: name \t source \t transport \t command_or_url
            for s in servers {
                let (transport, target) = match &s.transport {
                    McpTransport::Stdio { command, args, .. } => {
                        let joined = if args.is_empty() {
                            command.clone()
                        } else {
                            format!("{} {}", command, args.join(" "))
                        };
                        ("stdio", joined)
                    }
                    McpTransport::Http { url, .. } => ("http", url.clone()),
                };
                println!(
                    "{}\t{}\t{}\t{}",
                    tsv_field(&s.id.0),
                    tsv_field(s.source.label()),
                    transport,
                    tsv_field(&target)
                );
            }
        }
        Format::Pretty => {
            if servers.is_empty() {
                println!(
                    "(no MCP servers found — run `mcpctl config sources` to check config paths)"
                );
                return;
            }
            println!("{:<24} {:<16} TRANSPORT", "SERVER", "SOURCE");
            for s in servers {
                let t = match &s.transport {
                    McpTransport::Stdio { command, args, .. } => {
                        format!("stdio: {} {}", command, args.join(" "))
                    }
                    McpTransport::Http { url, .. } => format!("http:  {url}"),
                };
                println!("{:<24} {:<16} {}", s.id.0, s.source.label(), t);
            }
        }
    }
}

// ==== server list --probe ======================================================

pub struct ProbeRow {
    pub cfg: McpServerConfig,
    pub ms: u64,
    pub status: ProbeStatus,
    pub detail: String,
}

pub enum ProbeStatus {
    Ok,
    Error,
    Timeout,
}

impl ProbeStatus {
    fn label(&self) -> &'static str {
        match self {
            ProbeStatus::Ok => "ok",
            ProbeStatus::Error => "error",
            ProbeStatus::Timeout => "timeout",
        }
    }
}

pub fn print_server_list_probed(rows: &[ProbeRow], as_json: bool) {
    match list_format(as_json) {
        Format::Json => {
            let arr: Vec<_> = rows
                .iter()
                .map(|r| {
                    let transport = match &r.cfg.transport {
                        McpTransport::Stdio { command, .. } => {
                            json!({"type": "stdio", "command": command})
                        }
                        McpTransport::Http { url, .. } => json!({"type": "http", "url": url}),
                    };
                    json!({
                        "id": r.cfg.id,
                        "source": r.cfg.source.label(),
                        "transport": transport,
                        "ms": r.ms,
                        "status": r.status.label(),
                        "detail": r.detail,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        }
        Format::Tsv => {
            // columns: name \t source \t transport \t status \t ms \t detail
            for r in rows {
                let (transport, _target) = match &r.cfg.transport {
                    McpTransport::Stdio { .. } => ("stdio", ""),
                    McpTransport::Http { .. } => ("http", ""),
                };
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    tsv_field(&r.cfg.id.0),
                    r.cfg.source.label(),
                    transport,
                    r.status.label(),
                    r.ms,
                    tsv_field(&r.detail),
                );
            }
        }
        Format::Pretty => {
            println!(
                "{:<24} {:<14} {:<8} {:<8} DETAIL",
                "SERVER", "SOURCE", "STATUS", "MS"
            );
            for r in rows {
                println!(
                    "{:<24} {:<14} {:<8} {:<8} {}",
                    r.cfg.id.0,
                    r.cfg.source.label(),
                    r.status.label(),
                    r.ms,
                    r.detail,
                );
            }
        }
    }
}

// ==== server show ==============================================================
// Single-record detail; TSV doesn't help (agent can already extract `key: value`
// with awk). Keep key:value for pretty/tsv, JSON for --json.

pub fn print_server_show(
    s: &McpServerConfig,
    shadows: &[McpServerConfig],
    reveal: bool,
    as_json: bool,
) {
    if as_json {
        let shadow_arr: Vec<_> = shadows
            .iter()
            .map(|sh| {
                json!({
                    "source": sh.source.label(),
                    "source_path": sh.source_path,
                    "transport": &sh.transport,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": s.id,
                "source": s.source.label(),
                "source_path": s.source_path,
                "transport": &s.transport,
                "shadows": shadow_arr,
            }))
            .unwrap_or_default()
        );
        return;
    }
    println!("server:      {}", s.id);
    println!("source:      {}", s.source.label());
    println!("source_path: {}", s.source_path.display());
    match &s.transport {
        McpTransport::Stdio { command, args, env } => {
            println!("transport:   stdio");
            println!("command:     {command}");
            if !args.is_empty() {
                println!("args:        {}", args.join(" "));
            }
            if !env.is_empty() {
                println!("env:");
                for (k, v) in redact_map(env, reveal) {
                    println!("  {k}={v}");
                }
            }
        }
        McpTransport::Http { url, headers } => {
            println!("transport:   http");
            println!("url:         {url}");
            if !headers.is_empty() {
                println!("headers:");
                for (k, v) in redact_map(headers, reveal) {
                    println!("  {k}: {v}");
                }
            }
        }
    }
    if !shadows.is_empty() {
        println!("shadows:");
        for sh in shadows {
            let t = match &sh.transport {
                McpTransport::Stdio { command, .. } => format!("stdio: {command}"),
                McpTransport::Http { url, .. } => format!("http: {url}"),
            };
            println!(
                "  - {} ({}) — {}",
                sh.source.label(),
                sh.source_path.display(),
                t
            );
        }
    }
}

// ==== tool list ================================================================

pub fn print_tool_list(server_id: &str, tools: &[Tool], desc_chars: usize, as_json: bool) {
    match list_format(as_json) {
        Format::Json => {
            let arr: Vec<_> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema.as_ref(),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        }
        Format::Tsv => {
            for t in tools {
                let desc = clip_desc(t.description.as_deref().unwrap_or(""), desc_chars);
                println!("{}\t{}", tsv_field(&t.name), tsv_field(&desc));
            }
        }
        Format::Pretty => {
            if tools.is_empty() {
                println!("(server '{server_id}' exposes no tools)");
                return;
            }
            for t in tools {
                let desc = clip_desc(t.description.as_deref().unwrap_or(""), desc_chars);
                println!("{:<32} {desc}", t.name);
            }
        }
    }
}

/// First line only, then hard-clip to `max_chars` with ellipsis. `max_chars=0`
/// disables the char-level clip but still takes first line (description \n is
/// never useful for a list row).
fn clip_desc(s: &str, max_chars: usize) -> String {
    let first = s.lines().next().unwrap_or("");
    if max_chars == 0 || first.chars().count() <= max_chars {
        return first.to_string();
    }
    let truncated: String = first.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

/// Render a tool's input schema as a compact one-line signature:
/// `tool_name(required_a: type, optional_b?: type=default, c: "x"|"y")`.
///
/// Deliberately best-effort: JSON Schema is Turing-complete-ish in practice
/// (anyOf / oneOf / $ref / conditionals). On shapes we can't summarize the
/// whole arg falls back to `<json>`, and the caller can get the full schema
/// with `--json`. The goal is to be *short and truthful for the common case*,
/// not lossless.
pub fn render_signature(name: &str, schema: &serde_json::Map<String, serde_json::Value>) -> String {
    use serde_json::Value;

    let Some(Value::Object(props)) = schema.get("properties") else {
        return format!("{name}()");
    };
    let required: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // Required first (agents scan them first), then optional. Within each
    // group, order-stable from the JSON Schema's own iteration.
    let mut req_parts: Vec<String> = Vec::new();
    let mut opt_parts: Vec<String> = Vec::new();
    for (key, prop) in props {
        let is_required = required.contains(key.as_str());
        let ty = render_schema_type(prop);
        let default = prop
            .get("default")
            .map(|d| {
                let rendered = serde_json::to_string(d).unwrap_or_default();
                format!("={rendered}")
            })
            .unwrap_or_default();
        let piece = if is_required {
            format!("{key}: {ty}{default}")
        } else {
            format!("{key}?: {ty}{default}")
        };
        if is_required {
            req_parts.push(piece);
        } else {
            opt_parts.push(piece);
        }
    }
    req_parts.extend(opt_parts);
    format!("{name}({})", req_parts.join(", "))
}

fn render_schema_type(prop: &serde_json::Value) -> String {
    use serde_json::Value;
    // enum → "a"|"b"|"c"
    if let Some(Value::Array(vs)) = prop.get("enum") {
        return vs
            .iter()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("|");
    }
    // const → literal
    if let Some(c) = prop.get("const") {
        return serde_json::to_string(c).unwrap_or_default();
    }
    match prop.get("type") {
        Some(Value::String(s)) if s == "array" => {
            let item_ty = prop
                .get("items")
                .map(render_schema_type)
                .unwrap_or_else(|| "any".into());
            format!("{item_ty}[]")
        }
        Some(Value::String(s)) if s == "object" => "object".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("|"),
        _ => {
            // anyOf/oneOf as fallback
            if let Some(Value::Array(arr)) = prop.get("anyOf").or_else(|| prop.get("oneOf")) {
                return arr
                    .iter()
                    .map(render_schema_type)
                    .collect::<Vec<_>>()
                    .join("|");
            }
            "any".into()
        }
    }
}

// ==== tool show ================================================================
// Schema is inherently nested; JSON is the correct default. Plain mode shows
// metadata as key:value lines plus the schema pretty-printed as JSON — agents
// can just pipe through `jq`/`awk` from there.

pub fn print_tool_show(tool: &Tool, signature: bool, as_json: bool) {
    if signature {
        println!(
            "{}",
            render_signature(&tool.name, tool.input_schema.as_ref())
        );
        return;
    }
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "input_schema": tool.input_schema.as_ref(),
                "output_schema": tool.output_schema.as_ref().map(|s| s.as_ref()),
            }))
            .unwrap_or_default()
        );
        return;
    }
    println!("tool:        {}", tool.name);
    if let Some(title) = &tool.title {
        println!("title:       {title}");
    }
    if let Some(d) = &tool.description {
        println!("description: {d}");
    }
    println!("input_schema:");
    println!(
        "{}",
        serde_json::to_string_pretty(tool.input_schema.as_ref()).unwrap_or_default()
    );
}

// ==== call result ==============================================================
// Tool's own text is the data. No TSV (would be lossy for multi-line bodies).

pub fn print_call_result(result: &CallToolResult, as_json: bool, structured_only: bool) {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
        return;
    }
    if result.is_error == Some(true) {
        eprintln!("(tool reported error)");
    }
    // `--structured`: prefer the machine-readable structured_content. Falls
    // back to text content if the tool didn't return any — that way agent
    // callers always get *something* and can branch on exit code alone.
    if structured_only {
        if let Some(structured) = &result.structured_content {
            println!(
                "{}",
                serde_json::to_string_pretty(structured).unwrap_or_default()
            );
            return;
        }
        // fall through to content text
    }
    for content in &result.content {
        if let Some(text) = content.as_text() {
            println!("{}", text.text);
        } else if let Some(img) = content.as_image() {
            println!("[image: {} bytes, {}]", img.data.len(), img.mime_type);
        } else {
            println!(
                "[non-text content: {}]",
                serde_json::to_string(content).unwrap_or_default()
            );
        }
    }
    if !structured_only {
        if let Some(structured) = &result.structured_content {
            println!("---");
            println!(
                "{}",
                serde_json::to_string_pretty(structured).unwrap_or_default()
            );
        }
    }
}

// ==== config sources ===========================================================

pub fn print_config_sources(sources: &[crate::config::DiscoveredSource], as_json: bool) {
    match list_format(as_json) {
        Format::Json => {
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
        }
        Format::Tsv => {
            // columns: source \t exists(true|false) \t path
            for s in sources {
                println!(
                    "{}\t{}\t{}",
                    s.source.label(),
                    s.exists,
                    tsv_field(&s.path.display().to_string())
                );
            }
        }
        Format::Pretty => {
            println!("{:<16} {:<8} PATH", "SOURCE", "EXISTS");
            for s in sources {
                println!(
                    "{:<16} {:<8} {}",
                    s.source.label(),
                    if s.exists { "yes" } else { "no" },
                    s.path.display()
                );
            }
        }
    }
}

// ==== tail =====================================================================
// Streaming notifications. ndjson for --json (one object per line — appendable,
// parseable mid-stream). Pretty mode: timestamped "[HH:MM:SS.mmm] kind — …".
// TSV: `timestamp \t kind \t payload_json_compact`.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TailKind {
    Progress,
    Log,
    ResourceUpdated,
    ResourceListChanged,
    ToolListChanged,
    PromptListChanged,
    Cancelled,
    Custom,
}

impl TailKind {
    pub fn label(self) -> &'static str {
        match self {
            TailKind::Progress => "progress",
            TailKind::Log => "log",
            TailKind::ResourceUpdated => "resource_updated",
            TailKind::ResourceListChanged => "resource_list_changed",
            TailKind::ToolListChanged => "tool_list_changed",
            TailKind::PromptListChanged => "prompt_list_changed",
            TailKind::Cancelled => "cancelled",
            TailKind::Custom => "custom",
        }
    }

    pub fn from_cli_label(s: &str) -> Option<Self> {
        Some(match s {
            "progress" => TailKind::Progress,
            "log" | "logging_message" => TailKind::Log,
            "resource_updated" => TailKind::ResourceUpdated,
            "resource_list_changed" => TailKind::ResourceListChanged,
            "tool_list_changed" => TailKind::ToolListChanged,
            "prompt_list_changed" => TailKind::PromptListChanged,
            "cancelled" => TailKind::Cancelled,
            "custom" => TailKind::Custom,
            _ => return None,
        })
    }
}

pub struct TailEvent {
    pub kind: TailKind,
    pub payload: serde_json::Value,
}

pub fn print_tail_event(evt: &TailEvent, as_json: bool) {
    // Wall-clock with millisecond precision. No external deps — we format
    // from SystemTime ourselves.
    let ts = format_local_ts(std::time::SystemTime::now());

    if as_json {
        // ndjson: one compact object per line so `tail -f | jq` works.
        let obj = json!({
            "ts": ts,
            "kind": evt.kind.label(),
            "payload": evt.payload,
        });
        println!("{}", serde_json::to_string(&obj).unwrap_or_default());
        return;
    }

    match list_format(false) {
        Format::Tsv | Format::Json => {
            // Format::Json not reachable here (as_json handled above); treat
            // both non-pretty branches as TSV to keep stream-ability.
            let compact = serde_json::to_string(&evt.payload).unwrap_or_else(|_| "null".into());
            println!("{}\t{}\t{}", ts, evt.kind.label(), tsv_field(&compact));
        }
        Format::Pretty => {
            let summary = pretty_tail_summary(evt);
            println!("[{ts}] {} — {summary}", evt.kind.label());
        }
    }
}

fn pretty_tail_summary(evt: &TailEvent) -> String {
    use serde_json::Value;
    match evt.kind {
        TailKind::Progress => {
            let progress = evt.payload.get("progress").and_then(Value::as_f64);
            let total = evt.payload.get("total").and_then(Value::as_f64);
            let message = evt
                .payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("");
            match (progress, total) {
                (Some(p), Some(t)) => format!("{p}/{t} {message}"),
                (Some(p), None) => format!("{p} {message}"),
                _ => message.to_string(),
            }
        }
        TailKind::Log => {
            let level = evt
                .payload
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let data = evt.payload.get("data");
            let data_str = match data {
                Some(Value::String(s)) => s.clone(),
                Some(v) => serde_json::to_string(v).unwrap_or_default(),
                None => String::new(),
            };
            format!("{level}: {data_str}")
        }
        TailKind::ResourceUpdated => evt
            .payload
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or("(no uri)")
            .to_string(),
        TailKind::Cancelled => evt
            .payload
            .get("requestId")
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default(),
        TailKind::ResourceListChanged | TailKind::ToolListChanged | TailKind::PromptListChanged => {
            String::new()
        }
        TailKind::Custom => serde_json::to_string(&evt.payload).unwrap_or_default(),
    }
}

fn format_local_ts(t: std::time::SystemTime) -> String {
    // HH:MM:SS.mmm in *UTC* — the point of tail is to correlate events, not
    // to match wall-clock perfectly. Zero-dep formatting.
    let dur = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

// ==== introspect ===============================================================
// Single-session dump: server transport + tool list (with schemas or
// signatures). TSV mode emits one line per tool so it composes with awk.

pub fn print_introspect(
    server: &str,
    cfg: &McpServerConfig,
    tools: &[Tool],
    prompts: Option<&[Prompt]>,
    resources: Option<&[Resource]>,
    signature: bool,
    desc_chars: usize,
    as_json: bool,
) {
    match list_format(as_json) {
        Format::Json => {
            let tool_arr: Vec<_> = tools
                .iter()
                .map(|t| {
                    let mut obj = json!({
                        "name": t.name,
                        "description": t.description,
                    });
                    if signature {
                        obj["signature"] =
                            json!(render_signature(&t.name, t.input_schema.as_ref()));
                    } else {
                        obj["input_schema"] = json!(t.input_schema.as_ref());
                        if let Some(out) = &t.output_schema {
                            obj["output_schema"] = json!(out.as_ref());
                        }
                    }
                    obj
                })
                .collect();
            let transport = match &cfg.transport {
                McpTransport::Stdio { command, .. } => json!({"type": "stdio", "command": command}),
                McpTransport::Http { url, .. } => json!({"type": "http", "url": url}),
            };
            let prompt_arr: Option<Vec<_>> = prompts.map(|ps| {
                ps.iter()
                    .map(|p| {
                        json!({
                            "name": p.name,
                            "description": p.description,
                            "arguments": p.arguments.as_deref().unwrap_or(&[]).len(),
                        })
                    })
                    .collect()
            });
            let resource_arr: Option<Vec<_>> = resources.map(|rs| {
                rs.iter()
                    .map(|r| {
                        json!({
                            "uri": r.uri,
                            "name": r.name,
                            "mime_type": r.mime_type,
                        })
                    })
                    .collect()
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "server": server,
                    "source": cfg.source.label(),
                    "transport": transport,
                    "tools": tool_arr,
                    "prompts": prompt_arr,
                    "resources": resource_arr,
                }))
                .unwrap_or_default()
            );
        }
        Format::Tsv => {
            for t in tools {
                let col2 = if signature {
                    render_signature(&t.name, t.input_schema.as_ref())
                } else {
                    clip_desc(t.description.as_deref().unwrap_or(""), desc_chars)
                };
                println!("tool\t{}\t{}", tsv_field(&t.name), tsv_field(&col2));
            }
            if let Some(ps) = prompts {
                for p in ps {
                    println!(
                        "prompt\t{}\t{}",
                        tsv_field(&p.name),
                        tsv_field(p.description.as_deref().unwrap_or(""))
                    );
                }
            }
            if let Some(rs) = resources {
                for r in rs {
                    println!("resource\t{}\t{}", tsv_field(&r.uri), tsv_field(&r.name));
                }
            }
        }
        Format::Pretty => {
            println!("server:    {server}");
            println!("source:    {}", cfg.source.label());
            match &cfg.transport {
                McpTransport::Stdio { command, args, .. } => {
                    println!("transport: stdio: {command} {}", args.join(" "));
                }
                McpTransport::Http { url, .. } => {
                    println!("transport: http: {url}");
                }
            }
            println!("tools ({}):", tools.len());
            for t in tools {
                if signature {
                    println!("  {}", render_signature(&t.name, t.input_schema.as_ref()));
                } else {
                    let desc = clip_desc(t.description.as_deref().unwrap_or(""), desc_chars);
                    println!("  {:<30} {desc}", t.name);
                }
            }
            if let Some(ps) = prompts {
                println!("prompts ({}):", ps.len());
                for p in ps {
                    let desc = clip_desc(p.description.as_deref().unwrap_or(""), desc_chars);
                    println!("  {:<30} {desc}", p.name);
                }
            }
            if let Some(rs) = resources {
                println!("resources ({}):", rs.len());
                for r in rs {
                    println!("  {}", r.uri);
                }
            }
        }
    }
}

// ==== server check =============================================================
// Up to 2 rows (handshake, list_tools) + optional error. TSV emits one row per
// stage so `awk -F'\t' '$3=="error"'` triggers on failure.

pub struct CheckRow<'a> {
    pub stage: &'a str,
    pub ms: Option<u64>,
    pub status: CheckStatus,
    pub detail: String,
}

pub enum CheckStatus {
    Ok,
    Error,
    Skipped,
}

impl CheckStatus {
    fn label(&self) -> &'static str {
        match self {
            CheckStatus::Ok => "ok",
            CheckStatus::Error => "error",
            CheckStatus::Skipped => "skipped",
        }
    }
}

pub fn print_check(server: &str, rows: &[CheckRow<'_>], as_json: bool) {
    match list_format(as_json) {
        Format::Json => {
            let arr: Vec<_> = rows
                .iter()
                .map(|r| {
                    json!({
                        "stage": r.stage,
                        "ms": r.ms,
                        "status": r.status.label(),
                        "detail": r.detail,
                    })
                })
                .collect();
            let ok = rows.iter().all(|r| !matches!(r.status, CheckStatus::Error));
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "server": server,
                    "ok": ok,
                    "stages": arr,
                }))
                .unwrap_or_default()
            );
        }
        Format::Tsv => {
            // columns: stage \t ms \t status \t detail
            for r in rows {
                let ms = r.ms.map(|m| m.to_string()).unwrap_or_else(|| "-".into());
                println!(
                    "{}\t{}\t{}\t{}",
                    r.stage,
                    ms,
                    r.status.label(),
                    tsv_field(&r.detail)
                );
            }
        }
        Format::Pretty => {
            println!("server:      {server}");
            for r in rows {
                let ms =
                    r.ms.map(|m| format!("{m} ms"))
                        .unwrap_or_else(|| "-".into());
                let label = format!("{}:", r.stage);
                match r.status {
                    CheckStatus::Ok => println!("{label:<12} {ms} — {}", r.detail),
                    CheckStatus::Error => {
                        println!("{label:<12} FAILED\n  {}", r.detail.replace('\n', "\n  "))
                    }
                    CheckStatus::Skipped => println!("{label:<12} skipped"),
                }
            }
        }
    }
}

// ==== prompt list =============================================================

pub fn print_prompt_list(server: &str, prompts: &[Prompt], as_json: bool) {
    match list_format(as_json) {
        Format::Json => {
            let arr: Vec<_> = prompts
                .iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "description": p.description,
                        "arguments": p.arguments.as_deref().unwrap_or(&[]).iter().map(|a| json!({
                            "name": a.name,
                            "description": a.description,
                            "required": a.required.unwrap_or(false),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        }
        Format::Tsv => {
            for p in prompts {
                println!(
                    "{}\t{}\t{}",
                    tsv_field(&p.name),
                    tsv_field(p.description.as_deref().unwrap_or("")),
                    p.arguments.as_deref().map(|a| a.len()).unwrap_or(0),
                );
            }
        }
        Format::Pretty => {
            if prompts.is_empty() {
                println!("(no prompts — server may not declare prompts capability)");
                return;
            }
            println!("Prompts on {server}:");
            for p in prompts {
                let desc = p.description.as_deref().unwrap_or("");
                let args = p.arguments.as_deref().map(|a| a.len()).unwrap_or(0);
                println!("  {:<30} {} ({} args)", p.name, desc, args);
            }
        }
    }
}

// ==== prompt get ==============================================================

pub fn print_prompt_get(server: &str, prompt: &str, result: &GetPromptResult, as_json: bool) {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
        return;
    }
    if std::io::stdout().is_terminal() {
        println!("Prompt {server}/{prompt}:");
        if let Some(desc) = &result.description {
            println!("  description: {desc}");
        }
        println!("  messages: {}", result.messages.len());
    }
    for msg in &result.messages {
        let role = format!("{:?}", msg.role).to_lowercase();
        match &msg.content {
            PromptMessageContent::Text { text } => {
                if std::io::stdout().is_terminal() {
                    println!("[{role}] {text}");
                } else {
                    println!("{role}\ttext\t{}", tsv_field(text));
                }
            }
            PromptMessageContent::Image { image } => {
                let mime = &image.mime_type;
                if std::io::stdout().is_terminal() {
                    println!("[{role}] <image {mime}>");
                } else {
                    println!("{role}\timage\t{mime}");
                }
            }
            PromptMessageContent::Resource { resource } => {
                let uri = match &resource.resource {
                    ResourceContents::TextResourceContents { uri, .. } => uri.as_str(),
                    ResourceContents::BlobResourceContents { uri, .. } => uri.as_str(),
                };
                if std::io::stdout().is_terminal() {
                    println!("[{role}] <resource {uri}>");
                } else {
                    println!("{role}\tresource\t{uri}");
                }
            }
            PromptMessageContent::ResourceLink { link } => {
                let uri = &link.uri;
                if std::io::stdout().is_terminal() {
                    println!("[{role}] <resource-link {uri}>");
                } else {
                    println!("{role}\tresource-link\t{uri}");
                }
            }
        }
    }
}

// ==== resource list ===========================================================

pub fn print_resource_list(server: &str, resources: &[Resource], as_json: bool) {
    match list_format(as_json) {
        Format::Json => {
            let arr: Vec<_> = resources
                .iter()
                .map(|r| {
                    json!({
                        "uri": r.uri,
                        "name": r.name,
                        "description": r.description,
                        "mime_type": r.mime_type,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        }
        Format::Tsv => {
            for r in resources {
                println!(
                    "{}\t{}\t{}",
                    tsv_field(&r.uri),
                    tsv_field(&r.name),
                    tsv_field(r.mime_type.as_deref().unwrap_or("")),
                );
            }
        }
        Format::Pretty => {
            if resources.is_empty() {
                println!("(no resources — server may not declare resources capability)");
                return;
            }
            println!("Resources on {server}:");
            for r in resources {
                let mime = r.mime_type.as_deref().unwrap_or("?");
                let desc = r.description.as_deref().unwrap_or("");
                println!("  {:<50} [{mime}] {desc}", r.uri);
            }
        }
    }
}

// ==== resource read ===========================================================

pub fn print_resource_read(server: &str, uri: &str, result: &ReadResourceResult, as_json: bool) {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
        return;
    }
    for content in &result.contents {
        match content {
            ResourceContents::TextResourceContents {
                text, mime_type, ..
            } => {
                if std::io::stdout().is_terminal() {
                    println!(
                        "# {server}/{uri} ({})",
                        mime_type.as_deref().unwrap_or("text/plain")
                    );
                    println!("{text}");
                } else {
                    print!("{text}");
                }
            }
            ResourceContents::BlobResourceContents {
                blob, mime_type, ..
            } => {
                if std::io::stdout().is_terminal() {
                    println!(
                        "# {server}/{uri} ({}) — {} bytes base64",
                        mime_type.as_deref().unwrap_or("application/octet-stream"),
                        blob.len()
                    );
                    println!("{blob}");
                } else {
                    print!("{blob}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().cloned().expect("schema must be an object")
    }

    #[test]
    fn signature_required_vs_optional() {
        let s = schema(json!({
            "type": "object",
            "properties": {
                "q": {"type": "string"},
                "limit": {"type": "number", "default": 10},
            },
            "required": ["q"],
        }));
        assert_eq!(
            render_signature("search", &s),
            "search(q: string, limit?: number=10)"
        );
    }

    #[test]
    fn signature_enum_and_array() {
        let s = schema(json!({
            "type": "object",
            "properties": {
                "mode": {"enum": ["hybrid", "lex"]},
                "tags": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["mode", "tags"],
        }));
        let sig = render_signature("q", &s);
        assert!(sig.contains("mode: \"hybrid\"|\"lex\""));
        assert!(sig.contains("tags: string[]"));
    }

    #[test]
    fn signature_no_properties() {
        let s = schema(json!({"type": "object"}));
        assert_eq!(render_signature("ping", &s), "ping()");
    }

    #[test]
    fn signature_any_of_fallback() {
        let s = schema(json!({
            "type": "object",
            "properties": {
                "x": {"anyOf": [{"type": "string"}, {"type": "number"}]},
            },
            "required": ["x"],
        }));
        assert_eq!(render_signature("f", &s), "f(x: string|number)");
    }

    #[test]
    fn clip_desc_truncates_at_char_boundary() {
        let input = "hello world this is a long description";
        assert_eq!(clip_desc(input, 0), input);
        assert_eq!(clip_desc(input, 10), "hello wor…");
        assert_eq!(clip_desc("short", 10), "short");
    }

    #[test]
    fn clip_desc_first_line_only() {
        assert_eq!(clip_desc("first line\nsecond", 0), "first line");
    }

    #[test]
    fn tsv_field_replaces_control_chars() {
        assert_eq!(tsv_field("a\tb\nc\rd"), "a b c d");
    }

    #[test]
    fn tail_kind_roundtrip() {
        for k in [
            TailKind::Progress,
            TailKind::Log,
            TailKind::ResourceUpdated,
            TailKind::ResourceListChanged,
            TailKind::ToolListChanged,
            TailKind::PromptListChanged,
            TailKind::Cancelled,
            TailKind::Custom,
        ] {
            assert_eq!(TailKind::from_cli_label(k.label()), Some(k));
        }
        assert_eq!(
            TailKind::from_cli_label("logging_message"),
            Some(TailKind::Log)
        );
        assert_eq!(TailKind::from_cli_label("bogus"), None);
    }

    #[test]
    fn pretty_tail_summary_progress() {
        let evt = TailEvent {
            kind: TailKind::Progress,
            payload: json!({"progress": 3, "total": 10, "message": "indexing"}),
        };
        assert_eq!(pretty_tail_summary(&evt), "3/10 indexing");
    }

    #[test]
    fn pretty_tail_summary_log() {
        let evt = TailEvent {
            kind: TailKind::Log,
            payload: json!({"level": "info", "data": "hello"}),
        };
        assert_eq!(pretty_tail_summary(&evt), "info: hello");
    }
}
