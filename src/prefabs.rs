use crate::Engine;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use shared::mcp_protocol::text_result;
use shared::scene::{Entity, Prefab, PrefabNode, Scene, Transform};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) fn merge(target: &mut Value, patch: &Value) {
    if let Some(fields) = patch.as_object() {
        if !target.is_object() {
            *target = json!({});
        }
        for (key, value) in fields {
            if value.is_null() {
                target.as_object_mut().unwrap().remove(key);
            } else {
                merge(&mut target[key], value);
            }
        }
    } else {
        *target = patch.clone();
    }
}

type Matrix = [[f64; 4]; 4];
fn matrix(t: Transform) -> Matrix {
    let [x, y, z] = t.rotation_deg.map(|v| (v as f64).to_radians());
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rotation = [
        [cy * cz, -cy * sz, sy],
        [cx * sz + sx * sy * cz, cx * cz - sx * sy * sz, -sx * cy],
        [sx * sz - cx * sy * cz, sx * cz + cx * sy * sz, cx * cy],
    ];
    let mut m = [[0.; 4]; 4];
    m[3][3] = 1.;
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = rotation[i][j] * t.scale[j] as f64;
        }
        m[i][3] = t.position[i] as f64;
    }
    m
}
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}
fn inverse(m: Matrix) -> Result<Matrix> {
    let mut a = m;
    let mut b = matrix(Transform::default());
    for col in 0..4 {
        let pivot = (col..4)
            .max_by(|i, j| a[*i][col].abs().total_cmp(&a[*j][col].abs()))
            .unwrap();
        ensure!(a[pivot][col].abs() > 1e-12, "singular prefab transform");
        a.swap(col, pivot);
        b.swap(col, pivot);
        let divisor = a[col][col];
        for j in 0..4 {
            a[col][j] /= divisor;
            b[col][j] /= divisor;
        }
        for i in 0..4 {
            if i != col {
                let f = a[i][col];
                for j in 0..4 {
                    a[i][j] -= f * a[col][j];
                    b[i][j] -= f * b[col][j];
                }
            }
        }
    }
    Ok(b)
}
fn decompose(m: Matrix) -> Result<Transform> {
    let mut scale: [f64; 3] =
        std::array::from_fn(|j| (0..3).map(|i| m[i][j] * m[i][j]).sum::<f64>().sqrt());
    ensure!(
        scale.iter().all(|s| s.is_finite() && *s > 1e-8),
        "prefab scale must be nonzero and finite"
    );
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det < 0. {
        scale[0] = -scale[0];
    }
    let r: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| m[i][j] / scale[j]));
    for a in 0..3 {
        for b in a + 1..3 {
            ensure!((0..3).map(|i|r[i][a]*r[i][b]).sum::<f64>().abs()<1e-5,"prefab hierarchy produces shear; bake it into geometry or use uniform parent scales");
        }
    }
    let y = r[0][2].clamp(-1., 1.).asin();
    let (x, z) = if y.cos().abs() > 1e-6 {
        ((-r[1][2]).atan2(r[2][2]), (-r[0][1]).atan2(r[0][0]))
    } else {
        (r[2][1].atan2(r[1][1]), 0.)
    };
    let result = Transform {
        position: std::array::from_fn(|i| m[i][3] as f32),
        rotation_deg: [x, y, z].map(|v| v.to_degrees() as f32),
        scale: scale.map(|v| v as f32),
    };
    ensure!(
        result
            .position
            .iter()
            .chain(&result.rotation_deg)
            .chain(&result.scale)
            .all(|v| v.is_finite()),
        "nonfinite prefab transform"
    );
    Ok(result)
}
fn compose(a: Transform, b: Transform) -> Result<Transform> {
    decompose(multiply(matrix(a), matrix(b)))
}

