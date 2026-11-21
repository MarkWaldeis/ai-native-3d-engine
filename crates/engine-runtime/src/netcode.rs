use super::{ActiveRegion, Data, ScriptInstance, Simulation};
use crate::physics::Physics;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared::scene::Entity;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ragdoll: Option<NetRagdollState>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct NetRagdollState {
    pub active: bool,
    pub enabled: bool,
    pub upright_rotation_deg: [f32; 3],
    pub parts: Vec<NetBoneState>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct NetBoneState {
    pub name: String,
    pub position: [f32; 3],
    pub rotation_deg: [f32; 3],
    pub linvel: [f32; 3],
    pub angvel: [f32; 3],
    pub radius: f32,
    pub half_height: f32,
    pub sleeping: bool,
}

impl Simulation {
    pub fn net_next_id(&self) -> u64 {
        self.data.borrow().next_id
    }

    pub fn apply_net_next_id(&mut self, next_id: u64) -> Result<()> {
        let mut data=self.data.borrow_mut();
        let minimum=data.scene.entities.iter().map(|entity|entity.id).max().unwrap_or(0)+1;
        ensure!((minimum..=i64::MAX as u64+1).contains(&next_id), "authoritative next_id must exceed all live entity IDs and fit the allocation range");
        data.next_id=next_id;
        Ok(())
    }

    /// Scripted entities whose on_start hook has not yet been prepared.
    pub fn net_pending_starts(&self) -> Vec<u64> {
        self.data.borrow().scene.entities.iter()
            .filter(|entity| entity.script.is_some() && !self.scripts.contains_key(&entity.id))
            .map(|entity|entity.id).collect()
    }

    /// Restore start bookkeeping after authoritative definitions and script state.
    pub fn apply_net_pending_starts(&mut self, pending: &[u64]) -> Result<()> {
        self.prepare_net_scripts(false)?;
        self.scripts.retain(|id,_| !pending.contains(id));
        Ok(())
    }

    /// Insert or replace an authoritative entity without replaying its on_start hook.
    /// Script state and body velocities are supplied separately by the snapshot.
    pub fn insert_net_entity(&mut self, mut entity: Entity) -> Result<()> {
        super::validate_entity(&entity)?;
        if let Some(animator)=&mut entity.animator {
            if !animator.machine.states.contains_key(&animator.current_state)
                || animator.previous_state.as_ref().is_some_and(|state| !animator.machine.states.contains_key(state))
                || animator.machine.parameters.keys().any(|key| !animator.values.contains_key(key)) {
                crate::animation::reset(animator);
            }
        }
        let next = entity.id.checked_add(1).context("entity id overflow")?;
        ensure!(
            self.data.borrow().entity_index(entity.id).is_some()
                || self.data.borrow().scene.entities.len() < super::MAX_ENTITIES,
            "simulation entity limit exceeded"
        );
        let saved = self.save_state();
        let result = (|| {
            self.remove_net_entity(entity.id)?;
            {
                let mut data = self.data.borrow_mut();
                data.next_id = data.next_id.max(next);
                data.scene.entities.push(entity);
                data.scene.entities.sort_by_key(|e| e.id);
                data.refresh_player();
            }
            self.sync_physics()?;
            self.prepare_net_scripts(false)?;
            Ok(())
        })();
        if result.is_err() {
            self.restore_state(&saved);
        }
        result
    }

    /// Remove an authoritative entity and its body/script bookkeeping. Missing IDs are harmless.
    pub fn remove_net_entity(&mut self, id: u64) -> Result<()> {
        {
            let mut data = self.data.borrow_mut();
            data.scene.entities.retain(|e| e.id != id);
            data.entity_inputs.remove(&id);
            data.velocities.remove(&id);
            data.grounded.remove(&id);
            data.dirty.remove(&id);
            data.refresh_player();
        }
        self.scripts.remove(&id);
        self.inactive.remove(&id);
        self.sync_physics()
    }

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
        let mut states: Vec<_> = data
            .scene
            .entities
            .iter()
            .map(|entity| {
                let (linvel, angvel) = physics.net_velocities(entity.id);
                NetEntityState {
                    id: entity.id,
                    position: entity.transform.position,
                    rotation_deg: entity.transform.rotation_deg,
                    linvel,
                    angvel,
                    ragdoll: physics.net_ragdoll_state(entity.id),
                }
            })
            .collect();
        states.sort_by_key(|state| state.id);
        states
    }

    /// Correct known entities; unknown IDs are ignored, not spawned.
    pub fn apply_net_entity_states(&mut self, states: &[NetEntityState]) -> Result<()> {
        let saved = self.save_state();
        let result = self.apply_net_states_inner(states);
        if result.is_err() {
            self.restore_state(&saved);
        }
        result
    }

    fn apply_net_states_inner(&mut self, states: &[NetEntityState]) -> Result<()> {
        let mut data = self.data.borrow_mut();
        let mut physics = self.physics.borrow_mut();
        for state in states {
            if data.entity_index(state.id).is_some() {
                if let Some(ragdoll) = state.ragdoll.as_ref().filter(|rig| !rig.active) {
                    physics.prepare_net_ragdoll(state.id, ragdoll)?;
                }
            }
        }
        for state in states {
            if let Some(index) = data.entity_index(state.id) {
                if let Some(ragdoll) = state.ragdoll.as_ref().filter(|rig| rig.active) {
                    physics.prepare_net_ragdoll(state.id, ragdoll)?;
                }
                let transform = &mut data.scene.entities[index].transform;
                transform.position = state.position;
                transform.rotation_deg = state.rotation_deg;
                physics.apply_net_pose(state, transform);
                if let Some(ragdoll) = &state.ragdoll {
                    physics.apply_net_bones(state.id, ragdoll);
                }
                data.velocities.insert(state.id, state.linvel);
            }
        }
        physics.refresh_queries();
        Ok(())
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
