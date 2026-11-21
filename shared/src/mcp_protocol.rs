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

fn schema(mut props: Value, required: &[&str]) -> Value {
    props["agent"] = json!({"type": "string", "minLength": 1, "description": "Agent identity; overrides HTTP X-Agent-Id; defaults to anonymous"});
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
        ToolDef { name: "play", description: "Start a separate simulation from an authoring snapshot. Hooks on_start run at tick 0; authoring edits are disabled until stop. One shared play session per Engine.", input_schema: schema(json!({}), &[]) },
        ToolDef {
            name: "step", description: "Advance the active simulation at fixed 60 Hz. Inputs are zero-based absolute tick events, applied before that tick's hooks and held until changed. No authoring revisions are changed.",
            input_schema: schema(json!({"ticks":{"type":"integer", "minimum":0, "maximum":36000}, "inputs":{"type":"array", "maxItems":4096, "items":{"type":"object", "properties":{"tick":{"type":"integer", "minimum":0}, "action":{"type":"string"}, "value":{"type":"number"}}, "required":["tick","action","value"]}}}), &["ticks"]),
        },
        ToolDef { name: "stop", description: "Discard the play session and return to unchanged authoring state. Idempotent.", input_schema: schema(json!({}), &[]) },
        ToolDef { name: "get_sim_state", description: "Read active simulation tick, transforms, tags, global state map, bounded logs and failed flag.", input_schema: schema(json!({}), &[]) },
        ToolDef {
            name: "simulate", description: "Run a fresh isolated authoring snapshot. Assertions: entity+position_near+tolerance?, entity+y_greater_than, state+equals, or entities_with_tag+count. Optional at_tick N checks after N steps (0 = after on_start); otherwise check final tick. Returns passed/actual per assertion. screenshot appends a final PNG image content block. Does not replace active play state.",
            input_schema: schema(json!({"ticks":{"type":"integer", "minimum":0, "maximum":36000}, "inputs":{"type":"array", "maxItems":4096, "items":{"type":"object", "properties":{"tick":{"type":"integer", "minimum":0}, "action":{"type":"string"}, "value":{"type":"number"}}, "required":["tick","action","value"]}}, "asserts":{"type":"array", "maxItems":1024, "items":{"type":"object", "properties":{"entity":{"type":"string"}, "position_near":{"type":"array", "items":{"type":"number"}, "minItems":3,"maxItems":3}, "tolerance":{"type":"number", "minimum":0}, "y_greater_than":{"type":"number"}, "state":{"type":"string"}, "equals":{}, "entities_with_tag":{"type":"string"}, "count":{"type":"integer","minimum":0}, "at_tick":{"type":"integer","minimum":0}}}}, "screenshot":{"type":"boolean", "default":false}}), &["ticks"]),
        },
        ToolDef {
            name: "query", description: "Compact entity query over active simulation, or authoring scene when stopped. Filters combine with AND; name is exact, near is a sphere in world coordinates.",
            input_schema: schema(json!({"tag":{"type":"string"}, "name":{"type":"string"}, "near":{"type":"object", "properties":{"point":{"type":"array", "items":{"type":"number"}, "minItems":3,"maxItems":3}, "radius":{"type":"number", "minimum":0}}, "required":["point","radius"]}}), &[]),
        },
        ToolDef {
            name: "blender_status", description: "Check local Blender bridge connection/version; if disconnected, returns startup instructions.",
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "blender_start", description: "Connect to an existing local bridge or launch headless Blender and wait for ping. Engine-owned Blender stops when the engine exits.",
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "blender_exec", description: "Execute trusted Python on Blender's main thread. Returns captured stdout and optional JSON-serializable result variable. Local machine access: trusted agents only.",
            input_schema: schema(json!({"code":{"type":"string"}}), &["code"]),
        },
        ToolDef {
            name: "blender_scene_info", description: "List Blender objects, stable ai_uid, local Blender Z-up transforms (rotation radians), world_transform and ai_* custom props.",
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "blender_sync", description: "Export Blender meshes to assets/blender and atomically upsert by source_uid, preserving entity ids. Converts Blender Z-up to engine Y-up. Optional objects filters by name or ai_uid. prune removes only previously synced objects absent from Blender, even when filtered. Respects entity locks and expected_revision; journals one scene mutation.",
            input_schema: schema(json!({"objects":{"type":"array", "items":{"type":"string"}}, "prune":{"type":"boolean", "default":false}, "expected_revision":expected_rev()}), &[]),
        },
        ToolDef {
            name: "claim_entities",
            description: "Lock existing entity ids for this agent; fails atomically if any belongs to another agent. Renew by claiming again. TTL defaults to 60 seconds.",
            input_schema: schema(json!({"ids": {"type":"array", "items":{"type":"integer", "minimum":0}}, "ttl_s":{"type":"integer", "minimum":1, "maximum":86400}}), &["ids"]),
        },
        ToolDef {
            name: "release_entities",
            description: "Release this agent's locks. Cannot release another agent's locks.",
            input_schema: schema(json!({"ids":{"type":"array", "items":{"type":"integer", "minimum":0}}}), &["ids"]),
        },
        ToolDef { name: "list_locks", description: "List unexpired locks with owner and remaining TTL.", input_schema: schema(json!({}), &[]) },
        ToolDef {
            name: "get_changes_since",
            description: "Journal entries after revision, with agent, tool and affected entity ids. Retains 1024 entries; oldest_revision indicates retention floor. Scene mutations only; task/lock metadata does not change scene revision.",
            input_schema: schema(json!({"revision":{"type":"integer", "minimum":0}}), &["revision"]),
        },
        ToolDef {
            name: "task_create", description: "Create a pending task; dependencies must be existing task ids.",
            input_schema: schema(json!({"title":{"type":"string", "minLength":1}, "description":{"type":"string"}, "depends_on":{"type":"array", "items":{"type":"integer", "minimum":1}}}), &["title"]),
        },
        ToolDef {
            name: "task_claim", description: "Claim a pending task only after all dependencies are completed.",
            input_schema: schema(json!({"id":{"type":"integer", "minimum":1}}), &["id"]),
        },
        ToolDef {
            name: "task_complete", description: "Complete a task claimed by this agent.",
            input_schema: schema(json!({"id":{"type":"integer", "minimum":1}, "note":{"type":"string"}}), &["id"]),
        },
        ToolDef {
            name: "task_list", description: "List tasks, optionally filtered by status.",
            input_schema: schema(json!({"status":{"type":"string", "enum":["pending", "claimed", "completed"]}}), &[]),
        },
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
