use rmcp::model::{CallToolResult, Tool};
use serde_json::json;

use crate::config::{redact_map, McpServerConfig, McpTransport};

pub fn print_server_list(servers: &[&McpServerConfig], as_json: bool) {
    if as_json {
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
        return;
    }

    if servers.is_empty() {
        println!("(no MCP servers found — run `cmcp config sources` to check config paths)");
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

pub fn print_server_show(s: &McpServerConfig, reveal: bool, as_json: bool) {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": s.id,
                "source": s.source.label(),
                "source_path": s.source_path,
                "transport": &s.transport,
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
}

pub fn print_tool_list(server_id: &str, tools: &[Tool], as_json: bool) {
    if as_json {
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
        return;
    }
    if tools.is_empty() {
        println!("(server '{server_id}' exposes no tools)");
        return;
    }
    for t in tools {
        let desc = t
            .description
            .as_deref()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("");
        println!("{:<32} {desc}", t.name);
    }
}

pub fn print_tool_show(tool: &Tool, as_json: bool) {
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

pub fn print_call_result(result: &CallToolResult, as_json: bool) {
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
    if let Some(structured) = &result.structured_content {
        println!("---");
        println!(
            "{}",
            serde_json::to_string_pretty(structured).unwrap_or_default()
        );
    }
}
