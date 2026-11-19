//! engine-mcp: newline-delimited JSON-RPC 2.0 framing (MCP stdio transport).
//!
//! Owns message parsing + MCP lifecycle (initialize / tools/list / tools/call /
//! ping). The actual tool execution is delegated to a `ToolHandler` implemented
//! by the binary, which owns World + Renderer + AssetPipeline.

use serde_json::{json, Value};
use shared::mcp_protocol::{builtin_tools, error_result, JsonRpcRequest, JsonRpcResponse};

pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Implemented by the host (main.rs) — executes one MCP tool call and returns
/// the tools/call `result` payload (or an error message).
pub trait ToolHandler {
    fn handle(&mut self, name: &str, args: &Value) -> Result<Value, String>;
}

/// Parse one stdin line. Returns Some(response_json) if a response should be
/// written to stdout, None for notifications / empty lines / unparseable input.
pub fn handle_line(line: &str, handler: &mut dyn ToolHandler) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return serde_json::to_value(JsonRpcResponse::err(
                Value::Null,
                -32700,
                format!("parse error: {e}"),
            ))
            .ok();
        }
    };
    let resp = dispatch(&req, handler);
    if resp.is_null() {
        None
    } else {
        Some(resp)
    }
}

fn dispatch(req: &JsonRpcRequest, handler: &mut dyn ToolHandler) -> Value {
    let id = req.id.clone();
    match req.method.as_str() {
        "initialize" => respond(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "ai-engine", "version": env!("CARGO_PKG_VERSION")}
            }),
        ),
        "ping" => respond(id, json!({})),
        "tools/list" => respond(id, json!({"tools": builtin_tools()})),
        "tools/call" => {
            let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = req.params.get("arguments").cloned().unwrap_or(json!({}));
            match handler.handle(name, &args) {
                Ok(v) => respond(id, v),
                Err(msg) => respond(id, error_result(msg)),
            }
        }
        // MCP notifications: never answer (even if an id was wrongly sent)
        m if m.starts_with("notifications/") => Value::Null,
        other => serde_json::to_value(JsonRpcResponse::err(
            id.unwrap_or(Value::Null),
            -32601,
            format!("method not found: {other}"),
        ))
        .unwrap_or(Value::Null),
    }
}

fn respond(id: Option<Value>, result: Value) -> Value {
    match id {
        // notification-style requests get no response per JSON-RPC
        None => Value::Null,
        Some(id) => serde_json::to_value(JsonRpcResponse::ok(id, result)).unwrap_or(Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    impl ToolHandler for Echo {
        fn handle(&mut self, name: &str, args: &Value) -> Result<Value, String> {
            if name == "fail" {
                return Err("boom".into());
            }
            Ok(json!({"content": [{"type": "text", "text": format!("{name}:{args}")}]}))
        }
    }

    #[test]
    fn initialize_and_tools() {
        let mut h = Echo;
        let r = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            &mut h,
        )
        .unwrap();
        assert_eq!(r["result"]["serverInfo"]["name"], "ai-engine");
        let r = handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, &mut h).unwrap();
        assert!(r["result"]["tools"].as_array().unwrap().len() >= 8);
    }

    #[test]
    fn tools_call_and_errors() {
        let mut h = Echo;
        let r = handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"x","arguments":{"a":1}}}"#,
            &mut h,
        )
        .unwrap();
        assert!(r["result"]["content"].is_array());
        let r = handle_line(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"fail"}}"#,
            &mut h,
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], true);
        // notifications produce no output
        assert!(handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &mut h
        )
        .is_none());
        assert!(handle_line("not json", &mut h).is_some()); // parse error response
    }
}
