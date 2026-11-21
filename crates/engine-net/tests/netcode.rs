mod support;
use anyhow::Result;
use engine_net::{
    protocol::{decode, encode},
    transport::{LinkConfig, SimNetwork, Transport},
    ClientConfig, ClientMessage, InputFrame, Interpolator, NetClient, NetServer, ServerConfig,
    ServerMessage, Snapshot,
};
use engine_runtime::NetEntityState;
use serde_json::json;
use std::collections::BTreeMap;

fn join(server: &mut NetServer, client: &mut NetClient, peer: u64) -> Result<()> {
    client.receive(
        server
            .receive(
                peer,
                ClientMessage::Hello {
                    name: format!("p{peer}"),
                },
            )?
            .unwrap(),
        0.0,
    )
}

#[test]
fn zero_latency_prediction_is_bit_exact_without_corrections() -> Result<()> {
    let mut server = NetServer::new(support::scene(), BTreeMap::new(), ServerConfig::default())?;
    let config = ClientConfig {
        initial_lead_ticks: 0,
        max_lead_ticks: 0,
        jitter_buffer_ticks: 0,
        ..ClientConfig::default()
    };
    let mut client = NetClient::new(support::scene(), BTreeMap::new(), config)?;
    join(&mut server, &mut client, 1)?;
    for tick in 0..240 {
        let message = client.tick(
            support::actions(0, tick, 120),
            tick as f64 * engine_net::TICK_MS,
        )?;
        server.receive(1, message)?;
        let snapshots = server.tick()?;
        let a = client.simulation().net_entity_states();
        let b = server.simulation().net_entity_states();
        assert_bits(&a, &b);
        assert_eq!(
            client.simulation().net_script_state(),
            server.simulation().net_script_state()
        );
        for (_, snapshot) in snapshots {
            client.receive(snapshot, tick as f64 * engine_net::TICK_MS)?;
        }
    }
    assert_eq!(client.stats().corrections, 0);
    Ok(())
}

