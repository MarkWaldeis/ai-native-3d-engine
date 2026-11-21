//! Shooter bookkeeping shared by the Rhai combat API: health, pending hits,
//! requested visual effects and combat events for the presentation layer.
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) const FX_KINDS: [&str; 5] = ["muzzle", "tracer", "impact", "blood", "explosion"];
pub(crate) const MAX_RAYCASTS_PER_TICK: u32 = 4096;
const MAX_FX: usize = 512;
const MAX_EVENTS: usize = 256;
const MAX_HITS: usize = 64;
pub(crate) const DEFAULT_HEALTH: f64 = 100.0;
const MAX_VARS: usize = 32;

#[derive(Default)]
pub(crate) struct Combat {
    pub health: BTreeMap<u64, f64>,
    pub hits: BTreeMap<u64, Vec<Value>>,
    pub fx: Vec<Value>,
    pub events: Vec<Value>,
    pub player: Option<u64>,
    pub raycasts: u32,
    pub vars: BTreeMap<u64, serde_json::Map<String, Value>>,
}

impl Combat {
    pub fn clear_output(&mut self) {
        self.fx.clear();
        self.events.clear();
    }

    pub fn take_output(&mut self) -> Value {
        json!({"fx": std::mem::take(&mut self.fx), "events": std::mem::take(&mut self.events)})
    }

    pub fn push_fx(&mut self, kind: &str, pos: [f64; 3], dir: [f64; 3]) {
        if self.fx.len() < MAX_FX {
            self.fx.push(json!({"kind": kind, "pos": pos, "dir": dir}));
        }
    }