fn entity(key: &str, value: &Value, root: &Path) -> Result<Entity> {
    let allowed = [
        "name",
        "kind",
        "path",
        "size",
        "radius",
        "height",
        "segments",
        "width",
        "door_width",
        "door_height",
        "transform",
        "color",
        "visible",
        "tags",
        "props",
        "collider",
        "rigidbody",
        "script",
    ];
    for field in value
        .as_object()
        .context("entity/override must be an object")?
        .keys()
    {
        ensure!(
            allowed.contains(&field.as_str()),
            "unsupported prefab entity field {field}"
        );
    }
    ensure!(
        value["props"].get("_prefab").is_none(),
        "_prefab is reserved"
    );
    let mut base = json!({"id":0,"name":key,"kind":"box","size":[1,1,1]});
    merge(&mut base, value);
    let mut entity: Entity = serde_json::from_value(base)?;
    ensure!(entity.props.is_object(), "prefab props must be an object");
    if let Some(script) = &entity.script {
        entity.script = Some(engine_core::freeze_script(script, root)?);
    }
    let scene = Scene {
        entities: vec![entity.clone()],
        ..Scene::default()
    };
    engine_core::validate_scene_paths(&scene, root)?;
    decompose(matrix(entity.transform))?;
    Ok(entity)
}

pub(crate) fn ordered(prefab: &Prefab) -> Result<Vec<&PrefabNode>> {
    ensure!(
        !prefab.nodes.is_empty() && prefab.nodes.len() <= 256,
        "prefab needs 1..=256 nodes"
    );
    let keys: BTreeSet<_> = prefab.nodes.iter().map(|n| n.key.as_str()).collect();
    ensure!(
        keys.len() == prefab.nodes.len() && keys.iter().all(|s| !s.is_empty()),
        "prefab node keys must be unique and nonempty"
    );
    ensure!(
        prefab.nodes.iter().filter(|n| n.parent.is_none()).count() == 1,
        "prefab needs exactly one root"
    );
    ensure!(
        prefab
            .nodes
            .iter()
            .all(|n| n.parent.as_deref().is_none_or(|p| keys.contains(p))),
        "unknown prefab parent"
    );
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    while result.len() < prefab.nodes.len() {
        let before = result.len();
        for n in &prefab.nodes {
            if !seen.contains(n.key.as_str())
                && n.parent.as_deref().is_none_or(|p| seen.contains(p))
            {
                seen.insert(n.key.as_str());
                result.push(n);
            }
        }
        ensure!(result.len() > before, "cyclic prefab hierarchy");
    }
    Ok(result)
}

fn definition(args: &Value, root: &Path) -> Result<Prefab> {
    let mut nodes = Vec::new();
    for node in args["nodes"].as_array().context("nodes must be an array")? {
        ensure!(
            node.as_object()
                .context("node must be an object")?
                .keys()
                .all(|k| ["key", "parent", "entity"].contains(&k.as_str())),
            "unknown prefab node field"
        );
        let key = node["key"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("node key must be nonempty")?;
        let parent = match node.get("parent") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_str().context("parent must be a string")?.into()),
        };
        nodes.push(PrefabNode {
            key: key.into(),
            parent,
            entity: entity(key, &node["entity"], root)?,
        });
    }
    let prefab = Prefab { nodes };
    ordered(&prefab)?;
    Ok(prefab)
}

fn next(id: &mut u64) -> Result<u64> {
    let current = *id;
    *id = id.checked_add(1).context("entity ID space exhausted")?;
    Ok(current)
}

pub(crate) fn expand(
    scene: &mut Scene,
    name: &str,
    placement: Transform,
    overrides: &Value,
    existing: &[Entity],
    id: &mut u64,
    root: &Path,
) -> Result<Vec<u64>> {
    let prefab = scene.prefabs.get(name).context("unknown prefab")?;
    let result = build_instance(prefab, name, placement, overrides, existing, id, root)?;
    let ids = result.iter().map(|e| e.id).collect();
    let removed: BTreeSet<_> = existing.iter().map(|e| e.id).collect();
    if !removed.is_empty() {
        scene.entities.retain(|e| !removed.contains(&e.id));
    }
    scene.entities.extend(result);
    Ok(ids)
}

