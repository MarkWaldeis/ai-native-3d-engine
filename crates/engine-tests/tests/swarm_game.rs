use ai_engine::Engine;
use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn call(engine: &mut Engine, agent: &str, tool: &str, mut args: Value) -> Value {
    args["agent"] = json!(agent);
    let response=engine_mcp::handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string(),engine).unwrap();
    response["result"].clone()
}
fn data(response: Value) -> Value {
    assert_ne!(response["isError"], true, "{response}");
    serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap()
}
fn error(response: Value, needle: &str) {
    assert_eq!(response["isError"], true, "{response}");
    assert!(response.to_string().contains(needle), "{response}");
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "swarm-game-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(p.join("assets")).unwrap();
        Self(p)
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn script_writer_compiles_checks_locks_cas_paths_and_journals() {
    let project = Project::new();
    let mut engine = Engine::new();
    engine.assets_dir = project.0.join("assets");
    let id = data(call(
        &mut engine,
        "Lead",
        "spawn_primitive",
        json!({"kind":"box","name":"player","size":[1,1,1]}),
    ))["id"]
        .as_u64()
        .unwrap();
    data(call(
        &mut engine,
        "Artist",
        "claim_entities",
        json!({"ids":[id]}),
    ));
    let write = json!({"ids":[id],"path":"player.rhai","source":"fn on_start(ctx) {ctx.state_set(\"ready\",true);}"});
    error(
        call(&mut engine, "Skripter", "write_script", write.clone()),
        "locked by Artist",
    );
    data(call(
        &mut engine,
        "Artist",
        "release_entities",
        json!({"ids":[id]}),
    ));
    data(call(&mut engine, "Skripter", "write_script", write.clone()));
    let contents = std::fs::read_to_string(project.0.join("player.rhai")).unwrap();
    let revision = engine.world.revision;
    for (args, needle) in [
        (json!({"ids":[],"path":"empty.rhai","source":""}), "empty"),
        (
            json!({"ids":[id],"path":"../escape.rhai","source":""}),
            "relative",
        ),
        (
            json!({"ids":[999],"path":"missing.rhai","source":""}),
            "no entity",
        ),
        (
            json!({"ids":[id],"path":"player.rhai","source":"fn broken( {"}),
            "compile",
        ),
        (
            json!({"ids":[id],"path":"player.rhai","source":"","expected_revision":0}),
            "revision conflict",
        ),
    ] {
        let result = call(&mut engine, "Skripter", "write_script", args);
        if needle == "compile" {
            assert_eq!(result["isError"], true);
        } else {
            error(result, needle);
        }
    }
    assert_eq!(
        std::fs::read_to_string(project.0.join("player.rhai")).unwrap(),
        contents
    );
    assert_eq!(engine.world.revision, revision);
    let journal = data(call(
        &mut engine,
        "QA",
        "get_changes_since",
        json!({"revision":0}),
    ));
    assert_eq!(journal["entries"][1]["agent"], "Skripter");
    assert_eq!(journal["entries"][1]["ids"], json!([id]));
    data(call(&mut engine, "QA", "play", json!({})));
    error(
        call(&mut engine, "Skripter", "write_script", write),
        "disabled during play",
    );
}

#[test]
fn contact_grounding_preserves_gravity_and_wall_is_not_ground() {
    let script="fn on_tick(ctx,dt) {ctx.state_set(\"grounded\",ctx.grounded());let v=ctx.velocity();if ctx.input(\"jump\")>0.0 && ctx.grounded(){v[1]=5.0;}ctx.set_velocity(v);ctx.state_set(\"vy\",v[1]);}";
    let scene=serde_json::from_value(json!({"entities":[
        {"id":1,"name":"floor","kind":"box","size":[10,1,10],"transform":{"position":[0,-0.5,0]},"collider":{"shape":"box"}},
        {"id":2,"name":"player","kind":"sphere","radius":0.5,"segments":12,"transform":{"position":[0,3,0]},"collider":{"shape":"sphere"},"rigidbody":{"type":"dynamic"},"props":{"ai_lock_rotation":true},"script":{"source":script}}
    ]})).unwrap();
    let mut sim = Simulation::new(scene, &root()).unwrap();
    sim.step(150, &[]).unwrap();
    assert_eq!(sim.state()["state"]["grounded"], true);
    assert!(sim.state()["state"]["vy"].as_f64().unwrap().abs() < 0.01);
    sim.step(
        10,
        &[Input {
            tick: 150,
            action: "jump".into(),
            value: 1.0,
        }],
    )
    .unwrap();
    assert_eq!(sim.state()["state"]["grounded"], false);
    assert!(sim.scene().entities[1].transform.position[1] > 1.0);
    let scene=serde_json::from_value(json!({"entities":[
        {"id":1,"name":"wall","kind":"box","size":[0.2,10,10],"transform":{"position":[0.75,3,0]},"collider":{"shape":"box"}},
        {"id":2,"name":"player","kind":"sphere","radius":0.5,"segments":12,"transform":{"position":[0,3,0]},"collider":{"shape":"sphere"},"rigidbody":{"type":"dynamic"},"script":{"source":"fn on_tick(ctx,dt){ctx.set_velocity([1,0,0]);ctx.state_set(\"grounded\",ctx.grounded());}"}}
    ]})).unwrap();
    let mut wall = Simulation::new(scene, &root()).unwrap();
    wall.step(90, &[]).unwrap();
    assert_eq!(wall.state()["state"]["grounded"], false);
    let mut empty=Simulation::new(serde_json::from_value(json!({"entities":[{"id":1,"name":"no-body","kind":"box","size":[1,1,1],"script":{"source":script}}]})).unwrap(),&root()).unwrap();
    assert!(empty.step(1, &[]).is_err());
}

#[test]
fn scene_bundle_copies_glbs_without_mutating_authoring() {
    let project = Project::new();
    let mut engine = Engine::new();
    engine.assets_dir = project.0.join("assets");
    std::fs::copy(
        root().join("assets/suzanne.glb"),
        project.0.join("assets/model.glb"),
    )
    .unwrap();
    data(call(
        &mut engine,
        "Artist",
        "spawn_primitive",
        json!({"kind":"gltf","path":"model.glb","name":"model"}),
    ));
    let before = engine.world.scene.clone();
    data(call(
        &mut engine,
        "Lead",
        "save_scene",
        json!({"path":"garden.json","bundle_assets":true}),
    ));
    let saved: Value =
        serde_json::from_slice(&std::fs::read(project.0.join("garden.json")).unwrap()).unwrap();
    let asset = saved["entities"][0]["path"].as_str().unwrap();
    assert!(!Path::new(asset).is_absolute());
    assert!(project.0.join(asset).is_file());
    assert_eq!(engine.world.scene, before);
    let mesh = engine_geo::load_gltf(&root().join("examples/sky_garden/assets/1.glb")).unwrap();
    assert!(mesh.vertices.iter().any(|v| v.color != [1.0; 4]));
}

#[test]
fn sky_garden_simulate_proves_jumps_crystals_win_and_fall_respawn() {
    let root = root();
    let mut engine = Engine::new();
    engine.assets_dir = root.join("assets");
    data(call(
        &mut engine,
        "QA",
        "load_scene",
        json!({"path":root.join("examples/sky_garden/scene.json")}),
    ));
    let validation: Value = serde_json::from_slice(
        &std::fs::read(root.join("examples/sky_garden/validation.json")).unwrap(),
    )
    .unwrap();
    for name in ["win", "fall"] {
        let report = data(call(
            &mut engine,
            "QA",
            "simulate",
            validation[name].clone(),
        ));
        assert_eq!(report["passed"], true, "{name}: {report}");
    }
}
