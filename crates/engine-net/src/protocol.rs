//! JSON wire format. Ticks identify states before executing that tick's inputs.
use anyhow::{bail, Result};
use engine_runtime::NetEntityState;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Actions = BTreeMap<String, f64>;
pub type PeerId = u64;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputFrame {
    pub tick: u64,
    pub seq: u64,
    pub actions: Actions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    Hello {
        name: String,
    },
    Input {
        frames: Vec<InputFrame>,
        ack_snapshot_tick: Option<u64>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerMessage {
    Welcome {
        client_id: u64,
        entity_id: u64,
        server_tick: u64,
    },
    Snapshot(Snapshot),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub server_tick: u64,
    pub last_processed_seq: Option<u64>,
    pub input_late_ticks: u64,
    pub entities: Vec<NetEntityState>,
    pub removed: Vec<u64>,
    pub script_state: Value,
    pub baseline_tick: Option<u64>,
}

pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(message)?)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > 4 * 1024 * 1024 {
        bail!("network message exceeds 4 MiB");
    }
    Ok(serde_json::from_slice(bytes)?)
}

impl Snapshot {
    /// The caller must retain exactly this reconstructed state as the baseline.
    pub fn delta(&self, baseline: &Self, epsilon: f32) -> Self {
        let old: BTreeMap<_, _> = baseline.entities.iter().map(|e| (e.id, e)).collect();
        let mut delta = self.clone();
        delta.baseline_tick = Some(baseline.server_tick);
        delta
            .entities
            .retain(|e| old.get(&e.id).is_none_or(|b| different(e, b, epsilon)));
        delta.removed = baseline
            .entities
            .iter()
            .filter(|e| !self.entities.iter().any(|s| s.id == e.id))
            .map(|e| e.id)
            .collect();
        delta
    }

    pub fn expand(&self, baseline: Option<&Self>) -> Result<Self> {
        let mut full = self.clone();
        if let Some(tick) = self.baseline_tick {
            let baseline = baseline
                .filter(|s| s.server_tick == tick && s.baseline_tick.is_none())
                .ok_or_else(|| anyhow::anyhow!("missing snapshot baseline {tick}"))?;
            let mut entities: BTreeMap<_, _> = baseline
                .entities
                .iter()
                .cloned()
                .map(|e| (e.id, e))
                .collect();
            for id in &self.removed {
                entities.remove(id);
            }
            for entity in &self.entities {
                entities.insert(entity.id, entity.clone());
            }
            full.entities = entities.into_values().collect();
        }
        full.baseline_tick = None;
        full.removed.clear();
        Ok(full)
    }
}

pub(crate) fn different(a: &NetEntityState, b: &NetEntityState, epsilon: f32) -> bool {
    a.position
        .iter()
        .chain(&a.rotation_deg)
        .chain(&a.linvel)
        .chain(&a.angvel)
        .zip(
            b.position
                .iter()
                .chain(&b.rotation_deg)
                .chain(&b.linvel)
                .chain(&b.angvel),
        )
        .any(|(a, b)| (a - b).abs() > epsilon || !a.is_finite() || !b.is_finite())
}
