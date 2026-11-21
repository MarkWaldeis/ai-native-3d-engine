use serde_json::{json, Value};

pub fn tools() -> Vec<Value> {
    [
        ("animation_clips", "Inspect names/durations of clips in a project-confined GLB. Supply id or path.", vec![]),
        ("animator_validate", "Lint an entity's proposed machine against actual asset clips: unknown clips/parameters/states and unreachable states are errors. Does not mutate.", vec!["id"]),
        ("animator_set", "Set an animator machine from data using actual GLB clips; resets current state/time. Honors entity locks, CAS, atomic batches and persistence. Parameters are finite numbers; Rhai bool maps to 0/1.", vec!["id","machine"]),
    ].into_iter().map(|(name,description,required)| json!({"name":name,"description":description,"inputSchema":{"type":"object","required":required,"properties":{
        "id":{"type":"integer","minimum":0},"path":{"type":"string"},"machine":{"type":"object"},"speed":{"type":"number","minimum":0,"maximum":100},"blend_time":{"type":"number","minimum":0,"maximum":60},"expected_revision":{"type":"integer"},"agent":{"type":"string"}
    }}})).collect()
}
