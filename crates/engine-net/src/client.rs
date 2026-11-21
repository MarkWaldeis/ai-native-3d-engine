use crate::{inputs, protocol::*, valid_actions, Interpolator, NetStats, SimSource, TICK_MS};
use anyhow::{bail, Result};
use engine_runtime::{NetEntityState, SimState, Simulation};
use serde_json::Value;
use shared::scene::{Entity, Scene};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub initial_lead_ticks: u64,
    pub max_lead_ticks: u64,
    pub jitter_buffer_ticks: u64,
    pub history_capacity: usize,
    pub redundant_frames: usize,
    pub position_epsilon: f32,
    pub interpolation_delay_ticks: f64,
    pub smoothing_ms: f64,
    pub snapshot_interval: u64,
}
impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            initial_lead_ticks: 6,
            max_lead_ticks: 30,
            jitter_buffer_ticks: 2,
            history_capacity: 128,
            redundant_frames: 32,
            position_epsilon: 1e-3,
            interpolation_delay_ticks: 6.0,
            smoothing_ms: 100.0,
            snapshot_interval: 2,
        }
    }
}

struct Checkpoint {
    state: SimState,
    entities: Vec<NetEntityState>,
    definitions: Vec<Entity>,
    script: Value,
}

pub struct NetClient {
    simulation: Simulation,
    initial_scene: Scene,
    sources: SimSource,
    config: ClientConfig,
    entity: Option<u64>,
    client_id: Option<u64>,
    sequence: u64,
    history: BTreeMap<u64, Checkpoint>,
    frames: BTreeMap<u64, InputFrame>,
    sent_at: BTreeMap<u64, (f64, u64)>,
    snapshots: BTreeMap<u64, Snapshot>,
    ack: Option<u64>,
    processed: Option<u64>,
    last_actions: Actions,
    interpolator: Interpolator,
    stats: NetStats,
    comparisons: u64,
    total_error: f64,
    bytes: u64,
    first_snapshot_ms: Option<f64>,
    smooth_offset: [f32; 3],
    smooth_tick: u64,
}

impl NetClient {
    pub fn new(scene: Scene, sources: impl Into<SimSource>, config: ClientConfig) -> Result<Self> {
        if config.history_capacity < 2
            || config.initial_lead_ticks == 0
            || config.max_lead_ticks == 0
            || config.redundant_frames == 0
            || config.redundant_frames > 128
            || config.max_lead_ticks >= config.history_capacity as u64
            || config.initial_lead_ticks > config.max_lead_ticks
            || config.snapshot_interval == 0
            || !config.position_epsilon.is_finite()
            || config.position_epsilon < 0.0
            || !config.smoothing_ms.is_finite()
            || config.smoothing_ms <= 0.0
            || !config.interpolation_delay_ticks.is_finite()
            || config.interpolation_delay_ticks < 0.0
        {
            bail!("invalid client configuration");
        }
        let mut interpolator = Interpolator::default();
        let sources = sources.into();
        interpolator.delay_ticks = config.interpolation_delay_ticks;
        let stats = NetStats {
            lead_ticks: config.initial_lead_ticks,
            ..NetStats::default()
        };
        let mut client = Self {
            initial_scene: scene.clone(),
            simulation: sources.build(scene)?,
            sources,
            config,
            entity: None,
            client_id: None,
            sequence: 0,
            history: BTreeMap::new(),
            frames: BTreeMap::new(),
            sent_at: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            ack: None,
            processed: None,
            last_actions: Actions::new(),
            interpolator,
            stats,
            comparisons: 0,
            total_error: 0.0,
            bytes: 0,
            first_snapshot_ms: None,
            smooth_offset: [0.0; 3],
            smooth_tick: 0,
        };
        client.checkpoint();
        Ok(client)
    }
    pub fn simulation(&self) -> &Simulation {
        &self.simulation
    }
    pub fn stats(&self) -> &NetStats {
        &self.stats
    }
    pub fn entity_id(&self) -> Option<u64> {
        self.entity
    }
    pub fn client_id(&self) -> Option<u64> {
        self.client_id
    }
    pub fn latest_snapshot(&self) -> Option<&Snapshot> {
        self.ack.and_then(|t| self.snapshots.get(&t))
    }
    pub fn predicted_states_at(&self, tick: u64) -> Option<&[NetEntityState]> {
        self.history.get(&tick).map(|c| c.entities.as_slice())
    }

