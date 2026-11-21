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
    pub physics: Rc<RefCell<crate::physics::Physics>>,
}
type ScriptResult<T> = Result<T, Box<EvalAltResult>>;

fn animation_parameter(ctx: &mut Context, name: &str, value: f64) -> ScriptResult<()> {
    if !value.is_finite() { return Err("animation parameter must be finite".into()); }
    let mut data = ctx.data.borrow_mut();
    let animator = data.scene.entities.iter_mut().find(|e| e.id == ctx.id)
        .and_then(|e| e.animator.as_mut()).ok_or("entity has no animator")?;
    if !animator.machine.parameters.contains_key(name) { return Err(format!("unknown animation parameter {name}").into()); }
    animator.values.insert(name.into(), value);
    Ok(())
}

pub(crate) fn vector(value: Array) -> ScriptResult<[f32; 3]> {
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

pub(crate) fn array(value: [f32; 3]) -> Array {
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
    crate::combat::register(&mut engine);
    engine.register_fn("collision_events", |ctx: &mut Context| -> ScriptResult<Dynamic> {
        let events = ctx.physics.borrow().collision_events(ctx.id);
        rhai::serde::to_dynamic(events).map_err(|e| e.to_string().into())
    });
    engine.register_fn("set_ragdoll", |ctx: &mut Context, active: bool| set_ragdoll(ctx, ctx.id, active));
    engine.register_fn("set_ragdoll", |ctx: &mut Context, id: INT, active: bool| set_ragdoll(ctx, id as u64, active));
    engine.register_fn("ragdoll_active", |ctx: &mut Context, id: INT| ctx.physics.borrow().ragdoll_active(id as u64));
    // Only deterministic primitives and the explicit Context API are exposed.
    for symbol in ["eval", "sleep", "timestamp", "elapsed"] {
        engine.disable_symbol(symbol);
    }
    let printed = data.clone();
    engine.on_print(move |message| printed.borrow_mut().log(message.to_owned()));
    engine.on_debug(move |message, _, _| data.borrow_mut().log(message.to_owned()));
    engine.register_type_with_name::<Context>("Context");
    engine.register_fn("nav_set_target",|ctx:&mut Context,target:Array|->ScriptResult<bool>{
        let target=vector(target)?;let mut data=ctx.data.borrow_mut();
        let scene=&mut data.scene;let mesh=scene.navmesh.as_ref().ok_or("scene has no baked navmesh")?;
        let entity=scene.entities.iter_mut().find(|e|e.id==ctx.id).ok_or("entity not found")?;
        let a=entity.nav_agent.as_mut().ok_or("entity has no nav_agent")?;
        crate::navigation::set_target(mesh,entity.transform.position,a,target).map_err(|e|e.to_string().into())
    });
    engine.register_fn("nav_path",|ctx:&mut Context|->ScriptResult<Array>{
        let data=ctx.data.borrow();let a=data.scene.entities.iter().find(|e|e.id==ctx.id).and_then(|e|e.nav_agent.as_ref()).ok_or("entity has no nav_agent")?;
        Ok(a.path[a.next.min(a.path.len())..].iter().copied().map(|p|Dynamic::from(array(p))).collect())
    });
    engine.register_fn("has_animator", |ctx: &mut Context| ctx.data.borrow().scene.entities.iter().any(|e| e.id == ctx.id && e.animator.is_some()));
    engine.register_fn("animator_set", |ctx: &mut Context, name: &str, value: f64| animation_parameter(ctx, name, value));
    engine.register_fn("animator_set", |ctx: &mut Context, name: &str, value: INT| animation_parameter(ctx, name, value as f64));
    engine.register_fn("animator_set", |ctx: &mut Context, name: &str, value: bool| animation_parameter(ctx, name, if value { 1.0 } else { 0.0 }));
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
            queue_force(ctx, ctx.id, Force::Impulse(ctx.id, value))
        },
    );
    engine.register_fn(
        "apply_impulse",
        |ctx: &mut Context, id: INT, value: Array| -> ScriptResult<()> {
            let id = id as u64;
            queue_force(ctx, id, Force::Impulse(id, vector(value)?))
        },
    );
    engine.register_fn(
        "apply_force",
        |ctx: &mut Context, value: Array| -> ScriptResult<()> {
            queue_force(ctx, ctx.id, Force::Applied(ctx.id, vector(value)?))
        },
    );
    engine.register_fn(
        "set_velocity",
        |ctx: &mut Context, value: Array| -> ScriptResult<()> {
            let value = vector(value)?;
            queue_force(ctx, ctx.id, Force::Velocity(ctx.id, value))
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
        ctx.data.borrow().input(ctx.id, name)
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

fn queue_force(ctx: &Context, id: u64, force: Force) -> ScriptResult<()> {
    let mut data = ctx.data.borrow_mut();
    let entity = data
        .scene
        .entities
        .iter()
        .find(|e| e.id == id)
        .ok_or("entity not found")?;
    if entity.rigidbody.is_none() && entity.collider.is_none() && entity.props.get("ai_physics").is_none() {
        return Err("impulse/velocity needs a rigidbody".into());
    }
    push_force(&mut data, force)
}

fn push_force(data: &mut Data, force: Force) -> ScriptResult<()> {
    if data.forces.len() >= 4096 {
        return Err("at most 4096 physics commands per tick".into());
    }
    data.forces.push(force);
    Ok(())
}

fn set_ragdoll(ctx: &Context, id: u64, active: bool) -> ScriptResult<()> {
    if !ctx.physics.borrow().has_ragdoll(id) {
        return Err("entity has no ragdoll preset".into());
    }
    push_force(&mut ctx.data.borrow_mut(), Force::Ragdoll(id, active))
}
