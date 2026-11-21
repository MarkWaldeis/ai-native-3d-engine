use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn sim(entities: Value) -> Simulation {
    Simulation::from_sources(
        serde_json::from_value(json!({"entities":entities})).unwrap(),
        BTreeMap::new(),
    )
    .unwrap()
}

fn input(tick: u64, action: &str) -> Input {
    Input {
        tick,
        action: action.into(),
        value: 1.0,
    }
}

#[test]
fn applied_force_is_cleared_after_one_tick() {
    let mut simulation = sim(json!([
        {"id":1,"name":"body","kind":"box","size":[1,1,1],
         "rigidbody":{"type":"dynamic","mass":2},"collider":{"shape":"box"},
         "script":{"source":"fn on_start(ctx) { ctx.apply_force([120.0, 0.0, 0.0]); }"}}
    ]));
    let mut positions = [0.0; 3];
    for position in &mut positions {
        simulation.step(1, &[]).unwrap();
        *position = simulation.scene().entities[0].transform.position[0];
    }
    assert!(positions[0] > 0.0);
    assert!((positions[2] - 2.0 * positions[1] + positions[0]).abs() < 1.0e-6);
}

#[test]
fn contacts_sensors_filters_and_joints_are_deterministic() {
    let scene = json!([
        {"id":1,"name":"floor","kind":"box","size":[20,1,20],"transform":{"position":[0,-0.5,0]},
         "collider":{"shape":"box"}},
        {"id":2,"name":"sensor","kind":"box","size":[2,0.1,2],"transform":{"position":[0,1.5,0]},
         "collider":{"shape":"box"},"props":{"ai_physics":{"sensor":true,"groups":{"memberships":2,"filter":4}}}},
        {"id":3,"name":"falling","kind":"sphere","radius":0.2,"segments":8,"transform":{"position":[0,3,0]},
         "collider":{"shape":"sphere"},"rigidbody":{"type":"dynamic"},
         "props":{"ai_physics":{"ccd":true,"linear_damping":0.1,"groups":{"memberships":4,"filter":4294967295u64}}},
         "script":{"source":r#"
            fn on_tick(ctx, dt) {
                for e in ctx.collision_events() {
                    ctx.state_set(e.type + "_" + e.phase, true);
                }
            }
         "#}},
        {"id":4,"name":"mismatched","kind":"sphere","radius":0.2,"segments":8,
         "transform":{"position":[0.7,3,0]},"collider":{"shape":"sphere"},"rigidbody":{"type":"dynamic"},
         "props":{"ai_physics":{"groups":{"memberships":8,"filter":4294967295u64}}}},
        {"id":5,"name":"anchor","kind":"box","size":[1,1,1],"transform":{"position":[4,3,0]},
         "rigidbody":{"type":"static"}},
        {"id":6,"name":"jointed","kind":"box","size":[0.2,0.2,0.2],"transform":{"position":[4,2,0]},
         "collider":{"shape":"box"},"rigidbody":{"type":"dynamic"},
         "props":{"ai_physics":{"joints":[{"type":"revolute","axis":[1,0,0],"limits_deg":[-20,20],
            "target":5,"anchor":[0,1,0]}]}}}
    ]);
    let (mut first, mut second) = (sim(scene.clone()), sim(scene));
    let mut events = Vec::new();
    for _ in 0..120 {
        first.step(1, &[]).unwrap();
        second.step(1, &[]).unwrap();
        let first_output = first.take_events();
        assert_eq!(first_output, second.take_events());
        assert_eq!(first.state(), second.state());
        events.extend(first_output["events"].as_array().unwrap().iter().cloned());
    }
    assert!(events.contains(&json!({"type":"sensor","phase":"started","a":2,"b":3})));
    assert!(events.contains(&json!({"type":"sensor","phase":"stopped","a":2,"b":3})));
    assert!(events.contains(&json!({"type":"contact","phase":"started","a":1,"b":3})));
    assert!(!events
        .iter()
        .any(|event| event["type"] == "sensor" && event["b"] == 4));
    assert_eq!(
        first.state()["state"],
        json!({"contact_started":true,"sensor_started":true,"sensor_stopped":true})
    );
    let scene = first.scene();
    assert!((scene.entities[2].transform.position[1] - 0.2).abs() < 0.03);
    assert!((scene.entities[5].transform.position[1] - 2.0).abs() < 0.03);
}

#[test]
fn head_hit_applies_authoritative_damage_and_impulse_and_ragdoll_can_reset() {
    let entities = json!([
        {"id":1,"name":"floor","kind":"box","size":[20,1,20],"transform":{"position":[0,-0.5,0]},
         "collider":{"shape":"box"}},
        {"id":2,"name":"target","kind":"box","size":[1,1,1],"transform":{"position":[0,2,0]},
         "collider":{"shape":"box"},"rigidbody":{"type":"kinematic","mass":11},
         "props":{"ai_health":100,"ai_physics":{"ragdoll":{"preset":"humanoid"},
            "hitboxes":[{"name":"head","position":[0,0.75,0],"shape":{"shape":"sphere","radius":0.2},"damage_multiplier":2}]}}},
        {"id":3,"name":"performer","kind":"sphere","radius":0.1,"segments":8,
         "script":{"source":r#"
            fn on_tick(ctx, dt) {
                if ctx.input("hit") > 0.0 && ctx.memo("did_hit") != true {
                    ctx.set_memo("did_hit", true);
                    let query = #{origin:[0.0, 2.75, -3.0], direction:[0.0,0.0,1.0], distance:5.0};
                    let hit = ctx.raycast(query);
                    ctx.state_set("region", hit.hitbox);
                    hit.damage_multiplier = 99.0;
                    ctx.state_set("health", ctx.damage(hit, 60.0, [0.0, 0.0, 2.0]));
                    query.shape = #{shape:"sphere",radius:0.1};
                    ctx.state_set("sweep", ctx.shape_cast(query).hit);
                }
                if ctx.input("reset") > 0.0 && ctx.ragdoll_active(2) {
                    ctx.set_ragdoll(2, false);
                }
            }
         "#}}
    ]);
    let mut simulation = sim(entities.clone());
    let mut replay = sim(entities);
    for sim in [&mut simulation, &mut replay] {
        sim.step(1, &[input(0, "hit")]).unwrap();
        assert_eq!(
            sim.state()["state"],
            json!({"region":"head","health":0.0,"sweep":false})
        );
        assert_eq!(
            sim.ragdoll_states()[0]["parts"].as_array().unwrap().len(),
            11
        );
        assert!(sim.take_events()["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == "damage"
                && event["hitbox"] == "head"
                && event["amount"] == 120.0));
        sim.step(20, &[]).unwrap();
    }
    assert_eq!(simulation.ragdoll_states(), replay.ragdoll_states());
    let ragdoll = simulation.ragdoll_states();
    let head = &ragdoll[0]["parts"][2];
    assert_eq!(head["name"], "head");
    assert!(head["position"][2].as_f64().unwrap() > 0.05, "{head}");
    simulation.step(1, &[input(21, "reset")]).unwrap();
    assert_eq!(simulation.ragdoll_states(), json!([]));
}
