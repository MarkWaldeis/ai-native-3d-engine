use ai_engine::Engine;
use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use shared::scene::Scene;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn arena() -> Scene {
    serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/net_arena/scene.json")).unwrap(),
    )
    .unwrap()
}

fn host() -> Engine {
    let mut engine = Engine::new();
    engine.assets_dir = root().join("assets");
    engine.world.scene = arena();
    engine
}

fn rpc(engine: &mut Engine, method: &str, params: Value) -> Value {
    engine_mcp::handle_line(
        &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
        engine,
    )
    .unwrap()
}

fn call(engine: &mut Engine, name: &str, args: Value) -> Value {
    rpc(engine, "tools/call", json!({"name":name,"arguments":args}))["result"].clone()
}

fn data(result: Value) -> Value {
    assert_ne!(result["isError"], true, "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn net_simulate_is_discoverable_deterministic_and_isolated_from_authoring_and_play() {
    let mut engine = host();
    let catalog = rpc(&mut engine, "tools/list", json!({}));
    let tool = catalog["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "net_simulate")
        .unwrap();
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_eq!(tool["inputSchema"]["properties"]["clients"]["maximum"], 8);
    assert!(engine_mcp::agent_guide().contains("**net_simulate**"));
    data(call(&mut engine, "play", json!({})));
    data(call(&mut engine, "step", json!({"ticks":7})));
    let play = data(call(&mut engine, "get_sim_state", json!({})));
    let authoring = engine.world.scene.clone();
    let revision = engine.world.revision;
    let args = json!({"clients":2,"ticks":480,"rtt_ms":100,"jitter_ms":20,"loss":0.05,"seed":17,
        "inputs":[[{"tick":0,"action":"move_x","value":1},{"tick":180,"action":"move_x","value":0}],
                  [{"tick":0,"action":"move_x","value":-0.7},{"tick":180,"action":"move_x","value":0}]]});
    let first = data(call(&mut engine, "net_simulate", args.clone()));
    let second = data(call(&mut engine, "net_simulate", args));
    assert_eq!(first, second, "seeded sessions must replay exactly");
    assert_eq!(first["server_tick"], 480);
    assert_eq!(first["compared_clients"], 2);
    assert_eq!(first["converged"], true, "{first}");
    assert!(first["packets_dropped"].as_u64().unwrap() > 0);
    for client in first["clients"].as_array().unwrap() {
        assert_eq!(client["comparison_tick"], 480);
        assert!(client["server_position"].is_array());
        assert!(client["client_position"].is_array());
        assert!(client["stats"]["rtt_ms"].is_number());
    }
    assert_eq!(engine.world.scene, authoring);
    assert_eq!(engine.world.revision, revision);
    assert_eq!(data(call(&mut engine, "get_sim_state", json!({}))), play);
}

#[test]
fn arena_four_slots_scoring_and_scene_overrides_work() {
    let mut engine = host();
    let report = data(call(
        &mut engine,
        "net_simulate",
        json!({"clients":4,"ticks":120,"rtt_ms":0,"jitter_ms":0,"loss":0,
        "scene":"examples/net_arena/scene.json"}),
    ));
    assert_eq!(report["compared_clients"], 4);
    assert_eq!(report["max_position_delta"], 0.0);
    let mut ids: Vec<_> = report["clients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["entity_id"].as_u64().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![2, 3, 4, 5]);
    let override_report = data(call(
        &mut engine,
        "net_simulate",
        json!({"clients":1,"ticks":1,"rtt_ms":0,"jitter_ms":0,"loss":0,
        "scene":arena()}),
    ));
    assert_eq!(override_report["server_tick"], 1);
    let mut simulation = Simulation::new(arena(), &root()).unwrap();
    simulation
        .step(
            180,
            &[Input {
                tick: 0,
                entity: Some(2),
                action: "move_x".into(),
                value: 1.0,
            }],
        )
        .unwrap();
    assert_eq!(simulation.net_script_state()["score_2"], 1);
    assert!(
        simulation
            .net_entity_states()
            .iter()
            .find(|s| s.id == 6)
            .unwrap()
            .position[0]
            > 0.1,
        "crate should be pushable"
    );
}

#[test]
fn net_simulate_empty_unconnected_and_invalid_requests_are_explicit() {
    let mut engine = host();
    let empty = data(call(
        &mut engine,
        "net_simulate",
        json!({"clients":1,"ticks":0}),
    ));
    assert_eq!(empty["server_tick"], 0);
    assert_eq!(empty["max_position_delta"], Value::Null);
    assert_eq!(empty["converged"], false);
    let lost = data(call(
        &mut engine,
        "net_simulate",
        json!({"clients":1,"ticks":30,"loss":1}),
    ));
    assert_eq!(lost["compared_clients"], 0);
    assert_eq!(lost["clients"][0]["entity_id"], Value::Null);
    assert_eq!(lost["converged"], false);
    for args in [
        json!({}),
        json!({"clients":0,"ticks":1}),
        json!({"clients":9,"ticks":1}),
        json!({"clients":5,"ticks":1}),
        json!({"clients":1,"ticks":36001}),
        json!({"clients":1,"ticks":-1}),
        json!({"clients":1,"ticks":1,"rtt_ms":-1}),
        json!({"clients":1,"ticks":1,"jitter_ms":5001}),
        json!({"clients":1,"ticks":1,"loss":1.1}),
        json!({"clients":1,"ticks":1,"seed":-1}),
        json!({"clients":1,"ticks":1,"inputs":[[{"tick":1,"action":"x","value":0}]]}),
        json!({"clients":1,"ticks":1,"inputs":[[{"tick":0,"action":"x","value":0,"entity":3}]]}),
        json!({"clients":1,"ticks":1,"inputs":[[],[]]}),
        json!({"clients":1,"ticks":1,"scene":"../escape.json"}),
        json!({"clients":1,"ticks":1,"scene":{"entities":[]}}),
        json!({"clients":1,"ticks":1,"typo":true}),
    ] {
        let response = call(&mut engine, "net_simulate", args.clone());
        assert_eq!(response["isError"], true, "{args}: {response}");
    }
    let mut bad = arena();
    bad.entities[1].script = Some(shared::components::Script::Path("../escape.rhai".into()));
    assert_eq!(
        call(
            &mut engine,
            "net_simulate",
            json!({"clients":1,"ticks":1,"scene":bad})
        )["isError"],
        true
    );
}
