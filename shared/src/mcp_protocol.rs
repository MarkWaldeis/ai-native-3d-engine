//! MCP-over-stdio protocol types (newline-delimited JSON-RPC 2.0).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    /// Absent on notifications.
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

/// The AI-facing tool surface. Kept intentionally small for v0.1.
pub fn builtin_tools() -> Vec<ToolDef> {
    let vec3 = || json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3});
    let color = || json!({"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4});
    // Optional optimistic-concurrency token accepted by every mutating tool:
    // the call is rejected when the world has moved past this revision.
    let expected_rev = || {
        json!({"type": "integer", "description": "optimistic concurrency: apply only if world.revision still equals this value"})
    };
    vec![
        ToolDef {
            name: "get_scene_state",
            description: "Return the entire scene as JSON (entities, transforms, camera).",
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "spawn_primitive",
            description: "Spawn a mesh entity: procedural (box, plane, sphere, cylinder, wall_with_door) or a GLB asset from assets/ (kind=gltf, path).",
            input_schema: schema(
                json!({
                    "kind": {"type": "string", "enum": ["box", "plane", "sphere", "cylinder", "wall_with_door", "gltf"]},
                    "name": {"type": "string"},
                    "position": vec3(), "rotation_deg": vec3(), "scale": vec3(), "color": color(),
                    "expected_revision": expected_rev(),
                    "path": {"type": "string", "description": "GLB file path for kind=gltf (relative resolves to assets/)"},
                    "size": {"type": "array", "items": {"type": "number"}},
                    "radius": {"type": "number"}, "height": {"type": "number"},
                    "width": {"type": "number"}, "door_width": {"type": "number"}, "door_height": {"type": "number"},
                    "segments": {"type": "integer"}
                }),
                &["kind"],
            ),
        },
        ToolDef {
            name: "set_transform",
            description: "Set position / rotation (deg) / scale of an entity.",
            input_schema: schema(
                json!({"id": {"type": "integer"}, "position": vec3(), "rotation_deg": vec3(), "scale": vec3(), "expected_revision": expected_rev()}),
                &["id"],
            ),
        },
        ToolDef {
            name: "set_color",
            description: "Set the RGBA color of an entity.",
            input_schema: schema(json!({"id": {"type": "integer"}, "color": color(), "expected_revision": expected_rev()}), &["id", "color"]),
        },
        ToolDef {
            name: "remove_entity",
            description: "Remove an entity by id.",
            input_schema: schema(json!({"id": {"type": "integer"}, "expected_revision": expected_rev()}), &["id"]),
        },
        ToolDef {
            name: "set_camera",
            description: "Set camera position / target / fov.",
            input_schema: schema(
                json!({"position": vec3(), "target": vec3(), "fov_deg": {"type": "number"}, "expected_revision": expected_rev()}),
                &[],
            ),
        },
        ToolDef {
            name: "get_screenshot",
            description: "Render the scene headless and return a base64 PNG image.",
            input_schema: schema(
                json!({"width": {"type": "integer"}, "height": {"type": "integer"}}),
                &[],
            ),
        },
        ToolDef {
            name: "save_scene",
            description: "Persist the current scene to a .json file.",
            input_schema: schema(json!({"path": {"type": "string"}}), &["path"]),
        },
        ToolDef {
            name: "load_scene",
            description: "Load a scene from a .json file, replacing the current state.",
            input_schema: schema(json!({"path": {"type": "string"}, "expected_revision": expected_rev()}), &["path"]),
        },
        ToolDef {
            name: "get_revision",
            description: "Return the current world revision — pass it back as expected_revision on mutations for optimistic concurrency between agents.",
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "rollback",
            description: "Restore the scene to an earlier revision (per-revision snapshot ring, last 64 mutations). The restore bumps the revision again.",
            input_schema: schema(json!({"revision": {"type": "integer"}}), &["revision"]),
        },
        ToolDef {
            name: "apply_batch",
            description: "Apply several mutating commands atomically in one call — the engine's 'tick'. Commands execute in array order; on error the batch stops (earlier commands stay applied). Each entry: {\"tool\": \"<name>\", \"arguments\": {...}}.",
            input_schema: schema(
                json!({
                    "commands": {
                        "type": "array",
                        "items": {"type": "object", "properties": {
                            "tool": {"type": "string"},
                            "arguments": {"type": "object"}
                        }, "required": ["tool"]},
                    }
                }),
                &["commands"],
            ),
        },
        ToolDef {
            name: "list_assets",
            description: "List files in the assets/ directory.",
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "run_blender_script",
            description: "Run a Python script via headless Blender (blender --background --factory-startup --python).",
            input_schema: schema(json!({"script": {"type": "string"}}), &["script"]),
        },
    ]
}

/// MCP tools/call result helpers.
pub fn text_result(text: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": text.into()}]})
}

pub fn image_result(base64_png: impl Into<String>) -> Value {
    json!({"content": [{"type": "image", "data": base64_png.into(), "mimeType": "image/png"}]})
}

pub fn error_result(message: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": message.into()}], "isError": true})
}
