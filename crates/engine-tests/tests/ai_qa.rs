use ai_engine::Engine;
use engine_render::inspection::{self, Bounds};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn host(scene: Value) -> Engine {
    let mut e = Engine::new();
    e.world.scene = serde_json::from_value(scene).unwrap();
    e.assets_dir = root().join("assets");
    e
}
fn call(e: &mut Engine, name: &str, args: Value) -> Value {
    engine_mcp::handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string(),e).unwrap()["result"].clone()
}
fn data(result: Value) -> Value {
    assert_ne!(result["isError"], true, "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}
fn error(result: Value, needle: &str) {
    assert_eq!(result["isError"], true, "{result}");
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(needle),
        "{result}"
    );
}
fn box_entity(id: u64, name: &str, position: [f32; 3]) -> Value {
    json!({"id":id,"name":name,"kind":"box","size":[1,1,1],"transform":{"position":position}})
}

#[test]
fn validation_covers_findings_fixes_and_does_not_execute_scripts() {
    let mut entities = vec![
        json!({"id":1,"name":"floor","kind":"box","size":[20,1,20],"transform":{"position":[0,-0.5,0]}}),
        box_entity(2, "crate", [0.0, 2.0, 0.0]),
        box_entity(3, "crate", [0.1, 2.0, 0.0]),
        box_entity(4, "", [30.0, 1.0, 0.0]),
        json!({"id":5,"name":"missing","kind":"gltf","path":"definitely-missing-qa.glb"}),
        json!({"id":6,"name":"broken","kind":"box","size":[1,1,1],"script":{"source":"fn on_tick(ctx, dt) {\n let x = ;\n}"}}),
        json!({"id":7,"name":"dynamic","kind":"box","size":[1,1,1],"rigidbody":{"type":"dynamic"},"transform":{"position":[-3,3,0]}}),
        json!({"id":8,"name":"flat","kind":"box","size":[1,1,1],"transform":{"scale":[0,-1,1]}}),
        json!({"id":9,"name":"not-executed","kind":"box","size":[1,1,1],"script":{"source":"fn on_start(ctx) { throw \"must not run\"; }"}}),
    ];
    entities[3]["transform"]["position"] = json!([1000, 1, 1000]);
    let mut e = host(json!({"entities":entities}));
    let before = e.world.scene.clone();
    let findings = data(call(&mut e, "validate_scene", json!({})));
    for code in [
        "floating",
        "heavy_overlap",
        "missing_asset",
        "script_compile",
        "outside_camera",
        "duplicate_name",
        "empty_name",
        "dynamic_without_collider",
        "invalid_scale",
    ] {
        assert!(
            findings
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == code),
            "missing {code}: {findings}"
        );
    }
    for finding in findings.as_array().unwrap() {
        for key in ["severity", "entity", "code", "message", "suggested_fix"] {
            assert!(finding.get(key).is_some());
        }
        if finding["code"] == "script_compile" {
            assert_eq!(finding["entity"], 6);
            assert!(finding["message"].as_str().unwrap().contains(":2:"));
        }
    }
    let fix = findings
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["code"] == "floating" && f["entity"] == 2)
        .unwrap()["suggested_fix"]
        .clone();
    data(call(
        &mut e,
        fix["tool"].as_str().unwrap(),
        fix["arguments"].clone(),
    ));
    assert_eq!(e.world.entity(2).unwrap().transform.position[1], 0.5);
    assert_ne!(before, e.world.scene);
}

#[test]
fn intentional_floating_triggers_scripts_and_dynamic_bodies_are_not_flagged() {
    let mut objects = vec![
        json!({"id":1,"name":"floor","kind":"box","size":[30,1,30],"transform":{"position":[0,-0.5,0]}}),
    ];
    for (id, extra) in [
        (2, json!({"tags":["floating"]})),
        (3, json!({"props":{"ai_floating":true}})),
        (4, json!({"tags":["trigger"]})),
        (5, json!({"props":{"ai_trigger":true}})),
        (6, json!({"script":{"source":"fn on_tick(ctx,dt) {}"}})),
        (
            7,
            json!({"rigidbody":{"type":"dynamic"},"collider":{"shape":"box"}}),
        ),
    ] {
        let mut object = box_entity(id, &format!("entity-{id}"), [id as f32, 3.0, 0.0]);
        object
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        objects.push(object);
    }
    let mut e = host(json!({"entities":objects}));
    let findings = data(call(&mut e, "validate_scene", json!({})));
    assert!(
        !findings
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["code"] == "floating"),
        "{findings}"
    );
    let scene = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/coin_game/scene.json")).unwrap(),
    )
    .unwrap();
    let mut e = host(scene);
    let before = e.world.scene.clone();
    let findings = data(call(&mut e, "validate_scene", json!({})));
    assert!(
        !findings
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["code"] == "floating" && [3, 4, 5].contains(&f["entity"].as_u64().unwrap())),
        "{findings}"
    );
    assert_eq!(before, e.world.scene);
    assert_eq!(e.world.revision, 0);
    assert_eq!(
        data(call(&mut host(json!({})), "validate_scene", json!({}))),
        json!([])
    );
}

