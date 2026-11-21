use super::{Data, MAX_ENTITIES};
use crate::physics::Force;
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, INT};
use serde_json::{json, Value};
use shared::scene::Entity;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub(crate) struct Context {
    pub id: u64,
    pub data: Rc<RefCell<Data>>,
}
type ScriptResult<T> = Result<T, Box<EvalAltResult>>;

fn vector(value: Array) -> ScriptResult<[f32; 3]> {
    if value.len() != 3 {
        return Err("expected a three-number vector".into());
    }
    let mut out = [0.0; 3];
    for (i, value) in value.into_iter().enumerate() {
        out[i] = value
            .clone()
            .try_cast::<f64>()
            .or_else(|| value.try_cast::<INT>().map(|n| n as f64))
            .ok_or("vector components must be numbers")? as f32;
        if !out[i].is_finite() {
            return Err("vector components must be finite".into());
        }
    }
    Ok(out)
}

fn array(value: [f32; 3]) -> Array {
    value.into_iter().map(|n| Dynamic::from(n as f64)).collect()
}

fn position(ctx: &mut Context, id: INT) -> ScriptResult<Array> {
    let data = ctx.data.borrow();
    let entity = data
        .scene
        .entities
        .iter()
        .find(|e| e.id == id as u64)
        .ok_or("entity not found")?;
    Ok(array(entity.transform.position))
}