fn build_instance(
    prefab: &Prefab,
    name: &str,
    placement: Transform,
    overrides: &Value,
    existing: &[Entity],
    id: &mut u64,
    root: &Path,
) -> Result<Vec<Entity>> {
    let nodes = ordered(prefab)?;
    let patches = overrides
        .as_object()
        .context("overrides must map node keys to JSON merge patches")?;
    ensure!(
        patches
            .keys()
            .all(|key| nodes.iter().any(|n| &n.key == key)),
        "override refers to unknown node"
    );
    let mut transforms = BTreeMap::new();
    let mut result = Vec::new();
    let mut ids = Vec::new();
    let instance = match existing
        .iter()
        .find(|e| e.props["_prefab"]["node"] == nodes[0].key)
    {
        Some(e) => e.id,
        None => next(id)?,
    };
    for node in nodes {
        let mut value = serde_json::to_value(&node.entity)?;
        value.as_object_mut().unwrap().remove("id");
        value.as_object_mut().unwrap().remove("source_uid");
        if let Some(patch) = patches.get(&node.key) {
            entity(&node.key, patch, root)?;
            merge(&mut value, patch);
        }
        let mut materialized = entity(&node.key, &value, root)?;
        let parent = node
            .parent
            .as_ref()
            .map(|p| transforms[p])
            .unwrap_or(placement);
        materialized.transform = compose(parent, materialized.transform)?;
        transforms.insert(node.key.clone(), materialized.transform);
        materialized.id = match existing
            .iter()
            .find(|e| e.props["_prefab"]["node"] == node.key)
        {
            Some(e) => e.id,
            None => {
                if result.is_empty() {
                    instance
                } else {
                    next(id)?
                }
            }
        };
        materialized.source_uid = existing
            .iter()
            .find(|e| e.id == materialized.id)
            .and_then(|e| e.source_uid.clone());
        materialized.props["_prefab"] = json!({"name":name,"node":node.key,"instance":instance});
        if result.is_empty() {
            materialized.props["_prefab"]["placement"] = json!(placement);
            materialized.props["_prefab"]["overrides"] = overrides.clone();
        }
        ids.push(materialized.id);
        result.push(materialized);
    }
    Ok(result)
}

