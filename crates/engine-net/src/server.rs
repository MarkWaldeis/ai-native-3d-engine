use crate::{inputs, protocol::*, valid_actions, NetStats, SimSource, TICK_MS};
use anyhow::{bail, Result};
use engine_runtime::{Input, Simulation};
use shared::scene::Scene;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub snapshot_interval: u64,
    pub timeout_ticks: u64,
    pub history_capacity: usize,
    pub max_input_lead: u64,
    pub delta_epsilon: f32,
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            snapshot_interval: 2,
            timeout_ticks: 300,
            history_capacity: 128,
            max_input_lead: 120,
            delta_epsilon: 1e-6,
        }
    }
}

struct Client {
    entity: u64,
    frames: BTreeMap<u64, InputFrame>,
    seen: BTreeSet<u64>,
    last: Actions,
    processed: Option<u64>,
    ack: Option<u64>,
    snapshots: BTreeMap<u64, Snapshot>,
    late: u64,
    last_heard: u64,
}

pub struct NetServer {
    simulation: Simulation,
    config: ServerConfig,
    slots: Vec<u64>,
    initial_ids: BTreeSet<u64>,
    clients: BTreeMap<PeerId, Client>,
    released: Vec<Input>,
    stats: NetStats,
    snapshot_bytes: u64,
}

impl NetServer {
    pub fn new(scene: Scene, sources: impl Into<SimSource>, config: ServerConfig) -> Result<Self> {
        if config.snapshot_interval == 0
            || config.history_capacity < 2
            || config.timeout_ticks == 0
            || !config.delta_epsilon.is_finite()
            || config.delta_epsilon < 0.0
        {
            bail!("invalid server configuration");
        }
        let mut slots: Vec<_> = scene
            .entities
            .iter()
            .filter(|e| e.tags.iter().any(|t| t == "net_player"))
            .map(|e| e.id)
            .collect();
        slots.sort_unstable();
        Ok(Self {
            initial_ids: scene.entities.iter().map(|e| e.id).collect(),
            simulation: sources.into().build(scene)?,
            config,
            slots,
            clients: BTreeMap::new(),
            released: vec![],
            stats: NetStats::default(),
            snapshot_bytes: 0,
        })
    }
    pub fn simulation(&self) -> &Simulation {
        &self.simulation
    }
    pub fn stats(&self) -> &NetStats {
        &self.stats
    }
    pub fn assigned_entity(&self, peer: PeerId) -> Option<u64> {
        self.clients.get(&peer).map(|c| c.entity)
    }

    pub fn receive(
        &mut self,
        peer: PeerId,
        message: ClientMessage,
    ) -> Result<Option<ServerMessage>> {
        let tick = self.simulation.tick;
        match message {
            ClientMessage::Hello { name } => {
                if name.len() > 128 {
                    bail!("name too long");
                }
                if !self.clients.contains_key(&peer) {
                    let entity = self
                        .slots
                        .iter()
                        .find(|id| !self.clients.values().any(|c| c.entity == **id))
                        .copied()
                        .ok_or_else(|| anyhow::anyhow!("no free player slot"))?;
                    self.clients.insert(
                        peer,
                        Client {
                            entity,
                            frames: BTreeMap::new(),
                            seen: BTreeSet::new(),
                            last: Actions::new(),
                            processed: None,
                            ack: None,
                            snapshots: BTreeMap::new(),
                            late: 0,
                            last_heard: tick,
                        },
                    );
                }
                let client = self.clients.get_mut(&peer).unwrap();
                client.last_heard = tick;
                Ok(Some(ServerMessage::Welcome {
                    client_id: peer,
                    entity_id: client.entity,
                    server_tick: tick,
                }))
            }
            ClientMessage::Input {
                frames,
                ack_snapshot_tick,
            } => {
                let Some(client) = self.clients.get_mut(&peer) else {
                    return Ok(None);
                };
                if frames.len() > 128 || frames.iter().any(|f| !valid_actions(&f.actions)) {
                    bail!("invalid input frames");
                }
                client.last_heard = tick;
                if ack_snapshot_tick.is_none() {
                    client.ack = None;
                }
                if let Some(ack) = ack_snapshot_tick.filter(|t| client.snapshots.contains_key(t)) {
                    if client.ack.is_none_or(|old| ack > old) {
                        client.ack = Some(ack);
                    }
                }
                for frame in frames {
                    if client.processed.is_some_and(|seq| frame.seq <= seq)
                        || client.seen.contains(&frame.seq)
                    {
                        continue;
                    }
                    if frame.tick > tick.saturating_add(self.config.max_input_lead) {
                        continue;
                    }
                    client.seen.insert(frame.seq);
                    if frame.tick < tick {
                        client.late += 1;
                        self.stats.late_inputs += 1;
                    } else {
                        client.frames.entry(frame.tick).or_insert(frame);
                    }
                }
                while client.seen.len() > 512 {
                    client.seen.pop_first();
                }
                Ok(None)
            }
        }
    }