    pub fn push_event(&mut self, event: Value) {
        if self.events.len() == MAX_EVENTS {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    pub fn push_hit(&mut self, target: u64, hit: Value) {
        let hits = self.hits.entry(target).or_default();
        if hits.len() == MAX_HITS {
            hits.remove(0);
        }
        hits.push(hit);
    }
}

/// Unit view direction for absolute yaw/pitch in radians (yaw 0 looks along -Z).
pub(crate) fn look_dir(yaw: f64, pitch: f64) -> [f64; 3] {
    [-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos()]
}

/// Rotates local (strafe, forward=-Z) movement into world space by yaw; clamps to length 1.
pub(crate) fn move_dir(move_x: f64, move_z: f64, yaw: f64) -> [f64; 3] {
    let length = (move_x * move_x + move_z * move_z).sqrt();
    let scale = if length > 1.0 { 1.0 / length } else { 1.0 };
    let (x, z) = (move_x * scale, move_z * scale);
    [x * yaw.cos() + z * yaw.sin(), 0.0, -x * yaw.sin() + z * yaw.cos()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_and_move_directions_share_one_convention() {
        let forward = look_dir(0.0, 0.0);
        assert!((forward[2] + 1.0).abs() < 1e-9);
        let turned = look_dir(std::f64::consts::FRAC_PI_2, 0.0);
        assert!((turned[0] + 1.0).abs() < 1e-9);
        let walk = move_dir(0.0, -1.0, std::f64::consts::FRAC_PI_2);
        assert!((walk[0] - turned[0]).abs() < 1e-9 && (walk[2] - turned[2]).abs() < 1e-9);
        let diagonal = move_dir(1.0, -1.0, 0.0);
        assert!(((diagonal[0].powi(2) + diagonal[2].powi(2)).sqrt() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn output_queues_are_bounded() {
        let mut combat = Combat::default();
        for i in 0..1000 {
            combat.push_fx("impact", [0.0; 3], [0.0, 1.0, 0.0]);
            combat.push_event(json!({"n": i}));
            combat.push_hit(1, json!({"n": i}));
        }
        assert_eq!(combat.fx.len(), MAX_FX);
        assert_eq!(combat.events.len(), MAX_EVENTS);
        assert_eq!(combat.hits[&1].len(), MAX_HITS);
        let out = combat.take_output();
        assert_eq!(out["fx"].as_array().unwrap().len(), MAX_FX);
        assert!(combat.fx.is_empty() && combat.events.is_empty());
    }
}

use crate::script::{array, vector, Context};
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, INT};

type ScriptResult<T> = Result<T, Box<EvalAltResult>>;

fn f3(v: [f32; 3]) -> [f64; 3] {
    [v[0] as f64, v[1] as f64, v[2] as f64]
}

fn own_position(ctx: &Context) -> ScriptResult<[f32; 3]> {
    let data = ctx.data.borrow();
    let index = data.entity_index(ctx.id).ok_or("entity not found")?;
    Ok(data.scene.entities[index].transform.position)
}

fn cast(ctx: &Context, origin: [f32; 3], dir: [f32; 3], max_dist: f64) -> ScriptResult<Option<crate::physics::RayHit>> {
    count_query(ctx)?;
    let max_dist = if max_dist.is_finite() { max_dist.clamp(0.0, 10_000.0) as f32 } else { 0.0 };
    Ok(ctx.physics.borrow().raycast(origin, dir, max_dist, Some(ctx.id)))
}

fn count_query(ctx: &Context) -> ScriptResult<()> {
    let mut data = ctx.data.borrow_mut();
    if data.combat.raycasts >= MAX_RAYCASTS_PER_TICK {
        return Err(format!("at most {MAX_RAYCASTS_PER_TICK} physics queries per tick").into());
    }
    data.combat.raycasts += 1;
    Ok(())
}

fn physics_query(ctx: &Context, name: &str, options: Map) -> ScriptResult<Dynamic> {
    count_query(ctx)?;
    let mut query: crate::physics_config::Query =
        rhai::serde::from_dynamic(&Dynamic::from(options)).map_err(|e| e.to_string())?;
    query.filter.exclude.get_or_insert(ctx.id);
    let value = ctx.physics.borrow().query(name, query).map_err(|e| e.to_string())?;
    rhai::serde::to_dynamic(value).map_err(|e| e.to_string().into())
}

fn ray_map(hit: Option<crate::physics::RayHit>) -> Map {
    let mut map = Map::new();
    map.insert("hit".into(), Dynamic::from(hit.is_some()));
    let hit = hit.unwrap_or(crate::physics::RayHit { id: 0, point: [0.0; 3], normal: [0.0; 3], distance: 0.0 });
    map.insert("id".into(), Dynamic::from(hit.id as INT));
    map.insert("point".into(), Dynamic::from(array(hit.point)));
    map.insert("normal".into(), Dynamic::from(array(hit.normal)));
    map.insert("distance".into(), Dynamic::from(hit.distance as f64));
    map
}

fn health_of(data: &crate::Data, id: u64) -> f64 {
    if let Some(health) = data.combat.health.get(&id) {
        return *health;
    }
    if data.combat.player == Some(id) {
        if let Some(health) = data.state.get("health").and_then(|v| v.as_f64()) {
            return health;
        }
    }
    data.entity_index(id)
        .and_then(|i| data.scene.entities[i].props.get("ai_health").and_then(|v| v.as_f64()))
        .unwrap_or(DEFAULT_HEALTH)
}

fn set_health(data: &mut crate::Data, id: u64, health: f64) {
    let health = if health.is_finite() { health.max(0.0) } else { 0.0 };
    data.combat.health.insert(id, health);
    if data.combat.player == Some(id) {
        data.state.insert("health".into(), json!(health));
    }
}

fn line_of_sight(ctx: &Context, from: [f32; 3], to: [f32; 3], target: Option<u64>) -> ScriptResult<bool> {
    let delta = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    if distance < 1e-4 {
        return Ok(true);
    }
    Ok(match cast(ctx, from, delta, distance as f64)? {
        None => true,
        Some(hit) => Some(hit.id) == target || hit.distance >= distance - 0.05,
    })
}

pub(crate) fn register(engine: &mut Engine) {
    for name in ["raycast", "shape_cast", "overlap"] {
        engine.register_fn(name, move |ctx: &mut Context, options: Map| physics_query(ctx, name, options));
    }
    engine.register_fn("raycast", |ctx: &mut Context, origin: Array, dir: Array, max_dist: f64| -> ScriptResult<Map> {
        Ok(ray_map(cast(ctx, vector(origin)?, vector(dir)?, max_dist)?))
    });
    engine.register_fn("player_id", |ctx: &mut Context| -> INT {
        ctx.data.borrow().combat.player.map_or(-1, |id| id as INT)
    });
    engine.register_fn("player_pos", |ctx: &mut Context| -> Array {
        let data = ctx.data.borrow();
        data.combat
            .player
            .and_then(|id| data.entity_index(id))
            .map_or_else(Array::new, |i| array(data.scene.entities[i].transform.position))
    });
    engine.register_fn("aim_dir", |ctx: &mut Context| -> Array {
        let data = ctx.data.borrow();
        let input = |name: &str| data.inputs.get(name).copied().unwrap_or(0.0);
        look_dir(input("look_yaw"), input("look_pitch")).into_iter().map(Dynamic::from).collect()
    });
    engine.register_fn("move_dir", |ctx: &mut Context| -> Array {
        let data = ctx.data.borrow();
        let input = |name: &str| data.inputs.get(name).copied().unwrap_or(0.0);
        move_dir(input("move_x"), input("move_z"), input("look_yaw")).into_iter().map(Dynamic::from).collect()
    });
    engine.register_fn("health", |ctx: &mut Context| -> f64 { health_of(&ctx.data.borrow(), ctx.id) });
    engine.register_fn("health_of", |ctx: &mut Context, id: INT| -> f64 { health_of(&ctx.data.borrow(), id as u64) });
    engine.register_fn("set_health", |ctx: &mut Context, health: f64| {
        let id = ctx.id;
        set_health(&mut ctx.data.borrow_mut(), id, health)
    });
    engine.register_fn("set_health", |ctx: &mut Context, id: INT, health: f64| {
        set_health(&mut ctx.data.borrow_mut(), id as u64, health)
    });
    engine.register_fn("damage", |ctx: &mut Context, id: INT, amount: f64| -> ScriptResult<f64> {
        if !amount.is_finite() {
            return Err("damage amount must be finite".into());
        }
        let (source, target) = (ctx.id, id as u64);
        let mut data = ctx.data.borrow_mut();
        let Some(index) = data.entity_index(target) else { return Ok(-1.0) };
        let entity = &data.scene.entities[index];
        let damageable = entity.props.get("ai_health").is_some()
            || entity.tags.iter().any(|t| matches!(t.as_str(), "enemy" | "player" | "damageable"));
        let before = health_of(&data, target);
        if !damageable || before <= 0.0 {
            return Ok(-1.0);
        }
        let after = (before - amount).max(0.0);
        set_health(&mut data, target, after);
        let target_name = data.scene.entities[index].name.clone();
        let source_name = data.entity_index(source).map(|i| data.scene.entities[i].name.clone()).unwrap_or_default();
        let killed = after <= 0.0;
        data.combat.push_hit(target, json!({"source": source, "amount": amount, "health": after, "killed": killed}));
        let player = data.combat.player;
        data.combat.push_event(json!({
            "type": "damage", "source": source, "target": target, "amount": amount,
            "health": after, "killed": killed, "by_player": player == Some(source), "to_player": player == Some(target),
        }));
        if killed {
            data.combat.push_event(json!({
                "type": "kill", "source": source, "target": target,
                "source_name": source_name, "target_name": target_name, "by_player": player == Some(source),
            }));
        }
        Ok(after)
    });
    engine.register_fn("prop", |ctx: &mut Context, key: &str| -> ScriptResult<Dynamic> {
        let data = ctx.data.borrow();
        let index = data.entity_index(ctx.id).ok_or("entity not found")?;
        let value = match data.scene.entities[index].props.get(key) {
            Some(Value::String(text)) if text.trim_start().starts_with(['{', '[']) => {
                serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
            }
            Some(value) => value.clone(),
            None => Value::Null,
        };
        rhai::serde::to_dynamic(value).map_err(|e| e.to_string().into())
    });
    engine.register_fn("memo", |ctx: &mut Context, key: &str| -> ScriptResult<Dynamic> {
        let value = ctx.data.borrow().combat.vars.get(&ctx.id).and_then(|vars| vars.get(key)).cloned();
        rhai::serde::to_dynamic(value.unwrap_or(Value::Null)).map_err(|e| e.to_string().into())
    });
    engine.register_fn("set_memo", |ctx: &mut Context, key: &str, value: Dynamic| -> ScriptResult<()> {
        let value: Value = rhai::serde::from_dynamic(&value).map_err(|e| e.to_string())?;
        let id = ctx.id;
        let mut data = ctx.data.borrow_mut();
        let vars = data.combat.vars.entry(id).or_default();
        if key.len() > 64 || value.to_string().len() > 4096 || (!vars.contains_key(key) && vars.len() >= MAX_VARS) {
            return Err(format!("entity memo limit exceeded ({MAX_VARS} keys, 4096 bytes each)").into());
        }
        vars.insert(key.to_owned(), value);
        Ok(())
    });
    engine.register_fn("hits", |ctx: &mut Context| -> ScriptResult<Array> {
        let id = ctx.id;
        let hits = ctx.data.borrow_mut().combat.hits.remove(&id).unwrap_or_default();
        hits.into_iter()
            .map(|hit| rhai::serde::to_dynamic(hit).map_err(|e| e.to_string().into()))
            .collect()
    });
    fn emit(ctx: &mut Context, kind: &str, pos: Array, dir: Array) -> ScriptResult<()> {
        if !FX_KINDS.contains(&kind) {
            return Err(format!("unknown fx kind {kind:?}; use one of {}", FX_KINDS.join(", ")).into());
        }
        let (pos, dir) = (vector(pos)?, vector(dir)?);
        ctx.data.borrow_mut().combat.push_fx(kind, f3(pos), f3(dir));
        Ok(())
    }
    engine.register_fn("emit_fx", emit);
    engine.register_fn("emit_fx", |ctx: &mut Context, kind: &str, pos: Array| {
        emit(ctx, kind, pos, array([0.0, 1.0, 0.0]))
    });
    engine.register_fn("emit_event", |ctx: &mut Context, event: Map| -> ScriptResult<()> {
        let value: Value = rhai::serde::from_dynamic(&Dynamic::from(event)).map_err(|e| e.to_string())?;
        if value.to_string().len() > 2048 {
            return Err("event exceeds 2048 bytes".into());
        }
        ctx.data.borrow_mut().combat.push_event(value);
        Ok(())
    });
    engine.register_fn("line_of_sight", |ctx: &mut Context, from: Array, to: Array| -> ScriptResult<bool> {
        line_of_sight(ctx, vector(from)?, vector(to)?, None)
    });
    engine.register_fn("line_of_sight", |ctx: &mut Context, from: Array, to: Array, target: INT| -> ScriptResult<bool> {
        line_of_sight(ctx, vector(from)?, vector(to)?, Some(target as u64))
    });
    engine.register_fn("can_see", |ctx: &mut Context, target: INT, max_dist: f64| -> ScriptResult<bool> {
        let from = own_position(ctx)?;
        let to = {
            let data = ctx.data.borrow();
            let Some(index) = data.entity_index(target as u64) else { return Ok(false) };
            data.scene.entities[index].transform.position
        };
        let d = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
        if d as f64 > max_dist {
            return Ok(false);
        }
        line_of_sight(ctx, from, to, Some(target as u64))
    });
    engine.register_fn("distance_to", |ctx: &mut Context, target: Array| -> ScriptResult<f64> {
        let (from, to) = (own_position(ctx)?, vector(target)?);
        Ok((((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2)) as f64).sqrt())
    });
    engine.register_fn("steer", |ctx: &mut Context, target: Array, speed: f64| -> ScriptResult<Array> {
        let (from, to) = (own_position(ctx)?, vector(target)?);
        let vy = ctx.data.borrow().velocities.get(&ctx.id).map_or(0.0, |v| v[1]) as f64;
        let (dx, dz) = ((to[0] - from[0]) as f64, (to[2] - from[2]) as f64);
        let distance = (dx * dx + dz * dz).sqrt();
        if distance < 0.25 || !(speed > 0.0) {
            return Ok(vec![Dynamic::from(0.0_f64), Dynamic::from(vy), Dynamic::from(0.0_f64)]);
        }
        let (base_x, base_z) = (dx / distance, dz / distance);
        let probe = (1.5 + speed * 0.3).min(distance);
        let mut chosen = (-base_x, -base_z);
        for angle in [0.0_f64, 0.6, -0.6, 1.2, -1.2, 1.9, -1.9] {
            let (s, c) = angle.sin_cos();
            let (x, z) = (base_x * c - base_z * s, base_x * s + base_z * c);
            if cast(ctx, from, [x as f32, 0.0, z as f32], probe)?.is_none() {
                chosen = (x, z);
                break;
            }
        }
        Ok(vec![Dynamic::from(chosen.0 * speed), Dynamic::from(vy), Dynamic::from(chosen.1 * speed)])
    });
}
