use anyhow::{bail, Result};
use rapier3d::prelude::*;
use shared::components::{BodyType, Collider as Shape};
use shared::scene::{Entity, Scene, Transform};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) enum Force {
    Impulse(u64, [f32; 3]),
    Velocity(u64, [f32; 3]),
}

pub(crate) struct Physics {
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad: BroadPhaseMultiSap,
    narrow: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    joints: ImpulseJointSet,
    multibody: MultibodyJointSet,
    ccd: CCDSolver,
    handles: BTreeMap<u64, RigidBodyHandle>,
}

fn pose(t: &Transform) -> Isometry<Real> {
    Isometry::from_parts(
        Translation::from(vector![t.position[0], t.position[1], t.position[2]]),
        Rotation::from_euler_angles(
            t.rotation_deg[0].to_radians(),
            t.rotation_deg[1].to_radians(),
            t.rotation_deg[2].to_radians(),
        ),
    )
}

impl Physics {
    pub(crate) fn new() -> Self {
        Self {
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad: BroadPhaseMultiSap::new(),
            narrow: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            joints: ImpulseJointSet::new(),
            multibody: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
            handles: BTreeMap::new(),
        }
    }

    pub(crate) fn reconcile(&mut self, scene: &Scene) -> Result<()> {
        let live: BTreeSet<_> = scene.entities.iter().map(|e| e.id).collect();
        let removed: Vec<_> = self
            .handles
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in removed {
            let handle = self.handles.remove(&id).unwrap();
            self.bodies.remove(
                handle,
                &mut self.islands,
                &mut self.colliders,
                &mut self.joints,
                &mut self.multibody,
                true,
            );
        }
        for entity in &scene.entities {
            if self.handles.contains_key(&entity.id)
                || (entity.collider.is_none() && entity.rigidbody.is_none())
            {
                continue;
            }
            let config = entity.rigidbody.clone().unwrap_or_default();
            let friction_rule = match entity.props.get("ai_friction_combine").and_then(|v| v.as_str()) {
                None | Some("average") => CoefficientCombineRule::Average,
                Some("min") => CoefficientCombineRule::Min,
                Some("max") => CoefficientCombineRule::Max,
                Some("multiply") => CoefficientCombineRule::Multiply,
                Some(_) => bail!("ai_friction_combine must be average, min, max, or multiply"),
            };
            if !config.mass.is_finite()
                || config.mass <= 0.0
                || !config.friction.is_finite()
                || config.friction < 0.0
                || !(0.0..=1.0).contains(&config.restitution)
            {
                bail!(
                    "entity {}: mass must be positive, friction nonnegative, restitution in [0,1]",
                    entity.name
                );
            }
            let builder = match config.kind {
                BodyType::Static => RigidBodyBuilder::fixed(),
                BodyType::Dynamic => RigidBodyBuilder::dynamic(),
                BodyType::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
            };
            let collider = entity
                .collider
                .as_ref()
                .map(|shape| collider(entity, shape))
                .transpose()?;
            let builder = if collider.is_none() {
                builder.additional_mass(config.mass)
            } else {
                builder
            };
            let builder = if entity.props["ai_lock_rotation"].as_bool() == Some(true) { builder.lock_rotations() } else { builder };
            let handle = self
                .bodies
                .insert(builder.position(pose(&entity.transform)).build());
            if let Some(collider) = collider {
                self.colliders.insert_with_parent(
                    collider
                        .mass(config.mass)
                        .friction(config.friction)
                        .friction_combine_rule(friction_rule)
                        .restitution(config.restitution)
                        .build(),
                    handle,
                    &mut self.bodies,
                );
            }
            self.handles.insert(entity.id, handle);
        }
        Ok(())
    }

    pub(crate) fn step(
        &mut self,
        scene: &mut Scene,
        dirty: &BTreeSet<u64>,
        forces: Vec<Force>,
    ) -> Result<()> {
        self.reconcile(scene)?;
        for entity in &scene.entities {
            if dirty.contains(&entity.id) {
                if let Some(handle) = self.handles.get(&entity.id) {
                    self.bodies[*handle].set_position(pose(&entity.transform), true);
                }
            }
        }
        for force in forces {
            let (id, value, impulse) = match force {
                Force::Impulse(id, v) => (id, v, true),
                Force::Velocity(id, v) => (id, v, false),
            };
            if let Some(handle) = self.handles.get(&id) {
                let body = &mut self.bodies[*handle];
                let vector = vector![value[0], value[1], value[2]];
                if impulse {
                    body.apply_impulse(vector, true);
                } else {
                    body.set_linvel(vector, true);
                }
            } else if scene.entities.iter().any(|e| e.id == id) {
                bail!("entity {id} needs a rigidbody for impulse/velocity");
            }
        }
        let parameters = IntegrationParameters {
            dt: super::DT,
            ..Default::default()
        };
        self.pipeline.step(
            &vector![0.0, -9.81, 0.0],
            &parameters,
            &mut self.islands,
            &mut self.broad,
            &mut self.narrow,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.joints,
            &mut self.multibody,
            &mut self.ccd,
            None,
            &(),
            &(),
        );
        for entity in &mut scene.entities {
            if let Some(handle) = self.handles.get(&entity.id) {
                let body = &self.bodies[*handle];
                entity.transform.position = [
                    body.translation().x,
                    body.translation().y,
                    body.translation().z,
                ];
                let (x, y, z) = body.rotation().euler_angles();
                entity.transform.rotation_deg = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
            }
        }
        Ok(())
    }