    pub fn disconnect(&mut self, peer: PeerId) {
        if let Some(client) = self.clients.remove(&peer) {
            self.released.extend(inputs(
                self.simulation.tick,
                client.entity,
                &Actions::new(),
                &client.last,
            ));
        }
    }

    /// Execute one 60 Hz authority tick, returning outgoing per-peer snapshots.
    pub fn tick(&mut self) -> Result<Vec<(PeerId, ServerMessage)>> {
        let tick = self.simulation.tick;
        let expired: Vec<_> = self
            .clients
            .iter()
            .filter(|(_, c)| tick.saturating_sub(c.last_heard) >= self.config.timeout_ticks)
            .map(|(id, _)| *id)
            .collect();
        for peer in expired {
            self.disconnect(peer);
        }
        let mut events = std::mem::take(&mut self.released);
        for client in self.clients.values_mut() {
            if let Some(frame) = client.frames.remove(&tick) {
                events.extend(inputs(tick, client.entity, &frame.actions, &client.last));
                client.last = frame.actions;
                client.processed = Some(frame.seq);
            } else {
                events.extend(inputs(tick, client.entity, &client.last, &client.last));
            }
        }
        self.simulation.step(1, &events)?;
        let tick = self.simulation.tick;
        if !tick.is_multiple_of(self.config.snapshot_interval) {
            return Ok(vec![]);
        }
        let mut messages = vec![];
        let players: Vec<_> = self.clients.values().map(|client| client.entity).collect();
        for (&peer, client) in &mut self.clients {
            let full = Snapshot {
                server_tick: tick,
                last_processed_seq: client.processed,
                input_late_ticks: client.late,
                entities: self.simulation.net_entity_states(),
                spawned: self
                    .simulation
                    .scene()
                    .entities
                    .into_iter()
                    .filter(|e| !self.initial_ids.contains(&e.id))
                    .collect(),
                removed: vec![],
                players: players.clone(),
                script_state: self.simulation.net_script_state(),
                baseline_tick: None,
            };
            let baseline = client.ack.and_then(|t| client.snapshots.get(&t));
            let wire = baseline.map_or_else(
                || full.clone(),
                |b| full.delta(b, self.config.delta_epsilon),
            );
            let reconstructed = wire.expand(baseline)?;
            let message = ServerMessage::Snapshot(wire);
            self.snapshot_bytes += encode(&message)?.len() as u64;
            client.snapshots.insert(tick, reconstructed);
            while client.snapshots.len() > self.config.history_capacity {
                client.snapshots.pop_first();
            }
            messages.push((peer, message));
        }
        self.stats.snapshot_bytes_per_second =
            self.snapshot_bytes as f64 / (tick as f64 * TICK_MS / 1000.0);
        Ok(messages)
    }
}
