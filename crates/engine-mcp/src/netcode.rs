use serde_json::{json, Value};

pub(crate) fn tool() -> Value {
    json!({
        "name":"net_simulate",
        "description":"Run an isolated 60 Hz authoritative server and 1-8 predicted clients over a seeded virtual network. Uses the authoring scene (or scene JSON/project-relative path), net_player slots in join order, per-client input event lists indexed from zero. Returns stats and final positions compared at the same server tick; null deltas mean not connected or no matching history. Does not change authoring or play. / Isolierter Multiplayer-Netztest.",
        "inputSchema":{
            "type":"object","required":["clients","ticks"],
            "properties":{
                "clients":{"type":"integer","minimum":1,"maximum":8},
                "ticks":{"type":"integer","minimum":0,"maximum":36000},
                "rtt_ms":{"type":"number","minimum":0,"maximum":5000,"default":120},
                "jitter_ms":{"type":"number","minimum":0,"maximum":5000,"default":20},
                "loss":{"type":"number","minimum":0,"maximum":1,"default":0.05},
                "seed":{"type":"integer","minimum":0,"default":1},
                "scene":{"oneOf":[{"type":"object"},{"type":"string"}]},
                "inputs":{"type":"array","maxItems":8,"items":{"type":"array","maxItems":4096,"items":{
                    "type":"object","required":["tick","action","value"],"additionalProperties":false,
                    "properties":{"tick":{"type":"integer","minimum":0},"action":{"type":"string","minLength":1,"maxLength":128},"value":{"type":"number"}}
                }}},
                "agent":{"type":"string"}
            },
            "additionalProperties":false
        }
    })
}
