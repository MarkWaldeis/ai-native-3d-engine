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
    root: std::path::PathBuf,
    terrain: BTreeMap<u64, (String, engine_terrain::Terrain)>,
    terrain_handles: BTreeMap<(u64, u32, u32), ColliderHandle>,
    query: QueryPipeline,
    owners: std::collections::HashMap<RigidBodyHandle, u64>,
}

pub(crate) struct RayHit {
    pub id: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub distance: f32,
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

fn nearby_terrain_tiles(
    config: &engine_terrain::TerrainConfig,
    offset: [f32; 3],
    centers: &[[f64; 2]],
    radius: f64,
) -> BTreeSet<usize> {
    let grid = config.grid();
    let mut tiles = BTreeSet::new();
    for center in centers {
        let local = [center[0] - offset[0] as f64, center[1] - offset[2] as f64];
        let lower: [i64; 2] = std::array::from_fn(|i| {
            (((local[i] - radius) / config.tile_m).ceil() - 1.0).clamp(0.0, grid[i] as f64) as i64
        });
        let upper: [i64; 2] = std::array::from_fn(|i| {
            ((local[i] + radius) / config.tile_m)
                .floor()
                .clamp(-1.0, grid[i] as f64 - 1.0) as i64
        });
        for z in lower[1]..=upper[1] {
            for x in lower[0]..=upper[0] {
                let min_x = x as f64 * config.tile_m;
                let min_z = z as f64 * config.tile_m;
                if (local[0] - local[0].clamp(min_x, min_x + config.tile_m)).powi(2)
                    + (local[1] - local[1].clamp(min_z, min_z + config.tile_m)).powi(2)
                    <= radius * radius
                {
                    tiles.insert(z as usize * grid[0] as usize + x as usize);
                }
            }
        }
    }
    tiles
}

#[cfg(test)]
mod terrain_tests {
    use super::*;

    #[test]
    fn mesh_and_parry_heightfield_use_the_same_diagonal() {
        let config = engine_terrain::TerrainConfig {
            size_m: [256.0; 2], samples: 2, height_scale: 0.0, ..Default::default()
        };
        let mut terrain = engine_terrain::Terrain::generate(config.clone()).unwrap();
        let tile = &mut terrain.tiles[0];
        tile.heights[10] = 200.0;
        let mesh = tile.mesh(&config);
        let heightfield = rapier3d::parry::shape::HeightField::with_flags(
            DMatrix::from_fn(2, 2, |row, col| tile.height(&config, col, row)),
            vector![256.0, 1.0, 256.0],
            rapier3d::parry::shape::HeightFieldFlags::FIX_INTERNAL_EDGES,
        );
        let (a, b) = heightfield.triangles_at(0, 0);
        for (triangle, indices) in [a.unwrap(), b.unwrap()].iter().zip(mesh.indices.chunks(3)) {
            for point in [triangle.a, triangle.b, triangle.c] {
                let local = [point.x + 128.0, point.y, point.z + 128.0];
                assert!(indices.iter().any(|i| mesh.vertices[*i as usize].position == local));
            }
        }
        assert_eq!(terrain.sample(128.0, 128.0).unwrap(), 0.0);
    }

