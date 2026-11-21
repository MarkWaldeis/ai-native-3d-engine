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
                let mut entity = Entity {
                    id: existing.first().map(|e| e.id).unwrap_or(0),
                    name: item["name"].as_str().unwrap_or("Blender object").into(),
                    mesh: PrimitiveKind::Gltf { path: path.into() },
                    transform,
                    color: [1.0;4],
                    visible: existing.first().map(|e| e.visible).unwrap_or(true),
                    source_uid: Some(uid.into()),
                    props: existing.first().map(|e|e.props.clone()).unwrap_or(json!({})),
                    tags: existing.first().map(|e|e.tags.clone()).unwrap_or_default(),
                    collider: existing.first().and_then(|e|e.collider.clone()),
                    rigidbody: existing.first().and_then(|e|e.rigidbody.clone()),
                    script: existing.first().and_then(|e|e.script.clone()),
                };
                merge_gameplay(&mut entity, &item["props"], self.assets_dir.parent().unwrap_or(std::path::Path::new(".")))?;
                entities.push(entity);
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

    pub(crate) fn push_blender_props(&mut self, args: &Value, agent: &str) -> Result<Value,String> {
        let ids: Vec<u64> = serde_json::from_value(args["ids"].clone()).map_err(|e|e.to_string())?;
        if ids.is_empty() { return Err("ids must not be empty".into()); }
        self.check_entity_locks(&ids, agent)?;
        let mut objects = Vec::new();
        let mut seen = HashSet::new();
        for id in ids {
            let entity = self.world.entity(id).ok_or_else(||format!("no entity {id}"))?;
            let uid = entity.source_uid.as_ref().ok_or("entity is not linked to Blender")?;
            if !seen.insert(uid.clone()) { return Err(format!("duplicate ai_uid {uid}")); }
            let mut props = entity.props.as_object().ok_or("entity props must be an object")?.iter().filter(|(k,_)|k.starts_with("ai_") && k.as_str() != "ai_uid" && k.as_str() != "ai_deleted_props").map(|(k,v)|(k.clone(),v.clone())).collect::<serde_json::Map<_,_>>();
            for (key,value) in gameplay(entity).as_object().unwrap() { props.insert(key.clone(),value.clone()); }
            objects.push(json!({"ai_uid":uid,"props":props,"expected":raw_baseline(entity)}));
        }
        let result = self.blender.request(json!({"command":"push_props","objects":objects})).map_err(|e|e.to_string())?;
        Ok(text_result(result.to_string()))
    }
}

fn gameplay(entity: &Entity) -> Value {
    json!({"ai_tags":entity.tags,"ai_collider":entity.collider,"ai_rigidbody":entity.rigidbody,"ai_script":entity.script})
}

fn raw_baseline(entity: &Entity) -> Value {
    entity.props.get("_blender_props").cloned().unwrap_or_else(|| {
        json!(entity.props.as_object().into_iter().flat_map(|props|props.iter()).filter(|(key,_)|key.starts_with("ai_") && key.as_str() != "ai_uid" && key.as_str() != "ai_deleted_props").map(|(key,value)|(key.clone(),value.clone())).collect::<serde_json::Map<_,_>>())
    })
}

fn merge_gameplay(entity: &mut Entity, incoming: &Value, root: &std::path::Path) -> anyhow::Result<()> {
    let mut incoming = incoming.as_object().ok_or_else(||anyhow::anyhow!("Blender props must be an object"))?.clone();
    if let Some(deleted) = incoming.remove("ai_deleted_props") {
        let deleted: Vec<String> = serde_json::from_value(decode_prop(&deleted))?;
        for key in deleted {
            anyhow::ensure!(key.starts_with("ai_") && key != "ai_uid" && key != "ai_deleted_props", "invalid deleted gameplay prop {key}");
            incoming.insert(key,Value::Null);
        }
    }
    let mut previous = raw_baseline(entity);
    let mut baseline = entity.props.get("_blender_gameplay").cloned().unwrap_or(json!({}));
    for (key,raw) in incoming {
        if key == "ai_uid" { continue; }
        if previous.get(&key) == Some(&raw) { continue; }
        let value = decode_prop(&raw);
        let current = gameplay(entity);
        if let Some(current) = current.get(&key) {
            let normalized = match key.as_str() {
                "ai_tags" => json!(blender_tags(&json!({"ai_tags":raw}))?),
                "ai_script" => {
                    let script: Option<shared::components::Script> = serde_json::from_value(value.clone())?;
                    json!(script.map(|script|engine_core::freeze_script(&script,root)).transpose()?)
                }
                "ai_collider" => json!(component_prop(&json!({"ai_collider":raw}),"ai_collider","shape")?.map(serde_json::from_value::<shared::components::Collider>).transpose()?),
                "ai_rigidbody" => json!(component_prop(&json!({"ai_rigidbody":raw}),"ai_rigidbody","type")?.map(serde_json::from_value::<shared::components::RigidBody>).transpose()?),
                _ => unreachable!(),
            };
            let default = if key == "ai_tags" { json!([]) } else { Value::Null };
            let old = baseline.get(&key).unwrap_or(&default);
            anyhow::ensure!(current == old || current == &normalized, "Blender gameplay conflict on {} {key}; use blender_push_props or resolve the field explicitly", entity.name);
            match key.as_str() {
                "ai_tags" => entity.tags = serde_json::from_value(normalized.clone())?,
                "ai_script" => entity.script = serde_json::from_value(normalized.clone())?,
                "ai_collider" => entity.collider = serde_json::from_value(normalized.clone())?,
                "ai_rigidbody" => entity.rigidbody = serde_json::from_value(normalized.clone())?,
                _ => unreachable!(),
            }
            baseline[&key] = normalized;
        } else {
            anyhow::ensure!(entity.props.get(&key).is_none() || entity.props.get(&key) == previous.get(&key) || entity.props.get(&key) == Some(&raw), "Blender gameplay conflict on {} {key}",entity.name);
        }
        if value.is_null() { entity.props.as_object_mut().unwrap().remove(&key); }
        else { entity.props[&key] = raw.clone(); }
        previous[&key] = raw;
    }
    entity.props["_blender_props"] = previous;
    entity.props["_blender_gameplay"] = baseline;
    Ok(())
}

fn decode_prop(value: &Value) -> Value {
    value.as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_else(|| value.clone())
}

fn component_prop(props: &Value, key: &str, discriminator: &str) -> anyhow::Result<Option<Value>> {
    Ok(props.get(key).filter(|v| !decode_prop(v).is_null()).map(|v| {
        let value = decode_prop(v);
        if let Some(kind) = value.as_str() { json!({(discriminator):kind}) } else { value }
    }))
}

fn blender_tags(props: &Value) -> anyhow::Result<Vec<String>> {
    match props.get("ai_tags") {
        None | Some(Value::Null) => Ok(vec![]),
        Some(value) => {
            let decoded = decode_prop(value);
            if decoded.is_null() { return Ok(vec![]); }
            if let Some(text) = decoded.as_str() { Ok(text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()) }
            else { Ok(serde_json::from_value(decoded)?) }
        }
    }
}
