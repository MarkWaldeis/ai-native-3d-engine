use engine_runtime::{Input, Simulation};
use serde_json::json;
use shared::scene::Scene;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn frictionless_character_falls_along_a_wall_instead_of_sticking() {
    fn run(rule: &str) -> f32 {
        let scene: Scene = serde_json::from_value(json!({"entities":[
            {"id":1,"name":"wall","kind":"box","size":[0.2,20,10],"transform":{"position":[0.75,0,0]},"collider":{"shape":"box"},"rigidbody":{"type":"static","friction":0.7}},
            {"id":2,"name":"player","kind":"sphere","radius":0.35,"segments":12,"transform":{"position":[0,3,0]},"collider":{"shape":"capsule","radius":0.35,"half_height":0.25},"rigidbody":{"type":"dynamic","friction":0},"props":{"ai_lock_rotation":true,"ai_friction_combine":rule},"script":{"source":"fn on_tick(ctx,dt){let v=ctx.velocity();v[0]=3.5;ctx.set_velocity(v);}"}}
        ]})).unwrap();
        let mut sim = Simulation::new(scene, &root()).unwrap();
        sim.step(120, &[]).unwrap();
        sim.scene().entities[1].transform.position[1]
    }
    assert!(
        run("average") > 2.0,
        "counterfactual must reproduce the wall trap"
    );
    assert!(
        run("min") < -5.0,
        "zero-friction character must fall while pressing against a wall"
    );
    let mut invalid = controller(0.62, 100.0).scene();
    invalid.entities[1].props["ai_friction_combine"] = json!("invalid");
    let error = Simulation::new(invalid, &root()).err().unwrap().to_string();
    assert!(error.contains("ai_friction_combine must be"));
}

fn controller(height: f32, floor_width: f32) -> Simulation {
    let source =
        std::fs::read_to_string(root().join("examples/sky_garden/scripts/player.rhai")).unwrap();
    let scene: Scene = serde_json::from_value(json!({"entities":[
        {"id":1,"name":"floor","kind":"box","size":[floor_width,1,20],"transform":{"position":[0,-0.5,0]},"collider":{"shape":"box"}},
        {"id":2,"name":"Milo","kind":"sphere","radius":0.35,"segments":12,"transform":{"position":[0,height,0]},"collider":{"shape":"capsule","radius":0.35,"half_height":0.25},"rigidbody":{"type":"dynamic","friction":0},"props":{"ai_lock_rotation":true,"ai_friction_combine":"min"},"script":{"source":source}},
        {"id":3,"name":"Portal","kind":"box","size":[1,1,1],"transform":{"position":[100,0,0]}}
    ]})).unwrap();
    Simulation::new(scene, &root()).unwrap()
}

#[test]
fn prelanding_tap_is_buffered_and_holding_does_not_autojump() {
    let mut sim = controller(3.0, 100.0);
    while sim.scene().entities[1].transform.position[1] > 0.95 {
        sim.step(1, &[]).unwrap();
    }
    assert_eq!(sim.state()["state"]["_coyote"], 0);
    let tick = sim.tick;
    sim.step(
        100,
        &[
            Input {
                tick,
                action: "action".into(),
                value: 1.0,
            },
            Input {
                tick: tick + 1,
                action: "action".into(),
                value: 0.0,
            },
        ],
    )
    .unwrap();
    assert_eq!(sim.state()["state"]["Sprünge"], 1);
    let mut held = controller(0.62, 100.0);
    held.step(12, &[]).unwrap();
    held.step(
        130,
        &[Input {
            tick: 12,
            action: "action".into(),
            value: 1.0,
        }],
    )
    .unwrap();
    assert_eq!(held.state()["state"]["Sprünge"], 1);
}

#[test]
fn edge_grace_and_acceleration_allow_a_late_touch_jump() {
    let mut sim = controller(0.62, 4.0);
    sim.step(
        1,
        &[Input {
            tick: 0,
            action: "move_x".into(),
            value: 1.0,
        }],
    )
    .unwrap();
    let first_x = sim.scene().entities[1].transform.position[0];
    assert!(first_x > 0.0 && first_x < 3.2 / 60.0);
    for _ in 0..150 {
        sim.step(1, &[]).unwrap();
        if sim.state()["state"]["_coyote"] == 2 {
            break;
        }
    }
    assert_eq!(sim.state()["state"]["_coyote"], 2);
    sim.step(
        12,
        &[Input {
            tick: sim.tick,
            action: "action".into(),
            value: 1.0,
        }],
    )
    .unwrap();
    assert_eq!(sim.state()["state"]["Sprünge"], 1);
    assert!(sim.scene().entities[1].transform.position[1] > 0.6);
}
