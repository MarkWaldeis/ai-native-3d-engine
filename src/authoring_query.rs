use crate::Engine;
use anyhow::{ensure, Context, Result};
use engine_core::query_index::components;
use serde_json::{json, Value};
use shared::scene::{Entity, PrimitiveKind};
use std::collections::{BTreeMap, BTreeSet};

fn vector(value: &Value) -> Result<[f64; 3]> {
    let v: [f64; 3] = serde_json::from_value(value.clone())?;
    ensure!(v.iter().all(|n| n.is_finite()), "vector must be finite");
    Ok(v)
}
fn short(value: &str) -> String {
    value.chars().take(128).collect()
}
fn prefab(e: &Entity) -> Option<&str> {
    e.props["_prefab"]["name"].as_str()
}
fn buckets(map: BTreeMap<String, u64>) -> Value {
    let distinct = map.len();
    let mut list: Vec<_> = map.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    json!({"distinct":distinct,"top":list.into_iter().take(64).map(|(name,count)|json!({"name":short(&name),"count":count})).collect::<Vec<_>>()})
}
impl Engine {
    pub(crate) fn authoring_query(&self, name: &str, args: &Value) -> Result<Value> {
        let scene = &self.world.scene;
        if name == "get_scene_summary" {
            let mut tags = BTreeMap::new();
            let mut comps = BTreeMap::new();
            let mut prefabs = BTreeMap::new();
            let mut regions = BTreeMap::new();
            let mut assets = BTreeSet::new();
            let mut instances = BTreeSet::new();
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for e in &scene.entities {
                for i in 0..3 {
                    min[i] = min[i].min(e.transform.position[i]);
                    max[i] = max[i].max(e.transform.position[i]);
                }
                for tag in e.tags.iter().collect::<BTreeSet<_>>() {
                    *tags.entry(tag.clone()).or_insert(0) += 1;
                }
                for c in components(e) {
                    *comps.entry(c.into()).or_insert(0) += 1;
                }
                if let (Some(p), Some(instance)) =
                    (prefab(e), e.props["_prefab"]["instance"].as_u64())
                {
                    if instances.insert(instance) {
                        *prefabs.entry(p.into()).or_insert(0) += 1;
                    }
                }
                let p = e.transform.position;
                *regions
                    .entry(format!(
                        "{},{}",
                        (p[0] / 128.).floor() as i64,
                        (p[2] / 128.).floor() as i64
                    ))
                    .or_insert(0) += 1;
                if let PrimitiveKind::Gltf { path } = &e.mesh {
                    assets.insert(path);
                }
            }
            return Ok(
                json!({"revision":self.world.revision,"entities":scene.entities.len(),"bounds":if scene.entities.is_empty(){Value::Null}else{json!({"min":min,"max":max})},"tags":buckets(tags),"components":buckets(comps),"prefabs":buckets(prefabs),"prefab_definitions":scene.prefabs.len(),"assets":assets.len(),"regions_128m":buckets(regions)}),
            );
        }
        ensure!(
            args.as_object()
                .context("arguments must be an object")?
                .keys()
                .all(|k| [
                    "agent",
                    "expected_revision",
                    "tag",
                    "component",
                    "prefab",
                    "bbox",
                    "near",
                    "region",
                    "limit",
                    "cursor"
                ]
                .contains(&k.as_str())),
            "unknown entity query argument"
        );
        for key in ["tag", "component", "prefab"] {
            ensure!(
                args.get(key)
                    .is_none_or(|v| v.as_str().is_some_and(|s| !s.is_empty())),
                "{key} must be a nonempty string"
            );
        }
        if let Some(c) = args["component"].as_str() {
            ensure!(
                ["mesh", "collider", "rigidbody", "script", "prefab", "animator", "nav_agent"].contains(&c),
                "unknown component {c}"
            );
        }
        let limit = args
            .get("limit")
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n > 0 && *n <= 500)
                    .context("limit must be in 1..=500")
            })
            .transpose()?
            .unwrap_or(100) as usize;
        let bbox = args
            .get("bbox")
            .map(|v| -> Result<_> {
                let min = vector(&v["min"])?;
                let max = vector(&v["max"])?;
                ensure!((0..3).all(|i| min[i] <= max[i]), "invalid bbox");
                Ok((min, max))
            })
            .transpose()?;
        let near = args
            .get("near")
            .map(|v| -> Result<_> {
                let center = vector(&v["point"])?;
                let radius = v["radius"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.)
                    .context("radius must be nonnegative and finite")?;
                Ok((center, radius))
            })
            .transpose()?;
        let region = args
            .get("region")
            .map(|v| -> Result<_> {
                let x = v["x"].as_i64().context("region.x must be an integer")?;
                let z = v["z"].as_i64().context("region.z must be an integer")?;
                let size = v
                    .get("chunk_size")
                    .map(|v| {
                        v.as_f64()
                            .filter(|n| n.is_finite() && *n > 0.)
                            .context("chunk_size must be positive")
                    })
                    .transpose()?
                    .unwrap_or(128.);
                Ok((x, z, size))
            })
            .transpose()?;
        let mut filters = args.clone();
        for key in ["agent", "expected_revision", "limit", "cursor"] {
            filters.as_object_mut().unwrap().remove(key);
        }
        let fingerprint = filters
            .to_string()
            .bytes()
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
            });
        let after = if let Some(cursor) = args.get("cursor") {
            let parts: Vec<_> = cursor
                .as_str()
                .context("cursor must be a string")?
                .split(':')
                .collect();
            ensure!(parts.len() == 3, "invalid cursor");
            ensure!(
                parts[0].parse::<u64>()? == self.world.revision,
                "stale query cursor; restart pagination at the current revision"
            );
            ensure!(
                parts[2].parse::<u64>()? == fingerprint,
                "query cursor filters changed"
            );
            Some(parts[1].parse::<u64>()?)
        } else {
            None
        };
        let index = self.world.authoring_index();
        let mut matches = index.candidates(
            args["tag"].as_str(),
            args["component"].as_str(),
            args["prefab"].as_str(),
            region.filter(|(_,_,size)| *size == 128.0).map(|(x,z,_)| (x,z)),
        );
        if bbox.is_some() || near.is_some() || region.is_some_and(|(_,_,size)| size != 128.0) {
            matches.retain(|row| {
                let e = &scene.entities[*row];
                let p = e.transform.position.map(f64::from);
                !(bbox.is_some_and(|(min, max)| (0..3).any(|i| p[i] < min[i] || p[i] > max[i]))
                    || near.is_some_and(|(center, radius)| {
                        (0..3).map(|i| (p[i] - center[i]).powi(2)).sum::<f64>() > radius * radius
                    })
                    || region.is_some_and(|(x, z, size)| {
                        (p[0] / size).floor() as i64 != x || (p[2] / size).floor() as i64 != z
                    }))
            });
        }
        let total = matches.len();
        if let Some(after) = after {
            let first = matches.partition_point(|row| scene.entities[*row].id <= after);
            matches.drain(..first);
        }
        let more = matches.len() > limit;
        matches.truncate(limit);
        let cursor = if more {
            Some(format!(
                "{}:{}:{fingerprint}",
                self.world.revision,
                scene.entities[*matches.last().unwrap()].id
            ))
        } else {
            None
        };
        Ok(
            json!({"revision":self.world.revision,"total":total,"next_cursor":cursor,"entities":matches.iter().map(|row| { let e=&scene.entities[*row]; json!({"id":e.id,"name":short(&e.name),"position":e.transform.position,"tags":e.tags.iter().take(8).map(|t|short(t)).collect::<Vec<_>>(),"components":components(e),"prefab":prefab(e).map(short)}) }).collect::<Vec<_>>()}),
        )
    }
}
