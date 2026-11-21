//! Tick-aligned, transport-independent authoritative networking at 60 Hz.
mod client;
pub mod interpolation;
pub mod protocol;
mod server;
pub mod simulate;
pub mod transport;

pub use client::{ClientConfig, NetClient};
use engine_runtime::Input;
pub use interpolation::Interpolator;
pub use protocol::{Actions, ClientMessage, InputFrame, PeerId, ServerMessage, Snapshot};
use serde::Serialize;
pub use server::{NetServer, ServerConfig};

pub const TICK_MS: f64 = 1000.0 / 60.0;

#[derive(Clone, Debug, Default, Serialize)]
pub struct NetStats {
    pub rtt_ms: f64,
    pub lead_ticks: u64,
    pub corrections: u64,
    pub max_prediction_error: f32,
    pub avg_prediction_error: f64,
    pub snapshot_bytes_per_second: f64,
    pub late_inputs: u64,
    /// Estimated missing snapshot intervals (may include reordered packets).
    pub packets_lost: u64,
}

pub(crate) fn inputs(tick: u64, entity: u64, actions: &Actions, previous: &Actions) -> Vec<Input> {
    let mut values: Actions = previous.keys().map(|key| (key.clone(), 0.0)).collect();
    values.extend(actions.clone());
    values
        .into_iter()
        .map(|(action, value)| Input {
            tick,
            entity: Some(entity),
            action,
            value,
        })
        .collect()
}

pub(crate) fn valid_actions(actions: &Actions) -> bool {
    actions.len() <= 64
        && actions
            .iter()
            .all(|(key, value)| !key.is_empty() && key.len() <= 128 && value.is_finite())
}
