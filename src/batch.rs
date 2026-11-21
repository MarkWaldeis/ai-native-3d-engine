use crate::Engine;
use serde_json::{json, Value};
use shared::mcp_protocol::text_result;

impl Engine {
    pub(crate) fn atomic_batch(&mut self, args: &Value) -> Result<Value, String> {
        let commands = args["commands"]
            .as_array()
            .ok_or("missing array arg 'commands'")?;
        for command in commands {
            let tool = command["tool"]
                .as_str()
                .ok_or("batch command requires tool")?;
            if !matches!(
                tool,
                "spawn_primitive" | "set_transform" | "set_color" | "remove_entity" | "set_camera"
            ) {
                return Err(format!(
                    "tool {tool} is not allowed in an atomic scene batch"
                ));
            }
            if command
                .get("arguments")
                .is_some_and(|v| !v.is_object() || v.get("expected_revision").is_some())
            {
                return Err("atomic command arguments must be an object without expected_revision; use the batch revision".into());
            }
        }
        if commands.is_empty() {
            return Ok(text_result(
                json!({"mode":"atomic","applied":0,"revision":self.world.revision,"ids":[]})
                    .to_string(),
            ));
        }
        let original_world = self.world.clone();
        let original_swarm = self.swarm.clone();
        let mut ids = Vec::new();
        for command in commands {
            let mut arguments = command.get("arguments").cloned().unwrap_or(json!({}));
            arguments["agent"] = args.get("agent").cloned().unwrap_or(json!("anonymous"));
            if let Err(error) = self.handle_agent(command["tool"].as_str().unwrap(), &arguments) {
                self.world = original_world;
                self.swarm = original_swarm;
                return Err(error);
            }
            if let Some(entry) = self.swarm.journal.back() {
                ids.extend(
                    entry["ids"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_u64),
                );
            }
        }
        ids.sort_unstable();
        ids.dedup();
        let candidate = std::mem::replace(&mut self.world, original_world);
        self.swarm = original_swarm;
        self.world.commit_atomic(candidate);
        self.swarm.journal.push_back(json!({"revision":self.world.revision,"agent":args.get("agent").cloned().unwrap_or(json!("anonymous")),"tool":"apply_batch","ids":ids}));
        while self.swarm.journal.len() > 1024 {
            self.swarm.journal.pop_front();
        }
        Ok(text_result(json!({"mode":"atomic","applied":commands.len(),"revision":self.world.revision,"ids":ids}).to_string()))
    }
}