    fn checkpoint(&mut self) {
        self.history.insert(
            self.simulation.tick,
            Checkpoint {
                state: self.simulation.save_state(),
                entities: self.simulation.net_entity_states(),
                definitions: self.simulation.scene().entities,
                script: self.simulation.net_script_state(),
            },
        );
        while self.history.len() > self.config.history_capacity {
            self.history.pop_first();
        }
        let oldest = *self.history.first_key_value().unwrap().0;
        self.frames.retain(|tick, _| *tick >= oldest);
        while self.sent_at.len() > self.config.history_capacity {
            self.sent_at.pop_first();
        }
    }

    /// Predict one tick. Send the returned message even when there are no actions.
    pub fn tick(&mut self, actions: Actions, now_ms: f64) -> Result<ClientMessage> {
        let entity = self
            .entity
            .ok_or_else(|| anyhow::anyhow!("client has not received Welcome"))?;
        if !valid_actions(&actions) || !now_ms.is_finite() {
            bail!("invalid client inputs/time");
        }
        let tick = self.simulation.tick;
        let frame = InputFrame {
            tick,
            seq: self.sequence,
            actions,
        };
        self.sequence += 1;
        let events = inputs(tick, entity, &frame.actions, &self.last_actions);
        self.simulation.step(1, &events)?;
        self.last_actions = frame.actions.clone();
        self.sent_at
            .insert(frame.seq, (now_ms, self.stats.lead_ticks));
        self.frames.insert(tick, frame);
        self.checkpoint();
        Ok(self.input_message())
    }

    /// Drive this with a synchronized server clock. Lead increases add prediction
    /// ticks; lead decreases pause prediction rather than discarding user history.
    pub fn advance_to(
        &mut self,
        server_tick: u64,
        actions: Actions,
        now_ms: f64,
    ) -> Result<ClientMessage> {
        let target = server_tick.saturating_add(self.stats.lead_ticks.max(1));
        if target.saturating_sub(self.simulation.tick) > self.config.history_capacity as u64 {
            bail!("client clock jump exceeds history");
        }
        while self.simulation.tick < target {
            self.tick(actions.clone(), now_ms)?;
        }
        Ok(self.input_message())
    }

    pub fn input_message(&self) -> ClientMessage {
        let mut frames: Vec<_> = self
            .frames
            .values()
            .filter(|f| self.processed.is_none_or(|seq| f.seq > seq))
            .rev()
            .take(self.config.redundant_frames)
            .cloned()
            .collect();
        frames.reverse();
        ClientMessage::Input {
            frames,
            ack_snapshot_tick: self.ack,
        }
    }

    pub fn receive(&mut self, message: ServerMessage, now_ms: f64) -> Result<()> {
        if !now_ms.is_finite() {
            bail!("invalid receive time");
        }
        match message {
            ServerMessage::Welcome {
                client_id,
                entity_id,
                server_tick,
            } => {
                if self.entity.is_some() {
                    return Ok(());
                }
                if !self
                    .simulation
                    .scene()
                    .entities
                    .iter()
                    .any(|e| e.id == entity_id && e.tags.iter().any(|t| t == "net_player"))
                {
                    bail!("assigned entity is not a player in the local scene");
                }
                self.entity = Some(entity_id);
                self.client_id = Some(client_id);
                // Bootstrap late joiners at the authority tick; the first full
                // snapshot supplies authoritative poses and script state.
                while self.simulation.tick < server_tick {
                    self.simulation.step(1, &[])?;
                }
                self.history.clear();
                self.checkpoint();
            }
            ServerMessage::Snapshot(wire) => self.snapshot(wire, now_ms)?,
        }
        Ok(())
    }

