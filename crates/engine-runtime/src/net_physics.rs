use super::*;
use crate::NetEntityState;

impl Clone for Physics {
    fn clone(&self) -> Self {
        Self {
            // Rapier's non-Clone pipeline holds only disposable solver workspace.
            pipeline: PhysicsPipeline::new(),
            islands: self.islands.clone(),
            broad: self.broad.clone(),
            narrow: self.narrow.clone(),
            bodies: self.bodies.clone(),
            colliders: self.colliders.clone(),
            joints: self.joints.clone(),
            multibody: self.multibody.clone(),
            ccd: self.ccd.clone(),
            handles: self.handles.clone(),
            root: self.root.clone(),
            terrain: self.terrain.clone(),
            terrain_sources: self.terrain_sources.clone(),
            terrain_handles: self.terrain_handles.clone(),
            query: self.query.clone(),
            owners: self.owners.clone(),
        }
    }
}

impl Physics {
    pub(crate) fn net_velocities(&self, id: u64) -> ([f32; 3], [f32; 3]) {
        self.handles.get(&id).map_or(([0.0; 3], [0.0; 3]), |handle| {
            let body = &self.bodies[*handle];
            ((*body.linvel()).into(), (*body.angvel()).into())
        })
    }

    pub(crate) fn apply_net_pose(&mut self, state: &NetEntityState, transform: &Transform) {
        if let Some(handle) = self.handles.get(&state.id) {
            let body = &mut self.bodies[*handle];
            body.set_position(pose(transform), true);
            body.set_linvel(state.linvel.into(), true);
            body.set_angvel(state.angvel.into(), true);
        }
        self.bodies.propagate_modified_body_positions_to_colliders(&mut self.colliders);
    }
}
