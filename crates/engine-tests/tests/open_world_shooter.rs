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

fn input(tick: u64, action: &str, value: f64) -> Input {
    Input { entity: None, tick, action: action.into(), value }
}

fn checkpoint_simulation(fixture: &str) -> Simulation {
    let mut scene: Scene = serde_json::from_value(read(fixture)).unwrap();
    scene.entities.retain(|e| e.name != "guard");
    if let Some(commander) = scene.entities.iter_mut().find(|e| e.name == "commander") {
        commander.script = None;
    }
    let source = std::fs::read_to_string(root().join("examples/open_world_shooter/scripts/player.rhai")).unwrap();
    let source = format!("{}\n{}", source.replace("fn on_tick(ctx,dt)", "fn player_tick(ctx,dt)"), r#"
fn on_tick(ctx,dt) {
    if ctx.input("test_fatal") > 0.5 {
        let p=ctx.position();
        ctx.set_position([p[0]+20.0,p[1],p[2]]);
        ctx.set_velocity([12.0,7.0,6.0]);
        ctx.state_set("ammo",0);
        ctx.state_set("ammo_reserve",ctx.input("test_reserve").to_int());
        ctx.state_set("_reload",1.0);
        ctx.state_set("_fire_cd",2.0);
        ctx.damage(ctx.id(),1000.0);
    }
    if ctx.input("test_destroy_tower") > 0.5 {
        let tower=ctx.find_by_name("radio-tower");
        if tower >= 0 { ctx.damage(tower,1000.0); }
    }
    player_tick(ctx,dt);
    ctx.state_set("_test_checkpoint",ctx.memo("checkpoint"));
}
"#);
    scene.entities.iter_mut().find(|e| e.name == "player").unwrap().script = Some(Script::Inline { source });
    Simulation::new(scene, &root()).unwrap()
}

fn player_position(simulation: &Simulation) -> [f32; 3] {
    simulation.scene().entities.iter().find(|e| e.name == "player").unwrap().transform.position
}

fn assert_player_at(simulation: &Simulation, position: [f32; 3]) {
    let actual = player_position(simulation);
    for axis in 0..3 {
        assert!((actual[axis]-position[axis]).abs() < 0.15, "{actual:?} != {position:?}");
    }
    assert!(!simulation.failed, "{}", simulation.state());
}

fn checkpoint_death(simulation: &mut Simulation, reserve: f64) {
    let tick = simulation.tick;
    simulation.step(2, &[
        input(tick, "test_fatal", 1.0),
        input(tick, "test_reserve", reserve),
        input(tick+1, "test_fatal", 0.0),
    ]).unwrap();
}

fn soldier_volley_simulation(placements: &[([f32; 3], &str)]) -> Simulation {
    let mut scene: Scene = serde_json::from_value(read("examples/open_world_shooter/tests/damage.json")).unwrap();
    let guard = scene.entities.iter().find(|e| e.name == "guard").unwrap().clone();
    scene.entities.retain(|e| e.name != "guard");
    let player_source = std::fs::read_to_string(root().join("examples/open_world_shooter/scripts/player.rhai")).unwrap();
    scene.entities.iter_mut().find(|e| e.name == "player").unwrap().script = Some(Script::Inline {
        source: format!("{}\nfn on_tick(ctx,dt) {{ ctx.set_health(10000.0); player_tick(ctx,dt); }}",
            player_source.replace("fn on_tick(ctx,dt)", "fn player_tick(ctx,dt)")),
    });
    let soldier_source = std::fs::read_to_string(root().join("examples/open_world_shooter/scripts/soldier.rhai")).unwrap();
    let source = format!("{}\n{}", soldier_source.replace("fn on_tick(ctx,dt)", "fn soldier_tick(ctx,dt)"), r#"
fn on_tick(ctx,dt) {
    if ctx.input("hold_first") > 0.5 && ctx.id() < 103 { return; }
    if ctx.input("test_kill_first") > 0.5 && ctx.id() == 100 { ctx.damage(ctx.id(),1000.0); }
    if ctx.health() > 0.0 {
        let m=ctx.memo("ai");
        m.mode="alert"; m.sight=true; m.cooldown=0.0;
        ctx.set_memo("ai",m);
    }
    soldier_tick(ctx,dt);
}
"#);
    for (i, (position, group)) in placements.iter().enumerate() {
        let mut soldier = guard.clone();
        soldier.id = 100+i as u64;
        soldier.name = format!("guard-{i}");
        soldier.transform.position = *position;
        soldier.tags = vec!["enemy".into(), "soldier".into(), format!("grp_{group}")];
        soldier.script = Some(Script::Inline { source: source.clone() });
        scene.entities.push(soldier);
    }
    Simulation::new(scene, &root()).unwrap()
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
fn shooter_checkpoint_save_restores_mission_ammo_enemy_and_position() {
    let mut base: Scene = serde_json::from_value(read("examples/open_world_shooter/scene.json")).unwrap();
    let checkpoint = base.entities.iter().find(|e| e.name == "checkpoint-bridge").unwrap().clone();
    let bridge = base.entities.iter().find(|e| e.name == "bridge-marker").unwrap().transform.position;
    let enemy = base.entities.iter().find(|e| e.tags.iter().any(|t| t == "grp_alpha") && e.script.is_some()).unwrap().id;
    let source = std::fs::read_to_string(root().join("examples/open_world_shooter/scripts/player.rhai")).unwrap();
    let source = format!("{}\n{}", source.replace("fn on_tick(ctx,dt)", "fn player_tick(ctx,dt)"), format!(r#"
fn on_tick(ctx,dt) {{
    if ctx.input("checkpoint_test") > 0.5 {{
        if ctx.input("checkpoint_test") < 1.5 {{ ctx.set_position([{bx},{by},{bz}]); }}
        else {{ ctx.set_position([{x},{y},{z}]); }}
        ctx.damage({enemy},1000.0);
    }}
    player_tick(ctx,dt);
}}
"#, bx=bridge[0], by=bridge[1]+0.9, bz=bridge[2], x=checkpoint.transform.position[0], y=checkpoint.transform.position[1]+1.0, z=checkpoint.transform.position[2]));
    base.entities.iter_mut().find(|e| e.name == "player").unwrap().script = Some(Script::Inline { source });
    let mut sim = Simulation::new(base.clone(), &root()).unwrap();
    assert!(sim.state()["state"]["_autosave"].is_null());
    sim.step(1, &[
        Input { entity: None, tick: 0, action: "checkpoint_test".into(), value: 1.0 },
        Input { entity: None, tick: 0, action: "fire".into(), value: 1.0 },
    ]).unwrap();
    sim.step(9, &[
        Input { entity: None, tick: 1, action: "fire".into(), value: 0.0 },
    ]).unwrap();
    sim.step(1, &[Input { entity: None, tick: 10, action: "checkpoint_test".into(), value: 2.0 }]).unwrap();
    sim.step(19, &[Input { entity: None, tick: 11, action: "checkpoint_test".into(), value: 0.0 }]).unwrap();
    assert!(!sim.failed, "{}", sim.state());
    assert_eq!(sim.state()["state"]["_autosave"], 1);
    assert_eq!(sim.state()["state"]["ammo"], 29);
    assert_eq!(sim.state()["state"]["kills"], 1);
    assert_eq!(sim.state()["state"]["_stage"], 1);
    let save = sim.save_game().unwrap();
    let mut loaded = Simulation::load_game(base.clone(), &root(), save.clone()).unwrap();
    for key in ["_stage", "objective", "ammo", "ammo_reserve", "kills", "_dead_alpha", "_autosave"] {
        assert_eq!(loaded.state()["state"][key], sim.state()["state"][key], "{key}");
    }
    assert_eq!(loaded.save_game().unwrap()["combat"], save["combat"]);
    let player_pos = |s: &Simulation| s.scene().entities.iter().find(|e| e.name == "player").unwrap().transform.position;
    assert_eq!(player_pos(&loaded), player_pos(&sim));
    loaded.step(310, &[]).unwrap();
    assert!(!loaded.failed, "{}", loaded.state());
    assert_eq!(loaded.state()["state"]["_autosave"], 1);
    assert!(!loaded.scene().entities.iter().any(|e| e.id == enemy));
    let restored = Simulation::load_game(base, &root(), loaded.save_game().unwrap()).unwrap();
    assert!(!restored.scene().entities.iter().any(|e| e.id == enemy));
    assert_eq!(restored.state()["state"]["kills"], 1);
    println!("Rio Verde checkpoint: request=1, ammo=29, mission=1, enemy death/removal and position restored");
}

#[test]
fn shooter_generated_checkpoints_have_no_validation_findings() {
    let mut engine = Engine::new();
    engine.assets_dir = root().join("assets");
    engine.world.load_with_root(&root().join("examples/open_world_shooter/scene.json"), &root()).unwrap();
    let checkpoint_ids: Vec<_> = engine.world.scene.entities.iter().filter(|e| e.tags.iter().any(|t| t == "checkpoint")).map(|e| e.id).collect();
    assert_eq!(checkpoint_ids.len(), 4);
    let result = engine_mcp::handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"validate_scene","arguments":{}}}).to_string(), &mut engine).unwrap();
    let findings: Value = serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let actionable: Vec<_> = findings.as_array().unwrap().iter().filter(|f| f["entity"].as_u64().is_some_and(|id| checkpoint_ids.contains(&id))).collect();
    assert!(actionable.is_empty(), "{actionable:?}");
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
fn shooter_soldier_damage_and_accuracy_fall_with_distance() {
    let mut hits = Vec::new();
    for (distance, minimum, maximum) in [(15.0, 7.0, 7.0), (40.0, 4.6, 5.0), (80.0, 7.0/3.0, 7.0/3.0)] {
        let mut simulation = soldier_volley_simulation(&[([-2700.0, 30.0, -2700.0-distance], "town")]);
        simulation.step(30, &[]).unwrap();
        let output = simulation.take_events();
        let damage: Vec<_> = output["events"].as_array().unwrap().iter()
            .filter(|e| e["type"] == "damage" && e["target"] == 1)
            .map(|e| e["amount"].as_f64().unwrap()).collect();
        assert!(!damage.is_empty(), "distance={distance}: {output}");
        assert!(damage.iter().all(|&d| d >= minimum-1e-6 && d <= maximum+1e-6), "distance={distance}: {damage:?}");
        hits.push(damage.len());
        assert!(!simulation.failed);
    }
    assert!(hits[0] > hits[1] && hits[1] > hits[2], "{hits:?}");
    println!("Soldier falloff: hits at 15/40/80m={hits:?}; damage=7/~4.7/2.33");
}

#[test]
fn shooter_soldier_groups_limit_shooters_and_reassign_vacant_slots() {
    let mut placements: Vec<_> = (0..6).map(|i| ([-2725.0+i as f32*10.0, 30.0, -2735.0], "town")).collect();
    placements.push(([-2700.0, 30.0, -2665.0], "patrol"));
    let mut simulation = soldier_volley_simulation(&placements);
    let mut town_fired = 0;
    let mut patrol_fired = 0;
    for tick in 0..120 {
        let inputs = match tick {
            12 => vec![input(tick, "test_kill_first", 1.0)],
            13 => vec![input(tick, "test_kill_first", 0.0)],
            14 => vec![input(tick, "hold_first", 1.0)],
            _ => vec![],
        };
        simulation.step(1, &inputs).unwrap();
        let output = simulation.take_events();
        let flashes: Vec<_> = output["fx"].as_array().unwrap().iter().filter(|e| e["kind"] == "muzzle").collect();
        let town = flashes.iter().filter(|e| e["pos"][2].as_f64().unwrap() < -2700.0).count();
        let patrol = flashes.len()-town;
        assert!(town <= 3 && patrol <= 1, "tick={tick}: {output}");
        town_fired += town;
        patrol_fired += patrol;
        let state = simulation.state_summary()["state"].clone();
        let slots = state["_shooters_town"].as_array().unwrap();
        assert_eq!(slots.len(), 3, "tick={tick}: {state}");
        if tick == 0 {
            assert_eq!(slots.iter().map(|s| s["id"].as_u64().unwrap()).collect::<Vec<_>>(), [100, 101, 102]);
        }
        if tick == 12 { assert!(!slots.iter().any(|s| s["id"] == 100)); }
    }
    let state = simulation.state_summary()["state"].clone();
    let owners: Vec<_> = state["_shooters_town"].as_array().unwrap().iter().map(|s| s["id"].as_u64().unwrap()).collect();
    assert!(owners.iter().all(|id| [103, 104, 105].contains(id)), "{owners:?}");
    assert_eq!(state["_shooters_patrol"].as_array().unwrap().len(), 1);
    assert_eq!(state["_shooters_patrol"][0]["id"], 106);
    assert!(town_fired > 0 && patrol_fired > 0);
    assert!(!simulation.failed);
}

#[test]
fn shooter_death_before_first_checkpoint_returns_to_echo() {
    let mut simulation = checkpoint_simulation("examples/open_world_shooter/tests/damage.json");
    let spawn = player_position(&simulation);
    assert_eq!(simulation.state_summary()["state"]["lives"], 3);
    simulation.step(30, &[]).unwrap();
    checkpoint_death(&mut simulation, 0.0);
    let state = simulation.state_summary()["state"].clone();
    assert_player_at(&simulation, spawn);
    assert_eq!(state["lives"], 2);
    assert_eq!(state["health"], 100.0);
    assert_eq!(state["ammo"], 30);
    assert_eq!(state["ammo_reserve"], 150);
    assert_eq!(state["_reload"], 0.0);
    assert_eq!(state["_fire_cd"], 0.0);
    assert_eq!(state["_stage"], 0);
    assert_eq!(state["lost"], false);
    assert!(state["objective"].as_str().unwrap().contains("Checkpoint – Leben 2/3"));
    simulation.step(60, &[]).unwrap();
    assert_player_at(&simulation, spawn);
    assert_eq!(simulation.state_summary()["state"]["lives"], 2);
}

#[test]
fn shooter_checkpoint_respawn_keeps_progress_and_dead_enemies() {
    let mut simulation = checkpoint_simulation("examples/open_world_shooter/tests/objectives.json");
    let spawn = player_position(&simulation);
    simulation.step(20, &[
        input(0, "look_pitch", -0.035),
        input(0, "fire", 1.0),
        input(15, "fire", 0.0),
    ]).unwrap();
    assert_eq!(simulation.state_summary()["state"]["_stage"], 2);
    simulation.step(330, &[input(20, "move_x", 1.0), input(50, "move_x", 0.0)]).unwrap();
    let before = simulation.state_summary()["state"].clone();
    let checkpoint: [f32; 3] = serde_json::from_value(before["_test_checkpoint"].clone()).unwrap();
    assert!(checkpoint[0] > spawn[0]+0.5, "{checkpoint:?} == {spawn:?}");
    assert_eq!(before["_stage"], 3);
    assert_eq!(before["kills"], 1);
    assert!(!simulation.scene().entities.iter().any(|e| ["alpha-soldier", "intel"].contains(&e.name.as_str())));
    checkpoint_death(&mut simulation, 0.0);
    let state = simulation.state_summary()["state"].clone();
    assert_player_at(&simulation, checkpoint);
    for key in ["_stage", "kills", "_dead_alpha", "_tower_hp", "_commander_dead"] {
        assert_eq!(state[key], before[key], "{key}");
    }
    assert!(state["_tick"].as_i64().unwrap() > before["_tick"].as_i64().unwrap());
    assert_eq!(state["lives"], 2);
    assert_eq!(state["health"], 100.0);
    assert_eq!(state["lost"], false);
    assert!(!simulation.scene().entities.iter().any(|e| ["alpha-soldier", "intel"].contains(&e.name.as_str())));

    let tick = simulation.tick;
    simulation.step(40, &[
        input(tick, "move_x", 1.0), input(tick+30, "move_x", 0.0),
        input(tick+31, "test_destroy_tower", 1.0), input(tick+32, "test_destroy_tower", 0.0),
    ]).unwrap();
    let state = simulation.state_summary()["state"].clone();
    let next: [f32; 3] = serde_json::from_value(state["_test_checkpoint"].clone()).unwrap();
    assert_eq!(state["_stage"], 4);
    assert!(next[0] > checkpoint[0]+2.0);
    checkpoint_death(&mut simulation, 240.0);
    assert_player_at(&simulation, next);
    let state = simulation.state_summary()["state"].clone();
    assert_eq!(state["lives"], 1);
    assert_eq!(state["ammo"], 30);
    assert_eq!(state["ammo_reserve"], 240);
    assert_eq!(state["_stage"], 4);
    assert_eq!(state["kills"], 1);
    assert!(!simulation.scene().entities.iter().any(|e| e.name == "radio-tower"));
}

#[test]
fn shooter_third_death_exhausts_lives_and_still_loses() {
    let mut simulation = checkpoint_simulation("examples/open_world_shooter/tests/damage.json");
    for lives in [2, 1, 0] {
        checkpoint_death(&mut simulation, 0.0);
        let state = simulation.state_summary()["state"].clone();
        assert_eq!(state["lives"], lives);
        assert_eq!(state["lost"], lives == 0);
        assert_eq!(state["health"], if lives == 0 { 0.0 } else { 100.0 });
        assert_eq!(state["won"], false);
    }
    simulation.step(60, &[]).unwrap();
    let state = simulation.state_summary()["state"].clone();
    assert_eq!(state["lives"], 0);
    assert_eq!(state["lost"], true);
    assert_eq!(state["health"], 0.0);
    assert_eq!(state["ammo"], 0);
    assert_eq!(state["objective"], "Gefallen – Neustart");
    assert!(!simulation.failed);
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
    assert_eq!(simulation.state_summary()["state"]["lives"], 3);
    let generated = read("examples/open_world_shooter/scene.json");
    let player = generated["entities"].as_array().unwrap().iter().find(|e| e["name"] == "player").unwrap();
    assert!(player["props"]["ai_ui"]["hud_keys"].as_array().unwrap().contains(&json!("lives")));
    simulation
        .step(
            120,
            &[Input { entity: None,
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
                Input { entity: None,
                    tick: 0,
                    action: "look_pitch".into(),
                    value: (8.35_f64 / 25.0).atan(),
                },
                Input { entity: None,
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
        Input { entity: None,tick:0,action:"move_z".into(),value:-1.0},
        Input { entity: None,tick:0,action:"fire".into(),value:1.0},
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
    simulation.step(1, &[Input { entity: None,tick:0,action:"fire".into(),value:1.0}]).unwrap();
    simulation.step(1, &[
        Input { entity: None,tick:1,action:"fire".into(),value:0.0},
        Input { entity: None,tick:1,action:"reload".into(),value:1.0},
    ]).unwrap();
    assert!(simulation.state()["state"]["_reload"].as_f64().unwrap() > 0.0);
    simulation.step(121, &[Input { entity: None,tick:2,action:"reload".into(),value:0.0}]).unwrap();
    assert_eq!(simulation.state()["state"]["ammo"],30);
    assert_eq!(simulation.state()["state"]["ammo_reserve"],149);
    assert_eq!(simulation.state()["state"]["_reload"],0.0);
}

#[test]
fn shooter_travel_speed_only_scales_horizontal_movement() {
    let mut distances = Vec::new();
    for multiplier in [0.0, -2.0, 1.0, 4.0, 99.0] {
        let mut scene = read("examples/open_world_shooter/tests/combat.json");
        scene["entities"].as_array_mut().unwrap().retain(|e| e["name"] != "alpha-soldier");
        let mut simulation = Simulation::new(serde_json::from_value(scene).unwrap(), &root()).unwrap();
        simulation.step(30, &[
            Input { entity: None,tick:0,action:"move_x".into(),value:1.0},
            Input { entity: None,tick:0,action:"fire".into(),value:1.0},
            Input { entity: None,tick:0,action:"travel_speed".into(),value:multiplier},
        ]).unwrap();
        let snapshot=simulation.scene();
        let p=snapshot.entities.iter().find(|e|e.name=="player").unwrap().transform.position;
        distances.push(p[0]+2700.0);
        assert!((p[1]-30.9).abs()<0.1);
        assert_eq!(simulation.state_summary()["state"]["ammo"],25);
    }
    for i in [1,2] {assert!((distances[i]-distances[0]).abs()<0.01);}
    for i in [3,4] {assert!((distances[i]-4.0*distances[0]).abs()<0.2, "{distances:?}");}
    assert!((distances[3]-distances[4]).abs()<0.01);
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
        simulation.step(1, &[Input { entity: None,tick:240,action:"move_x".into(),value:1.0}]).unwrap();
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
                Input { entity: None,tick:300,action:action.into(),value:sign as f64},
                Input { entity: None,tick:300,action:"sprint".into(),value:1.0},
            ]).unwrap();
            let snapshot = simulation.scene();
            let p = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
            assert!(p[axis].abs() <= 3260.01, "axis={axis}, sign={sign}: escaped at {p:?}");
            simulation.step(180, &[Input { entity: None,tick:480,action:action.into(),value:-sign as f64}]).unwrap();
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
        simulation.step(1, &[Input { entity: None,tick:60,action:"force_fall".into(),value:1.0}]).unwrap();
        simulation.step(60, &[Input { entity: None,tick:61,action:"force_fall".into(),value:0.0}]).unwrap();
        let snapshot = simulation.scene();
        let after = snapshot.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
        for axis in 0..3 {
            assert!((after[axis]-before[axis]).abs() < 0.15, "terrain={with_terrain}: {before:?} -> {after:?}");
        }
        assert_eq!(simulation.state()["state"]["health"].as_f64(), Some(100.0));
        assert!(!simulation.failed, "{}", simulation.state());
    }
}
