#[allow(dead_code)]
mod support;

use anyhow::Result;
use engine_net::{
    protocol::{decode, encode},
    transport::LinkConfig,
    ClientConfig, ClientMessage, Interpolator, NetClient, ServerMessage, Snapshot, TICK_MS,
};
use engine_runtime::{Input, NetEntityState, Simulation};
use serde_json::{json, Value};
use shared::scene::Scene;
use std::collections::BTreeMap;

fn scene() -> Scene {
    serde_json::from_value(json!({"entities":[
        {"id":1,"name":"floor","kind":"box","size":[30,1,30],"transform":{"position":[0,-0.5,0]},"collider":{"shape":"box"}},
        {"id":2,"name":"controller","kind":"box","size":[1,1,1],"tags":["net_player"],"transform":{"position":[-8,1,0]},
         "script":{"source":"fn on_tick(ctx,dt) { if ctx.input(\"move_x\").abs() > 0.0 && !ctx.ragdoll_active(4) { ctx.set_ragdoll(4,true); } }"}},
        {"id":3,"name":"observer","kind":"box","size":[1,1,1],"tags":["net_player"],"transform":{"position":[8,1,0]}},
        {"id":4,"name":"target","kind":"box","size":[1,1,1],"transform":{"position":[0,3,0],"rotation_deg":[0,25,0]},
         "collider":{"shape":"box"},"rigidbody":{"type":"kinematic","mass":11},
         "props":{"ai_physics":{"ragdoll":{"preset":"humanoid"},"hitboxes":[{"name":"head","position":[0,0.75,0],"shape":{"shape":"sphere","radius":0.15}}]}}}
    ]})).unwrap()
}

fn sim() -> Simulation {
    Simulation::from_sources(scene(), BTreeMap::new()).unwrap()
}
fn activate(sim: &mut Simulation) {
    sim.step(
        1,
        &[Input {
            tick: sim.tick,
            entity: Some(2),
            action: "move_x".into(),
            value: 1.0,
        }],
    )
    .unwrap();
}
fn target(states: &[NetEntityState]) -> &NetEntityState {
    states.iter().find(|s| s.id == 4).unwrap()
}
#[track_caller]
fn close(a: &NetEntityState, b: &NetEntityState, tolerance: f32) {
    let (a, b) = (a.ragdoll.as_ref().unwrap(), b.ragdoll.as_ref().unwrap());
    assert_eq!(a.active, b.active);
    assert_eq!(a.enabled, b.enabled);
    assert_eq!(a.parts.len(), b.parts.len());
    for (a, b) in a.parts.iter().zip(&b.parts) {
        assert_eq!(a.name, b.name);
        let name = &a.name;
        for (component, (a, b)) in a
            .position
            .iter()
            .chain(&a.linvel)
            .chain(&a.angvel)
            .zip(b.position.iter().chain(&b.linvel).chain(&b.angvel))
            .enumerate()
        {
            assert!((a - b).abs() < tolerance, "bone {name} component {component}: {a} != {b}, tolerance {tolerance}");
        }
    }
}
fn head_hit(sim: &Simulation) -> Value {
    let states = sim.net_entity_states();
    let head = &target(&states).ragdoll.as_ref().unwrap().parts[2];
    sim.physics_query("raycast",&json!({"origin":[head.position[0],head.position[1],head.position[2]-2.0],"direction":[0,0,1],"distance":4})).unwrap()
}
fn snapshot(states: Vec<NetEntityState>, tick: u64) -> Snapshot {
    Snapshot {
        server_tick: tick,
        next_id: Some(5),
        last_processed_seq: None,
        input_late_ticks: 0,
        entities: states,
        spawned: vec![],
        pending_starts: vec![],
        players: vec![2, 3],
        removed: vec![],
        script_state: json!({}),
        baseline_tick: None,
    }
}

