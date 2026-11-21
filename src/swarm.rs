use crate::Engine;
use serde_json::{json, Value};
use shared::mcp_protocol::text_result;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Default, Clone, PartialEq)]
pub(crate) struct Swarm {
    pub(crate) locks: BTreeMap<u64, (String, Instant)>,
    pub(crate) journal: VecDeque<Value>,
    pub(crate) tasks: Vec<Value>,
    pub(crate) task_leases: BTreeMap<u64, Instant>,
    pub(crate) task_journal: VecDeque<Value>,
    pub(crate) task_sequence: u64,
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
        let secured = self.secure_arguments(name, args)?;
        let args = &secured;
        if name.starts_with("blender_") && self.blender.needs_project_configuration() {
            let root = self.assets_dir.parent().unwrap_or(std::path::Path::new(".")).canonicalize().map_err(|e|e.to_string())?;
            let (identity,port) = crate::project_state::configuration(&root).map_err(|e|e.to_string())?;
            self.blender = asset_pipeline::bridge::BlenderBridge::for_project(port,identity);
        }
        if let Some(value) = args.get("expected_revision") {
            let revision = value.as_u64().ok_or("expected_revision must be a nonnegative integer")?;
            if revision != self.world.revision { return Err(format!("revision conflict: expected {revision}, world is at {}", self.world.revision)); }
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
            "task_create" | "task_claim" | "task_renew" | "task_release" | "task_complete" | "task_list" | "get_task_changes_since" => self.task_tool(name,args,agent)?,
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
        if self.simulation.is_some() && (name.starts_with("prefab_") || matches!(name, "spawn_primitive" | "set_transform" | "set_color" | "remove_entity" | "set_camera" | "load_scene" | "rollback" | "blender_sync" | "blender_push_props" | "write_script")) {
            return Err("authoring edits are disabled during play; call stop before editing the scene".into());
        }
        if matches!(name,"animator_set"|"nav_bake") && self.simulation.is_some() { return Err("authoring edits are disabled during play; call stop before editing the scene".into()); }
        let global = matches!(name, "load_scene" | "rollback" | "nav_bake");
        let targeted = matches!(name, "set_transform" | "set_color" | "remove_entity" | "animator_set");
        for (id, (owner, _)) in &self.swarm.locks {
            if owner != agent && (global || (targeted && args["id"].as_u64() == Some(*id))) {
                return Err(format!("entity {id} locked by {owner}"));
            }
        }
        let before = self.world.revision;
        if name=="write_script" {
            let affected = self.script_affected_ids(args)?;
            self.check_entity_locks(&affected,agent)?;
            let references = self.script_reference_ids(args)?;
            let selected = ids(args,"ids")?;
            if references.len() > 1 || references.iter().any(|id|!selected.contains(id)) {
                for id in &references {
                    if !self.swarm.locks.get(id).is_some_and(|(owner,expiry)|owner == agent && *expiry > Instant::now()) { return Err(format!("shared script requires a claim on entity {id} by {agent}")); }
                }
            }
        }
        let mut affected: Vec<u64> = if global {
            self.world.scene.entities.iter().map(|e| e.id).collect()
        } else if targeted {
            vec![integer(args, "id")?]
        } else {
            vec![]
        };
        if name=="write_script" { affected=self.script_affected_ids(args)?; }
        let prefab_edit=targeted && name != "animator_set" && self.world.entity(args["id"].as_u64().unwrap_or(0)).is_some_and(|e|e.props.get("_prefab").is_some());
        let result = if name.starts_with("prefab_") {self.prefab_tool(name,args,agent)?}else if prefab_edit {self.prefab_edit(name,args,agent)?}else if name == "blender_sync" { self.sync_blender(args, agent)? } else if name == "blender_push_props" { self.push_blender_props(args,agent)? } else { self.handle_tool(name, args)? };
        if self.world.revision > before && name != "apply_batch" {
            if name == "blender_sync" || name == "animator_set" || name.starts_with("prefab_") || prefab_edit {
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
