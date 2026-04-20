use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use serde_json::{Map, Value};

use crate::cli::CallArgs;
use crate::config::{load_all, ServerId};
use crate::error::{CmcpError, Result};
use crate::output::print_call_result;
use crate::session;

pub struct Invocation {
    pub server: ServerId,
    pub tool: String,
}

pub fn parse_uri(uri: &str) -> Result<Invocation> {
    // Accept either `mcp://server/tool` or the shorthand `server/tool` (like
    // `curl google.com` omitting the scheme). Any other scheme is an error.
    let rest = if let Some(r) = uri.strip_prefix("mcp://") {
        r
    } else if uri.contains("://") {
        return Err(CmcpError::InvalidUri(format!(
            "only mcp:// scheme is supported, got '{uri}'"
        )));
    } else {
        uri
    };
    let (server, tool) = rest
        .split_once('/')
        .ok_or_else(|| CmcpError::InvalidUri(format!("expected <server>/<tool>, got '{uri}'")))?;
    if server.is_empty() || tool.is_empty() {
        return Err(CmcpError::InvalidUri(format!(
            "server and tool must be non-empty: '{uri}'"
        )));
    }
    if !server
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(CmcpError::InvalidUri(format!(
            "server name must match [A-Za-z0-9_-]+: '{server}'"
        )));
    }
    let tool = tool.split(['?', '#']).next().unwrap_or(tool);
    Ok(Invocation {
        server: ServerId(server.to_string()),
        tool: tool.to_string(),
    })
}

pub fn merge_args(args: &CallArgs) -> Result<Map<String, Value>> {
    let mut base: Map<String, Value> = match &args.args_json {
        Some(s) => {
            let v: Value = serde_json::from_str(s).map_err(|e| CmcpError::InvalidArg {
                arg: "--args-json".into(),
                reason: e.to_string(),
            })?;
            match v {
                Value::Object(o) => o,
                other => {
                    return Err(CmcpError::InvalidArg {
                        arg: "--args-json".into(),
                        reason: format!("must be a JSON object, got {}", value_type(&other)),
                    })
                }
            }
        }
        None => Map::new(),
    };

    for kv in &args.args {
        let (k, v) = kv.split_once('=').ok_or_else(|| CmcpError::InvalidArg {
            arg: kv.clone(),
            reason: "expected key=value".into(),
        })?;
        if k.is_empty() {
            return Err(CmcpError::InvalidArg {
                arg: kv.clone(),
                reason: "key must be non-empty".into(),
            });
        }
        // Try to parse value as JSON (so numbers, bools, arrays, objects work),
        // fall back to string.
        let parsed =
            serde_json::from_str::<Value>(v).unwrap_or_else(|_| Value::String(v.to_string()));
        base.insert(k.to_string(), parsed);
    }
    Ok(base)
}

fn value_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub async fn run(uri: &str, args: CallArgs, verbose: bool) -> Result<()> {
    let inv = parse_uri(uri)?;
    let arguments = merge_args(&args)?;

    let servers = load_all()?;
    let cfg = servers
        .get(&inv.server)
        .ok_or_else(|| CmcpError::ServerNotFound(inv.server.clone()))?;

    let timeout = Duration::from_secs(args.timeout);
    let session = match tokio::time::timeout(timeout, session::connect(cfg, verbose)).await {
        Ok(s) => s?,
        Err(_) => return Err(CmcpError::Timeout(args.timeout)),
    };

    let mut params = CallToolRequestParams::new(inv.tool.clone());
    if !arguments.is_empty() {
        params = params.with_arguments(arguments);
    }

    let result = session::call_tool(&session, params, timeout).await;
    session::close(session).await;
    let result = result?;
    print_call_result(&result, args.json);
    if result.is_error == Some(true) {
        return Err(CmcpError::Service(format!(
            "tool '{}' returned is_error=true",
            inv.tool
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_uri() {
        let inv = parse_uri("mcp://github/search_repos").unwrap();
        assert_eq!(inv.server.0, "github");
        assert_eq!(inv.tool, "search_repos");
    }

    #[test]
    fn parses_scheme_less_shortcut() {
        let inv = parse_uri("cortex/list_projects").unwrap();
        assert_eq!(inv.server.0, "cortex");
        assert_eq!(inv.tool, "list_projects");
    }

    #[test]
    fn rejects_bad_prefix() {
        assert!(parse_uri("http://a/b").is_err());
    }

    #[test]
    fn rejects_missing_tool() {
        assert!(parse_uri("mcp://server").is_err());
        assert!(parse_uri("mcp://server/").is_err());
    }

    #[test]
    fn merge_args_json_typed() {
        let args = CallArgs {
            args: vec!["limit=5".into(), "q=rust".into(), "flag=true".into()],
            args_json: Some(r#"{"query":"base","limit":1}"#.into()),
            ..Default::default()
        };
        let m = merge_args(&args).unwrap();
        assert_eq!(m["q"], Value::String("rust".into()));
        assert_eq!(m["limit"], Value::Number(5.into()));
        assert_eq!(m["flag"], Value::Bool(true));
        assert_eq!(m["query"], Value::String("base".into()));
    }

    #[test]
    fn merge_args_rejects_non_object_json() {
        let args = CallArgs {
            args_json: Some("[1,2,3]".into()),
            ..Default::default()
        };
        assert!(merge_args(&args).is_err());
    }
}