#[test]
fn rollback_across_ragdoll_activation_restores_bodies_joints_and_hitboxes() {
    let mut sim = sim();
    let before = sim.save_state();
    activate(&mut sim);
    sim.step(8, &[]).unwrap();
    let expected = sim.net_entity_states();
    let hit = head_hit(&sim);
    assert_eq!(hit["id"], 4);
    assert_eq!(hit["hitbox"], "head");
    let active = sim.save_state();
    sim.step(10, &[]).unwrap();
    let later = sim.net_entity_states();
    sim.restore_state(&active);
    sim.step(10, &[]).unwrap();
    assert_eq!(sim.net_entity_states(), later);
    sim.restore_state(&before);
    assert!(
        !target(&sim.net_entity_states())
            .ragdoll
            .as_ref()
            .unwrap()
            .active
    );
    activate(&mut sim);
    sim.step(8, &[]).unwrap();
    assert_eq!(sim.net_entity_states(), expected);
    assert_eq!(head_hit(&sim), hit);
}

#[test]
fn authoritative_ragdoll_correction_updates_hitboxes_and_rejects_invalid_bones_atomically() {
    let mut server = sim();
    activate(&mut server);
    server.step(8, &[]).unwrap();
    let authoritative = server.net_entity_states();
    let mut client = sim();
    client.apply_net_entity_states(&authoritative).unwrap();
    close(
        target(&client.net_entity_states()),
        target(&authoritative),
        1e-5,
    );
    let (a, b) = (head_hit(&server), head_hit(&client));
    assert_eq!(a["hitbox"], b["hitbox"]);
    assert_eq!(b["id"], 4);
    for i in 0..3 {
        assert!((a["point"][i].as_f64().unwrap() - b["point"][i].as_f64().unwrap()).abs() < 1e-5);
    }
    let before = client.net_entity_states();
    let mut invalid = authoritative.clone();
    invalid[0].position[0] += 100.0;
    invalid
        .iter_mut()
        .find(|s| s.id == 4)
        .unwrap()
        .ragdoll
        .as_mut()
        .unwrap()
        .parts
        .pop();
    assert!(client.apply_net_entity_states(&invalid).is_err());
    assert_eq!(client.net_entity_states(), before);
    client
        .apply_net_entity_states(&sim().net_entity_states())
        .unwrap();
    assert!(
        !target(&client.net_entity_states())
            .ragdoll
            .as_ref()
            .unwrap()
            .active
    );
    assert_eq!(client.ragdoll_states(), json!([]));
    client.apply_net_entity_states(&[]).unwrap();
}

#[test]
fn ragdoll_delta_interpolates_bones_and_bounds_extrapolation_without_early_activation() -> Result<()>
{
    let mut sim = sim();
    let inactive = snapshot(sim.net_entity_states(), 0);
    activate(&mut sim);
    let a = snapshot(sim.net_entity_states(), 2);
    let mut b = a.clone();
    b.server_tick = 4;
    let rig = b
        .entities
        .iter_mut()
        .find(|s| s.id == 4)
        .unwrap()
        .ragdoll
        .as_mut()
        .unwrap();
    for bone in &mut rig.parts {
        bone.position[0] += 2.0;
        bone.linvel = [2.0, 0.0, 0.0];
        bone.rotation_deg = [0.0, 90.0, 0.0];
        bone.angvel = [0.0; 3];
        bone.sleeping = false;
    }
    let delta = b.delta(&a, 1e-5);
    assert_eq!(delta.entities.len(), 1);
    let decoded: Snapshot = decode(&encode(&delta)?)?;
    assert_eq!(decoded.expand(Some(&a))?.entities, b.entities);
    assert!(a.delta(&a, 1e-5).entities.is_empty());
    assert_eq!(inactive.delta(&b, 1e-5).entities.len(), 1);
    let mut interp = Interpolator::default();
    interp.push(&inactive);
    interp.push(&a);
    interp.push(&b);
    assert!(!target(&interp.sample(1.0)).ragdoll.as_ref().unwrap().active);
    let mid = interp.sample(3.0);
    for (left, middle) in target(&a.entities)
        .ragdoll
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .zip(&target(&mid).ragdoll.as_ref().unwrap().parts)
    {
        assert!((middle.position[0] - left.position[0] - 1.0).abs() < 1e-5);
        assert!((middle.rotation_deg[1] - (left.rotation_deg[1] + 90.0) * 0.5).abs() < 1e-3);
    }
    let far = interp.sample(1000.0);
    let (last, far) = (
        target(&b.entities).ragdoll.as_ref().unwrap(),
        target(&far).ragdoll.as_ref().unwrap(),
    );
    for (last, far) in last.parts.iter().zip(&far.parts) {
        assert!((far.position[0] - last.position[0] - 0.5).abs() < 1e-5);
    }
    assert!(Interpolator::default().sample(0.0).is_empty());
    Ok(())
}