    #[test]
    fn bounded_candidates_match_full_grid_including_edges_and_outside() {
        let config = engine_terrain::TerrainConfig {
            size_m: [1024.0, 768.0],
            ..Default::default()
        };
        let offset = [100.0, 0.0, -200.0];
        let centers = [
            [356.0, 56.0],
            [100.0, -200.0],
            [1200.0, 600.0],
            [-900.0, -1200.0],
        ];
        for radius in [0.0, 1.0, 100.0, 256.0, 1024.0] {
            let grid = config.grid();
            let mut expected = BTreeSet::new();
            for z in 0..grid[1] {
                for x in 0..grid[0] {
                    let min_x = offset[0] as f64 + x as f64 * config.tile_m;
                    let min_z = offset[2] as f64 + z as f64 * config.tile_m;
                    if centers.iter().any(|p: &[f64; 2]| {
                        (p[0] - p[0].clamp(min_x, min_x + config.tile_m)).powi(2)
                            + (p[1] - p[1].clamp(min_z, min_z + config.tile_m)).powi(2)
                            <= radius * radius
                    }) {
                        expected.insert((z * grid[0] + x) as usize);
                    }
                }
            }
            assert_eq!(
                nearby_terrain_tiles(&config, offset, &centers, radius),
                expected
            );
            let reversed: Vec<_> = centers.iter().rev().copied().collect();
            assert_eq!(
                nearby_terrain_tiles(&config, offset, &reversed, radius),
                expected
            );
        }
        assert!(nearby_terrain_tiles(&config, offset, &[], 512.0).is_empty());
        assert!(nearby_terrain_tiles(&config, offset, &[[1e6, 1e6]], 10.0).is_empty());
        assert_eq!(
            nearby_terrain_tiles(&config, offset, &[[356.0, 56.0]], 0.0).len(),
            4
        );
    }
}

impl Physics {
    pub(crate) fn new(root: std::path::PathBuf) -> Self {
        Self {
            root,
            terrain: BTreeMap::new(),
            terrain_handles: BTreeMap::new(),
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
            query: QueryPipeline::new(),
            owners: std::collections::HashMap::new(),
        }
    }

    pub(crate) fn refresh_queries(&mut self) {
        self.query.update(&self.colliders);
    }

    /// Casts a ray against enabled, non-sensor colliders. `id` 0 means terrain/unowned.
    pub(crate) fn raycast(&self, origin: [f32; 3], dir: [f32; 3], max_dist: f32, exclude: Option<u64>) -> Option<RayHit> {
        let length = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if !(length > 1e-6) || !(max_dist > 0.0) || origin.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let ray = Ray::new(
            point![origin[0], origin[1], origin[2]],
            vector![dir[0] / length, dir[1] / length, dir[2] / length],
        );
        let bodies = &self.bodies;
        let enabled = |_: ColliderHandle, collider: &Collider| {
            collider.parent().map_or(true, |parent| bodies.get(parent).is_some_and(|b| b.is_enabled()))
        };
        let mut filter = QueryFilter::new().exclude_sensors().predicate(&enabled);
        if let Some(handle) = exclude.and_then(|id| self.handles.get(&id)) {
            filter = filter.exclude_rigid_body(*handle);
        }
        let (collider, hit) = self.query.cast_ray_and_get_normal(bodies, &self.colliders, &ray, max_dist, true, filter)?;
        let id = self.colliders[collider]
            .parent()
            .and_then(|parent| self.owners.get(&parent).copied())
            .or_else(|| self.terrain_handles.iter().find(|(_, h)| **h == collider).map(|(k, _)| k.0))
            .unwrap_or(0);
        let p = ray.point_at(hit.time_of_impact);
        Some(RayHit {
            id,
            point: [p.x, p.y, p.z],
            normal: [hit.normal.x, hit.normal.y, hit.normal.z],
            distance: hit.time_of_impact,
        })
    }

    pub(crate) fn reconcile(&mut self, scene: &Scene) -> Result<()> {
        self.reconcile_terrain(scene)?;
        let live: BTreeSet<_> = scene.entities.iter().map(|e| e.id).collect();
        let removed: Vec<_> = self
            .handles
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in removed {
            let handle = self.handles.remove(&id).unwrap();
            self.owners.remove(&handle);
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
            self.owners.insert(handle, entity.id);
        }
        Ok(())
    }