fn assert_bits(a: &[NetEntityState], b: &[NetEntityState]) {
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(b) {
        assert_eq!(a.id, b.id);
        let bits = |s: &NetEntityState| {
            s.position
                .iter()
                .chain(&s.rotation_deg)
                .chain(&s.linvel)
                .chain(&s.angvel)
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(bits(a), bits(b));
    }
}

#[test]
fn lossy_two_client_prediction_converges_with_bounded_corrections() -> Result<()> {
    let mut run = support::Harness::new(
        LinkConfig {
            latency_ms: 50.0,
            jitter_ms: 20.0,
            loss: 0.05,
            duplication: 0.03,
            reorder_ms: 10.0,
        },
        6,
    )?;
    for _ in 0..720 {
        run.step(360)?;
    }
    assert!(run.network.dropped_packets() > 0);
    assert!(run.snapshots_received > 500);
    for client in &run.clients {
        let own = client.entity_id().unwrap();
        let snapshot = client.latest_snapshot().unwrap();
        let predicted = client
            .predicted_states_at(snapshot.server_tick)
            .unwrap()
            .iter()
            .find(|s| s.id == own)
            .unwrap();
        let authority = snapshot.entities.iter().find(|s| s.id == own).unwrap();
        assert!(
            distance(predicted.position, authority.position) <= 1e-3,
            "matching snapshot: {:?} {:?}",
            predicted,
            authority
        );
        let current = client
            .simulation()
            .net_entity_states()
            .into_iter()
            .find(|s| s.id == own)
            .unwrap();
        let final_server = run
            .server
            .simulation()
            .net_entity_states()
            .into_iter()
            .find(|s| s.id == own)
            .unwrap();
        assert!(
            distance(current.position, final_server.position) <= 1e-3,
            "settled client {:?}, server {:?}, stats {:?}",
            current,
            final_server,
            client.stats()
        );
        assert!(
            client.stats().corrections > 0 && client.stats().corrections < 350,
            "{:?}",
            client.stats()
        );
        assert!((1..=30).contains(&client.stats().lead_ticks));
    }
    Ok(())
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f32>()
        .sqrt()
}

#[test]
fn reconciliation_replays_history_smooths_visual_error_and_recovers_baselines() -> Result<()> {
    let mut server = NetServer::new(support::scene(), BTreeMap::new(), ServerConfig::default())?;
    let mut client = NetClient::new(support::scene(), BTreeMap::new(), ClientConfig::default())?;
    join(&mut server, &mut client, 1)?;
    assert_eq!(client.client_id(), Some(1));
    // Deliberately omit the predicted movement on the server, then correct tick 2
    // while the client is already at tick 5.
    for tick in 0..5 {
        client.tick(support::actions(0, tick, 10), tick as f64 * 17.0)?;
    }
    server.tick()?;
    let (_, message) = server.tick()?.pop().unwrap();
    let before = client
        .render_states(0.0)
        .into_iter()
        .find(|s| s.id == 2)
        .unwrap()
        .position;
    client.receive(message, 100.0)?;
    assert_eq!(client.simulation().tick, 5);
    assert_eq!(client.stats().corrections, 1);
    let rendered = client
        .render_states(0.0)
        .into_iter()
        .find(|s| s.id == 2)
        .unwrap()
        .position;
    assert!(
        distance(before, rendered) < 1e-6,
        "visual correction should not snap"
    );
    let corrected = client
        .simulation()
        .net_entity_states()
        .into_iter()
        .find(|s| s.id == 2)
        .unwrap()
        .position;
    assert!(distance(before, corrected) > 1e-3);
    let settled = client
        .render_states(6.0)
        .into_iter()
        .find(|s| s.id == 2)
        .unwrap()
        .position;
    assert!(
        distance(settled, corrected) < 1e-6,
        "offset must decay within 100 ms"
    );
    // A missing delta baseline clears the ACK and elicits a new full snapshot.
    let mut invalid = client.latest_snapshot().unwrap().clone();
    invalid.server_tick = 4;
    invalid.baseline_tick = Some(999);
    client.receive(ServerMessage::Snapshot(invalid), 110.0)?;
    let ClientMessage::Input {
        ack_snapshot_tick, ..
    } = client.input_message()
    else {
        panic!("input expected");
    };
    assert_eq!(ack_snapshot_tick, None);
    server.receive(1, client.input_message())?;
    server.tick()?;
    let (_, full) = server.tick()?.pop().unwrap();
    let ServerMessage::Snapshot(full) = full else {
        panic!("snapshot expected");
    };
    assert_eq!(full.baseline_tick, None);
    client.receive(ServerMessage::Snapshot(full), 120.0)?;
    assert_eq!(client.latest_snapshot().unwrap().server_tick, 4);
    Ok(())
}

#[test]
fn reconciliation_restores_predicted_pickups_and_ignores_unknown_entities() -> Result<()> {
    let mut scene = support::scene();
    scene
        .entities
        .iter_mut()
        .find(|e| e.id == 2)
        .unwrap()
        .script = Some(shared::components::Script::Inline {
        source: "fn on_tick(ctx, dt) { if ctx.input(\"pickup\") > 0.0 { ctx.remove(4); } }".into(),
    });
    let mut server = NetServer::new(scene.clone(), BTreeMap::new(), ServerConfig::default())?;
    let mut client = NetClient::new(scene, BTreeMap::new(), ClientConfig::default())?;
    join(&mut server, &mut client, 1)?;
    for tick in 0..2 {
        client.tick(BTreeMap::from([("pickup".into(), 1.0)]), tick as f64 * 17.0)?;
        let messages = server.tick()?;
        if tick == 1 {
            assert!(!client
                .simulation()
                .scene()
                .entities
                .iter()
                .any(|e| e.id == 4));
            let (_, ServerMessage::Snapshot(mut snapshot)) = messages.into_iter().next().unwrap()
            else {
                panic!("snapshot expected");
            };
            let mut unknown = snapshot.entities[0].clone();
            unknown.id = 999;
            snapshot.entities.push(unknown);
            client.receive(ServerMessage::Snapshot(snapshot), 34.0)?;
        }
    }
    assert_bits(
        &client.simulation().net_entity_states(),
        &server.simulation().net_entity_states(),
    );
    assert_eq!(client.stats().corrections, 1);
    Ok(())
}

#[test]
fn invalid_inputs_configs_and_removed_entities_have_explicit_behavior() -> Result<()> {
    assert!(NetServer::new(
        support::scene(),
        BTreeMap::new(),
        ServerConfig {
            snapshot_interval: 0,
            ..ServerConfig::default()
        }
    )
    .is_err());
    assert!(NetClient::new(
        support::scene(),
        BTreeMap::new(),
        ClientConfig {
            position_epsilon: f32::NAN,
            ..ClientConfig::default()
        }
    )
    .is_err());
    let mut server = NetServer::new(support::scene(), BTreeMap::new(), ServerConfig::default())?;
    let mut client = NetClient::new(support::scene(), BTreeMap::new(), ClientConfig::default())?;
    assert!(client.tick(BTreeMap::new(), 0.0).is_err());
    join(&mut server, &mut client, 1)?;
    assert!(server
        .receive(
            1,
            ClientMessage::Input {
                frames: vec![InputFrame {
                    tick: 0,
                    seq: 0,
                    actions: BTreeMap::from([("move_x".into(), f64::NAN)])
                }],
                ack_snapshot_tick: None
            }
        )
        .is_err());
    // Empty action frames are valid and zero every previously supplied action.
    server.receive(1, client.tick(BTreeMap::new(), 0.0)?)?;
    server.tick()?;
    server.receive(1, client.tick(BTreeMap::new(), 17.0)?)?;
    let (_, message) = server.tick()?.pop().unwrap();
    let ServerMessage::Snapshot(mut full) = message else {
        panic!("snapshot expected");
    };
    full.entities.retain(|s| s.id != 4);
    client.receive(ServerMessage::Snapshot(full.clone()), 40.0)?;
    assert!(!client
        .simulation()
        .scene()
        .entities
        .iter()
        .any(|e| e.id == 4));
    full.server_tick = 3;
    full.entities[0].position[0] = f32::NAN;
    assert!(client.receive(ServerMessage::Snapshot(full), 50.0).is_err());
    Ok(())
}

fn snapshot(tick: u64, x: f32) -> Snapshot {
    Snapshot {
        server_tick: tick,
        last_processed_seq: None,
        input_late_ticks: 0,
        baseline_tick: None,
        entities: vec![NetEntityState {
            id: 1,
            position: [x, 0.0, 0.0],
            rotation_deg: [0.0; 3],
            linvel: [2.0, 0.0, 0.0],
            angvel: [0.0; 3],
        }],
        removed: vec![],
        script_state: json!({}),
    }
}

#[test]
fn interpolation_is_bracketed_monotone_and_extrapolation_is_bounded() {
    let mut interp = Interpolator::default();
    assert!(interp.render(0.0).is_empty());
    let arrivals = [
        (2, 2),
        (5, 4),
        (6, 6),
        (10, 8),
        (11, 10),
        (12, 12),
        (16, 14),
        (17, 16),
        (19, 18),
        (21, 20),
    ];
    let mut previous = None;
    for (arrival, tick) in arrivals {
        interp.push(&snapshot(tick, tick as f32 / 30.0));
        let current = interp.render(0.0)[0].position[0];
        if let Some((old_arrival, old)) = previous {
            assert!(current >= old - 1e-6);
            assert!(current - old <= 2.0 * (arrival - old_arrival) as f32 / 60.0 * 3.0 + 1e-6);
        }
        previous = Some((arrival, current));
    }
    for tick in 2..20 {
        let position = interp.sample(tick as f64 + 0.5)[0].position[0];
        assert!((position - (tick as f32 + 0.5) / 30.0).abs() < 1e-5);
    }
    let far = interp.sample(10000.0)[0].position[0];
    assert!((far - (20.0 / 30.0 + 0.5)).abs() < 1e-6);
    interp.push(&snapshot(10, 10.0 / 30.0)); // reordered arrival cannot rewind render time
    assert!(interp.render(0.0)[0].position[0] >= previous.unwrap().1);
}

#[test]
fn delta_roundtrip_saves_bytes_removes_entities_and_requires_exact_baseline() -> Result<()> {
    let mut old = snapshot(10, 0.0);
    old.entities = (1..=100)
        .map(|id| {
            let mut e = old.entities[0].clone();
            e.id = id;
            e
        })
        .collect();
    let mut full = old.clone();
    full.server_tick = 12;
    full.entities[1].position[0] = 1.0;
    full.entities.pop();
    let delta = full.delta(&old, 1e-6);
    let delta_bytes = encode(&ServerMessage::Snapshot(delta.clone()))?;
    let full_bytes = encode(&ServerMessage::Snapshot(full.clone()))?;
    println!(
        "snapshot bytes: full={}, delta={}",
        full_bytes.len(),
        delta_bytes.len()
    );
    assert!(delta_bytes.len() < full_bytes.len() / 10);
    let ServerMessage::Snapshot(decoded) = decode(&delta_bytes)? else {
        panic!("snapshot expected");
    };
    assert_eq!(decoded.expand(Some(&old))?.entities, full.entities);
    assert!(decoded.expand(None).is_err());
    assert_eq!(delta.removed, vec![100]);
    assert!(decode::<ClientMessage>(b"bad json").is_err());
    Ok(())
}

#[test]
fn slots_late_join_timeout_late_inputs_and_entity_isolation() -> Result<()> {
    let config = ServerConfig {
        timeout_ticks: 20,
        ..ServerConfig::default()
    };
    let mut server = NetServer::new(support::scene(), BTreeMap::new(), config)?;
    let input = |tick, seq, x| ClientMessage::Input {
        frames: vec![InputFrame {
            tick,
            seq,
            actions: BTreeMap::from([("move_x".into(), x)]),
        }],
        ack_snapshot_tick: None,
    };
    server.receive(99, input(0, 0, 100.0))?; // no assignment, no effect
    server.receive(1, ClientMessage::Hello { name: "A".into() })?;
    assert_eq!(server.assigned_entity(1), Some(2));
    server.receive(1, input(0, 0, -1.0))?;
    for _ in 0..10 {
        server.tick()?;
    }
    let states = server.simulation().net_entity_states();
    assert!(states[1].position[0] < -3.1);
    assert!(
        (states[2].position[0] - 3.0).abs() < 1e-6,
        "A must not drive B"
    );
    server.receive(1, input(0, 1, 20.0))?;
    server.receive(1, input(0, 1, 20.0))?;
    assert_eq!(server.stats().late_inputs, 1); // redundant late frames are counted once
    let mut late = NetClient::new(support::scene(), BTreeMap::new(), ClientConfig::default())?;
    join(&mut server, &mut late, 2)?;
    assert_eq!(late.entity_id(), Some(3));
    assert_eq!(late.simulation().tick, 10);
    for _ in 0..2 {
        for (peer, message) in server.tick()? {
            if peer == 2 {
                late.receive(message, 200.0)?;
            }
        }
    }
    assert!(late.latest_snapshot().is_some());
    server.disconnect(1);
    server.tick()?;
    server.receive(
        3,
        ClientMessage::Hello {
            name: "replacement".into(),
        },
    )?;
    assert_eq!(server.assigned_entity(3), Some(2));
    server.tick()?;
    assert_eq!(server.simulation().net_entity_states()[1].linvel[0], 0.0);
    for _ in 0..22 {
        server.tick()?;
    }
    assert_eq!(server.assigned_entity(2), None);
    assert_eq!(server.assigned_entity(3), None);
    Ok(())
}

#[test]
fn simulated_links_are_seeded_and_respect_clock_loss_duplication_and_reordering() -> Result<()> {
    let run = || -> Result<Vec<(u64, Vec<u8>)>> {
        let net = SimNetwork::new(42, LinkConfig::default());
        net.set_link(
            1,
            2,
            LinkConfig {
                latency_ms: 10.0,
                jitter_ms: 5.0,
                loss: 0.2,
                duplication: 0.5,
                reorder_ms: 20.0,
            },
        );
        let mut sender = net.endpoint(1);
        let mut receiver = net.endpoint(2);
        for i in 0..100u8 {
            sender.send(2, &[i])?;
        }
        assert!(receiver.recv()?.is_none());
        net.advance_to(100.0)?;
        let mut output = vec![];
        while let Some(packet) = receiver.recv()? {
            output.push(packet);
        }
        assert!(net.dropped_packets() > 0);
        assert!(output.windows(2).any(|w| w[1].1[0] < w[0].1[0]));
        assert!(output
            .iter()
            .any(|p| output.iter().filter(|q| *q == p).count() > 1));
        assert!(net.advance_to(90.0).is_err());
        Ok(output)
    };
    assert_eq!(run()?, run()?);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn udp_localhost_smoke_receives_authority_snapshots() -> Result<()> {
    use engine_net::transport::UdpTransport;
    use std::time::{Duration, Instant};
    let (Ok(mut server_socket), Ok(mut client_socket)) = (
        UdpTransport::bind("127.0.0.1:0"),
        UdpTransport::bind("127.0.0.1:0"),
    ) else {
        eprintln!("SKIP: localhost UDP binding unavailable");
        return Ok(());
    };
    client_socket.add_peer(0, server_socket.local_addr()?);
    let mut server = NetServer::new(support::scene(), BTreeMap::new(), ServerConfig::default())?;
    client_socket.send(0, &encode(&ClientMessage::Hello { name: "udp".into() })?)?;
    let started = Instant::now();
    let mut next_tick = 0.0;
    let mut snapshots = 0;
    while started.elapsed() < Duration::from_millis(250) {
        while let Some((peer, bytes)) = server_socket.recv()? {
            if let Some(message) = server.receive(peer, decode(&bytes)?)? {
                server_socket.send(peer, &encode(&message)?)?;
            }
        }
        if started.elapsed().as_secs_f64() >= next_tick {
            for (peer, message) in server.tick()? {
                server_socket.send(peer, &encode(&message)?)?;
            }
            next_tick += 1.0 / 60.0;
        }
        while let Some((_, bytes)) = client_socket.recv()? {
            if matches!(decode::<ServerMessage>(&bytes)?, ServerMessage::Snapshot(_)) {
                snapshots += 1;
            }
        }
        std::thread::yield_now();
    }
    assert!(snapshots >= 2, "received {snapshots} snapshots");
    Ok(())
}