    fn snapshot(&mut self, wire: Snapshot, now_ms: f64) -> Result<()> {
        if self.entity.is_none()
            || self
                .snapshots
                .last_key_value()
                .is_some_and(|(&tick, _)| wire.server_tick <= tick)
        {
            return Ok(());
        }
        let baseline = wire
            .baseline_tick
            .and_then(|tick| self.snapshots.get(&tick));
        if wire.baseline_tick.is_some() && baseline.is_none() {
            // Request a full snapshot on the next outbound input packet.
            self.ack = None;
            return Ok(());
        }
        let snapshot = wire.expand(baseline)?;
        if snapshot.entities.iter().any(|s| {
            s.position
                .iter()
                .chain(&s.rotation_deg)
                .chain(&s.linvel)
                .chain(&s.angvel)
                .any(|v| !v.is_finite())
        }) {
            bail!("non-finite snapshot entity");
        }
        let bytes = encode(&ServerMessage::Snapshot(wire))?.len();
        self.bytes += bytes as u64;
        let start = *self.first_snapshot_ms.get_or_insert(now_ms);
        self.stats.snapshot_bytes_per_second =
            self.bytes as f64 / ((now_ms - start + TICK_MS) / 1000.0);
        if let Some(previous) = self.ack {
            self.stats.packets_lost += (snapshot.server_tick - previous)
                .saturating_sub(self.config.snapshot_interval)
                / self.config.snapshot_interval;
        }
        if let Some(seq) = snapshot.last_processed_seq {
            if self.processed.is_none_or(|old| seq > old) {
                if let Some(&(sent, lead)) = self.sent_at.get(&seq) {
                    // Remove intentional future-input buffering from the round trip.
                    let sample = (now_ms - sent - (lead as f64 * TICK_MS)).max(0.0) * 2.0;
                    self.stats.rtt_ms = if self.stats.rtt_ms == 0.0 {
                        sample
                    } else {
                        self.stats.rtt_ms * 0.9 + sample * 0.1
                    };
                }
                self.processed = Some(seq);
            }
        }
        let late = snapshot
            .input_late_ticks
            .saturating_sub(self.stats.late_inputs);
        self.stats.late_inputs = snapshot.input_late_ticks;
        let measured = ((self.stats.rtt_ms / (2.0 * TICK_MS)).ceil() as u64
            + self.config.jitter_buffer_ticks)
            .clamp(1, self.config.max_lead_ticks);
        if late > 0 {
            self.stats.lead_ticks = (self.stats.lead_ticks + 1).min(self.config.max_lead_ticks);
        } else if measured > self.stats.lead_ticks {
            self.stats.lead_ticks = measured.min(self.config.max_lead_ticks);
        } else if snapshot.server_tick.is_multiple_of(120) && measured + 1 < self.stats.lead_ticks {
            self.stats.lead_ticks = self.stats.lead_ticks.saturating_sub(1).max(1);
        }
        self.reconcile(&snapshot)?;
        self.interpolator.push(&snapshot);
        self.ack = Some(snapshot.server_tick);
        self.snapshots.insert(snapshot.server_tick, snapshot);
        while self.snapshots.len() > self.config.history_capacity {
            self.snapshots.pop_first();
        }
        Ok(())
    }

