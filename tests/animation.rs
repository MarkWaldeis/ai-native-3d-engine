use ai_engine::Engine;
use engine_mcp::ToolHandler;
use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use shared::scene::Scene;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn scene() -> Scene {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/sky_garden/scene.json")).unwrap(),
    )
    .unwrap()
}
fn call(engine: &mut Engine, name: &str, args: Value) -> Value {
    let result = engine
        .handle(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn deterministic_animation_parameters_transitions_looping_clamping_and_errors() {
    let mut scene = scene();
    let mut entity = scene.entities.iter().find(|e| e.id == 20).unwrap().clone();
    entity.rigidbody = None;
    entity.collider = None;
    entity.script=Some(serde_json::from_value(json!({"source":"fn on_tick(ctx, dt) { ctx.animator_set(\"speed\", ctx.input(\"move_x\")); ctx.animator_set(\"grounded\", ctx.input(\"jump\") < 0.5); }"})).unwrap());
    scene.entities = vec![entity.clone()];
    let mut batched = Simulation::new(scene.clone(), &root()).unwrap();
    let mut single = Simulation::new(scene.clone(), &root()).unwrap();
    let inputs:Vec<Input>=serde_json::from_value(json!([{"tick":0,"action":"move_x","value":1},{"tick":20,"action":"jump","value":1},{"tick":100,"action":"jump","value":0},{"tick":120,"action":"move_x","value":0}])).unwrap();
    batched.step(180, &inputs).unwrap();
    let mut states = std::collections::BTreeSet::new();
    for tick in 0..180 {
        single
            .step(
                1,
                &inputs
                    .iter()
                    .filter(|i| i.tick == tick)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        states.insert(
            single.animator_states()[0]["state"]["current_state"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        if tick == 0 {
            assert_eq!(
                single.animator_states()[0]["state"]["previous_state"],
                "idle"
            );
        }
        if tick == 90 {
            assert!(
                (single.scene().entities[0].animator.as_ref().unwrap().time
                    - entity
                        .animator
                        .as_ref()
                        .unwrap()
                        .clips
                        .iter()
                        .find(|c| c.name == "jump")
                        .unwrap()
                        .duration)
                    .abs()
                    < 1e-12
            );
        }
    }
    assert_eq!(
        states,
        std::collections::BTreeSet::from(["idle".into(), "walk".into(), "jump".into()])
    );
    assert_eq!(batched.animator_states(), single.animator_states());
    assert_eq!(single.animator_states()[0]["state"]["sequence"], 4);
    let mut animator = entity.animator.clone().unwrap();
    engine_runtime::animation::reset(&mut animator);
    animator.machine.states.get_mut("idle").unwrap().clip = "missing".into();
    assert!(engine_runtime::animation::validate(&animator)
        .unwrap_err()
        .to_string()
        .contains("unknown clip"));
    animator = entity.animator.clone().unwrap();
    animator.machine.transitions[0].conditions[0].parameter = "typo".into();
    assert!(engine_runtime::animation::validate(&animator)
        .unwrap_err()
        .to_string()
        .contains("unknown animation parameter"));
    animator = entity.animator.clone().unwrap();
    animator.machine.transitions.clear();
    assert!(engine_runtime::animation::validate(&animator)
        .unwrap_err()
        .to_string()
        .contains("unreachable"));
    animator = entity.animator.clone().unwrap();
    animator.clips.clear();
    assert!(engine_runtime::animation::validate(&animator).is_err());
    animator = entity.animator.clone().unwrap();
    animator.speed = f64::NAN;
    assert!(engine_runtime::animation::validate(&animator).is_err());
    let mut empty = Simulation::new(Scene::default(), &root()).unwrap();
    empty.step(5, &[]).unwrap();
    assert_eq!(empty.animator_states(), json!([]));
    let animated: Scene = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/sky_garden/scene.json")).unwrap(),
    )
    .unwrap();
    let mut baseline = animated.clone();
    let player = baseline.entities.iter_mut().find(|e| e.id == 20).unwrap();
    player.animator = None;
    player.mesh = shared::scene::PrimitiveKind::Gltf {
        path: "examples/sky_garden/assets/20.glb".into(),
    };
    let traces: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("tools/web_traces.json")).unwrap(),
    )
    .unwrap();
    let inputs: Vec<Input> = serde_json::from_value(traces[0]["inputs"].clone()).unwrap();
    let mut a = Simulation::new(animated, &root()).unwrap();
    let mut b = Simulation::new(baseline, &root()).unwrap();
    for tick in 0..240 {
        let events: Vec<_> = inputs.iter().filter(|i| i.tick == tick).cloned().collect();
        a.step(1, &events).unwrap();
        b.step(1, &events).unwrap();
        assert_eq!(
            a.state()["entities"],
            b.state()["entities"],
            "animation must not change collider centering or gameplay physics at tick {tick}"
        );
        assert_eq!(a.state()["state"], b.state()["state"]);
    }
    scene.entities[0].script = Some(
        serde_json::from_value(
            json!({"source":"fn on_tick(ctx, dt) { ctx.animator_set(\"typo\", 1); }"}),
        )
        .unwrap(),
    );
    assert!(Simulation::new(scene, &root())
        .unwrap()
        .step(1, &[])
        .unwrap_err()
        .to_string()
        .contains("unknown animation parameter"));
    println!("PASS animation deterministic ticks: idle/walk/jump, batching, blend clock, loop/clamp, empty scene and invalid data/Rhai parameters");
}

#[test]
fn animation_mcp_uses_asset_metadata_locks_cas_batches_rollback_and_checkpoint() {
    let project = root()
        .join("target")
        .join(format!("animation-project-{}", std::process::id()));
    ai_engine::project::initialize(&project, "empty").unwrap();
    std::fs::copy(
        root().join("examples/sky_garden/assets/milo_rig.glb"),
        project.join("assets/milo.glb"),
    )
    .unwrap();
    let mut engine = Engine::from_project(&project).unwrap();
    let id = call(
        &mut engine,
        "spawn_primitive",
        json!({"kind":"gltf","path":"milo.glb","name":"Milo"}),
    )["id"]
        .as_u64()
        .unwrap();
    let clips = call(&mut engine, "animation_clips", json!({"id":id}));
    assert_eq!(clips["clips"].as_array().unwrap().len(), 3);
    assert!(engine
        .handle("animation_clips", &json!({"path":"../../Cargo.toml"}))
        .is_err());
    let machine = scene()
        .entities
        .iter()
        .find_map(|e| e.animator.as_ref())
        .unwrap()
        .machine
        .clone();
    let revision = engine.world.revision;
    call(
        &mut engine,
        "animator_validate",
        json!({"id":id,"machine":machine}),
    );
    assert_eq!(engine.world.revision, revision);
    call(
        &mut engine,
        "claim_entities",
        json!({"ids":[id],"agent":"artist","ttl_ms":60000}),
    );
    assert!(engine
        .handle(
            "animator_set",
            &json!({"id":id,"machine":machine,"agent":"other"})
        )
        .unwrap_err()
        .contains("locked"));
    assert!(engine
        .handle(
            "animator_set",
            &json!({"id":id,"machine":machine,"agent":"artist","expected_revision":revision+100})
        )
        .unwrap_err()
        .contains("revision conflict"));
    call(
        &mut engine,
        "animator_set",
        json!({"id":id,"machine":machine,"agent":"artist","expected_revision":revision}),
    );
    let good = engine.world.revision;
    assert_eq!(
        engine
            .world
            .entity(id)
            .unwrap()
            .animator
            .as_ref()
            .unwrap()
            .clips,
        engine_geo::animation_clips(&project.join("assets/milo.glb")).unwrap()
    );
    let before = serde_json::to_value(&engine.world.scene).unwrap();
    assert!(engine.handle("apply_batch",&json!({"mode":"atomic","agent":"artist","commands":[{"tool":"animator_set","arguments":{"id":id,"machine":machine,"speed":2}},{"tool":"animator_set","arguments":{"id":id,"machine":{"initial":"bad","states":{}}}}]})).is_err());
    assert_eq!(serde_json::to_value(&engine.world.scene).unwrap(), before);
    call(
        &mut engine,
        "animator_set",
        json!({"id":id,"machine":machine,"agent":"artist","speed":2}),
    );
    call(
        &mut engine,
        "rollback",
        json!({"revision":good,"agent":"artist"}),
    );
    assert_eq!(
        engine
            .world
            .entity(id)
            .unwrap()
            .animator
            .as_ref()
            .unwrap()
            .speed,
        1.0
    );
    drop(engine);
    let mut resumed = Engine::from_project(&project).unwrap();
    assert_eq!(
        resumed
            .world
            .entity(id)
            .unwrap()
            .animator
            .as_ref()
            .unwrap()
            .machine,
        machine
    );
    call(&mut resumed, "play", json!({}));
    assert!(resumed
        .handle(
            "animator_set",
            &json!({"id":id,"machine":machine,"agent":"artist"})
        )
        .unwrap_err()
        .contains("during play"));
    drop(resumed);
    std::fs::remove_dir_all(&project).unwrap();
    println!("PASS animation MCP: actual GLB clips, confinement, read-only lint, locks/CAS, atomic rejection, rollback, restart and play guard");
}

#[test]
fn real_blender_skin_and_multiple_actions_survive_sync() {
    if asset_pipeline::find_blender().is_none() {
        eprintln!("SKIP: Blender animation test requires Blender");
        return;
    }
    let project = root()
        .join("target")
        .join(format!("animation-blender-{}", std::process::id()));
    ai_engine::project::initialize(&project, "empty").unwrap();
    let mut engine = Engine::from_project(&project).unwrap();
    call(&mut engine, "blender_start", json!({}));
    let recipe = root().join("tools/sky_garden/rigged_character.py");
    let code=format!("import importlib.util\nspec=importlib.util.spec_from_file_location('milo_recipe',{})\nm=importlib.util.module_from_spec(spec)\nspec.loader.exec_module(m)\nm.build()",serde_json::to_string(&recipe.to_string_lossy()).unwrap());
    call(&mut engine, "blender_exec", json!({"code":code}));
    call(&mut engine, "blender_sync", json!({}));
    assert_eq!(engine.world.scene.entities.len(), 1);
    let animator = engine.world.scene.entities[0].animator.as_ref().unwrap();
    let names: std::collections::BTreeSet<_> =
        animator.clips.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        std::collections::BTreeSet::from(["idle", "walk", "jump"])
    );
    let path = match &engine.world.scene.entities[0].mesh {
        shared::scene::PrimitiveKind::Gltf { path } => Path::new(path),
        _ => panic!(),
    };
    let bytes = std::fs::read(path).unwrap();
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let glb: Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    assert_eq!(glb["skins"].as_array().unwrap().len(), 1);
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects['MiloRig'].location=(3,4,5)"}),
    );
    call(&mut engine, "blender_sync", json!({}));
    assert_eq!(
        engine.world.scene.entities[0].transform.position,
        [3., 5., -4.]
    );
    drop(engine);
    std::fs::remove_dir_all(&project).unwrap();
    println!("PASS real Blender animation: built armature/weighted mesh, three actions exported in GLB and animator populated by blender_sync");
}
