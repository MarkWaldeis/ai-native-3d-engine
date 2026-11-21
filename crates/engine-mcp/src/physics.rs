use serde_json::{json, Value};

pub(crate) const NAMES: &[&str] = &["raycast", "shape_cast", "overlap"];

pub(crate) fn tools() -> Vec<Value> {
    let vec3 = json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}});
    NAMES.iter().map(|&name| {
        let (description, required) = match name {
            "raycast" => ("Cast a ray and return the nearest collider hit.", vec!["origin","direction","distance"]),
            "shape_cast" => ("Sweep a convex shape and return the nearest collider hit.", vec!["origin","direction","distance","shape"]),
            _ => ("Return up to 256 sorted, unique entity IDs intersecting a convex shape.", vec!["origin","shape"]),
        };
        json!({
            "name": name,
            "description": format!("{description} Uses live physics while playing; otherwise uses authoring colliders without running scripts."),
            "inputSchema": {"type":"object","required":required,"properties":{
                "origin":vec3, "rotation_deg":vec3, "direction":vec3,
                "distance":{"type":"number","exclusiveMinimum":0,"maximum":10000},
                "shape":{"type":"object","required":["shape"],"properties":{
                    "shape":{"enum":["box","sphere","capsule","convex_hull"]},
                    "half_extents":vec3,"radius":{"type":"number"},"half_height":{"type":"number"},
                    "vertices":{"type":"array","items":vec3}
                }},
                "filter":{"type":"object","properties":{
                    "exclude":{"type":"integer","minimum":0},"include_sensors":{"type":"boolean"},
                    "groups":{"type":"object","properties":{
                        "memberships":{"type":"integer","minimum":0,"maximum":4294967295u64},
                        "filter":{"type":"integer","minimum":0,"maximum":4294967295u64}
                    }}
                }}
            }}
        })
    }).collect()
}
