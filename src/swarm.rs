use crate::Engine;
use serde_json::{json, Value};
use shared::mcp_protocol::text_result;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Swarm {
    locks: BTreeMap<u64, (String, Instant)>,
    journal: VecDeque<Value>,
    tasks: Vec<Value>,
}

fn integer(args: &Value, key: &str) -> Result<u64, String> {
    args[key]
        .as_u64()
        .ok_or_else(|| format!("'{key}' must be a nonnegative integer"))
}

fn ids(args: &Value, key: &str) -> Result<Vec<u64>, String> {
    args[key]
        .as_array()
        .ok_or_else(|| format!("'{key}' must be an array of entity/task ids"))?
        .iter()
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| format!("'{key}' must contain nonnegative integers"))
        })
        .collect()
}

impl Engine {
    pub(crate) fn check_entity_locks(&self, ids: &[u64], agent: &str) -> Result<(), String> {
        for id in ids {
            if let Some((owner, expiry)) = self.swarm.locks.get(id) {
                if owner != agent && *expiry > Instant::now() { return Err(format!("entity {id} locked by {owner}")); }
            }
        }
        Ok(())
    }

    pub(crate) fn handle_agent(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        if !args.is_object() {
            return Err("tool arguments must be a JSON object".into());
        }
        let agent = match args.get("agent") {
            None => "anonymous",
            Some(v) => v
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("'agent' must be a nonempty string")?,
        };
        self.swarm
            .locks
            .retain(|_, (_, expiry)| *expiry > Instant::now());
        let result = match name {
            "claim_entities" | "release_entities" => {
                let ids = ids(args, "ids")?;
                for id in &ids {
                    if let Some((owner, _)) = self.swarm.locks.get(id) {
                        if owner != agent { return Err(format!("entity {id} locked by {owner}")); }
                    }
                    if name == "claim_entities" && self.world.entity(*id).is_none() {
                        return Err(format!("no entity {id}"));
                    }
                }
                let ttl = args.get("ttl_s").map(|_| integer(args, "ttl_s")).transpose()?.unwrap_or(60);
                if ttl == 0 || ttl > 86400 { return Err("ttl_s must be in 1..=86400".into()); }
                for id in ids {
                    if name == "claim_entities" {
                        self.swarm.locks.insert(id, (agent.into(), Instant::now() + Duration::from_secs(ttl)));
                    } else { self.swarm.locks.remove(&id); }
                }
                json!({"ok": true})
            }
            "list_locks" => json!(self.swarm.locks.iter().map(|(id, (owner, expiry))|
                json!({"id": id, "agent": owner, "ttl_s": expiry.saturating_duration_since(Instant::now()).as_secs_f64()})).collect::<Vec<_>>()),
            "get_changes_since" => {
                let revision = integer(args, "revision")?;
                json!({"entries": self.swarm.journal.iter().filter(|e| e["revision"].as_u64().unwrap() > revision).collect::<Vec<_>>(),
                    "current_revision": self.world.revision,
                    "oldest_revision": self.swarm.journal.front().map(|e| &e["revision"])})
            }
            "task_create" => {
                let title = args["title"].as_str().filter(|s| !s.trim().is_empty()).ok_or("'title' must be a nonempty string")?;
                let deps = if args.get("depends_on").is_some() { ids(args, "depends_on")? } else { vec![] };
                for dep in &deps {
                    if !self.swarm.tasks.iter().any(|t| t["id"] == *dep) { return Err(format!("no task {dep}; use task_list")); }
                }
                if args.get("description").is_some_and(|v| !v.is_string()) { return Err("'description' must be a string".into()); }
                let task = json!({"id": self.swarm.tasks.len() + 1, "title": title,
                    "description": args.get("description").cloned().unwrap_or(json!("")),
                    "depends_on": deps, "status": "pending", "agent": null});
                self.swarm.tasks.push(task.clone());
                task
            }
            "task_claim" | "task_complete" => {
                let id = integer(args, "id")?;
                let index = self.swarm.tasks.iter().position(|t| t["id"] == id).ok_or_else(|| format!("no task {id}; use task_list"))?;
                let task = &self.swarm.tasks[index];
                if name == "task_claim" {
                    if task["status"] != "pending" { return Err(format!("task {id} is {}; only pending tasks can be claimed", task["status"])); }
                    for dep in task["depends_on"].as_array().unwrap() {
                        if !self.swarm.tasks.iter().any(|t| t["id"] == *dep && t["status"] == "completed") {
                            return Err(format!("dependency {dep} is not completed"));
                        }
                    }
                    self.swarm.tasks[index]["agent"] = json!(agent);
                    self.swarm.tasks[index]["status"] = json!("claimed");
                } else {
                    if task["status"] != "claimed" || task["agent"] != agent { return Err(format!("task {id} must be claimed by {agent} before completion")); }
                    if args.get("note").is_some_and(|v| !v.is_string()) { return Err("'note' must be a string".into()); }
                    self.swarm.tasks[index]["status"] = json!("completed");
                    self.swarm.tasks[index]["note"] = args.get("note").cloned().unwrap_or(json!(""));
                }
                self.swarm.tasks[index].clone()
            }
            "task_list" => {
                if let Some(status) = args.get("status") {
                    if !["pending", "claimed", "completed"].iter().any(|s| status == s) {
                        return Err("status must be pending, claimed, or completed".into());
                    }
                }
                json!(self.swarm.tasks.iter().filter(|t| args.get("status").is_none_or(|s| t["status"] == *s)).collect::<Vec<_>>())
            }
            _ => return self.handle_scene_tool(name, args, agent),
        };
        Ok(text_result(result.to_string()))
    }

    fn handle_scene_tool(
        &mut self,
        name: &str,
        args: &Value,
        agent: &str,
    ) -> Result<Value, String> {
        if self.simulation.is_some() && matches!(name, "spawn_primitive" | "set_transform" | "set_color" | "remove_entity" | "set_camera" | "load_scene" | "rollback" | "blender_sync") {
            return Err("authoring edits are disabled during play; call stop before editing the scene".into());
        }
        let global = matches!(name, "load_scene" | "rollback");
        let targeted = matches!(name, "set_transform" | "set_color" | "remove_entity");
        for (id, (owner, _)) in &self.swarm.locks {
            if owner != agent && (global || (targeted && args["id"].as_u64() == Some(*id))) {
                return Err(format!("entity {id} locked by {owner}"));
            }
        }
        let before = self.world.revision;
        let mut affected: Vec<u64> = if global {
            self.world.scene.entities.iter().map(|e| e.id).collect()
        } else if targeted {
            vec![integer(args, "id")?]
        } else {
            vec![]
        };
        let result = if name == "blender_sync" { self.sync_blender(args, agent)? } else { self.handle_tool(name, args)? };
        if self.world.revision > before && name != "apply_batch" {
            if name == "blender_sync" {
                let summary: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).map_err(|e| e.to_string())?;
                affected = summary["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
            }
            if global {
                affected.extend(self.world.scene.entities.iter().map(|e| e.id));
            }
            if name == "spawn_primitive" {
                if let Some(id) = self.world.scene.entities.last().map(|e| e.id) {
                    affected.push(id);
                }
            }
            affected.sort_unstable();
            affected.dedup();
            self.swarm.journal.push_back(json!({"revision": self.world.revision, "agent": agent, "tool": name, "ids": affected}));
            while self.swarm.journal.len() > 1024 {
                self.swarm.journal.pop_front();
            }
        }
        Ok(result)
    }
}
