use super::{ActiveRegion, Data, ScriptInstance, Simulation};
use crate::physics::Physics;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// In-process rollback checkpoint, including physics contacts and script bookkeeping.
#[derive(Clone)]
pub struct SimState {
    data: Data,
    physics: Physics,
    scripts: BTreeMap<u64, ScriptInstance>,
    inactive: BTreeSet<u64>,
    active_region: Option<ActiveRegion>,
    tick: u64,
    failed: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct NetEntityState {
    pub id: u64,
    pub position: [f32; 3],
    pub rotation_deg: [f32; 3],
    pub linvel: [f32; 3],
    pub angvel: [f32; 3],
}

impl Simulation {
    pub fn save_state(&self) -> SimState {
        SimState {
            data: self.data.borrow().clone(),
            physics: self.physics.borrow().clone(),
            scripts: self.scripts.clone(),
            inactive: self.inactive.clone(),
            active_region: self.active_region,
            tick: self.tick,
            failed: self.failed,
        }
    }

    pub fn restore_state(&mut self, s: &SimState) {
        *self.data.borrow_mut() = s.data.clone();
        *self.physics.borrow_mut() = s.physics.clone();
        self.scripts = s.scripts.clone();
        self.inactive = s.inactive.clone();
        self.active_region = s.active_region;
        self.tick = s.tick;
        self.failed = s.failed;
    }

    pub fn net_entity_states(&self) -> Vec<NetEntityState> {
        let data = self.data.borrow();
        let physics = self.physics.borrow();
        let mut states: Vec<_> = data.scene.entities.iter().map(|entity| {
            let (linvel, angvel) = physics.net_velocities(entity.id);
            NetEntityState {
                id: entity.id,
                position: entity.transform.position,
                rotation_deg: entity.transform.rotation_deg,
                linvel,
                angvel,
            }
        }).collect();
        states.sort_by_key(|state| state.id);
        states
    }

    /// Correct known entities; unknown IDs are ignored, not spawned.
    pub fn apply_net_entity_states(&mut self, states: &[NetEntityState]) {
        let mut data = self.data.borrow_mut();
        let mut physics = self.physics.borrow_mut();
        for state in states {
            if let Some(index) = data.entity_index(state.id) {
                let transform = &mut data.scene.entities[index].transform;
                transform.position = state.position;
                transform.rotation_deg = state.rotation_deg;
                physics.apply_net_pose(state, transform);
                data.velocities.insert(state.id, state.linvel);
            }
        }
        physics.refresh_queries();
    }

    pub fn net_script_state(&self) -> Value {
        serde_json::to_value(&self.data.borrow().state).expect("JSON script state")
    }

    /// Replace the shared script-state map. Non-object values are ignored.
    pub fn apply_net_script_state(&mut self, v: Value) {
        if let Value::Object(map) = v {
            self.data.borrow_mut().state = map.into_iter().collect();
        }
    }
}
