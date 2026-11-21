use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn web_bundle_deduplicates_distinct_paths_and_lod_base_aliases_and_rejects_bad_lods() {
    let project = Project::new();
    let original = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/suzanne.glb");
    std::fs::copy(&original, project.0.join("assets/a.glb")).unwrap();
    std::fs::copy(&original, project.0.join("assets/b.glb")).unwrap();
    let mut scene:shared::scene::Scene=serde_json::from_value(json!({"entities":[{"id":1,"name":"A","kind":"gltf","path":"assets/a.glb","props":{"web_lods":{"1":"assets/b.glb","2":"assets/a.glb"}}},{"id":2,"name":"B","kind":"gltf","path":"assets/b.glb","props":{"web_lods":{}}}]})).unwrap();
    let pkg = project.0.join("pkg");
    std::fs::create_dir(&pkg).unwrap();
    std::fs::write(pkg.join("engine_web.js"), "export default function(){}").unwrap();
    std::fs::write(pkg.join("engine_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let site = project.0.join("site");
    let result = ai_engine::web::export_bundle(&scene, &project.0, &site, &pkg).unwrap();
    assert_eq!(result["assets"], 1);
    let exported: shared::scene::Scene =
        serde_json::from_slice(&std::fs::read(site.join("scene.json")).unwrap()).unwrap();
    assert_eq!(exported.entities[0].mesh, exported.entities[1].mesh);
    assert_eq!(
        exported.entities[0].props["web_lods"]["1"],
        exported.entities[0].props["web_lods"]["2"]
    );
    assert!(exported
        .entities
        .iter()
        .all(|e| e.props.get("web_mesh_bounds").is_some()));
    std::fs::write(project.0.join("assets/bad.glb"), b"bad").unwrap();
    scene.entities[0].props["web_lods"]["1"] = json!("assets/bad.glb");
    assert!(
        ai_engine::web::export_bundle(&scene, &project.0, &project.0.join("bad-site"), &pkg)
            .is_err()
    );
    assert!(!project.0.join("bad-site").exists());
}
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "large-authoring-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        ai_engine::project::initialize(&path, "empty").unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn call(engine: &mut Engine, name: &str, args: Value) -> Value {
    let reply = engine
        .handle(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_ne!(reply["isError"], true, "{reply}");
    serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap()
}
fn nodes() -> Value {
    json!([
        {"key":"base","entity":{"kind":"box","size":[2,1,2],"tags":["building"],"collider":{"shape":"box"}}},
        {"key":"roof","parent":"base","entity":{"kind":"box","size":[3,0.5,3],"transform":{"position":[0,2,0]},"color":[1,1,1,1]}}
    ])
}

#[test]
fn prefabs_propagate_overrides_and_hierarchy_with_locks_atomic_history_and_restart() {
    let project = Project::new();
    let mut engine = Engine::from_project(&project.0).unwrap();
    call(
        &mut engine,
        "prefab_create",
        json!({"name":"house","nodes":nodes()}),
    );
    let created = call(
        &mut engine,
        "prefab_instantiate",
        json!({"name":"house","instances":[{"overrides":{"roof":{"color":[0,0,1,1]}}},{"transform":{"position":[10,0,0]}}]}),
    );
    let ids: Vec<u64> = serde_json::from_value(created["ids"].clone()).unwrap();
    let root = created["instances"][0].as_u64().unwrap();
    let second = created["instances"][1].as_u64().unwrap();
    let child = engine
        .world
        .scene
        .entities
        .iter()
        .find(|e| e.props["_prefab"]["instance"] == root && e.props["_prefab"]["node"] == "roof")
        .unwrap()
        .id;
    call(
        &mut engine,
        "claim_entities",
        json!({"ids":ids,"agent":"owner"}),
    );
    let initial = engine.world.revision;
    let mut changed = nodes();
    changed[1]["entity"]["color"] = json!([1, 0, 0, 1]);
    assert!(engine
        .handle(
            "prefab_update",
            &json!({"name":"house","nodes":changed,"agent":"intruder"})
        )
        .is_err());
    assert_eq!(engine.world.revision, initial);
    assert!(engine
        .handle(
            "prefab_update",
            &json!({"name":"house","nodes":changed,"agent":"owner","expected_revision":"bad"})
        )
        .is_err());
    call(
        &mut engine,
        "prefab_update",
        json!({"name":"house","nodes":changed,"agent":"owner","expected_revision":initial}),
    );
    assert_eq!(engine.world.entity(child).unwrap().color, [0., 0., 1., 1.]);
    assert_eq!(
        engine
            .world
            .scene
            .entities
            .iter()
            .find(|e| e.props["_prefab"]["instance"] == second
                && e.props["_prefab"]["node"] == "roof")
            .unwrap()
            .color,
        [1., 0., 0., 1.]
    );
    assert_eq!(
        call(
            &mut engine,
            "get_changes_since",
            json!({"revision":initial})
        )["entries"][0]["ids"],
        json!(ids)
    );
    call(
        &mut engine,
        "set_transform",
        json!({"id":root,"position":[1,0,0],"scale":[2,2,2],"rotation_deg":[0,90,0],"agent":"owner"}),
    );
    assert_eq!(
        engine.world.entity(child).unwrap().transform.position,
        [1., 4., 0.]
    );
    call(
        &mut engine,
        "set_color",
        json!({"id":child,"color":[0,1,0,1],"agent":"owner"}),
    );
    call(
        &mut engine,
        "write_script",
        json!({"ids":[child],"path":"scripts/roof.rhai","source":"fn on_start(ctx){ctx.state_set(\"marker\",7);}","agent":"owner"}),
    );
    let mut new = nodes();
    new[0]["entity"]["size"] = json!([4, 1, 4]);
    call(
        &mut engine,
        "prefab_update",
        json!({"name":"house","nodes":new,"agent":"owner"}),
    );
    assert_eq!(engine.world.entity(child).unwrap().color, [0., 1., 0., 1.]);
    assert_eq!(
        engine.world.entity(child).unwrap().transform.position,
        [1., 4., 0.]
    );
    let before = engine.world.checkpoint();
    let checkpoint = std::fs::read(project.0.join(".ai-engine/state.json")).unwrap();
    let journal = call(&mut engine, "get_changes_since", json!({"revision":0}));
    assert!(engine.handle("apply_batch",&json!({"mode":"atomic","agent":"owner","commands":[{"tool":"prefab_create","arguments":{"name":"temporary","nodes":nodes()}},{"tool":"prefab_instantiate","arguments":{"name":"temporary"}},{"tool":"remove_entity","arguments":{"id":999999}}]})).is_err());
    assert_eq!(engine.world.checkpoint(), before);
    assert_eq!(
        std::fs::read(project.0.join(".ai-engine/state.json")).unwrap(),
        checkpoint
    );
    call(
        &mut engine,
        "apply_batch",
        json!({"mode":"atomic","agent":"owner","commands":[{"tool":"prefab_create","arguments":{"name":"batch","nodes":nodes()}},{"tool":"prefab_instantiate","arguments":{"name":"batch","layout":{"mode":"grid","counts":[2,2],"spacing":[3,4]}}}]}),
    );
    assert_eq!(
        engine.world.revision,
        before["revision"].as_u64().unwrap() + 1
    );
    call(&mut engine, "save_scene", json!({"path":"portable.json"}));
    drop(engine);
    std::fs::write(
        project.0.join("scripts/roof.rhai"),
        "fn on_start(ctx){ctx.state_set(\"marker\",99);}",
    )
    .unwrap();
    let mut engine = Engine::from_project(&project.0).unwrap();
    assert_eq!(engine.world.scene.prefabs.len(), 2);
    assert_eq!(
        call(
            &mut engine,
            "simulate",
            json!({"ticks":1,"asserts":[{"state":"marker","equals":7}]})
        )["passed"],
        true
    );
    assert!(engine
        .handle(
            "prefab_update",
            &json!({"name":"house","nodes":nodes(),"agent":"other"})
        )
        .is_err());
    call(
        &mut engine,
        "rollback",
        json!({"revision":initial,"agent":"owner"}),
    );
    assert_eq!(engine.world.scene.prefabs.len(), 1);
    assert_eq!(engine.world.scene.entities.len(), 4);
    assert_eq!(engine.world.entity(child).unwrap().color, [0., 0., 1., 1.]);
    call(
        &mut engine,
        "load_scene",
        json!({"path":"portable.json","agent":"owner"}),
    );
    assert_eq!(engine.world.scene.prefabs.len(), 2);
    assert!(journal["entries"].as_array().unwrap().len() > 3);
    call(
        &mut engine,
        "prefab_remove_instance",
        json!({"instance_id":root,"agent":"owner"}),
    );
    assert!(!engine
        .world
        .scene
        .entities
        .iter()
        .any(|e| e.props["_prefab"]["instance"] == root));
}

#[test]
fn bulk_layouts_are_seeded_bounded_and_rejections_are_nonmutating() {
    let project = Project::new();
    let mut engine = Engine::from_project(&project.0).unwrap();
    let node = json!([{"key":"root","entity":{}}]);
    call(
        &mut engine,
        "prefab_create",
        json!({"name":"prop","nodes":node}),
    );
    let revision = engine.world.revision;
    let scatter = json!({"name":"prop","layout":{"mode":"scatter","count":20,"min":[-20,0,-20],"max":[20,0,20],"seed":42}});
    call(&mut engine, "prefab_instantiate", scatter.clone());
    let transforms: Vec<_> = engine
        .world
        .scene
        .entities
        .iter()
        .map(|e| e.transform)
        .collect();
    call(&mut engine, "rollback", json!({"revision":revision}));
    call(&mut engine, "prefab_instantiate", scatter);
    assert_eq!(
        transforms,
        engine
            .world
            .scene
            .entities
            .iter()
            .map(|e| e.transform)
            .collect::<Vec<_>>()
    );
    call(&mut engine, "rollback", json!({"revision":revision}));
    call(
        &mut engine,
        "prefab_instantiate",
        json!({"name":"prop","layout":{"mode":"along_path","count":3,"points":[[0,0,0],[10,0,0],[10,0,10]]}}),
    );
    assert_eq!(
        engine
            .world
            .scene
            .entities
            .iter()
            .map(|e| e.transform.position)
            .collect::<Vec<_>>(),
        vec![[0., 0., 0.], [10., 0., 0.], [10., 0., 10.]]
    );
    let before = engine.world.checkpoint();
    call(
        &mut engine,
        "prefab_instantiate",
        json!({"name":"prop","layout":{"mode":"grid","counts":[0,0]}}),
    );
    assert_eq!(engine.world.checkpoint(), before);
    for args in [
        json!({"name":"prop","layout":{"mode":"scatter","count":1,"min":[0,0,0],"max":[1,1,1]}}),
        json!({"name":"prop","layout":{"mode":"grid","counts":[10000,10000]}}),
        json!({"name":"prop","overrides":{"missing":{"color":[1,0,0,1]}}}),
        json!({"name":"prop","layout":{"mode":"along_path","count":2,"points":[[0,0,0],[0,0,0]]}}),
        json!({"name":"prop","overrides":{"root":{"props":{"_prefab":{}}}}}),
        json!({"name":"prop","overrides":{"root":{"kind":"gltf","path":"../outside.glb"}}}),
    ] {
        assert!(
            engine.handle("prefab_instantiate", &args).is_err(),
            "{args}"
        );
        assert_eq!(engine.world.checkpoint(), before);
    }
    for bad in [
        json!([]),
        json!([{"key":"a","parent":"b","entity":{}},{"key":"b","parent":"a","entity":{}}]),
        json!([{"key":"a","entity":{}},{"key":"a","entity":{}}]),
    ] {
        assert!(engine
            .handle("prefab_create", &json!({"name":"bad","nodes":bad}))
            .is_err());
        assert_eq!(engine.world.checkpoint(), before);
    }
    assert!(engine
        .handle(
            "remove_entity",
            &json!({"id":engine.world.scene.entities[0].id})
        )
        .unwrap_err()
        .contains("prefab_remove_instance"));
}

#[test]
fn authoring_queries_page_filter_validate_cursors_and_bound_summaries() {
    let mut engine = Engine::new();
    assert_eq!(
        call(&mut engine, "query_entities", json!({}))["entities"],
        json!([])
    );
    assert_eq!(
        call(&mut engine, "get_scene_summary", json!({}))["bounds"],
        Value::Null
    );
    for p in [[-1, 0, -1], [0, 0, 0], [128, 0, 128]] {
        call(
            &mut engine,
            "spawn_primitive",
            json!({"kind":"box","position":p}),
        );
    }
    engine.world.scene.entities[0].tags = vec!["tree".into()];
    engine.world.scene.entities[1].collider =
        Some(shared::components::Collider::Box { half_extents: None });
    engine.world.scene.entities.reverse();
    let first = call(&mut engine, "query_entities", json!({"limit":2}));
    assert_eq!(first["total"], 3);
    assert_eq!(first["entities"][0]["id"], 1);
    let last = call(
        &mut engine,
        "query_entities",
        json!({"limit":2,"cursor":first["next_cursor"]}),
    );
    assert_eq!(last["entities"][0]["id"], 3);
    assert_eq!(last["next_cursor"], Value::Null);
    for args in [
        json!({"tag":"tree"}),
        json!({"component":"collider"}),
        json!({"bbox":{"min":[-2,-1,-2],"max":[-0.5,1,-0.5]}}),
        json!({"near":{"point":[0,0,0],"radius":0}}),
        json!({"region":{"x":-1,"z":-1}}),
    ] {
        assert_eq!(call(&mut engine, "query_entities", args)["total"], 1);
    }
    assert_eq!(
        call(
            &mut engine,
            "query_entities",
            json!({"tag":"tree","component":"collider"})
        )["total"],
        0
    );
    assert_eq!(
        call(&mut engine, "query_entities", json!({"prefab":"none"}))["total"],
        0
    );
    for args in [
        json!({"limit":0}),
        json!({"limit":501}),
        json!({"component":"typo"}),
        json!({"near":{"point":[0,0,0],"radius":-1}}),
        json!({"bbox":{"min":[1,0,0],"max":[0,0,0]}}),
        json!({"region":{"x":0,"z":0,"chunk_size":0}}),
        json!({"cursor":"bad"}),
        json!({"cursor":first["next_cursor"],"tag":"changed"}),
    ] {
        assert!(engine.handle("query_entities", &args).is_err(), "{args}");
    }
    call(&mut engine, "spawn_primitive", json!({"kind":"box"}));
    assert!(engine
        .handle("query_entities", &json!({"cursor":first["next_cursor"]}))
        .unwrap_err()
        .contains("stale"));
    for n in 0..1000 {
        let mut e = engine.world.scene.entities[0].clone();
        e.id = n + 10;
        e.tags = vec![format!("tag-{n}")];
        engine.world.scene.entities.push(e);
    }
    let summary = call(&mut engine, "get_scene_summary", json!({}));
    assert_eq!(summary["tags"]["top"].as_array().unwrap().len(), 64);
    assert!(summary["tags"]["distinct"].as_u64().unwrap() > 1000);
    assert!(summary.to_string().len() < 15000);
}