#[test]
fn typo_suggestions_cover_tools_primitives_query_and_assertions() {
    let mut e = host(json!({"entities":[box_entity(1,"player",[0.0,0.5,0.0])]}));
    let unknown = engine_mcp::handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_screensho"}}"#, &mut e).unwrap();
    assert_eq!(unknown["error"]["code"], -32602);
    assert!(unknown["error"]["message"].as_str().unwrap().contains("did you mean 'get_screenshot'"));
    error(
        call(&mut e, "spawn_primitive", json!({"kind":"spher"})),
        "did you mean 'sphere'",
    );
    error(
        call(&mut e, "query", json!({"name":"playr"})),
        "did you mean 'player'",
    );
    let report = data(call(
        &mut e,
        "simulate",
        json!({"ticks":0,"asserts":[{"entity":"playr","y_greater_than":0}]}),
    ));
    assert_eq!(report["passed"], false);
    assert_eq!(report["asserts"][0]["actual"], Value::Null);
    assert!(report["asserts"][0]["message"]
        .as_str()
        .unwrap()
        .contains("did you mean 'player'"));
    assert!(shared::suggestions::hint("unrelated", ["box"]).is_empty());
    assert_eq!(
        shared::suggestions::hint("bo", ["box"]),
        "; did you mean 'box'?"
    );
    error(call(&mut e, "get_screenshot", json!({"views":[]})), "1..=7");
    error(
        call(&mut e, "get_screenshot", json!({"views":["diagonal"]})),
        "valid views",
    );
    error(
        call(&mut e, "get_screenshot", json!({"overlay_ids":"yes"})),
        "boolean",
    );
}

#[test]
fn transformed_bounds_and_all_framed_cameras_fit_every_corner() {
    let entity = serde_json::from_value(box_entity(1, "box", [4.0, 2.0, -3.0])).unwrap();
    let b = inspection::bounds(&entity).unwrap();
    assert_eq!(b.min, [3.5, 1.5, -3.5]);
    let bounds = Bounds {
        min: [-5.5, -0.4, -2.5],
        max: [5.5, 1.7, 2.5],
    };
    for name in ["current", "front", "back", "left", "right", "top", "iso"] {
        for aspect in [0.5, 1.5, 3.0] {
            let camera =
                inspection::framed_camera(bounds, Default::default(), name, aspect).unwrap();
            assert!(camera.position.iter().all(|v| v.is_finite()));
            assert!(
                !inspection::outside_frustum(bounds, &camera, aspect),
                "{name}"
            );
        }
    }
    assert!(inspection::framed_camera(bounds, Default::default(), "invalid", 1.0).is_err());
}

#[test]
fn contact_sheet_labels_overlay_default_compatibility_and_empty_scene() {
    let scene = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/coin_game/scene.json")).unwrap(),
    )
    .unwrap();
    let mut e = host(scene);
    let plain = call(&mut e, "get_screenshot", json!({"width":480,"height":320}));
    if plain["isError"] == true {
        eprintln!("SKIP AI-QA screenshots — GPU unavailable: {plain}");
        return;
    }
    assert_eq!(
        plain,
        call(
            &mut e,
            "get_screenshot",
            json!({"width":480,"height":320,"overlay_ids":false})
        )
    );
    let overlay = call(
        &mut e,
        "get_screenshot",
        json!({"width":480,"height":320,"overlay_ids":true}),
    );
    assert_ne!(plain, overlay);
    let sheet = call(
        &mut e,
        "get_screenshot",
        json!({"views":["current","front","back","left","right","top","iso"],"overlay_ids":true}),
    );
    assert_ne!(sheet["isError"], true, "{sheet}");
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD
        .decode(sheet["content"][0]["data"].as_str().unwrap())
        .unwrap();
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1440);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 960);
    let blank = call(
        &mut host(json!({})),
        "get_screenshot",
        json!({"views":["top"],"overlay_ids":true}),
    );
    assert_ne!(blank["isError"], true, "{blank}");
}
