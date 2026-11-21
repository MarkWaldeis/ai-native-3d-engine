//! Reproducible finite headless sessions; no file, socket, or wall-clock access.
use crate::{
    protocol::{decode, encode},
    transport::{LinkConfig, SimNetwork, SimTransport, Transport},
    Actions, ClientConfig, ClientMessage, NetClient, NetServer, NetStats, ServerConfig, TICK_MS,
};
use anyhow::{ensure, Result};
use engine_runtime::MAX_TICKS;
use serde::{Deserialize, Serialize};
use shared::scene::Scene;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputEvent {
    pub tick: u64,
    pub action: String,
    pub value: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub clients: usize,
    pub ticks: u64,
    pub rtt_ms: f64,
    pub jitter_ms: f64,
    pub loss: f64,
    pub seed: u64,
    /// Outer array is client/slot order; events use local simulation ticks.
    pub inputs: Vec<Vec<InputEvent>>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            clients: 2,
            ticks: 600,
            rtt_ms: 120.0,
            jitter_ms: 20.0,
            loss: 0.05,
            seed: 1,
            inputs: vec![],
        }
    }
}

impl Options {
    pub fn validate(&self) -> Result<()> {
        ensure!((1..=8).contains(&self.clients), "clients must be in 1..=8");
        ensure!(self.ticks <= MAX_TICKS, "ticks must be in 0..={MAX_TICKS}");
        ensure!(
            self.rtt_ms.is_finite() && (0.0..=5000.0).contains(&self.rtt_ms),
            "rtt_ms must be in 0..=5000"
        );
        ensure!(
            self.jitter_ms.is_finite() && (0.0..=5000.0).contains(&self.jitter_ms),
            "jitter_ms must be in 0..=5000"
        );
        ensure!(
            self.loss.is_finite() && (0.0..=1.0).contains(&self.loss),
            "loss must be in 0..=1"
        );
        ensure!(
            self.inputs.len() <= self.clients,
            "inputs has more lists than clients"
        );
        for events in &self.inputs {
            ensure!(events.len() <= 4096, "at most 4096 input events per client");
            let actions: std::collections::BTreeSet<_> = events.iter().map(|e| &e.action).collect();
            ensure!(actions.len() <= 64, "at most 64 input actions per client");
            ensure!(events.iter().all(|e| e.tick<self.ticks && !e.action.is_empty() && e.action.len()<=128 && e.value.is_finite()),
                "input events require tick < ticks, nonempty action (max 128 bytes), and finite value");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct PlayerReport {
    pub client_id: u64,
    pub entity_id: Option<u64>,
    pub stats: NetStats,
    pub client_tick: u64,
    pub comparison_tick: Option<u64>,
    pub server_position: Option<[f32; 3]>,
    pub client_position: Option<[f32; 3]>,
    pub predicted_position: Option<[f32; 3]>,
    pub position_delta: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct SimulationReport {
    pub server_tick: u64,
    pub server_stats: NetStats,
    pub clients: Vec<PlayerReport>,
    pub max_position_delta: Option<f32>,
    pub compared_clients: usize,
    pub converged: bool,
    pub packets_dropped: u64,
}

struct Session {
    server: NetServer,
    clients: Vec<NetClient>,
    network: SimNetwork,
    authority: SimTransport,
    endpoints: Vec<SimTransport>,
}

impl Session {
    fn pump(&mut self, now: f64) -> Result<()> {
        for _ in 0..3 {
            self.network.advance_to(now)?;
            while let Some((peer, bytes)) = self.authority.recv()? {
                if let Some(response) = self.server.receive(peer, decode(&bytes)?)? {
                    self.authority.send(peer, &encode(&response)?)?;
                }
            }
            self.network.advance_to(now)?;
            for (client, endpoint) in self.clients.iter_mut().zip(&mut self.endpoints) {
                while let Some((_, bytes)) = endpoint.recv()? {
                    client.receive(decode(&bytes)?, now)?;
                }
            }
        }
        Ok(())
    }
}

pub fn run(
    scene: Scene,
    sources: BTreeMap<String, String>,
    mut options: Options,
) -> Result<SimulationReport> {
    options.validate()?;
    let slots = scene
        .entities
        .iter()
        .filter(|e| e.tags.iter().any(|t| t == "net_player"))
        .count();
    ensure!(
        slots >= options.clients,
        "scene needs at least {} net_player entities",
        options.clients
    );
    let network = SimNetwork::new(
        options.seed,
        LinkConfig {
            latency_ms: options.rtt_ms / 2.0,
            jitter_ms: options.jitter_ms,
            loss: options.loss,
            ..LinkConfig::default()
        },
    );
    let lead = if options.rtt_ms == 0.0 && options.jitter_ms == 0.0 {
        1
    } else {
        (((options.rtt_ms / 2.0 + options.jitter_ms) / TICK_MS).ceil() as u64 + 2).min(30)
    };
    let config = ClientConfig {
        initial_lead_ticks: lead,
        max_lead_ticks: if lead == 1 { 1 } else { 30 },
        jitter_buffer_ticks: if lead == 1 { 0 } else { 2 },
        ..ClientConfig::default()
    };
    let mut session = Session {
        server: NetServer::new(scene.clone(), sources.clone(), ServerConfig::default())?,
        clients: (0..options.clients)
            .map(|_| NetClient::new(scene.clone(), sources.clone(), config.clone()))
            .collect::<Result<_>>()?,
        authority: network.endpoint(0),
        endpoints: (1..=options.clients as u64)
            .map(|id| network.endpoint(id))
            .collect(),
        network,
    };
    options.inputs.resize_with(options.clients, Vec::new);
    for events in &mut options.inputs {
        events.sort_by_key(|e| e.tick);
    }
    let mut cursors = vec![0; options.clients];
    let mut actions = vec![Actions::new(); options.clients];
    for tick in 0..options.ticks {
        let now = tick as f64 * TICK_MS;
        for (index, endpoint) in session.endpoints.iter_mut().enumerate() {
            if session.clients[index].entity_id().is_none() && tick.is_multiple_of(15) {
                endpoint.send(
                    0,
                    &encode(&ClientMessage::Hello {
                        name: format!("client-{}", index + 1),
                    })?,
                )?;
            }
        }
        session.pump(now)?;
        for (index, client) in session.clients.iter_mut().enumerate() {
            if client.entity_id().is_none() {
                continue;
            }
            let target = tick + client.stats().lead_ticks;
            while client.simulation().tick < target {
                let local_tick = client.simulation().tick;
                while let Some(event) = options.inputs[index]
                    .get(cursors[index])
                    .filter(|e| e.tick <= local_tick)
                {
                    actions[index].insert(event.action.clone(), event.value);
                    cursors[index] += 1;
                }
                client.tick(actions[index].clone(), now)?;
            }
            session.endpoints[index].send(0, &encode(&client.input_message())?)?;
        }
        session.pump(now)?;
        for (peer, message) in session.server.tick()? {
            session.authority.send(peer, &encode(&message)?)?;
        }
        session.pump(now)?;
    }
    let server_tick = session.server.simulation().tick;
    let server_states = session.server.simulation().net_entity_states();
    let clients: Vec<_> = session
        .clients
        .iter()
        .enumerate()
        .map(|(index, client)| {
            let entity = client.entity_id();
            let server_position = server_states
                .iter()
                .find(|s| Some(s.id) == entity)
                .map(|s| s.position);
            let client_position = client
                .predicted_states_at(server_tick)
                .and_then(|states| states.iter().find(|s| Some(s.id) == entity))
                .map(|s| s.position);
            let predicted_position = client
                .simulation()
                .net_entity_states()
                .into_iter()
                .find(|s| Some(s.id) == entity)
                .map(|s| s.position);
            let position_delta = server_position.zip(client_position).map(|(a, b)| {
                a.iter()
                    .zip(b)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    .sqrt()
            });
            PlayerReport {
                client_id: index as u64 + 1,
                entity_id: entity,
                stats: client.stats().clone(),
                client_tick: client.simulation().tick,
                comparison_tick: position_delta.map(|_| server_tick),
                server_position,
                client_position,
                predicted_position,
                position_delta,
            }
        })
        .collect();
    let compared_clients = clients
        .iter()
        .filter(|c| c.position_delta.is_some())
        .count();
    let max_position_delta = clients
        .iter()
        .filter_map(|c| c.position_delta)
        .reduce(f32::max);
    Ok(SimulationReport {
        server_tick,
        server_stats: session.server.stats().clone(),
        clients,
        max_position_delta,
        compared_clients,
        converged: compared_clients == options.clients
            && max_position_delta.is_some_and(|d| d <= 1e-3),
        packets_dropped: session.network.dropped_packets(),
    })
}