#[test]
fn lossy_ragdoll_reconciliation_and_late_join_preserve_activation_and_bone_hitboxes() -> Result<()>
{
    let scene = scene();
    let mut h = support::Harness::with_scene(
        scene.clone(),
        LinkConfig {
            latency_ms: 50.0,
            jitter_ms: 10.0,
            loss: 0.05,
            ..Default::default()
        },
        8,
    )?;
    let mut observed_server_only_activation = false;
    for _ in 0..600 {
        h.step(30)?;
        if target(&h.server.simulation().net_entity_states()).ragdoll.as_ref().unwrap().active {
            observed_server_only_activation |= h.clients.iter().any(|client| {
                client.entity_id() == Some(3) && !target(&client.simulation().net_entity_states()).ragdoll.as_ref().unwrap().active
            });
        }
    }
    assert!(observed_server_only_activation);
    let authoritative = h.server.simulation().net_entity_states();
    assert!(target(&authoritative).ragdoll.as_ref().unwrap().active);
    for client in &h.clients {
        let snapshot = client.latest_snapshot().unwrap();
        close(
            target(client.predicted_states_at(snapshot.server_tick).unwrap()),
            target(&snapshot.entities),
            1e-3,
        );
        let predicted = client.simulation().net_entity_states();
        for (a, b) in target(&predicted).ragdoll.as_ref().unwrap().parts.iter()
            .zip(&target(&authoritative).ragdoll.as_ref().unwrap().parts) {
            for axis in 0..3 {
                assert!((a.position[axis] - b.position[axis]).abs() < 0.02, "{} position differs across prediction lead", a.name);
            }
        }
        let (a, b) = (
            head_hit(h.server.simulation()),
            head_hit(client.simulation()),
        );
        assert_eq!(a["hitbox"], b["hitbox"]);
        assert_eq!(b["id"], 4);
        for i in 0..3 {
            assert!(
                (a["point"][i].as_f64().unwrap() - b["point"][i].as_f64().unwrap()).abs() < 0.02
            );
        }
        assert!(client.stats().corrections > 0);
    }
    h.server.disconnect(2);
    let mut late = NetClient::new(scene, BTreeMap::new(), ClientConfig::default())?;
    late.receive(
        h.server
            .receive(
                9,
                ClientMessage::Hello {
                    name: "late".into(),
                },
            )?
            .unwrap(),
        600.0 * TICK_MS,
    )?;
    let snapshot = loop {
        if let Some((_, ServerMessage::Snapshot(s))) =
            h.server.tick()?.into_iter().find(|(peer, _)| *peer == 9)
        {
            break s;
        }
    };
    let expected = target(&snapshot.entities).clone();
    late.receive(ServerMessage::Snapshot(snapshot), 601.0 * TICK_MS)?;
    close(
        target(&late.simulation().net_entity_states()),
        &expected,
        1e-4,
    );
    assert_eq!(head_hit(late.simulation())["id"], 4);
    Ok(())
}
