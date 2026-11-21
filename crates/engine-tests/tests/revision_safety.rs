use ai_engine::Engine;
use engine_core::World;
use engine_mcp::ToolHandler;
use engine_runtime::Simulation;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "revision-safety-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        Self(root)
    }
    fn engine(&self) -> Engine {
        let mut engine = Engine::new();
        engine.assets_dir = self.0.join("assets");
        engine
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn call(engine: &mut Engine, tool: &str, args: Value) -> Value {
    let result = engine
        .handle(tool, &args)
        .unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}
fn spawn(engine: &mut Engine) -> u64 {
    call(engine, "spawn_primitive", json!({"kind":"box"}))["id"]
        .as_u64()
        .unwrap()
}
fn journal(engine: &mut Engine) -> Value {
    call(engine, "get_changes_since", json!({"revision":0}))
}
fn source(x: u32) -> String {
    format!("fn on_start(ctx) {{ ctx.set_position([{x}.0,0.0,0.0]); }}")
}
fn positions(engine: &Engine, project: &Project) -> Vec<f32> {
    Simulation::new(engine.world.scene.clone(), &project.0)
        .unwrap()
        .scene()
        .entities
        .iter()
        .map(|e| e.transform.position[0])
        .collect()
}

#[test]
fn shared_script_claims_versions_journal_and_rollback_behavior() {
    let project = Project::new();
    let mut engine = project.engine();
    let a = spawn(&mut engine);
    let b = spawn(&mut engine);
    call(
        &mut engine,
        "write_script",
        json!({"ids":[a,b],"path":"shared.rhai","source":source(1)}),
    );
    let revision = engine.world.revision;
    let before = journal(&mut engine);
    call(
        &mut engine,
        "claim_entities",
        json!({"agent":"other","ids":[b]}),
    );
    let edit = json!({"agent":"writer","ids":[a],"path":"shared.rhai","source":source(5)});
    assert!(engine
        .handle("write_script", &edit)
        .unwrap_err()
        .contains("locked by other"));
    assert_eq!(engine.world.revision, revision);
    assert_eq!(journal(&mut engine), before);
    assert_eq!(
        std::fs::read_to_string(project.0.join("shared.rhai")).unwrap(),
        source(1)
    );
    call(
        &mut engine,
        "release_entities",
        json!({"agent":"other","ids":[b]}),
    );
    assert!(engine
        .handle("write_script", &edit)
        .unwrap_err()
        .contains("requires a claim"));
    call(
        &mut engine,
        "claim_entities",
        json!({"agent":"writer","ids":[a,b]}),
    );
    call(&mut engine, "write_script", edit);
    assert_eq!(positions(&engine, &project), vec![5.0, 1.0]);
    let web = Simulation::from_sources(engine.world.scene.clone(), Default::default()).unwrap();
    assert_eq!(
        web.scene()
            .entities
            .iter()
            .map(|e| e.transform.position[0])
            .collect::<Vec<_>>(),
        vec![5.0, 1.0]
    );
    assert_eq!(
        journal(&mut engine)["entries"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["ids"],
        json!([a, b])
    );
    call(
        &mut engine,
        "write_script",
        json!({"agent":"writer","ids":[a,b],"path":"shared.rhai","source":source(9)}),
    );
    assert_eq!(positions(&engine, &project), vec![9.0, 9.0]);
    call(
        &mut engine,
        "rollback",
        json!({"agent":"writer","revision":revision}),
    );
    std::fs::write(project.0.join("shared.rhai"), source(99)).unwrap();
    assert_eq!(positions(&engine, &project), vec![1.0, 1.0]);
    let serialized = serde_json::to_string(&engine.world.scene).unwrap();
    let restored = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        Simulation::new(restored, &project.0)
            .unwrap()
            .scene()
            .entities[0]
            .transform
            .position[0],
        1.0
    );
}

#[test]
fn load_rollback_ids_are_monotonic_and_duplicate_load_is_nonmutating() {
    let project = Project::new();
    let high = project.0.join("high.json");
    let low = project.0.join("low.json");
    std::fs::write(
        &high,
        json!({"entities":[{"id":1,"name":"one","kind":"box","size":[1,1,1]},{"id":2,"name":"two","kind":"box","size":[1,1,1]},{"id":100,"name":"high","kind":"box","size":[1,1,1]}]}).to_string(),
    )
    .unwrap();
    std::fs::write(
        &low,
        json!({"entities":[{"id":1,"name":"low","kind":"box","size":[1,1,1]}]}).to_string(),
    )
    .unwrap();
    let mut engine = project.engine();
    call(&mut engine, "load_scene", json!({"path":high}));
    let revision = engine.world.revision;
    assert_eq!(spawn(&mut engine), 101);
    call(&mut engine, "load_scene", json!({"path":low}));
    call(&mut engine, "rollback", json!({"revision":revision}));
    assert_eq!(spawn(&mut engine), 102);
    let before = engine.world.scene_json();
    let revision = engine.world.revision;
    let changes = journal(&mut engine);
    let entity = engine.world.scene.entities[0].clone();
    std::fs::write(&low, json!({"entities":[entity,entity]}).to_string()).unwrap();
    assert!(engine
        .handle("load_scene", &json!({"path":low}))
        .unwrap_err()
        .contains("duplicate entity id"));
    assert_eq!(engine.world.scene_json(), before);
    assert_eq!(engine.world.revision, revision);
    assert_eq!(journal(&mut engine), changes);
    assert_eq!(spawn(&mut engine), 103);
}

#[test]
fn loaded_script_sources_are_captured_before_external_file_edits() {
    let project = Project::new();
    let path = project.0.join("scene.json");
    std::fs::write(project.0.join("shared.rhai"), source(2)).unwrap();
    std::fs::write(&path,json!({"entities":[{"id":1,"name":"scripted","kind":"box","size":[1,1,1],"script":"shared.rhai"}]}).to_string()).unwrap();
    let mut world = World::new();
    world.load(&path).unwrap();
    std::fs::write(project.0.join("shared.rhai"), source(9)).unwrap();
    assert_eq!(
        Simulation::new(world.scene.clone(), &project.0)
            .unwrap()
            .scene()
            .entities[0]
            .transform
            .position[0],
        2.0
    );
}

#[test]
fn assets_fallback_script_origins_stay_locked_after_a_root_file_shadows_them() {
    let project = Project::new();
    let mut engine = project.engine();
    let asset = project.0.join("assets/shared.rhai");
    std::fs::write(&asset, source(1)).unwrap();
    let path = project.0.join("scene.json");
    std::fs::write(&path,json!({"entities":[{"id":1,"name":"one","kind":"box","size":[1,1,1],"script":"shared.rhai"},{"id":2,"name":"two","kind":"box","size":[1,1,1],"script":"shared.rhai"}]}).to_string()).unwrap();
    call(&mut engine, "load_scene", json!({"path":path}));
    std::fs::write(project.0.join("shared.rhai"), source(99)).unwrap();
    call(
        &mut engine,
        "claim_entities",
        json!({"agent":"other","ids":[2]}),
    );
    let revision = engine.world.revision;
    assert!(engine
        .handle(
            "write_script",
            &json!({"agent":"writer","ids":[1],"path":"assets/shared.rhai","source":source(5)})
        )
        .unwrap_err()
        .contains("locked by other"));
    assert_eq!(engine.world.revision, revision);
    assert_eq!(std::fs::read_to_string(asset).unwrap(), source(1));
    assert_eq!(positions(&engine, &project), vec![1.0, 1.0]);
}

#[test]
fn exhausted_ids_reject_spawn_and_overflowing_load_without_mutation() {
    let project = Project::new();
    let mut engine = project.engine();
    let path = project.0.join("ids.json");
    let scene =
        |id| json!({"entities":[{"id":id,"name":"last","kind":"box","size":[1,1,1]}]}).to_string();
    std::fs::write(&path, scene(u64::MAX - 1)).unwrap();
    call(&mut engine, "load_scene", json!({"path":path}));
    let before = engine.world.scene_json();
    let revision = engine.world.revision;
    let changes = journal(&mut engine);
    assert!(engine
        .handle("spawn_primitive", &json!({}))
        .unwrap_err()
        .contains("ID space exhausted"));
    std::fs::write(&path, scene(u64::MAX)).unwrap();
    assert!(engine
        .handle("load_scene", &json!({"path":path}))
        .unwrap_err()
        .contains("ID space exhausted"));
    assert_eq!(engine.world.scene_json(), before);
    assert_eq!(engine.world.revision, revision);
    assert_eq!(journal(&mut engine), changes);
}

#[test]
fn invalid_cas_and_rejected_atomic_batches_have_no_observable_commit() {
    let project = Project::new();
    let mut engine = project.engine();
    let id = spawn(&mut engine);
    let asset = project.0.join("assets/sentinel.glb");
    std::fs::write(&asset, b"unchanged").unwrap();
    let scene = engine.world.scene_json();
    let revision = engine.world.revision;
    let changes = journal(&mut engine);
    for expected in [json!("1"), Value::Null, json!(true), json!(-1), json!(1.5)] {
        assert!(engine
            .handle("spawn_primitive", &json!({"expected_revision":expected}))
            .unwrap_err()
            .contains("nonnegative integer"));
    }
    let commands = json!([{"tool":"set_color","arguments":{"id":id,"color":[1,0,0,1]}},{"tool":"remove_entity","arguments":{"id":999}}]);
    assert!(engine
        .handle(
            "apply_batch",
            &json!({"mode":"atomic","expected_revision":revision,"commands":commands})
        )
        .is_err());
    for tool in [
        "blender_exec",
        "blender_sync",
        "blender_push_props",
        "run_blender_script",
        "export_web",
        "save_scene",
        "load_scene",
        "rollback",
        "write_script",
        "apply_batch",
    ] {
        assert!(engine.handle("apply_batch",&json!({"mode":"atomic","commands":[{"tool":"remove_entity","arguments":{"id":id}},{"tool":tool,"arguments":{"out_dir":project.0.join("export")}}]})).unwrap_err().contains("not allowed"));
    }
    assert!(engine
        .handle(
            "apply_batch",
            &json!({"mode":"atomic","expected_revision":revision-1,"commands":[]})
        )
        .unwrap_err()
        .contains("revision conflict"));
    assert_eq!(engine.world.scene_json(), scene);
    assert_eq!(engine.world.revision, revision);
    assert_eq!(journal(&mut engine), changes);
    assert_eq!(std::fs::read(&asset).unwrap(), b"unchanged");
    assert!(!project.0.join("export").exists());
    assert_eq!(
        call(
            &mut engine,
            "apply_batch",
            json!({"mode":"atomic","commands":[]})
        )["applied"],
        0
    );
    assert_eq!(engine.world.revision, revision);
    call(
        &mut engine,
        "claim_entities",
        json!({"agent":"owner","ids":[id]}),
    );
    assert!(engine.handle("apply_batch",&json!({"agent":"other","mode":"atomic","commands":[{"tool":"spawn_primitive"},{"tool":"set_color","arguments":{"id":id}}]})).unwrap_err().contains("locked by owner"));
    assert_eq!(engine.world.scene_json(), scene);
    assert_eq!(journal(&mut engine), changes);
}

#[test]
fn atomic_batch_has_one_revision_snapshot_and_journal_partial_keeps_successes() {
    let project = Project::new();
    let mut engine = project.engine();
    let id = spawn(&mut engine);
    let revision = engine.world.revision;
    let result = call(
        &mut engine,
        "apply_batch",
        json!({"agent":"writer","mode":"atomic","expected_revision":revision,"commands":[{"tool":"set_color","arguments":{"id":id,"color":[1,0,0,1]}},{"tool":"spawn_primitive"}]}),
    );
    assert_eq!(result["revision"], revision + 1);
    assert_eq!(engine.world.revision, revision + 1);
    let changes = journal(&mut engine);
    let entries = changes["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1]["tool"], "apply_batch");
    assert_eq!(entries[1]["ids"], json!([1, 2]));
    call(&mut engine, "rollback", json!({"revision":revision}));
    assert_eq!(engine.world.scene.entities.len(), 1);
    assert_eq!(spawn(&mut engine), 3);
    let revision = engine.world.revision;
    let partial = engine.handle("apply_batch",&json!({"commands":[{"tool":"set_color","arguments":{"id":id,"color":[0,1,0,1]}},{"tool":"remove_entity","arguments":{"id":999}}]})).unwrap();
    assert_eq!(partial["isError"], true);
    assert_eq!(partial["mode"], "partial");
    assert_eq!(partial["applied"], 1);
    assert_eq!(engine.world.revision, revision + 1);
}

#[test]
fn real_blender_transform_script_push_conflict_and_explicit_deletion() {
    if asset_pipeline::find_blender().is_none() {
        eprintln!("SKIP: Blender not found; M7 roundtrip requires real Blender");
        return;
    }
    let project = Project::new();
    let mut engine = project.engine();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    engine.blender = asset_pipeline::bridge::BlenderBridge::new(port);
    let ping = call(&mut engine, "blender_start", json!({}));
    eprintln!("M7 real Blender {}", ping["version"]);
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.ops.object.select_all(action='SELECT')\nbpy.ops.object.delete(use_global=False)\nbpy.ops.mesh.primitive_cube_add()\nobj=bpy.context.object\nobj.name='Roundtrip'\nobj['ai_role']='actor'"}),
    );
    call(&mut engine, "blender_sync", json!({}));
    let id = engine.world.scene.entities[0].id;
    let uid = engine.world.scene.entities[0].source_uid.clone();
    call(
        &mut engine,
        "write_script",
        json!({"ids":[id],"path":"engine.rhai","source":source(1)}),
    );
    let script = engine.world.entity(id).unwrap().script.clone();
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects['Roundtrip'].location.x=2.0"}),
    );
    call(&mut engine, "blender_sync", json!({}));
    assert_eq!(engine.world.entity(id).unwrap().source_uid, uid);
    assert_eq!(engine.world.entity(id).unwrap().script, script);
    assert_eq!(engine.world.entity(id).unwrap().transform.position[0], 2.0);
    call(
        &mut engine,
        "claim_entities",
        json!({"agent":"owner","ids":[id]}),
    );
    assert!(engine
        .handle("blender_push_props", &json!({"agent":"other","ids":[id]}))
        .unwrap_err()
        .contains("locked by owner"));
    call(
        &mut engine,
        "release_entities",
        json!({"agent":"owner","ids":[id]}),
    );
    call(&mut engine, "blender_push_props", json!({"ids":[id]}));
    let info = call(&mut engine, "blender_scene_info", json!({}));
    let props = &info["objects"][0]["props"];
    assert_eq!(
        serde_json::from_str::<Value>(props["ai_script"].as_str().unwrap()).unwrap(),
        serde_json::to_value(&script).unwrap()
    );
    call(&mut engine, "blender_sync", json!({}));
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"del bpy.data.objects['Roundtrip']['ai_script']\nbpy.data.objects['Roundtrip'].location.x=3.0"}),
    );
    call(&mut engine, "blender_sync", json!({}));
    assert_eq!(engine.world.entity(id).unwrap().script, script);
    call(&mut engine, "blender_push_props", json!({"ids":[id]}));
    call(&mut engine, "blender_sync", json!({}));
    call(
        &mut engine,
        "write_script",
        json!({"ids":[id],"path":"engine.rhai","source":source(5)}),
    );
    call(
        &mut engine,
        "blender_exec",
        json!({"code":format!("import json\nbpy.data.objects['Roundtrip']['ai_script']=json.dumps({{'source':{}}})",serde_json::to_string(&source(9)).unwrap())}),
    );
    let before = engine.world.scene_json();
    let revision = engine.world.revision;
    let changes = journal(&mut engine);
    assert!(engine
        .handle("blender_sync", &json!({}))
        .unwrap_err()
        .contains("gameplay conflict"));
    assert!(engine
        .handle("blender_push_props", &json!({"ids":[id]}))
        .unwrap_err()
        .contains("gameplay conflict"));
    assert_eq!(engine.world.scene_json(), before);
    assert_eq!(engine.world.revision, revision);
    assert_eq!(journal(&mut engine), changes);
    let desired = serde_json::to_string(&engine.world.entity(id).unwrap().script).unwrap();
    call(
        &mut engine,
        "blender_exec",
        json!({"code":format!("bpy.data.objects['Roundtrip']['ai_script']={}",serde_json::to_string(&desired).unwrap())}),
    );
    call(&mut engine, "blender_push_props", json!({"ids":[id]}));
    call(&mut engine, "blender_sync", json!({}));
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects['Roundtrip']['ai_deleted_props']='[\"ai_script\",\"ai_role\"]'"}),
    );
    call(&mut engine, "blender_sync", json!({}));
    assert!(engine.world.entity(id).unwrap().script.is_none());
    assert!(engine
        .world
        .entity(id)
        .unwrap()
        .props
        .get("ai_role")
        .is_none());
    call(
        &mut engine,
        "write_script",
        json!({"ids":[id],"path":"engine.rhai","source":source(2)}),
    );
    call(&mut engine, "blender_push_props", json!({"ids":[id]}));
    call(&mut engine, "blender_sync", json!({}));
    assert!(engine.world.entity(id).unwrap().script.is_some());
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects['Roundtrip']['ai_script']='null'"}),
    );
    call(&mut engine, "blender_sync", json!({}));
    assert!(engine.world.entity(id).unwrap().script.is_none());
    assert!(engine
        .handle("blender_push_props", &json!({"ids":[]}))
        .is_err());
    assert!(engine
        .handle("blender_push_props", &json!({"ids":[999]}))
        .is_err());
}