    fn reconcile(&mut self, snapshot: &Snapshot) -> Result<()> {
        let mut snapshot = snapshot.clone();
        snapshot.entities.retain(|state| {
            self.initial_scene.entities.iter().any(|e| e.id == state.id)
                || snapshot.spawned.iter().any(|e| e.id == state.id)
        });
        let entity = self.entity.unwrap();
        let server = snapshot
            .entities
            .iter()
            .find(|s| s.id == entity)
            .ok_or_else(|| anyhow::anyhow!("snapshot missing owned player"))?;
        let tick = snapshot.server_tick;
        let predicted = self.history.get(&tick);
        let own = predicted.and_then(|c| c.entities.iter().find(|s| s.id == entity));
        let error = own.map_or(0.0, |s| distance(s.position, server.position));
        self.comparisons += 1;
        self.total_error += error as f64;
        self.stats.max_prediction_error = self.stats.max_prediction_error.max(error);
        self.stats.avg_prediction_error = self.total_error / self.comparisons as f64;
        let mismatch = error > self.config.position_epsilon
            || predicted.is_none_or(|c| {
                c.script != snapshot.script_state
                    || snapshot.spawned.iter().any(|definition| {
                        c.definitions
                            .iter()
                            .find(|e| e.id == definition.id)
                            .is_none_or(|e| !same_definition(e, definition))
                    })
                    || snapshot.entities.iter().any(|state| {
                        c.entities
                            .iter()
                            .find(|e| e.id == state.id)
                            .is_none_or(|e| {
                                crate::protocol::different(e, state, self.config.position_epsilon)
                            })
                    })
                    || c.entities
                        .iter()
                        .map(|e| e.id)
                        .collect::<std::collections::BTreeSet<_>>()
                        != snapshot.entities.iter().map(|e| e.id).collect()
            })
            || own.is_none_or(|s| {
                crate::protocol::different(s, server, self.config.position_epsilon)
            });
        if !mismatch {
            return Ok(());
        }
        let old_render = self
            .render_states(0.0)
            .into_iter()
            .find(|s| s.id == entity)
            .map(|s| s.position);
        let now = self.simulation.tick.max(tick);
        if let Some(checkpoint) = self.history.get(&tick) {
            self.simulation.restore_state(&checkpoint.state);
        } else {
            // Out-of-window corrections cannot replay missing inputs. Bootstrap
            // at T and retain only the history that is still available.
            self.simulation = self.sources.build(self.initial_scene.clone())?;
        }
        let ids: std::collections::BTreeSet<_> = snapshot.entities.iter().map(|s| s.id).collect();
        for entity in self.simulation.scene().entities {
            if !ids.contains(&entity.id) {
                self.simulation.remove_net_entity(entity.id)?;
            }
        }
        for entity in self.initial_scene.entities.iter().chain(&snapshot.spawned) {
            if !ids.contains(&entity.id) {
                continue;
            }
            let existing = self
                .simulation
                .scene()
                .entities
                .into_iter()
                .find(|e| e.id == entity.id);
            if existing
                .as_ref()
                .is_none_or(|e| !same_definition(e, entity))
            {
                self.simulation.insert_net_entity(entity.clone())?;
            }
        }
        self.simulation.tick = tick;
        self.simulation.apply_net_entity_states(&snapshot.entities);
        self.simulation
            .apply_net_script_state(snapshot.script_state.clone());
        self.history.retain(|at, _| *at < tick);
        self.checkpoint();
        let mut previous = self
            .frames
            .range(..tick)
            .next_back()
            .map(|(_, f)| f.actions.clone())
            .unwrap_or_default();
        while self.simulation.tick < now {
            let at = self.simulation.tick;
            let actions = self
                .frames
                .get(&at)
                .map(|f| f.actions.clone())
                .unwrap_or_else(|| previous.clone());
            self.simulation
                .step(1, &inputs(at, entity, &actions, &previous))?;
            previous = actions;
            self.checkpoint();
        }
        self.last_actions = previous;
        self.stats.corrections += 1;
        if let (Some(old), Some(new)) = (
            old_render,
            self.simulation
                .net_entity_states()
                .iter()
                .find(|s| s.id == entity),
        ) {
            self.smooth_offset = std::array::from_fn(|i| old[i] - new.position[i]);
            self.smooth_tick = self.simulation.tick;
        }
        Ok(())
    }

    /// Definitions for the currently rendered set. Diff IDs between calls to create/remove meshes.
    /// Authoritative definitions override predicted spawns; delayed removals follow interpolation.
    pub fn entity_definitions(&self) -> Vec<Entity> {
        let scene = self.simulation.scene();
        self.render_states(0.0)
            .into_iter()
            .filter_map(|state| {
                let definition = self
                    .snapshots
                    .values()
                    .rev()
                    .find_map(|s| s.spawned.iter().find(|e| e.id == state.id))
                    .or_else(|| {
                        self.initial_scene
                            .entities
                            .iter()
                            .find(|e| e.id == state.id)
                    })
                    .or_else(|| scene.entities.iter().find(|e| e.id == state.id));
                definition.map(|definition| {
                    let mut entity = definition.clone();
                    entity.transform.position = state.position;
                    entity.transform.rotation_deg = state.rotation_deg;
                    entity
                })
            })
            .collect()
    }

    /// Fractional ticks since the last prediction tick / newest received snapshot.
    /// Smoothing changes presentation only; never feed rendered poses into physics.
    pub fn render_states(&self, now_tick_fraction: f64) -> Vec<NetEntityState> {
        let mut states = self.interpolator.render(now_tick_fraction);
        states.retain(|s| Some(s.id) != self.entity);
        if let Some(mut own) = self
            .simulation
            .net_entity_states()
            .into_iter()
            .find(|s| Some(s.id) == self.entity)
        {
            let elapsed = (self.simulation.tick.saturating_sub(self.smooth_tick) as f64
                + now_tick_fraction.max(0.0))
                * TICK_MS;
            let decay = (1.0 - elapsed / self.config.smoothing_ms).clamp(0.0, 1.0) as f32;
            for i in 0..3 {
                own.position[i] += self.smooth_offset[i] * decay;
            }
            states.push(own);
        }
        states.sort_by_key(|s| s.id);
        states
    }
}

fn same_definition(a: &Entity, b: &Entity) -> bool {
    let mut a = a.clone();
    a.transform.position = b.transform.position;
    a.transform.rotation_deg = b.transform.rotation_deg;
    a == *b
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f32>()
        .sqrt()
}