    pub(crate) fn motion(&self) -> (BTreeMap<u64,[f32;3]>, BTreeSet<u64>) {
        let mut velocities=BTreeMap::new(); let mut grounded=BTreeSet::new();
        for (id,handle) in &self.handles {
            let body=&self.bodies[*handle]; let v=body.linvel();
            velocities.insert(*id,[v.x,v.y,v.z]);
            for collider in body.colliders() {
                if self.narrow.contact_pairs_with(*collider).any(|pair| pair.has_any_active_contact && pair.manifolds.iter().any(|manifold| {
                    let normal=if pair.collider1 == *collider { -manifold.data.normal.y } else {manifold.data.normal.y};
                    normal>0.6 && !manifold.data.solver_contacts.is_empty()
                })) { grounded.insert(*id); }
            }
        }
        (velocities,grounded)
    }
}

fn collider(entity: &Entity, shape: &Shape) -> Result<ColliderBuilder> {
    let positions: Vec<[f32; 3]> =
        if matches!(entity.mesh, shared::scene::PrimitiveKind::Gltf { .. })
            && entity.props.get("web_mesh_bounds").is_some()
        {
            let [min, max]: [[f32; 3]; 2] =
                serde_json::from_value(entity.props["web_mesh_bounds"].clone())?;
            if (0..3).any(|i| !min[i].is_finite() || !max[i].is_finite() || min[i] > max[i]) {
                bail!("invalid web mesh bounds");
            }
            (0..8)
                .map(|n| std::array::from_fn(|i| if n & (1 << i) == 0 { min[i] } else { max[i] }))
                .collect()
        } else {
            let mesh = match &entity.mesh {
                shared::scene::PrimitiveKind::Gltf { path } => {
                    engine_geo::load_gltf(std::path::Path::new(path))?
                }
                kind => engine_geo::generate(kind),
            };
            mesh.vertices.into_iter().map(|v| v.position).collect()
        };
    if positions.is_empty() {
        bail!("entity {} has no mesh bounds", entity.name);
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in positions {
        for i in 0..3 {
            let p = vertex[i] * entity.transform.scale[i];
            min[i] = min[i].min(p);
            max[i] = max[i].max(p);
        }
    }
    let half = std::array::from_fn::<_, 3, _>(|i| ((max[i] - min[i]) * 0.5).max(0.025));
    let center = std::array::from_fn::<_, 3, _>(|i| (max[i] + min[i]) * 0.5);
    let scale = entity.transform.scale.map(f32::abs);
    let builder = match shape {
        Shape::Box { half_extents } => {
            let half = half_extents
                .map(|v| std::array::from_fn(|i| v[i] * scale[i]))
                .unwrap_or(half);
            if half.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                bail!("box half_extents must be positive");
            }
            ColliderBuilder::cuboid(half[0], half[1], half[2])
        }
        Shape::Mesh => ColliderBuilder::cuboid(half[0], half[1], half[2]),
        Shape::Sphere { radius } => {
            let r = radius
                .map(|r| r * scale.iter().copied().fold(0.0, f32::max))
                .unwrap_or(half.iter().copied().fold(0.0, f32::max));
            if !r.is_finite() || r <= 0.0 {
                bail!("sphere radius must be positive");
            }
            ColliderBuilder::ball(r)
        }
        Shape::Capsule {
            radius,
            half_height,
        } => {
            let r = radius
                .map(|r| r * scale[0].max(scale[2]))
                .unwrap_or(half[0].max(half[2]));
            let height = half_height
                .map(|h| h * scale[1])
                .unwrap_or((half[1] - r).max(0.0));
            if !r.is_finite() || r <= 0.0 || !height.is_finite() || height < 0.0 {
                bail!("capsule radius must be positive and half_height nonnegative");
            }
            ColliderBuilder::capsule_y(height, r)
        }
    };
    Ok(builder.translation(vector![center[0], center[1], center[2]]))
}
