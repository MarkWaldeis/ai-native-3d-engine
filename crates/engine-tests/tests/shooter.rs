use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn inline(source: &str) -> Value {
    json!({ "source": source })
}

fn sim(entities: Value) -> Simulation {
    Simulation::new(serde_json::from_value(json!({ "entities": entities })).unwrap(), &root()).unwrap()
}

fn floor() -> Value {
    json!({"id":1,"name":"floor","kind":"box","size":[100,1,100],"transform":{"position":[0,-0.5,0]},"collider":{"shape":"box"},"rigidbody":{"type":"static"}})
}

fn input(tick: u64, action: &str, value: f64) -> Input {
    Input { entity: None, tick, action: action.into(), value }
}

#[test]
fn raycast_excludes_caller_and_reports_hit_fields() {
    let mut simulation = sim(json!([
        floor(),
        {"id":2,"name":"shooter","kind":"box","size":[1,1,1],"transform":{"position":[0,1,0]},"collider":{"shape":"box"},"rigidbody":{"type":"static"},
         "script":inline(r#"fn on_tick(ctx, dt) {
            let r = ctx.raycast([0.0, 1.0, 0.0], [0.0, 0.0, -2.0], 50.0);
            ctx.state_set("hit", r.hit); ctx.state_set("id", r.id); ctx.state_set("d", r.distance);
            ctx.state_set("point", r.point); ctx.state_set("normal", r.normal);
            let miss = ctx.raycast([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], 50.0);
            ctx.state_set("miss", miss.hit);
            let down = ctx.raycast([0.0, 1.0, 0.0], [0.0, -1.0, 0.0], 5.0);
            ctx.state_set("down", down.id);
         }"#)},
        {"id":3,"name":"target","kind":"box","size":[2,2,1],"transform":{"position":[0,1,-10]},"collider":{"shape":"box"},"rigidbody":{"type":"static"},"tags":["enemy"]}
    ]));
    simulation.step(1, &[]).unwrap();
    let state = simulation.state()["state"].clone();
    assert_eq!(state["hit"], true, "{state}");
    assert_eq!(state["id"], 3);
    assert!((state["d"].as_f64().unwrap() - 9.5).abs() < 1e-3, "{state}");
    assert!((state["point"][2].as_f64().unwrap() + 9.5).abs() < 1e-3);
    assert!((state["normal"][2].as_f64().unwrap() - 1.0).abs() < 1e-3);
    assert_eq!(state["miss"], false);
    assert_eq!(state["down"], 1, "caller box is excluded, floor is hit");
}

#[test]
fn damage_tracks_health_kills_events_and_player_state() {
    let mut simulation = sim(json!([
        floor(),
        {"id":2,"name":"player","kind":"sphere","radius":0.4,"segments":8,"transform":{"position":[0,1,0]},"tags":["player"],
         "script":inline(r#"fn on_start(ctx) { ctx.state_set("health", 50.0); }
            fn on_tick(ctx, dt) {
                if ctx.input("fire") > 0.5 {
                    ctx.state_set("left", ctx.damage(3, 60.0));
                    ctx.state_set("wall", ctx.damage(1, 10.0));
                    ctx.emit_fx("muzzle", ctx.position(), ctx.aim_dir());
                    ctx.emit_fx("impact", [1.0, 2.0, 3.0]);
                }
            }"#)},
        {"id":3,"name":"grunt","kind":"sphere","radius":0.4,"segments":8,"transform":{"position":[0,1,-5]},"tags":["enemy"],"props":{"ai_health":100},
         "script":inline(r#"fn on_tick(ctx, dt) {
                for hit in ctx.hits() { ctx.state_set("hit_by", hit.source); ctx.damage(hit.source, 20.0); }
                ctx.state_set("grunt", ctx.health());
            }"#)}
    ]));
    simulation.step(1, &[input(0, "fire", 1.0)]).unwrap();
    let out = simulation.take_events();
    let fx = out["fx"].as_array().unwrap();
    assert_eq!(fx.len(), 2);
    assert_eq!(fx[0]["kind"], "muzzle");
    assert!((fx[0]["dir"][2].as_f64().unwrap() + 1.0).abs() < 1e-9, "aim_dir at yaw 0 looks along -Z");
    assert_eq!(fx[1]["dir"], json!([0.0, 1.0, 0.0]));
    let events = out["events"].as_array().unwrap();
    assert_eq!(events[0]["type"], "damage");
    assert_eq!(events[0]["by_player"], true);
    simulation.step(1, &[input(1, "fire", 1.0)]).unwrap();
    let state = simulation.state()["state"].clone();
    assert_eq!(state["wall"], -1.0, "static walls are not damageable");
    assert_eq!(state["hit_by"], 2);
    // 100 - 60 = 40, second shot kills, grunt answered each hit with 20 damage (player 50 -> 10).
    assert_eq!(state["left"], 0.0);
    assert_eq!(state["health"], 10.0, "{state}");
    let events = simulation.take_events()["events"].clone();
    assert!(events.as_array().unwrap().iter().any(|e| e["type"] == "kill" && e["target_name"] == "grunt"), "{events}");
    simulation.step(1, &[input(2, "fire", 1.0)]).unwrap();
    assert_eq!(simulation.state()["state"]["left"], -1.0, "dead targets ignore damage");
}

#[test]
fn emit_fx_rejects_unknown_kinds() {
    let mut simulation = sim(json!([
        {"id":1,"name":"bad","kind":"sphere","radius":0.4,"segments":8,"transform":{"position":[0,1,0]},
         "script":inline(r#"fn on_tick(ctx, dt) { ctx.emit_fx("sparkle", [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]); }"#)}
    ]));
    let error = simulation.step(1, &[]).unwrap_err().to_string();
    assert!(error.contains("unknown fx kind") && error.contains("explosion"), "{error}");
}

#[test]
fn steering_routes_around_a_wall_and_line_of_sight_is_blocked() {
    let mut simulation = sim(json!([
        floor(),
        {"id":2,"name":"wall","kind":"box","size":[4,3,1],"transform":{"position":[0,1.5,-4]},"collider":{"shape":"box"},"rigidbody":{"type":"static"}},
        {"id":3,"name":"walker","kind":"sphere","radius":0.4,"segments":8,"transform":{"position":[0,1,0]},
         "collider":{"shape":"sphere","radius":0.4},"rigidbody":{"type":"dynamic","mass":1,"friction":0},"props":{"ai_lock_rotation":true},
         "script":inline(r#"fn on_tick(ctx, dt) {
                if ctx.memo("los") == () { ctx.set_memo("los", ctx.can_see(4, 50.0)); ctx.state_set("los", ctx.memo("los")); }
                ctx.set_velocity(ctx.steer(ctx.entity_position(4), 3.0));
            }"#)},
        {"id":4,"name":"goal","kind":"sphere","radius":0.3,"segments":8,"transform":{"position":[0,1,-10]},"tags":["enemy"],"collider":{"shape":"sphere","radius":0.3},"rigidbody":{"type":"static"}}
    ]));
    simulation.step(600, &[]).unwrap();
    assert_eq!(simulation.state()["state"]["los"], false);
    let walker = simulation.scene().entities.into_iter().find(|e| e.id == 3).unwrap();
    assert!(walker.transform.position[2] < -6.0, "walker passed the wall: {:?}", walker.transform.position);
}

#[test]
fn fps_arena_player_shoots_reloads_and_enemies_fight_back() {
    let scene = serde_json::from_str(&std::fs::read_to_string(root().join("examples/fps_arena/scene.json")).unwrap()).unwrap();
    let mut simulation = Simulation::new(scene, &root()).unwrap();
    let ids: Vec<u64> = simulation.scene().entities.iter().filter(|e| e.tags.iter().any(|t| t == "enemy")).map(|e| e.id).collect();
    let aim = |simulation: &Simulation, tick: u64| -> Vec<Input> {
        let scene = simulation.scene();
        let player = scene.entities.iter().find(|e| e.name == "player").unwrap().transform.position;
        let eye = [player[0] as f64, player[1] as f64 + 0.75, player[2] as f64];
        let target = scene.entities.iter().filter(|e| e.tags.iter().any(|t| t == "enemy")).map(|e| e.transform.position)
            .min_by(|a, b| {
                let d = |p: &[f32; 3]| (p[0] as f64 - eye[0]).powi(2) + (p[2] as f64 - eye[2]).powi(2);
                d(a).total_cmp(&d(b))
            }).unwrap();
        let (dx, dy, dz) = (target[0] as f64 - eye[0], target[1] as f64 - eye[1], target[2] as f64 - eye[2]);
        vec![input(tick, "look_yaw", (-dx).atan2(-dz)), input(tick, "look_pitch", dy.atan2((dx * dx + dz * dz).sqrt()))]
    };
    let spawn = simulation.scene().entities.iter().find(|e| e.name == "player").unwrap().transform.position;
    simulation.set_active_region(spawn[0], spawn[2], 30.0, 4.0).unwrap();
    let mut first = aim(&simulation, 0);
    first.push(input(0, "fire", 1.0));
    simulation.step(1, &first).unwrap();
    let far = simulation.scene().entities.iter().filter(|e| ids.contains(&e.id) && !simulation.entity_active(e.id)).count();
    assert!(far > 0, "distant enemies are paused by the simulation region");
    for tick in 1..60 {
        let mut inputs = aim(&simulation, tick);
        if tick == 59 { inputs.push(input(tick, "fire", 0.0)); }
        simulation.step(1, &inputs).unwrap();
    }
    simulation.step(400, &[input(60, "reload", 1.0), input(61, "reload", 0.0)]).unwrap();
    let state = simulation.state()["state"].clone();
    assert!(state["kills"].as_f64().unwrap() >= 1.0, "{state}");
    assert_eq!(state["ammo"], 30.0, "reloaded after firing: {state}");
    assert!(state["ammo_reserve"].as_f64().unwrap() < 120.0);
    assert!(state["health"].as_f64().unwrap() < 100.0, "nearby enemies return fire: {state}");
    assert!(simulation.scene().entities.iter().filter(|e| ids.contains(&e.id)).count() < ids.len(), "dead enemy removed itself");
}

#[test]
fn raycasts_see_colliders_in_on_start_and_respect_region_changes_immediately() {
    let mut simulation = sim(json!([
        floor(),
        {"id":2,"name":"player","kind":"box","size":[1,1,1],"transform":{"position":[0,1,0]},"tags":["player"],
         "script":inline(r#"fn on_start(ctx) { ctx.state_set("start", ctx.raycast([0.0, 1.0, 0.0], [0.0, 0.0, -1.0], 50.0).id); }
            fn on_tick(ctx, dt) { ctx.state_set("far", ctx.raycast([0.0, 1.0, 0.0], [0.0, 0.0, -1.0], 400.0).hit); }"#)},
        {"id":3,"name":"wall","kind":"box","size":[4,4,1],"transform":{"position":[0,1,-8]},"collider":{"shape":"box"},"rigidbody":{"type":"static"}}
    ]));
    assert_eq!(simulation.state()["state"]["start"], 3, "on_start ray hits the wall");

    let mut simulation = sim(json!([
        {"id":2,"name":"player","kind":"box","size":[1,1,1],"transform":{"position":[0,1,0]},"tags":["player"],
         "script":inline(r#"fn on_tick(ctx, dt) { ctx.state_set("moved", ctx.raycast([0.0, 1.0, 0.0], [0.0, 0.0, -1.0], 50.0).hit); }"#)},
        {"id":3,"name":"wall","kind":"box","size":[4,4,1],"transform":{"position":[0,1,-8]},"collider":{"shape":"box"},"rigidbody":{"type":"static"},
         "script":inline(r#"fn on_start(ctx) { ctx.set_position([20.0, 1.0, -8.0]); }"#)}
    ]));
    simulation.step(1, &[]).unwrap();
    assert_eq!(simulation.state()["state"]["moved"], false, "wall moved in on_start is no longer hit");

    let mut simulation = sim(json!([
        {"id":2,"name":"player","kind":"box","size":[1,1,1],"transform":{"position":[0,1,0]},"tags":["player"],
         "script":inline(r#"fn on_tick(ctx, dt) { ctx.state_set("far", ctx.raycast([0.0, 1.0, 0.0], [0.0, 0.0, -1.0], 400.0).hit); }"#)},
        {"id":3,"name":"far","kind":"box","size":[4,4,1],"transform":{"position":[0,1,-300]},"collider":{"shape":"box"},"rigidbody":{"type":"dynamic"}}
    ]));
    simulation.set_active_region(0.0, 0.0, 50.0, 0.0).unwrap();
    simulation.step(1, &[]).unwrap();
    assert_eq!(simulation.state()["state"]["far"], false, "body paused this tick is not hit");
}

#[test]
fn fps_arena_player_script_uses_defaults_for_empty_ai_fps() {
    let mut scene: Value = serde_json::from_str(&std::fs::read_to_string(root().join("examples/fps_arena/scene.json")).unwrap()).unwrap();
    for entity in scene["entities"].as_array_mut().unwrap() {
        if entity["tags"].as_array().is_some_and(|t| t.iter().any(|t| t == "player")) { entity["props"]["ai_fps"] = json!({}); }
    }
    let mut simulation = Simulation::new(serde_json::from_value(scene).unwrap(), &root()).unwrap();
    simulation.step(30, &[input(0, "move_z", -1.0), input(0, "fire", 1.0), input(0, "sprint", 1.0)]).unwrap();
    assert_eq!(simulation.state()["failed"], false);
    assert!(simulation.state()["state"]["ammo"].as_f64().unwrap() < 30.0);
}
