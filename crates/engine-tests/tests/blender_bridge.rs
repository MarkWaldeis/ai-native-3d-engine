use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::{json, Value};

fn call(engine: &mut Engine, name: &str, args: Value) -> Value {
    let result = engine
        .handle(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn real_blender_sync_preserves_ids_props_locks_and_journal() {
    if asset_pipeline::find_blender().is_none() {
        eprintln!("SKIP: Blender not found; install Blender or add blender to PATH");
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut engine = Engine::new();
    engine.assets_dir = root
        .join("target")
        .join(format!("blender-e2e-{}", std::process::id()));
    engine.blender = asset_pipeline::bridge::BlenderBridge::new(port);
    assert_eq!(
        call(&mut engine, "blender_status", json!({}))["connected"],
        false
    );
    let ping = call(&mut engine, "blender_start", json!({}));
    assert!(ping["background"].as_bool().unwrap());
    eprintln!("Blender E2E running with version {}", ping["version"]);
    assert_eq!(
        call(&mut engine, "blender_status", json!({}))["connected"],
        true
    );
    let code = std::fs::read_to_string(root.join("blender/sync_demo.py")).unwrap();
    let exec = call(&mut engine, "blender_exec", json!({"code":code}));
    assert_eq!(exec["result"]["objects"].as_array().unwrap().len(), 3);
    let exec = call(
        &mut engine,
        "blender_exec",
        json!({"code":"print('bridge stdout'); result = 42"}),
    );
    assert_eq!(exec["stdout"], "bridge stdout\n");
    assert_eq!(exec["result"], 42);
    assert!(engine
        .handle(
            "blender_exec",
            &json!({"code":"raise ValueError('deliberate failure')"})
        )
        .unwrap_err()
        .contains("deliberate failure"));
    let info = call(&mut engine, "blender_scene_info", json!({}));
    let cube_uid = info["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "BridgeCube")
        .unwrap()["ai_uid"]
        .clone();
    call(&mut engine, "blender_exec", json!({"code": "import json\ncube = bpy.data.objects['BridgeCube']\ncube['ai_tags'] = json.dumps(['player', 'test'])\ncube['ai_collider'] = 'box'\ncube['ai_rigidbody'] = json.dumps({'type': 'kinematic', 'mass': 2, 'friction': 0.6, 'restitution': 0.1})\ncube['ai_script'] = json.dumps({'source': 'fn on_start(ctx) { ctx.log(\"from Blender\"); }'})"}));
    call(&mut engine, "blender_sync", json!({"agent":"artist"}));
    let whole = engine
        .blender
        .request(
            json!({"command":"export", "dir":engine.assets_dir.join("whole"), "whole_scene":true}),
        )
        .unwrap();
    assert_eq!(whole["baked_world_transform"], true);
    assert_eq!(whole["objects"].as_array().unwrap().len(), 3);
    for item in whole["objects"].as_array().unwrap() {
        assert!(std::path::Path::new(item["path"].as_str().unwrap()).is_file());
        assert!(item["ai_uid"].is_string());
        assert!(item["transform"]["position"].is_array());
    }
    assert_eq!(engine.world.scene.entities.len(), 3);
    assert!(engine
        .world
        .scene
        .entities
        .iter()
        .all(|e| e.source_uid.is_some()));
    let cube = engine
        .world
        .scene
        .entities
        .iter()
        .find(|e| e.name == "BridgeCube")
        .unwrap()
        .clone();
    assert_eq!(cube.source_uid.as_deref(), cube_uid.as_str());
    assert_eq!(cube.props["ai_role"], "movable");
    assert_eq!(cube.tags, vec!["player", "test"]);
    assert!(matches!(cube.collider, Some(shared::components::Collider::Box { half_extents: None })));
    assert_eq!(cube.rigidbody.as_ref().unwrap().kind, shared::components::BodyType::Kinematic);
    assert_eq!(cube.rigidbody.as_ref().unwrap().mass, 2.0);
    assert!(matches!(cube.script, Some(shared::components::Script::Inline { .. })));
    let monkey = engine
        .world
        .scene
        .entities
        .iter()
        .find(|e| e.name == "BridgeMonkey")
        .unwrap();
    assert_eq!(monkey.props["ai_gameplay"]["health"], 100);
    for (actual, expected) in cube.transform.position.iter().zip([2.0, 0.8, 1.0]) {
        assert!((actual - expected).abs() < 0.001);
    }
    assert!((cube.transform.rotation_deg[1] - 35.0).abs() < 0.001);
    for (actual, expected) in cube.transform.scale.iter().zip([0.65, 0.8, 1.1]) {
        assert!((actual - expected).abs() < 0.001);
    }
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects['BridgeCube'].location = (3, -2, 1.2)"}),
    );
    call(
        &mut engine,
        "claim_entities",
        json!({"ids":[cube.id], "agent":"owner"}),
    );
    let before = engine.world.revision;
    assert!(engine
        .handle("blender_sync", &json!({"agent":"artist"}))
        .unwrap_err()
        .contains("locked by owner"));
    assert_eq!(engine.world.revision, before);
    call(
        &mut engine,
        "release_entities",
        json!({"ids":[cube.id], "agent":"owner"}),
    );
    call(
        &mut engine,
        "blender_sync",
        json!({"agent":"artist", "objects":[cube_uid], "expected_revision":before}),
    );
    assert_eq!(engine.world.scene.entities.len(), 3);
    let moved = engine.world.entity(cube.id).unwrap();
    assert_eq!(moved.source_uid, cube.source_uid);
    for (actual, expected) in moved.transform.position.iter().zip([3.0, 1.2, 2.0]) {
        assert!((actual - expected).abs() < 0.001);
    }
    let changes = call(&mut engine, "get_changes_since", json!({"revision":before}));
    assert_eq!(changes["entries"][0]["agent"], "artist");
    assert_eq!(changes["entries"][0]["tool"], "blender_sync");
    assert_eq!(changes["entries"][0]["ids"], json!([cube.id]));
    let revision = engine.world.revision;
    let empty = call(&mut engine, "blender_sync", json!({"objects":[]}));
    assert_eq!(empty["ids"], json!([]));
    assert_eq!(engine.world.revision, revision);
    assert!(engine
        .handle("blender_sync", &json!({"expected_revision":0}))
        .unwrap_err()
        .contains("revision conflict"));
    assert!(engine
        .handle("blender_sync", &json!({"objects":["not-an-object"]}))
        .unwrap_err()
        .contains("unknown mesh objects"));
    let screenshot = engine.handle("get_screenshot", &json!({"width":320, "height":240}));
    match screenshot {
        Ok(screenshot) => {
            let bytes = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                screenshot["content"][0]["data"].as_str().unwrap(),
            )
            .unwrap();
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
            assert!(bytes.len() > 500);
        }
        Err(error) => {
            assert!(error.starts_with("renderer init:"), "{error}");
            eprintln!("SKIP: Blender E2E screenshot: {error}");
        }
    }
    call(
        &mut engine,
        "blender_exec",
        json!({"code":"bpy.data.objects.remove(bpy.data.objects['BridgeCube'], do_unlink=True)"}),
    );
    call(&mut engine, "blender_sync", json!({"objects":[]}));
    assert!(
        engine.world.entity(cube.id).is_some(),
        "prune defaults false"
    );
    call(
        &mut engine,
        "claim_entities",
        json!({"ids":[cube.id], "agent":"owner"}),
    );
    assert!(engine
        .handle("blender_sync", &json!({"objects":[], "prune":true}))
        .unwrap_err()
        .contains("locked by owner"));
    call(
        &mut engine,
        "blender_sync",
        json!({"objects":[], "prune":true, "agent":"owner"}),
    );
    assert!(engine.world.entity(cube.id).is_none());
    assert_eq!(
        engine.world.scene.entities.len(),
        2,
        "filtered prune must not delete other Blender objects"
    );
    call(
        &mut engine,
        "rollback",
        json!({"revision":revision, "agent":"owner"}),
    );
    assert_eq!(
        engine.world.entity(cube.id).unwrap().source_uid,
        cube.source_uid
    );
    assert_eq!(
        engine.world.entity(cube.id).unwrap().props["ai_role"],
        "movable"
    );
    let save = engine.assets_dir.join("saved-scene.json");
    call(&mut engine, "save_scene", json!({"path":save}));
    let loaded: shared::scene::Scene =
        serde_json::from_slice(&std::fs::read(save).unwrap()).unwrap();
    assert_eq!(loaded.entities.len(), 3);
    assert_eq!(
        loaded
            .entities
            .iter()
            .find(|e| e.id == cube.id)
            .unwrap()
            .source_uid,
        cube.source_uid
    );
}

#[test]
fn old_scene_entities_default_blender_metadata() {
    let entity: shared::scene::Entity =
        serde_json::from_value(json!({"id":1, "name":"old", "kind":"box", "size":[1,1,1]}))
            .unwrap();
    assert_eq!(entity.source_uid, None);
    assert_eq!(entity.props, json!({}));
}