fn members(scene: &Scene, instance: u64) -> Vec<Entity> {
    scene
        .entities
        .iter()
        .filter(|e| e.props["_prefab"]["instance"].as_u64() == Some(instance))
        .cloned()
        .collect()
}
fn placements(args: &Value) -> Result<Vec<(Transform, Value)>> {
    let base: Transform =
        serde_json::from_value(args.get("transform").cloned().unwrap_or(json!({})))?;
    let common = args.get("overrides").cloned().unwrap_or(json!({}));
    ensure!(common.is_object(), "overrides must be an object");
    if let Some(instances) = args.get("instances") {
        ensure!(
            args.get("layout").is_none(),
            "choose instances or layout, not both"
        );
        let list = instances.as_array().context("instances must be an array")?;
        ensure!(list.len() <= 10000, "too many instances");
        return list
            .iter()
            .map(|v| {
                let t = serde_json::from_value(v.get("transform").cloned().unwrap_or(json!({})))?;
                let mut patch = common.clone();
                if let Some(p) = v.get("overrides") {
                    merge(&mut patch, p);
                }
                Ok((compose(base, t)?, patch))
            })
            .collect();
    }
    let layout = args
        .get("layout")
        .cloned()
        .unwrap_or(json!({"mode":"grid","counts":[1,1]}));
    let mode = layout["mode"]
        .as_str()
        .context("layout.mode must be grid, scatter or along_path")?;
    let vector = |v: &Value| -> Result<[f32; 3]> {
        let value: [f32; 3] = serde_json::from_value(v.clone())?;
        ensure!(value.iter().all(|n| n.is_finite()), "nonfinite vector");
        Ok(value)
    };
    let mut positions = Vec::new();
    match mode {
        "grid" => {
            let counts: [u32; 2] = serde_json::from_value(layout["counts"].clone())?;
            ensure!(
                u64::from(counts[0]) * u64::from(counts[1]) <= 10000,
                "too many grid instances"
            );
            let spacing: [f32; 2] =
                serde_json::from_value(layout.get("spacing").cloned().unwrap_or(json!([1, 1])))?;
            for z in 0..counts[1] {
                for x in 0..counts[0] {
                    positions.push(Transform::at(
                        x as f32 * spacing[0],
                        0.,
                        z as f32 * spacing[1],
                    ));
                }
            }
        }
        "scatter" => {
            let count = layout["count"]
                .as_u64()
                .filter(|n| *n <= 10000)
                .context("count must be in 0..=10000")?;
            let min = vector(&layout["min"])?;
            let max = vector(&layout["max"])?;
            ensure!((0..3).all(|i| min[i] <= max[i]), "invalid scatter bounds");
            let mut state = layout["seed"]
                .as_u64()
                .context("scatter requires an integer seed")?;
            let mut random = || {
                state = state.wrapping_add(0x9e3779b97f4a7c15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
                ((z ^ (z >> 31)) >> 40) as f32 / 16777216.
            };
            for _ in 0..count {
                positions.push(Transform {
                    position: std::array::from_fn(|i| min[i] + (max[i] - min[i]) * random()),
                    rotation_deg: [0., random() * 360., 0.],
                    ..Default::default()
                });
            }
        }
        "along_path" => {
            let points: Vec<[f32; 3]> = serde_json::from_value(layout["points"].clone())?;
            let count = layout["count"]
                .as_u64()
                .filter(|n| *n <= 10000)
                .context("count must be in 0..=10000")?;
            ensure!(
                points.len() >= 2 && points.iter().flatten().all(|n| n.is_finite()),
                "path needs at least two finite points"
            );
            let lengths: Vec<f32> = points
                .windows(2)
                .map(|p| {
                    (0..3)
                        .map(|i| (p[1][i] - p[0][i]).powi(2))
                        .sum::<f32>()
                        .sqrt()
                })
                .collect();
            let total: f32 = lengths.iter().sum();
            ensure!(
                total > 0. && total.is_finite(),
                "path has zero/nonfinite length"
            );
            for n in 0..count {
                let mut distance = total * n as f32 / (count.saturating_sub(1).max(1)) as f32;
                let mut segment = 0;
                while segment + 1 < lengths.len()
                    && (lengths[segment] == 0. || distance > lengths[segment])
                {
                    distance -= lengths[segment];
                    segment += 1;
                }
                let f = if lengths[segment] > 0. {
                    distance / lengths[segment]
                } else {
                    0.
                };
                positions.push(Transform::at(
                    points[segment][0] + (points[segment + 1][0] - points[segment][0]) * f,
                    points[segment][1] + (points[segment + 1][1] - points[segment][1]) * f,
                    points[segment][2] + (points[segment + 1][2] - points[segment][2]) * f,
                ));
            }
        }
        _ => bail!("unknown layout mode"),
    }
    positions
        .into_iter()
        .map(|p| Ok((compose(base, p)?, common.clone())))
        .collect()
}

impl Engine {
    pub(crate) fn import_blender_prefabs(
        &self,
        scene: &mut Scene,
        manifest: &Value,
        next_id: &mut u64,
    ) -> Result<Vec<u64>> {
        let root = self.assets_dir.parent().unwrap_or(Path::new("."));
        let mut changed = BTreeSet::new();
        for value in manifest["prefabs"].as_array().into_iter().flatten() {
            let name = value["name"]
                .as_str()
                .context("missing Blender prefab name")?;
            ensure!(name.starts_with("blender:"), "invalid Blender prefab name");
            let mut prefab: Prefab = serde_json::from_value(json!({"nodes":value["nodes"]}))?;
            for node in &mut prefab.nodes {
                let props = node.entity.props.clone();
                crate::blender::merge_gameplay(&mut node.entity, &props, root)?;
                if let Some(script) = &node.entity.script {
                    node.entity.script = Some(engine_core::freeze_script(script, root)?);
                }
            }
            ordered(&prefab)?;
            if scene.prefabs.get(name) != Some(&prefab) {
                changed.insert(name.to_string());
            }
            scene.prefabs.insert(name.into(), prefab);
        }
        let mut groups: BTreeMap<u64, Vec<Entity>> = BTreeMap::new();
        let mut sources = BTreeMap::new();
        for e in &scene.entities {
            if let Some(id) = e.props["_prefab"]["instance"].as_u64() {
                groups.entry(id).or_default().push(e.clone());
                if e.id == id {
                    if let Some(uid) = &e.source_uid {
                        sources.insert(uid.clone(), id);
                    }
                }
            }
        }
        let mut plans: BTreeMap<u64, (String, Transform, Value, Option<String>, Option<Value>)> =
            BTreeMap::new();
        for (id, entities) in &groups {
            let original = entities
                .iter()
                .find(|e| e.id == *id)
                .context("missing prefab root")?;
            if original.props["_prefab"]["name"]
                .as_str()
                .is_some_and(|s| changed.contains(s))
            {
                plans.insert(
                    *id,
                    (
                        original.props["_prefab"]["name"].as_str().unwrap().into(),
                        serde_json::from_value(original.props["_prefab"]["placement"].clone())?,
                        original.props["_prefab"]["overrides"].clone(),
                        original.source_uid.clone(),
                        None,
                    ),
                );
            }
        }
        for value in manifest["instances"].as_array().into_iter().flatten() {
            let uid = value["ai_uid"]
                .as_str()
                .context("missing Blender instance UID")?;
            let name = value["prefab"].as_str().context("missing Blender prefab")?;
            let id = match sources.get(uid) {
                Some(id) => *id,
                None => next(next_id)?,
            };
            let patch = plans
                .get(&id)
                .map(|v| v.2.clone())
                .or_else(|| {
                    groups
                        .get(&id)
                        .and_then(|g| g.iter().find(|e| e.id == id))
                        .map(|e| e.props["_prefab"]["overrides"].clone())
                })
                .unwrap_or(json!({}));
            plans.insert(
                id,
                (
                    name.into(),
                    serde_json::from_value(value["transform"].clone())?,
                    patch,
                    Some(uid.into()),
                    value.get("props").cloned(),
                ),
            );
        }
        let mut removed = BTreeSet::new();
        let mut generated = Vec::new();
        let mut affected = Vec::new();
        for (id, (name, placement, patch, source, props)) in plans {
            let mut existing = groups.remove(&id).unwrap_or_default();
            if existing.is_empty() {
                let first =
                    ordered(scene.prefabs.get(&name).context("missing Blender prefab")?)?[0];
                let mut placeholder = first.entity.clone();
                placeholder.id = id;
                placeholder.props["_prefab"] = json!({"node":first.key});
                existing.push(placeholder);
            }
            removed.extend(existing.iter().map(|e| e.id));
            let mut entities = build_instance(
                &scene.prefabs[&name],
                &name,
                placement,
                &patch,
                &existing,
                next_id,
                root,
            )?;
            for e in &mut entities {
                if let Some(source) = &source {
                    e.source_uid = Some(if e.id == id {
                        source.clone()
                    } else {
                        format!("{source}:{}", e.props["_prefab"]["node"].as_str().unwrap())
                    });
                }
            }
            if let Some(props) = props {
                crate::blender::merge_gameplay(&mut entities[0], &props, root)?;
            }
            affected.extend(entities.iter().map(|e| e.id));
            generated.extend(entities);
        }
        affected.extend(removed.iter().copied());
        scene.entities.retain(|e| !removed.contains(&e.id));
        scene.entities.extend(generated);
        Ok(affected)
    }

    pub(crate) fn prefab_member_ids(&self, ids: &[u64]) -> Vec<u64> {
        let roots: BTreeSet<_> = self
            .world
            .scene
            .entities
            .iter()
            .filter(|e| ids.contains(&e.id))
            .filter_map(|e| e.props["_prefab"]["instance"].as_u64())
            .collect();
        self.world
            .scene
            .entities
            .iter()
            .filter(|e| {
                e.props["_prefab"]["instance"]
                    .as_u64()
                    .is_some_and(|id| roots.contains(&id))
            })
            .map(|e| e.id)
            .collect()
    }

    pub(crate) fn prefab_tool(
        &mut self,
        name: &str,
        args: &Value,
        agent: &str,
    ) -> Result<Value, String> {
        let run = || -> Result<(Scene, Vec<u64>, Vec<u64>)> {
            let root = self.assets_dir.parent().unwrap_or(Path::new("."));
            let mut scene = self.world.scene.clone();
            let mut id = self.world.next_entity_id();
            let mut affected = Vec::new();
            let mut instances = Vec::new();
            match name {
                "prefab_create" | "prefab_update" => {
                    let key = args["name"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .context("prefab name must be nonempty")?;
                    ensure!(
                        scene.prefabs.contains_key(key) == (name == "prefab_update"),
                        "prefab already exists or is unknown"
                    );
                    let prefab = definition(args, root)?;
                    let old = scene.prefabs.get(key);
                    if let Some(old) = old {
                        ensure!(
                            ordered(old)?[0].key == ordered(&prefab)?[0].key,
                            "prefab root key is immutable"
                        );
                    }
                    scene.prefabs.insert(key.into(), prefab);
                    let mut groups: BTreeMap<u64, Vec<Entity>> = BTreeMap::new();
                    for e in &scene.entities {
                        if e.props["_prefab"]["name"] == key {
                            groups
                                .entry(
                                    e.props["_prefab"]["instance"]
                                        .as_u64()
                                        .context("invalid prefab binding")?,
                                )
                                .or_default()
                                .push(e.clone());
                        }
                    }
                    let mut generated = Vec::new();
                    let mut removed = BTreeSet::new();
                    for (instance, existing) in groups {
                        let original = existing
                            .iter()
                            .find(|e| e.id == instance)
                            .context("missing instance root")?;
                        removed.extend(existing.iter().map(|e| e.id));
                        let new = build_instance(
                            &scene.prefabs[key],
                            key,
                            serde_json::from_value(original.props["_prefab"]["placement"].clone())?,
                            &original.props["_prefab"]["overrides"],
                            &existing,
                            &mut id,
                            root,
                        )?;
                        affected.extend(new.iter().map(|e| e.id));
                        generated.extend(new);
                        instances.push(instance);
                    }
                    affected.extend(removed.iter().copied());
                    scene.entities.retain(|e| !removed.contains(&e.id));
                    scene.entities.extend(generated);
                }
                "prefab_instantiate" => {
                    let key = args["name"].as_str().context("prefab name required")?;
                    let count = scene
                        .prefabs
                        .get(key)
                        .context("unknown prefab")?
                        .nodes
                        .len();
                    let list = placements(args)?;
                    ensure!(
                        list.len() * count <= 100000,
                        "bulk expansion exceeds 100000 entities"
                    );
                    for (placement, overrides) in list {
                        let ids =
                            expand(&mut scene, key, placement, &overrides, &[], &mut id, root)?;
                        instances.push(ids[0]);
                        affected.extend(ids);
                    }
                }
                "prefab_set_overrides" | "prefab_remove_instance" => {
                    let instance = args["instance_id"]
                        .as_u64()
                        .context("instance_id required")?;
                    let existing = members(&scene, instance);
                    let original = existing
                        .iter()
                        .find(|e| e.id == instance)
                        .context("unknown prefab instance")?
                        .clone();
                    affected.extend(existing.iter().map(|e| e.id));
                    instances.push(instance);
                    if name == "prefab_remove_instance" {
                        scene.entities.retain(|e| !affected.contains(&e.id));
                    } else {
                        affected.extend(expand(
                            &mut scene,
                            original.props["_prefab"]["name"]
                                .as_str()
                                .context("invalid prefab binding")?,
                            serde_json::from_value(original.props["_prefab"]["placement"].clone())?,
                            &args["overrides"],
                            &existing,
                            &mut id,
                            root,
                        )?);
                    }
                }
                _ => bail!("unknown prefab tool"),
            }
            affected.sort_unstable();
            affected.dedup();
            self.check_entity_locks(&affected, agent)
                .map_err(anyhow::Error::msg)?;
            engine_core::validate_scene_paths(&scene, root)?;
            Ok((scene, affected, instances))
        };
        let (scene, ids, instances) = run().map_err(|e| e.to_string())?;
        if scene != self.world.scene {
            self.world
                .replace_authoring_scene(scene)
                .map_err(|e| e.to_string())?;
        }
        Ok(text_result(
            json!({"ok":true,"revision":self.world.revision,"ids":ids,"instances":instances})
                .to_string(),
        ))
    }

    pub(crate) fn prefab_edit(
        &mut self,
        name: &str,
        args: &Value,
        agent: &str,
    ) -> Result<Value, String> {
        let id = args["id"].as_u64().ok_or("id required")?;
        let entity = self.world.entity(id).ok_or("unknown entity")?;
        if name == "remove_entity" {
            return Err(
                "prefab members cannot be removed individually; use prefab_remove_instance".into(),
            );
        }
        let instance = entity.props["_prefab"]["instance"]
            .as_u64()
            .ok_or("invalid prefab binding")?;
        let root = self.world.entity(instance).ok_or("missing prefab root")?;
        let mut overrides = root.props["_prefab"]["overrides"].clone();
        let key = entity.props["_prefab"]["node"]
            .as_str()
            .ok_or("missing prefab node")?;
        let patch = if name == "set_color" {
            json!({"color":args.get("color").cloned().unwrap_or(json!([1,1,1,1]))})
        } else {
            let mut target = serde_json::to_value(entity.transform).unwrap();
            for k in ["position", "rotation_deg", "scale"] {
                if let Some(v) = args.get(k) {
                    target[k] = v.clone();
                }
            }
            let target: Transform = serde_json::from_value(target).map_err(|e| e.to_string())?;
            let template = self
                .world
                .scene
                .prefabs
                .get(
                    root.props["_prefab"]["name"]
                        .as_str()
                        .ok_or("missing prefab")?,
                )
                .ok_or("missing prefab template")?;
            let node = template
                .nodes
                .iter()
                .find(|n| n.key == key)
                .ok_or("missing node")?;
            let parent = if let Some(p) = &node.parent {
                members(&self.world.scene, instance)
                    .into_iter()
                    .find(|e| e.props["_prefab"]["node"] == *p)
                    .ok_or("missing parent")?
                    .transform
            } else {
                serde_json::from_value(root.props["_prefab"]["placement"].clone())
                    .map_err(|e| e.to_string())?
            };
            json!({"transform":decompose(multiply(inverse(matrix(parent)).map_err(|e|e.to_string())?,matrix(target))).map_err(|e|e.to_string())?})
        };
        merge(&mut overrides[key], &patch);
        self.prefab_tool(
            "prefab_set_overrides",
            &json!({"instance_id":instance,"overrides":overrides}),
            agent,
        )
    }
}
