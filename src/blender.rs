use crate::Engine;
use engine_core::Command;
use serde_json::{json, Value};
use shared::mcp_protocol::text_result;
use shared::scene::{Entity, PrimitiveKind, Transform};
use std::collections::HashSet;

impl Engine {
    pub(crate) fn sync_blender(&mut self, args: &Value, agent: &str) -> Result<Value, String> {
        let sync = || -> anyhow::Result<(Vec<Entity>, Vec<u64>)> {
            let objects = match args.get("objects") {
                None => None,
                Some(value) => {
                    let objects = value.as_array().ok_or_else(|| {
                        anyhow::anyhow!("objects must be an array of names or ai_uid strings")
                    })?;
                    if objects.iter().any(|v| !v.is_string()) {
                        anyhow::bail!("objects must contain strings; use blender_scene_info");
                    }
                    Some(objects)
                }
            };
            let prune = match args.get("prune") {
                None => false,
                Some(v) => v
                    .as_bool()
                    .ok_or_else(|| anyhow::anyhow!("prune must be boolean"))?,
            };
            if let Some(expected) = crate::expected_rev(args) {
                if expected != self.world.revision {
                    anyhow::bail!("revision conflict: expected {expected}, world is at {}; re-read scene state", self.world.revision);
                }
            }
            let directory = self.assets_dir.join("blender");
            std::fs::create_dir_all(&directory)?;
            let directory = directory.canonicalize()?;
            let mut request = json!({"command":"export", "dir":directory});
            if let Some(objects) = objects {
                request["objects"] = json!(objects);
            }
            let manifest = self.blender.request(request)?;
            let items = manifest["objects"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("bridge export missing objects array"))?;
            let mut seen = HashSet::new();
            let mut entities = Vec::new();
            for item in items {
                let uid = item["ai_uid"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("export missing ai_uid"))?;
                if !seen.insert(uid) {
                    anyhow::bail!("duplicate ai_uid {uid} in Blender export");
                }
                let existing: Vec<_> = self
                    .world
                    .scene
                    .entities
                    .iter()
                    .filter(|e| e.source_uid.as_deref() == Some(uid))
                    .collect();
                if existing.len() > 1 {
                    anyhow::bail!("scene has duplicate source_uid {uid}; remove duplicate entities before syncing");
                }
                let path = item["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("export missing GLB path"))?;
                engine_geo::load_gltf(std::path::Path::new(path))?;
                let transform: Transform = serde_json::from_value(item["transform"].clone())?;
                entities.push(Entity {
                    id: existing.first().map(|e| e.id).unwrap_or(0),
                    name: item["name"].as_str().unwrap_or("Blender object").into(),
                    mesh: PrimitiveKind::Gltf { path: path.into() },
                    transform,
                    color: serde_json::from_value(item["color"].clone())?,
                    visible: existing.first().map(|e| e.visible).unwrap_or(true),
                    source_uid: Some(uid.into()),
                    props: item["props"].clone(),
                    tags: blender_tags(&item["props"])? ,
                    collider: component_prop(&item["props"], "ai_collider", "shape")?.map(serde_json::from_value).transpose()?,
                    rigidbody: component_prop(&item["props"], "ai_rigidbody", "type")?.map(serde_json::from_value).transpose()?,
                    script: item["props"].get("ai_script").map(|v| serde_json::from_value(decode_prop(v))).transpose()?,
                });
            }
            let all = manifest["all_mesh_uids"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("bridge export missing all_mesh_uids"))?;
            let remove = if prune {
                self.world
                    .scene
                    .entities
                    .iter()
                    .filter(|e| {
                        e.source_uid
                            .as_ref()
                            .is_some_and(|uid| !all.iter().any(|v| v.as_str() == Some(uid)))
                    })
                    .map(|e| e.id)
                    .collect()
            } else {
                vec![]
            };
            Ok((entities, remove))
        };
        let (entities, remove) = sync().map_err(|e| e.to_string())?;
        let ids: Vec<_> = entities
            .iter()
            .filter_map(|e| (e.id != 0).then_some(e.id))
            .chain(remove.iter().copied())
            .collect();
        self.check_entity_locks(&ids, agent)?;
        let value = self
            .apply_cmd(
                Command::SyncBlender { entities, remove },
                crate::expected_rev(args),
            )
            .map_err(|e| e.to_string())?;
        Ok(text_result(value.to_string()))
    }
}

fn decode_prop(value: &Value) -> Value {
    value.as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_else(|| value.clone())
}

fn component_prop(props: &Value, key: &str, discriminator: &str) -> anyhow::Result<Option<Value>> {
    Ok(props.get(key).filter(|v| !v.is_null()).map(|v| {
        let value = decode_prop(v);
        if let Some(kind) = value.as_str() { json!({(discriminator):kind}) } else { value }
    }))
}

fn blender_tags(props: &Value) -> anyhow::Result<Vec<String>> {
    match props.get("ai_tags") {
        None => Ok(vec![]),
        Some(value) => {
            let decoded = decode_prop(value);
            if let Some(text) = decoded.as_str() { Ok(text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()) }
            else { Ok(serde_json::from_value(decoded)?) }
        }
    }
}
