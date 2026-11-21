use engine_runtime::{Input, Simulation};
use serde_json::json;
use std::collections::BTreeMap;

fn simulation() -> Simulation {
    let scene = serde_json::from_value(json!({"entities":[
        {"id":1,"name":"floor","kind":"box","size":[20,1,20],
         "transform":{"position":[0,-0.5,0]},"collider":{"shape":"box"},"rigidbody":{"type":"static"}},
        {"id":2,"name":"ball","kind":"sphere","radius":0.5,"segments":16,
         "transform":{"position":[0,1.1,0]},"collider":{"shape":"sphere","radius":0.5},
         "rigidbody":{"type":"dynamic"}, "script":{"source":r#"
            fn on_tick(ctx, dt) {
                let count = ctx.state_get("count");
                if count == () { count = 0; }
                ctx.state_set("count", count + 1);
                ctx.apply_impulse([ctx.input("push") * 0.01, 0.0, 0.0]);
                if ctx.input("spawn") > 0.0 {
                    ctx.remove(3);
                    ctx.spawn_entity(#{kind:"box",size:[0.5,0.5,0.5],
                        transform:#{position:[0.0,3.0,0.0]},
                        collider:#{shape:"box"},rigidbody:#{type:"dynamic"}});
                }
            }
         "#}},
        {"id":3,"name":"box","kind":"box","size":[1,1,1],
         "transform":{"position":[0,2.3,0]},"collider":{"shape":"box"},"rigidbody":{"type":"dynamic"}}
    ]})).unwrap();
    Simulation::from_sources(scene, BTreeMap::new()).unwrap()
}

fn input(tick: u64, entity: Option<u64>, action: &str, value: f64) -> Input {
    Input { tick, entity, action: action.into(), value }
}

#[test]
fn rollback_replays_collisions_script_state_and_entity_lifecycle_bit_exactly() {
    let mut sim = simulation();
    sim.step(30, &[]).unwrap();
    let saved = sim.save_state();
    let inputs = [input(30, Some(2), "push", 1.0), input(40, None, "spawn", 1.0),
        input(41, None, "spawn", 0.0)];
    let mut expected = Vec::new();
    for tick in 30..150 {
        let events: Vec<_> = inputs.iter().filter(|i| i.tick == tick).cloned().collect();
        sim.step(1, &events).unwrap();
        expected.push((sim.net_entity_states(), sim.net_script_state()));
    }
    assert!(!sim.scene().entities.iter().any(|e| e.id == 3));
    assert!(sim.scene().entities.iter().any(|e| e.id == 4));
    sim.restore_state(&saved);
    assert_eq!(sim.tick, 30);
    assert!(sim.scene().entities.iter().any(|e| e.id == 3));
    for (offset, (states, script)) in expected.into_iter().enumerate() {
        let tick = 30 + offset as u64;
        let events: Vec<_> = inputs.iter().filter(|i| i.tick == tick).cloned().collect();
        sim.step(1, &events).unwrap();
        let actual = sim.net_entity_states();
        for (a, b) in actual.iter().zip(&states) {
            assert_eq!(a.id, b.id);
            let bits = |s: &engine_runtime::NetEntityState| s.position.into_iter()
                .chain(s.rotation_deg).chain(s.linvel).chain(s.angvel).map(f32::to_bits).collect::<Vec<_>>();
            assert_eq!(bits(a), bits(b), "tick {tick}");
        }
        assert_eq!(actual.len(), states.len());
        assert_eq!(sim.net_script_state(), script);
    }
}

#[test]
fn rollback_and_corrections_preserve_active_ragdoll_physics() {
    let scene = serde_json::from_value(json!({"entities":[
        {"id":1,"name":"target","kind":"box","size":[1,1,1],
         "transform":{"position":[0,3,0]},"rigidbody":{"type":"kinematic"},
         "props":{"ai_physics":{"ragdoll":{"preset":"humanoid","start_active":true}}}}
    ]})).unwrap();
    let mut sim = Simulation::from_sources(scene, BTreeMap::new()).unwrap();
    sim.step(2, &[]).unwrap();
    let checkpoint = sim.save_state();
    let checkpoint_pose = sim.ragdoll_states();
    assert!(sim.net_entity_states()[0].linvel[1] < 0.0);
    let mut corrections = sim.net_entity_states();
    corrections[0].position[0] += 4.0;
    sim.apply_net_entity_states(&corrections);
    let corrected = sim.ragdoll_states();
    for (before, after) in checkpoint_pose[0]["parts"].as_array().unwrap().iter()
        .zip(corrected[0]["parts"].as_array().unwrap()) {
        let delta = after["position"][0].as_f64().unwrap() - before["position"][0].as_f64().unwrap();
        assert!((delta - 4.0).abs() < 1e-5, "{before} -> {after}");
    }
    sim.restore_state(&checkpoint);
    assert_eq!(sim.ragdoll_states(), checkpoint_pose);
    sim.step(4, &[]).unwrap();
    let expected = sim.ragdoll_states();
    sim.restore_state(&checkpoint);
    sim.step(4, &[]).unwrap();
    assert_eq!(sim.ragdoll_states(), expected);
}

#[test]
fn scoped_inputs_override_globals_and_helpers_use_hook_entity() {
    let source = r#"fn on_tick(ctx, dt) {
        ctx.state_set("input" + ctx.id().to_string(), ctx.input("move_x"));
        ctx.state_set("move" + ctx.id().to_string(), ctx.move_dir());
        ctx.state_set("aim" + ctx.id().to_string(), ctx.aim_dir());
    }"#;
    let entities: Vec<_> = (1..=2).map(|id| json!({"id":id,"name":format!("p{id}"),
        "kind":"box","size":[1,1,1],"script":{"source":source}})).collect();
    let mut sim = Simulation::from_sources(serde_json::from_value(json!({"entities":entities})).unwrap(), BTreeMap::new()).unwrap();
    sim.step(1, &[input(0,None,"move_x",1.0), input(0,Some(1),"move_x",0.0),
        input(0,Some(1),"look_yaw",1.0)]).unwrap();
    let state = sim.net_script_state();
    assert_eq!(state["input1"], 0.0);
    assert_eq!(state["input2"], 1.0);
    assert_eq!(state["move1"], json!([0.0,0.0,0.0]));
    assert_eq!(state["move2"], json!([1.0,0.0,0.0]));
    assert_ne!(state["aim1"], state["aim2"]);
    assert!(serde_json::from_value::<Input>(json!({"tick":0,"action":"a","value":0})).unwrap().entity.is_none());
    assert!(serde_json::from_value::<Input>(json!({"tick":0,"action":"a","value":0,"typo":1})).is_err());
}

#[test]
fn network_corrections_update_bodies_velocities_and_script_map() {
    let mut sim = simulation();
    sim.apply_net_entity_states(&[]);
    let mut states = sim.net_entity_states();
    assert_eq!(states.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1,2,3]);
    let ball = &mut states[1];
    ball.position = [5.0,5.0,0.0];
    ball.rotation_deg = [10.0,20.0,30.0];
    ball.linvel = [2.0,0.0,0.0];
    ball.angvel = [0.0,1.0,0.0];
    sim.apply_net_entity_states(&states);
    assert_eq!(sim.net_entity_states(), states);
    let mut unknown = states[1].clone();
    unknown.id = 999;
    sim.apply_net_entity_states(&[unknown]);
    assert_eq!(sim.net_entity_states(), states);
    sim.apply_net_script_state(json!({"count":40,"server":true}));
    sim.apply_net_script_state(json!(null));
    assert_eq!(sim.net_script_state(), json!({"count":40,"server":true}));
    sim.step(1, &[]).unwrap();
    assert!(sim.net_entity_states()[1].position[0] > 5.0);
    assert_eq!(sim.net_script_state()["count"], 41);
    sim.apply_net_script_state(json!({}));
    assert_eq!(sim.net_script_state(), json!({}));
}