    fn reconcile_terrain(&mut self, scene: &Scene) -> Result<()> {
        let mut wanted = BTreeSet::new();
        let mut live = BTreeSet::new();
        let centers: Vec<_> = scene
            .entities
            .iter()
            .filter(|e| {
                e.tags.iter().any(|t| t == "player")
                    || e.rigidbody
                        .as_ref()
                        .is_some_and(|r| r.kind == BodyType::Dynamic)
            })
            .map(|e| {
                [
                    e.transform.position[0] as f64,
                    e.transform.position[2] as f64,
                ]
            })
            .collect();
        let mut entities: Vec<_> = scene.entities.iter().collect();
        entities.sort_by_key(|e| e.id);
        for entity in entities {
            let Some(path) = entity.props.get("ai_terrain").and_then(|v| v.as_str()) else {
                continue;
            };
            anyhow::ensure!(
                entity.transform.rotation_deg == [0.0; 3] && entity.transform.scale == [1.0; 3],
                "terrain requires identity rotation and scale"
            );
            live.insert(entity.id);
            if self
                .terrain
                .get(&entity.id)
                .is_some_and(|(old, _)| old != path)
            {
                self.terrain.remove(&entity.id);
                let keys: Vec<_> = self
                    .terrain_handles
                    .keys()
                    .filter(|k| k.0 == entity.id)
                    .copied()
                    .collect();
                for key in keys {
                    let h = self.terrain_handles.remove(&key).unwrap();
                    self.colliders
                        .remove(h, &mut self.islands, &mut self.bodies, true);
                }
            }
            if !self.terrain.contains_key(&entity.id) {
                let file = engine_core::project_path(&self.root, std::path::Path::new(path))?;
                self.terrain.insert(
                    entity.id,
                    (path.to_owned(), engine_terrain::Terrain::load(&file)?),
                );
            }
            let radius = entity
                .props
                .get("ai_terrain_radius")
                .and_then(|v| v.as_f64())
                .unwrap_or(512.0);
            anyhow::ensure!(
                radius.is_finite() && radius >= 0.0,
                "terrain radius must be nonnegative"
            );
            let terrain = &self.terrain[&entity.id].1;
            let config = &terrain.manifest.config;
            for index in nearby_terrain_tiles(config, entity.transform.position, &centers, radius) {
                let tile = &terrain.tiles[index];
                let x = entity.transform.position[0] as f64 + tile.x as f64 * config.tile_m;
                let z = entity.transform.position[2] as f64 + tile.z as f64 * config.tile_m;
                let key = (entity.id, tile.x, tile.z);
                wanted.insert(key);
                if let Some(h) = self.terrain_handles.get(&key) {
                    self.colliders[*h].set_translation(vector![
                        (x + config.tile_m / 2.0) as f32,
                        entity.transform.position[1],
                        (z + config.tile_m / 2.0) as f32
                    ]);
                    continue;
                }
                let n = config.samples;
                let heights = DMatrix::from_fn(n, n, |row, col| tile.height(config, col, row));
                let collider = ColliderBuilder::heightfield_with_flags(
                    heights,
                    vector![config.tile_m as f32, 1.0, config.tile_m as f32],
                    rapier3d::parry::shape::HeightFieldFlags::FIX_INTERNAL_EDGES,
                )
                .translation(vector![
                    (x + config.tile_m / 2.0) as f32,
                    entity.transform.position[1],
                    (z + config.tile_m / 2.0) as f32
                ])
                .build();
                self.terrain_handles
                    .insert(key, self.colliders.insert(collider));
            }
        }
        let removed: Vec<_> = self
            .terrain_handles
            .keys()
            .filter(|k| !wanted.contains(k))
            .copied()
            .collect();
        for key in removed {
            let h = self.terrain_handles.remove(&key).unwrap();
            self.colliders
                .remove(h, &mut self.islands, &mut self.bodies, true);
        }
        self.terrain.retain(|id, _| live.contains(id));
        Ok(())
    }

    pub(crate) fn step(
        &mut self,
        scene: &mut Scene,
        dirty: &BTreeSet<u64>,
        forces: Vec<Force>,
        inactive: &BTreeSet<u64>,
    ) -> Result<()> {
        self.reconcile(scene)?;
        for (id, handle) in &self.handles {
            let body = &mut self.bodies[*handle];
            let enabled = body.is_fixed() || !inactive.contains(id);
            if body.is_enabled() != enabled {
                body.set_enabled(enabled);
                if enabled { body.wake_up(true); }
            }
        }
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
            Some(&mut self.query),
            &(),
            &(),
        );
        for entity in &mut scene.entities {
            if let Some(handle) = self.handles.get(&entity.id) {
                let body = &self.bodies[*handle];
                if !body.is_enabled() { continue; }
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
