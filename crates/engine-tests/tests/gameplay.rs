use ai_engine::Engine;
use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn host(scene: Value) -> Engine {
    let mut engine = Engine::new();
    engine.world.scene = serde_json::from_value(scene).unwrap();
    engine.assets_dir = root().join("assets");
    engine
}

fn call(engine: &mut Engine, name: &str, arguments: Value) -> Value {
    let response = engine_mcp::handle_line(&json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":name, "arguments":arguments}}).to_string(), engine).unwrap();
    response["result"].clone()
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

fn ball_scene() -> Value {
    json!({"entities":[
        {"id":1,"name":"floor","kind":"box","size":[10,1,10],"transform":{"position":[0,-0.5,0]},"collider":{"shape":"box"},"rigidbody":{"type":"static"}},
        {"id":2,"name":"ball","kind":"sphere","radius":0.5,"segments":16,"transform":{"position":[0,3,0]},"collider":{"shape":"sphere","radius":0.5},"rigidbody":{"type":"dynamic","mass":1,"friction":0.7,"restitution":0}}
    ]})
}

#[test]
fn physics_queries_use_live_colliders_and_respect_filters() {
    let mut engine = host(json!({"entities":[
        {"id":1,"name":"compound","kind":"box","size":[1,1,1],"props":{"ai_physics":{"collider":{
            "shape":"compound","parts":[{"shape":"box","half_extents":[0.5,0.5,0.5],"position":[2,0,0]},
            {"shape":"convex_hull","vertices":[[0,0,0],[1,0,0],[0,1,0],[0,0,1]],"position":[5,0,0]}]
        }}}},
        {"id":2,"name":"sensor","kind":"box","size":[1,1,1],"transform":{"position":[0,0,2]},
         "collider":{"shape":"box"},"props":{"ai_physics":{"sensor":true,"groups":{"memberships":2,"filter":4}}}},
        {"id":3,"name":"wall","kind":"box","size":[1,1,1],"transform":{"position":[0,0,4]},
         "props":{"ai_physics":{"collider":{"shape":"trimesh","vertices":[[-1,-1,0],[0,1,0],[1,-1,0]],"triangles":[[0,1,2]]}}}},
        {"id":4,"name":"moving","kind":"sphere","radius":0.2,"segments":8,
         "transform":{"position":[8,0,0]},"collider":{"shape":"sphere"},"rigidbody":{"type":"kinematic"},
         "script":{"source":"fn on_start(ctx) { ctx.set_velocity([-6.0, 0.0, 0.0]); }"}}
    ]}));
    let mut ray = json!({"origin":[0,0,0],"direction":[0,0,10],"distance":8});
    let wall = data(call(&mut engine, "raycast", ray.clone()));
    assert_eq!(wall["id"], 3);
    assert_eq!(wall["distance"], 4.0);
    ray["filter"] = json!({"include_sensors":true,"groups":{"memberships":4,"filter":2}});
    assert_eq!(data(call(&mut engine, "raycast", ray.clone()))["id"], 2);
    ray["filter"]["exclude"] = json!(2);
    assert_eq!(data(call(&mut engine, "raycast", ray))["id"], 3);

    let cast = data(call(&mut engine, "shape_cast",
        json!({"origin":[0,0,0],"direction":[1,0,0],"distance":8,"shape":{"shape":"sphere","radius":0.25}})));
    assert_eq!(cast["id"], 1);
    assert!((cast["distance"].as_f64().unwrap() - 1.25).abs() < 0.001);
    let overlap = json!({"origin":[7,0,0],"shape":{"shape":"sphere","radius":0.25}});
    assert_eq!(data(call(&mut engine, "overlap", overlap.clone()))["ids"], json!([]));
    data(call(&mut engine, "play", json!({})));
    data(call(&mut engine, "step", json!({"ticks":10})));
    assert_eq!(data(call(&mut engine, "overlap", overlap))["ids"], json!([4]));
    assert_eq!(engine.world.revision, 0);
}

#[test]
fn rapier_ball_rests_on_floor_without_mutating_authoring() {
    let scene = ball_scene();
    let mut engine = host(scene);
    let authoring = engine.world.scene.clone();
    let report = data(call(
        &mut engine,
        "simulate",
        json!({"ticks":600,"asserts":[
            {"entity":"ball","position_near":[0,3,0],"tolerance":0,"at_tick":0},
            {"entity":"ball","position_near":[0,0.5,0],"tolerance":0.03,"at_tick":480},
            {"entity":"ball","position_near":[0,0.5,0],"tolerance":0.03},
            {"entity":"ball","y_greater_than":0}
        ]}),
    ));
    assert_eq!(report["passed"], true, "{report}");
    assert_eq!(
        report["asserts"][1]["actual"], report["asserts"][2]["actual"],
        "ball should have settled"
    );
    assert_eq!(engine.world.scene, authoring);
    assert_eq!(engine.world.revision, 0);
}

#[test]
fn deterministic_physics_and_inputs_match_transform_bits_each_tick() {
    let mut scene = ball_scene();
    scene["entities"][1]["script"] = json!({"source":"fn on_tick(ctx, dt) { if ctx.input(\"jump\") > 0.0 { ctx.apply_impulse([0.05, 0.1, 0.0]); } }"});
    let scene: shared::scene::Scene = serde_json::from_value(scene).unwrap();
    let mut first = Simulation::new(scene.clone(), &root()).unwrap();
    let mut second = Simulation::new(scene, &root()).unwrap();
    for tick in 0..360 {
        let inputs = match tick {
            0 => vec![Input {
                tick,
                action: "jump".into(),
                value: 1.0,
            }],
            5 => vec![Input {
                tick,
                action: "jump".into(),
                value: 0.0,
            }],
            _ => vec![],
        };
        first.step(1, &inputs).unwrap();
        second.step(1, &inputs).unwrap();
        for (a, b) in first.scene().entities.iter().zip(second.scene().entities) {
            assert_eq!(a.id, b.id);
            let a = a
                .transform
                .position
                .into_iter()
                .chain(a.transform.rotation_deg)
                .chain(a.transform.scale)
                .map(f32::to_bits)
                .collect::<Vec<_>>();
            let b = b
                .transform
                .position
                .into_iter()
                .chain(b.transform.rotation_deg)
                .chain(b.transform.scale)
                .map(f32::to_bits)
                .collect::<Vec<_>>();
            assert_eq!(a, b, "tick {tick}");
        }
    }
}

#[test]
fn coin_game_input_collection_reports_assert_passes_and_failures() {
    let scene = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/coin_game/scene.json")).unwrap(),
    )
    .unwrap();
    let mut engine = host(scene);
    let args: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/coin_game/validation.json")).unwrap(),
    )
    .unwrap();
    let report = data(call(&mut engine, "simulate", args.clone()));
    assert_eq!(report["passed"], true, "{report}");
    assert_eq!(report["final_state"]["state"]["score"], 3);
    assert_eq!(report["final_state"]["logs"].as_array().unwrap().len(), 4);
    let report = data(call(
        &mut engine,
        "simulate",
        json!({"ticks":180,"asserts":[{"state":"score","equals":3},{"entities_with_tag":"coin","count":0},{"entity":"missing","y_greater_than":0}]}),
    ));
    assert_eq!(report["passed"], false);
    assert_eq!(report["asserts"][0]["actual"], 0);
    assert_eq!(report["asserts"][1]["actual"], 3);
    assert_eq!(report["asserts"][2]["actual"], Value::Null);
    assert!(report["asserts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["passed"] == false));
    data(call(&mut engine, "play", json!({})));
    error(call(&mut engine, "play", json!({})), "already playing");
    error(call(&mut engine, "set_color", json!({"id":2})), "call stop");
    data(call(
        &mut engine,
        "step",
        json!({"ticks":90,"inputs":[{"tick":0,"action":"move_x","value":1}]}),
    ));
    let state = data(call(&mut engine, "get_sim_state", json!({})));
    let player = state["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "player")
        .unwrap();
    assert!(player["transform"]["position"][0].as_f64().unwrap().abs() < 0.01);
    let query = data(call(
        &mut engine,
        "query",
        json!({"tag":"player","name":"player","near":{"point":[0,0.45,0],"radius":0.1}}),
    ));
    assert_eq!(query.as_array().unwrap().len(), 1);
    data(call(&mut engine, "simulate", json!({"ticks":0})));
    assert_eq!(
        data(call(&mut engine, "get_sim_state", json!({})))["tick"],
        90,
        "simulate must not replace play state"
    );
    data(call(&mut engine, "step", json!({"ticks":90})));
    assert_eq!(
        data(call(&mut engine, "get_sim_state", json!({})))["state"]["score"],
        3
    );
    data(call(&mut engine, "stop", json!({})));
    let query = data(call(&mut engine, "query", json!({"name":"player"})));
    assert_eq!(query[0]["position"], json!([-3.0, 0.45_f32, 0.0]));
    assert_eq!(
        data(call(&mut engine, "query", json!({"tag":"coin"})))
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn script_context_spawn_remove_rotation_state_logs_and_errors_are_bounded() {
    let code = r#"
fn on_start(ctx) {
    let id = ctx.spawn_entity(#{name:"temporary",kind:"box",size:[0.2,0.2,0.2],tags:["coin"]});
    ctx.state_set("spawned", id);
    ctx.set_rotation([0.0,25.0,0.0]);
    ctx.set_position([1.0,2.0,3.0]);
    ctx.log("ready");
}
fn on_tick(ctx, dt) {
    let id = ctx.find_by_name("temporary");
    if id >= 0 { ctx.remove(id); }
    ctx.state_set("count",ctx.find_by_tag("coin").len);
}
"#;
    let mut engine = host(
        json!({"entities":[{"id":1,"name":"scripted","kind":"box","size":[1,1,1],"script":{"source":code}}]}),
    );
    let report = data(call(
        &mut engine,
        "simulate",
        json!({"ticks":1,"asserts":[{"entity":"scripted","position_near":[1,2,3],"tolerance":0},{"state":"count","equals":0},{"entities_with_tag":"coin","count":0}]}),
    ));
    assert_eq!(report["passed"], true, "{report}");
    assert_eq!(
        report["final_state"]["entities"][0]["transform"]["rotation_deg"][1],
        25.0
    );
    engine.world.scene.entities[0].script = Some(shared::components::Script::Inline {
        source: "fn on_tick(ctx, dt) {\n  missing_function();\n}".into(),
    });
    error(
        call(&mut engine, "simulate", json!({"ticks":1})),
        "<scripted:inline>:2:",
    );
    data(call(&mut engine, "play", json!({})));
    error(
        call(&mut engine, "step", json!({"ticks":1})),
        "missing_function",
    );
    assert_eq!(
        data(call(&mut engine, "get_sim_state", json!({})))["failed"],
        true
    );
    error(
        call(&mut engine, "step", json!({"ticks":1})),
        "stop and play again",
    );
    data(call(&mut engine, "stop", json!({})));
    engine.world.scene.entities[0].script = Some(shared::components::Script::Inline {
        source: "fn on_tick(ctx, dt) { while true {} }".into(),
    });
    error(
        call(&mut engine, "simulate", json!({"ticks":1})),
        "operations",
    );
    engine.world.scene.entities[0].script = Some(shared::components::Script::Inline {
        source: "fn on_tick(ctx, dt) { eval(\"1+1\"); }".into(),
    });
    assert_eq!(
        call(&mut engine, "simulate", json!({"ticks":1}))["isError"],
        true
    );
    engine.world.scene.entities[0].script =
        Some(shared::components::Script::Path("../escape.rhai".into()));
    assert_eq!(
        call(&mut engine, "simulate", json!({"ticks":1}))["isError"],
        true
    );
}

#[test]
fn runtime_empty_and_invalid_requests_are_clear_errors() {
    let mut engine = host(json!({}));
    assert_eq!(data(call(&mut engine, "query", json!({}))), json!([]));
    let report = data(call(
        &mut engine,
        "simulate",
        json!({"ticks":0,"asserts":[]}),
    ));
    assert_eq!(report["passed"], true);
    error(call(&mut engine, "step", json!({"ticks":1})), "call play");
    error(call(&mut engine, "get_sim_state", json!({})), "call play");
    error(
        call(&mut engine, "simulate", json!({"ticks":36001})),
        "0..=36000",
    );
    error(
        call(
            &mut engine,
            "simulate",
            json!({"ticks":1,"inputs":[{"tick":1,"action":"jump","value":1}]}),
        ),
        "input events",
    );
    error(
        call(
            &mut engine,
            "simulate",
            json!({"ticks":0,"asserts":[{"state":"score","equals":0,"at_tick":1}]}),
        ),
        "at_tick",
    );
    error(
        call(
            &mut engine,
            "simulate",
            json!({"ticks":1,"asserts":[{"entity":"ball"}]}),
        ),
        "position_near or y_greater_than",
    );
    error(
        call(
            &mut engine,
            "query",
            json!({"near":{"point":[0,0,0],"radius":-1}}),
        ),
        "near.radius",
    );
    error(call(&mut engine, "query", json!({"tag":3})), "tag must");
    data(call(&mut engine, "stop", json!({})));
    let old: shared::scene::Entity =
        serde_json::from_value(json!({"id":1,"name":"old","kind":"box","size":[1,1,1]})).unwrap();
    assert!(
        old.tags.is_empty()
            && old.collider.is_none()
            && old.rigidbody.is_none()
            && old.script.is_none()
    );
    let tools = engine_mcp::handle_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        &mut engine,
    )
    .unwrap();
    for name in ["play", "step", "stop", "get_sim_state", "simulate", "query"] {
        assert!(tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == name));
    }
}

#[test]
fn capsule_and_mesh_bounds_colliders_and_invalid_physics() {
    let mut scene = ball_scene();
    scene["entities"][0]["collider"] = json!({"shape":"mesh-from-entity-bounds"});
    scene["entities"][1]["collider"] = json!({"shape":"capsule","radius":0.25,"half_height":0.5});
    let mut engine = host(scene);
    let report = data(call(
        &mut engine,
        "simulate",
        json!({"ticks":480,"asserts":[{"entity":"ball","position_near":[0,0.75,0],"tolerance":0.03}]}),
    ));
    assert_eq!(report["passed"], true, "{report}");
    engine.world.scene.entities[1]
        .rigidbody
        .as_mut()
        .unwrap()
        .mass = -1.0;
    error(
        call(&mut engine, "simulate", json!({"ticks":1})),
        "mass must be positive",
    );
}

#[test]
fn simulate_optional_screenshot_is_png_and_live_screenshot_uses_simulation() {
    let mut engine = host(ball_scene());
    let probe = call(
        &mut engine,
        "get_screenshot",
        json!({"width":64,"height":64}),
    );
    if probe["isError"] == true {
        eprintln!("SKIP simulate screenshot — GPU unavailable: {probe}");
        return;
    }
    let report = call(
        &mut engine,
        "simulate",
        json!({"ticks":120,"screenshot":true}),
    );
    assert_ne!(report["isError"], true, "{report}");
    assert_eq!(report["content"][1]["mimeType"], "image/png");
    assert!(report["content"][1]["data"]
        .as_str()
        .unwrap()
        .starts_with("iVBORw0KGgo"));
    data(call(&mut engine, "play", json!({})));
    let before = call(
        &mut engine,
        "get_screenshot",
        json!({"width":64,"height":64}),
    );
    data(call(&mut engine, "step", json!({"ticks":120})));
    let after = call(
        &mut engine,
        "get_screenshot",
        json!({"width":64,"height":64}),
    );
    assert_ne!(
        before["content"][0]["data"], after["content"][0]["data"],
        "simulation transforms must reach rendering"
    );
}
