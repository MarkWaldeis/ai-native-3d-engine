use serde_json::{json, Value};

pub const NAMES: &[&str] = &["save_game", "load_game"];

pub fn tools() -> Vec<Value> {
    vec![
        json!({"name":"save_game","description":"Save active play state as a versioned base-scene delta; optional project-confined .json path.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"path":{"type":"string"}}}}),
        json!({"name":"load_game","description":"Start play from current authoring scene and a save, without on_start; calls optional on_load. Stop existing play first. Reports base_changed for updated scenes.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"path":{"type":"string"},"save":{"type":"object"}},"oneOf":[{"required":["path"],"not":{"required":["save"]}},{"required":["save"],"not":{"required":["path"]}}]}}),
    ]
}
