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
    let health = state["health"].as_f64().unwrap();
    assert!(health < 100.0 && health > 0.0, "{state}");
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
    assert!(!scene.entities.iter().any(|e| e.tags.iter().any(|tag| tag == "terrain_fallback")));
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
    assert_eq!(simulation.state()["state"]["health"].as_f64(), Some(100.0));
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
fn shooter_heightfield_matches_analytic_placements() {
    let mut scene: Scene = serde_json::from_value(read("examples/open_world_shooter/scene.json")).unwrap();
    let helpers = std::fs::read_to_string(root().join("examples/open_world_shooter/scripts/_terrain.rhai.part")).unwrap();
    for entity in &mut scene.entities { entity.script = None; }
    scene.entities.iter_mut().find(|e| e.name == "player").unwrap().script = Some(Script::Inline {
        source: format!("{helpers}\n{}", r#"
fn on_start(ctx) {
    for name in ["player", "intel", "radio-tower", "commander", "tree-50", "lz-tree-0"] {
        let id = ctx.find_by_name(name);
        let p = ctx.entity_position(id);
        let actual = ctx.terrain_height(p[0], p[2]);
        ctx.state_set("height_"+name, actual);
        ctx.state_set("error_"+name, abs(actual - analytic_height(p[0], p[2])));
    }
}


"#),
    });
    let simulation = Simulation::new(scene, &root()).unwrap();
    let state = simulation.state();
    assert!(!simulation.failed, "{state}");
    for name in ["player", "intel", "radio-tower", "commander", "tree-50", "lz-tree-0"] {
        let error = state["state"][format!("error_{name}")].as_f64().unwrap();
        assert!(error < 0.15, "{name}: height interpolation error {error}m");
        println!("Shooter terrain {name}: height={}, analytic error={error:.6}m",state["state"][format!("height_{name}")]);
    }
}

#[test]
fn shooter_drone_can_be_shot_down_and_falls_before_removal() {
    let mut scene = read("examples/open_world_shooter/tests/combat.json");
    scene["entities"][1] = json!({
        "id": 9, "name": "test-drone", "kind": "sphere", "radius": 0.5, "segments": 8,
        "transform": {"position": [-2700,40,-2725]},
        "tags": ["enemy", "drone", "grp_alpha"],
        "script": "examples/open_world_shooter/scripts/drone.rhai",
        "collider":{"shape":"sphere","radius":0.7},"rigidbody":{"type":"kinematic"}
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

#[test]
fn shooter_dynamic_player_and_hitscan_are_blocked_by_cover() {
    let mut scene = read("examples/open_world_shooter/tests/combat.json");
    scene["entities"].as_array_mut().unwrap().push(json!({
        "id":100,"name":"blocking-wall","kind":"box","size":[8,4,1],
        "transform":{"position":[-2700,32,-2704]},"collider":{"shape":"box"}
    }));
    let mut simulation = Simulation::new(serde_json::from_value(scene).unwrap(), &root()).unwrap();
    simulation.step(120, &[
        Input {tick:0,action:"move_z".into(),value:-1.0},
        Input {tick:0,action:"fire".into(),value:1.0},
    ]).unwrap();
    let snapshot = simulation.scene();
    let player = snapshot.entities.iter().find(|e| e.name == "player").unwrap();
    assert!(player.transform.position[2] > -2703.2 && player.transform.position[2] < -2701.0, "{:?}", player.transform);
    assert_eq!(simulation.state()["state"]["kills"], 0);
    assert_eq!(simulation.state()["state"]["health"].as_f64(), Some(100.0));
    assert!(!simulation.failed);
}

#[test]
fn shooter_reload_exposes_animation_time_and_refills_from_reserve() {
    let scene: Scene = serde_json::from_value(read("examples/open_world_shooter/tests/combat.json")).unwrap();
    let mut simulation = Simulation::new(scene, &root()).unwrap();
    simulation.step(1, &[Input {tick:0,action:"fire".into(),value:1.0}]).unwrap();
    simulation.step(1, &[
        Input {tick:1,action:"fire".into(),value:0.0},
        Input {tick:1,action:"reload".into(),value:1.0},
    ]).unwrap();
    assert!(simulation.state()["state"]["_reload"].as_f64().unwrap() > 0.0);
    simulation.step(121, &[Input {tick:2,action:"reload".into(),value:0.0}]).unwrap();
    assert_eq!(simulation.state()["state"]["ammo"],30);
    assert_eq!(simulation.state()["state"]["ammo_reserve"],149);
    assert_eq!(simulation.state()["state"]["_reload"],0.0);
}

#[test]
fn shooter_player_walks_across_both_generated_bridges_without_jumping() {
    for label in ["south", "north"] {
        let mut scene: Scene = serde_json::from_value(read("examples/open_world_shooter/scene.json")).unwrap();
        let prefix = format!("bridge-{label}-");
        let center = scene.entities.iter().find(|e| e.name == format!("{prefix}0")).unwrap().transform.position;
        scene.entities.retain(|e| e.name == "player" || e.name == "terrain" || e.name == "intel" || e.name.starts_with(&prefix));
        scene.entities.iter_mut().find(|e| e.name == "player").unwrap().transform.position = [center[0] - 110.0, 50.0, center[2]];
        let mut simulation = Simulation::new(scene, &root()).unwrap();
        simulation.step(240, &[]).unwrap();
        simulation.step(1, &[Input {tick:240,action:"move_x".into(),value:1.0}]).unwrap();
        for _ in 0..48 {
            simulation.step(60, &[]).unwrap();
            let snapshot = simulation.scene();
            let p = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
            if (p[0] - center[0]).abs() < 59.0 {
                assert!(p[1] > 5.3, "{label}: fell below deck at {p:?}");
            }
        }
        let snapshot = simulation.scene();
        let p = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
        assert!(p[0] > center[0] + 105.0, "{label}: bridge crossing blocked at {p:?}");
        assert!(!simulation.failed, "{}", simulation.state());
        println!("Shooter bridge {label}: crossed deck and both ramps without jumping");
    }
}

#[test]
fn shooter_player_stays_inside_each_terrain_edge_and_can_walk_back() {
    for axis in [0, 2] {
        for sign in [-1.0_f32, 1.0] {
            let mut scene: Scene = serde_json::from_value(read("examples/open_world_shooter/scene.json")).unwrap();
            scene.entities.retain(|e| ["player", "terrain", "intel"].contains(&e.name.as_str()));
            let player = scene.entities.iter_mut().find(|e| e.name == "player").unwrap();
            player.transform.position = [0.0, 250.0, 0.0];
            player.transform.position[axis] = sign * 3259.8;
            let action = if axis == 0 { "move_x" } else { "move_z" };
            let mut simulation = Simulation::new(scene, &root()).unwrap();
            simulation.step(300, &[]).unwrap();
            simulation.step(180, &[
                Input {tick:300,action:action.into(),value:sign as f64},
                Input {tick:300,action:"sprint".into(),value:1.0},
            ]).unwrap();
            let snapshot = simulation.scene();
            let p = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
            assert!(p[axis].abs() <= 3260.01, "axis={axis}, sign={sign}: escaped at {p:?}");
            simulation.step(180, &[Input {tick:480,action:action.into(),value:-sign as f64}]).unwrap();
            let snapshot = simulation.scene();
            let p = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
            assert!(p[axis].abs() < 3254.0, "axis={axis}, sign={sign}: could not walk back at {p:?}");
            assert!(!simulation.failed, "{}", simulation.state());
        }
    }
}

#[test]
fn shooter_player_recovers_below_terrain_with_and_without_heightfields() {
    let source = std::fs::read_to_string(root().join("examples/open_world_shooter/scripts/player.rhai")).unwrap();
    let source = format!("{}\n{}", source.replace("fn on_tick(ctx,dt)", "fn player_tick(ctx,dt)"), r#"
fn on_tick(ctx,dt) {
    if ctx.input("force_fall") > 0.5 {
        let p=ctx.position();
        ctx.set_position([p[0],p[1]-50.0,p[2]]);
        ctx.set_velocity([12.0,-50.0,12.0]);
    }
    player_tick(ctx,dt);
}
"#);
    for with_terrain in [false, true] {
        let mut scene: Scene = serde_json::from_value(read(if with_terrain {
            "examples/open_world_shooter/scene.json"
        } else {
            "examples/open_world_shooter/tests/combat.json"
        })).unwrap();
        scene.entities.retain(|e| ["player", "terrain", "intel", "fixture-ground"].contains(&e.name.as_str()));
        scene.entities.iter_mut().find(|e| e.name == "player").unwrap().script = Some(Script::Inline {source:source.clone()});
        let mut simulation = Simulation::new(scene, &root()).unwrap();
        simulation.step(60, &[]).unwrap();
        let snapshot = simulation.scene();
        let before = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
        simulation.step(1, &[Input {tick:60,action:"force_fall".into(),value:1.0}]).unwrap();
        simulation.step(60, &[Input {tick:61,action:"force_fall".into(),value:0.0}]).unwrap();
        let snapshot = simulation.scene();
        let after = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
        for axis in 0..3 {
            assert!((after[axis]-before[axis]).abs() < 0.15, "terrain={with_terrain}: {before:?} -> {after:?}");
        }
        assert_eq!(simulation.state()["state"]["health"].as_f64(), Some(100.0));
        assert!(!simulation.failed, "{}", simulation.state());
    }
}
