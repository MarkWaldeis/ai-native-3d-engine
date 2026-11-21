//! engine-mcp: newline-delimited JSON-RPC 2.0 framing (MCP stdio transport).
//!
//! Owns message parsing + MCP lifecycle (initialize / tools/list / tools/call /
//! ping). The actual tool execution is delegated to a `ToolHandler` implemented
//! by the binary, which owns World + Renderer + AssetPipeline.

use serde_json::{json, Value};
use shared::mcp_protocol::{builtin_tools, error_result, JsonRpcRequest, JsonRpcResponse};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const SUPPORTED_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];
mod discovery;
pub mod terrain;
pub use discovery::agent_guide;

/// Implemented by the host (main.rs) — executes one MCP tool call and returns
/// the tools/call `result` payload (or an error message).
pub trait ToolHandler {
    fn handle(&mut self, name: &str, args: &Value) -> Result<Value, String>;
    fn scene(&self) -> Result<Value, String> {
        Err("scene resource unavailable".into())
    }
}

/// Parse one stdin line. Returns Some(response_json) if a response should be
/// written to stdout, None for notifications / empty lines / unparseable input.
pub fn handle_line(line: &str, handler: &mut dyn ToolHandler) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let value: Value = match serde_json::from_str(line) {
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
    let id = value.get("id").cloned();
    if value["jsonrpc"] != "2.0"
        || !value.is_object()
        || id
            .as_ref()
            .is_some_and(|id| !(id.is_null() || id.is_string() || id.is_number()))
    {
        return Some(rpc_error(Value::Null, -32600, "invalid JSON-RPC request"));
    }
    if value.get("method").is_none()
        && id.is_some()
        && (value.get("result").is_some() || value.get("error").is_some())
    {
        return None;
    }
    let mut req: JsonRpcRequest = match serde_json::from_value(value) {
        Ok(req) => req,
        Err(_) => return Some(rpc_error(Value::Null, -32600, "invalid JSON-RPC request")),
    };
    req.id = id;
    if !req.params.is_null() && !req.params.is_object() {
        return req
            .id
            .map(|id| rpc_error(id, -32602, "params must be an object"));
    }
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
                "protocolVersion": req.params["protocolVersion"].as_str().filter(|v| SUPPORTED_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                "instructions": "Read engine://guide, then engine://scene. Blender-first authoring; simulate and validate before exporting. Local trusted clients only.",
                "serverInfo": {"name": "ai-engine", "version": env!("CARGO_PKG_VERSION")}
            }),
        ),
        "ping" => respond(id, json!({})),
        "tools/list" => respond(id, json!({"tools": discovery::tools()})),
        "resources/list" => respond(id, discovery::resources()),
        "resources/read" => match discovery::read_resource(&req.params, handler) {
            Ok(value) => respond(id, value),
            Err((code, message)) => error(id, code, message),
        },
        "prompts/list" => respond(id, discovery::prompts()),
        "prompts/get" => match discovery::get_prompt(&req.params) {
            Ok(value) => respond(id, value),
            Err(message) => error(id, -32602, message),
        },
        "tools/call" => {
            let Some(name) = req.params.get("name").and_then(|v| v.as_str()) else {
                return error(id, -32602, "missing tool name");
            };
            let args = req.params.get("arguments").cloned().unwrap_or(json!({}));
            if !args.is_object() {
                return error(id, -32602, "arguments must be an object");
            }
            if !builtin_tools().iter().any(|tool| tool.name == name) && !terrain::NAMES.contains(&name) {
                return error(
                    id,
                    -32602,
                    format!(
                        "unknown tool: {name}{}",
                        shared::suggestions::hint(
                            name,
                            builtin_tools().iter().map(|tool| tool.name).chain(terrain::NAMES.iter().copied())
                        )
                    ),
                );
            }
            match handler.handle(name, &args) {
                Ok(v) => respond(id, v),
                Err(msg) => respond(id, error_result(msg)),
            }
        }
        // MCP notifications: never answer (even if an id was wrongly sent)
        m if m.starts_with("notifications/") => Value::Null,
        other => error(id, -32601, format!("method not found: {other}")),
    }
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    serde_json::to_value(JsonRpcResponse::err(id, code, message)).unwrap()
}