pub(crate) fn engine(data: Rc<RefCell<Data>>) -> Engine {
    let mut engine = Engine::new();
    engine
        .set_max_operations(50_000)
        .set_max_call_levels(32)
        .set_max_expr_depths(64, 32)
        .set_max_string_size(16_384)
        .set_max_array_size(1024)
        .set_max_map_size(256)
        .set_max_variables(256)
        .set_max_functions(128);
    // Only deterministic primitives and the explicit Context API are exposed.
    for symbol in ["eval", "sleep", "timestamp", "elapsed"] {
        engine.disable_symbol(symbol);
    }
    let printed = data.clone();
    engine.on_print(move |message| printed.borrow_mut().log(message.to_owned()));
    engine.on_debug(move |message, _, _| data.borrow_mut().log(message.to_owned()));
    engine.register_type_with_name::<Context>("Context");
    engine.register_fn("id", |ctx: &mut Context| ctx.id as INT);
    engine.register_fn("position", |ctx: &mut Context| {
        let id = ctx.id as INT;
        position(ctx, id)
    });
    engine.register_fn("entity_position", position);
    engine.register_fn("rotation", |ctx: &mut Context| -> ScriptResult<Array> {
        let data = ctx.data.borrow();
        let entity = data
            .scene
            .entities
            .iter()
            .find(|e| e.id == ctx.id)
            .ok_or("entity not found")?;
        Ok(array(entity.transform.rotation_deg))
    });
    engine.register_fn(
        "set_position",
        |ctx: &mut Context, value: Array| -> ScriptResult<()> {
            let value = vector(value)?;
            let mut data = ctx.data.borrow_mut();
            let entity = data
                .scene
                .entities
                .iter_mut()
                .find(|e| e.id == ctx.id)
                .ok_or("entity not found")?;
            entity.transform.position = value;
            data.dirty.insert(ctx.id);
            Ok(())
        },
    );
    engine.register_fn(
        "set_rotation",
        |ctx: &mut Context, value: Array| -> ScriptResult<()> {
            let value = vector(value)?;
            let mut data = ctx.data.borrow_mut();
            let entity = data
                .scene
                .entities
                .iter_mut()
                .find(|e| e.id == ctx.id)
                .ok_or("entity not found")?;
            entity.transform.rotation_deg = value;
            data.dirty.insert(ctx.id);
            Ok(())
        },
    );
    engine.register_fn(
        "apply_impulse",
        |ctx: &mut Context, value: Array| -> ScriptResult<()> {
            let value = vector(value)?;
            queue_force(ctx, Force::Impulse(ctx.id, value))
        },
    );
    engine.register_fn(
        "set_velocity",
        |ctx: &mut Context, value: Array| -> ScriptResult<()> {
            let value = vector(value)?;
            queue_force(ctx, Force::Velocity(ctx.id, value))
        },
    );
    engine.register_fn("find_by_name", |ctx: &mut Context, name: &str| -> INT {
        ctx.data
            .borrow()
            .scene
            .entities
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.id as INT)
            .unwrap_or(-1)
    });
    engine.register_fn("find_by_tag", |ctx: &mut Context, tag: &str| -> Array {
        ctx.data
            .borrow()
            .scene
            .entities
            .iter()
            .filter(|e| e.tags.iter().any(|t| t == tag))
            .map(|e| Dynamic::from(e.id as INT))
            .collect()
    });
    engine.register_fn("input", |ctx: &mut Context, name: &str| -> f64 {
        ctx.data.borrow().inputs.get(name).copied().unwrap_or(0.0)
    });
    engine.register_fn("velocity", |ctx: &mut Context| -> Array {
        array(ctx.data.borrow().velocities.get(&ctx.id).copied().unwrap_or([0.0;3]))
    });
    engine.register_fn("grounded", |ctx: &mut Context| -> bool {
        ctx.data.borrow().grounded.contains(&ctx.id)
    });
    engine.register_fn(
        "state_get",
        |ctx: &mut Context, key: &str| -> ScriptResult<Dynamic> {
            let value = ctx
                .data
                .borrow()
                .state
                .get(key)
                .cloned()
                .unwrap_or(Value::Null);
            rhai::serde::to_dynamic(value).map_err(|e| e.to_string().into())
        },
    );
    engine.register_fn(
        "state_set",
        |ctx: &mut Context, key: &str, value: Dynamic| -> ScriptResult<()> {
            let value: Value = rhai::serde::from_dynamic(&value)
                .map_err(|e| Box::<EvalAltResult>::from(e.to_string()))?;
            let mut data = ctx.data.borrow_mut();
            if key.len() > 256
                || value.to_string().len() > 65_536
                || (!data.state.contains_key(key) && data.state.len() >= 256)
            {
                return Err("global state limit exceeded".into());
            }
            data.state.insert(key.to_owned(), value);
            Ok(())
        },
    );
    engine.register_fn("remove", |ctx: &mut Context, id: INT| {
        ctx.data
            .borrow_mut()
            .scene
            .entities
            .retain(|e| e.id != id as u64);
    });
    engine.register_fn(
        "spawn_entity",
        |ctx: &mut Context, entity: Map| -> ScriptResult<INT> {
            let mut value: Value = rhai::serde::from_dynamic(&Dynamic::from(entity))
                .map_err(|e| Box::<EvalAltResult>::from(e.to_string()))?;
            let mut data = ctx.data.borrow_mut();
            if data.scene.entities.len() >= MAX_ENTITIES {
                return Err("simulation entity limit exceeded".into());
            }
            let id = data.next_id;
            value["id"] = json!(id);
            if value.get("name").is_none() {
                value["name"] = json!(format!("spawned-{id}"));
            }
            let entity: Entity = serde_json::from_value(value)
                .map_err(|e| Box::<EvalAltResult>::from(format!("spawn entity: {e}")))?;
            if matches!(entity.mesh, shared::scene::PrimitiveKind::Gltf { .. }) {
                return Err(
                    "scripts may spawn primitives only; import GLB assets in the authoring scene"
                        .into(),
                );
            }
            super::validate_entity(&entity)
                .map_err(|e| Box::<EvalAltResult>::from(e.to_string()))?;
            data.next_id += 1;
            data.scene.entities.push(entity);
            Ok(id as INT)
        },
    );
    engine.register_fn("log", |ctx: &mut Context, message: &str| {
        ctx.data
            .borrow_mut()
            .log(format!("entity {}: {message}", ctx.id))
    });
    engine
}

fn queue_force(ctx: &mut Context, force: Force) -> ScriptResult<()> {
    let mut data = ctx.data.borrow_mut();
    let entity = data
        .scene
        .entities
        .iter()
        .find(|e| e.id == ctx.id)
        .ok_or("entity not found")?;
    if entity.rigidbody.is_none() && entity.collider.is_none() {
        return Err("impulse/velocity needs a rigidbody".into());
    }
    if data.forces.len() >= 4096 {
        return Err("at most 4096 impulse/velocity calls per tick".into());
    }
    data.forces.push(force);
    Ok(())
}
