use ai_engine::Engine;
use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use shared::components::Script;
use shared::scene::Scene;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn read(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root().join(path)).unwrap()).unwrap()
}

fn validate(scene: &str, validation: &str) -> Value {
    let mut engine = Engine::new();
    engine.assets_dir = root().join("assets");
    engine.world.scene = serde_json::from_value(read(scene)).unwrap();
    let response = engine_mcp::handle_line(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"simulate","arguments":read(validation)}})
        .to_string(),
        &mut engine,
    )
    .unwrap();
    let result = &response["result"];
    assert_ne!(result["isError"], true, "{result}");
    let report: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["passed"], true, "{report}");
    report["final_state"]["state"].clone()
}

#[test]
fn shooter_scripts_compile_and_share_the_terrain_helpers() {
    let helpers = std::fs::read_to_string(
        root().join("examples/open_world_shooter/scripts/_terrain.rhai.part"),
    )
    .unwrap();
    for name in ["player", "soldier", "drone"] {
        let path = format!("examples/open_world_shooter/scripts/{name}.rhai");
        assert!(std::fs::read_to_string(root().join(&path))
            .unwrap()
            .starts_with(&helpers));
        Simulation::check_script(&Script::Path(path), name, &root()).unwrap();
    }
}

#[test]
fn shooter_player_fire_kills_an_enemy() {
    let state = validate(
        "examples/open_world_shooter/tests/combat.json",
        "examples/open_world_shooter/validation.json",
    );
    assert!(state["kills"].as_i64().unwrap() >= 1);
    println!("Shooter fire validation: kills={}", state["kills"]);
}

#[test]
fn shooter_enemy_fire_damages_the_player() {
    let state = validate(
        "examples/open_world_shooter/tests/damage.json",
        "examples/open_world_shooter/tests/damage_validation.json",
    );
    let health = state["health"].as_i64().unwrap();
    assert!(health < 100 && health > 0, "{state}");
    println!("Shooter damage validation: health={health}");
}

#[test]
fn shooter_objectives_complete_with_real_combat() {
    let state = validate(
        "examples/open_world_shooter/tests/objectives.json",
        "examples/open_world_shooter/tests/objectives_validation.json",
    );
    assert_eq!(state["won"], true);
    println!(
        "Shooter objective validation: won={}, kills={}",
        state["won"], state["kills"]
    );
}

#[test]
fn shooter_full_generated_world_runs_120_ticks_without_script_errors() {
    let scene: Scene =
        serde_json::from_value(read("examples/open_world_shooter/scene.json")).unwrap();
    let count = scene.entities.len();
    assert!(count <= 15_000, "{count} entities exceeds density budget");
    for name in [
        "player",
        "bridge-marker",
        "intel",
        "radio-tower",
        "commander",
    ] {
        assert!(scene.entities.iter().any(|entity| entity.name == name));
    }
    let mut simulation = Simulation::new(scene, &root()).unwrap();
    simulation
        .step(
            120,
            &[Input {
                tick: 0,
                action: "fire".into(),
                value: 1.0,
            }],
        )
        .unwrap();
    assert!(!simulation.failed);
    assert_eq!(simulation.tick, 120);
    assert_eq!(simulation.state()["state"]["health"], 100);
    println!("Shooter full world: {count} entities, 120 ticks, no script errors");
}

#[test]
fn shooter_missing_objectives_are_skipped_without_errors() {
    let mut scene = read("examples/open_world_shooter/tests/combat.json");
    scene["entities"].as_array_mut().unwrap().truncate(1);
    let mut simulation = Simulation::new(serde_json::from_value(scene).unwrap(), &root()).unwrap();
    simulation.step(60, &[]).unwrap();
    let state = simulation.state();
    assert_eq!(state["state"]["won"], true);
    assert_eq!(state["state"]["kills"], 0);
}

#[test]
fn shooter_drone_can_be_shot_down_and_falls_before_removal() {
    let mut scene = read("examples/open_world_shooter/tests/combat.json");
    scene["entities"][1] = json!({
        "id": 9, "name": "test-drone", "kind": "sphere", "radius": 0.5, "segments": 8,
        "transform": {"position": [-2700,40,-2725]},
        "tags": ["enemy", "drone", "grp_alpha"],
        "script": "examples/open_world_shooter/scripts/drone.rhai"
    });
    let mut simulation = Simulation::new(serde_json::from_value(scene).unwrap(), &root()).unwrap();
    simulation
        .step(
            30,
            &[
                Input {
                    tick: 0,
                    action: "look_pitch".into(),
                    value: (8.35_f64 / 25.0).atan(),
                },
                Input {
                    tick: 0,
                    action: "fire".into(),
                    value: 1.0,
                },
            ],
        )
        .unwrap();
    let state = simulation.state();
    assert_eq!(state["state"]["kills"], 1);
    assert!(simulation.scene().entities.iter().any(|e| e.id == 9));
    simulation.step(120, &[]).unwrap();
    assert!(!simulation.scene().entities.iter().any(|e| e.id == 9));
}