fn error(id: Option<Value>, code: i64, message: impl Into<String>) -> Value {
    id.map(|id| rpc_error(id, code, message))
        .unwrap_or(Value::Null)
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
        fn scene(&self) -> Result<Value, String> {
            Ok(json!({"entities":[]}))
        }
        fn handle(&mut self, name: &str, args: &Value) -> Result<Value, String> {
            if name == "remove_entity" {
                return Err("boom".into());
            }
            Ok(json!({"content": [{"type": "text", "text": format!("{name}:{args}")}]}))
        }
    }

    #[test]
    fn unknown_terrain_tool_suggests_the_registered_name() {
        let mut host = Echo;
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"terrain_sampl","arguments":{}}}"#,
            &mut host,
        ).unwrap();
        assert_eq!(response["error"]["code"], -32602);
        let message = response["error"]["message"].as_str().unwrap();
        assert!(message.contains("did you mean"), "{message}");
        assert!(message.contains("terrain_sample"), "{message}");
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
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_revision","arguments":{"a":1}}}"#,
            &mut h,
        )
        .unwrap();
        assert!(r["result"]["content"].is_array());
        let r = handle_line(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"remove_entity"}}"#,
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

    #[test]
    fn versions_resources_and_generated_guide_cover_every_tool() {
        let mut host = Echo;
        for version in SUPPORTED_VERSIONS.iter().copied().chain(["2099-01-01"]) {
            let response = handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version}}).to_string(), &mut host).unwrap();
            assert_eq!(
                response["result"]["protocolVersion"],
                if SUPPORTED_VERSIONS.contains(&version) {
                    version
                } else {
                    PROTOCOL_VERSION
                }
            );
            assert!(response["result"]["capabilities"]["resources"].is_object());
        }
        let guide = agent_guide();
        let listed = handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            &mut host,
        )
        .unwrap();
        for tool in listed["result"]["tools"].as_array().unwrap() {
            assert!(guide.contains(&format!("**{}**", tool["name"].as_str().unwrap())));
            assert!(tool["annotations"]["readOnlyHint"].is_boolean());
            assert!(tool["annotations"]["destructiveHint"].is_boolean());
        }
        let resources = handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#,
            &mut host,
        )
        .unwrap();
        for resource in resources["result"]["resources"].as_array().unwrap() {
            let read = handle_line(&json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":resource["uri"]}}).to_string(), &mut host).unwrap();
            assert_eq!(
                read["result"]["contents"][0]["mimeType"],
                resource["mimeType"]
            );
            assert!(read["result"]["contents"][0]["text"].is_string());
        }
    }

    #[test]
    fn discovery_and_jsonrpc_empty_and_error_cases() {
        let mut host = Echo;
        for (method, params, code) in [
            ("resources/read", json!({"uri":"engine://missing"}), -32002),
            ("resources/read", json!({}), -32602),
            ("prompts/get", json!({"name":"missing"}), -32602),
            ("prompts/get", json!({"name":"build_game"}), -32602),
            ("tools/call", json!({"name":"missing"}), -32602),
            (
                "tools/call",
                json!({"name":"get_revision","arguments":[]}),
                -32602,
            ),
            ("unknown/method", json!({}), -32601),
        ] {
            let response = handle_line(
                &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
                &mut host,
            )
            .unwrap();
            assert_eq!(response["error"]["code"], code, "{response}");
        }
        for bad in ["[]", "{}", r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#] {
            assert_eq!(
                handle_line(bad, &mut host).unwrap()["error"]["code"],
                -32600
            );
        }
        for notification in [
            "",
            r#"{"jsonrpc":"2.0","method":"ping"}"#,
            r#"{"jsonrpc":"2.0","method":"unknown"}"#,
        ] {
            assert!(handle_line(notification, &mut host).is_none());
        }
        let scene = handle_line(r#"{"jsonrpc":"2.0","id":null,"method":"resources/read","params":{"uri":"engine://scene"}}"#, &mut host).unwrap();
        assert_eq!(scene["id"], Value::Null);
        assert_eq!(
            serde_json::from_str::<Value>(scene["result"]["contents"][0]["text"].as_str().unwrap())
                .unwrap(),
            json!({"entities":[]})
        );
    }

    #[test]
    fn all_prompts_are_retrievable_with_declared_arguments() {
        let mut host = Echo;
        let prompts = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list"}"#,
            &mut host,
        )
        .unwrap();
        assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 3);
        for prompt in prompts["result"]["prompts"].as_array().unwrap() {
            let args = json!({"idea":"Ein Garten","goal":"Ein Agententeam"});
            let response = handle_line(&json!({"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":prompt["name"],"arguments":args}}).to_string(), &mut host).unwrap();
            assert_eq!(response["result"]["messages"][0]["role"], "user");
            assert!(response["result"]["messages"][0]["content"]["text"]
                .as_str()
                .unwrap()
                .contains("engine://guide"));
        }
    }
}
