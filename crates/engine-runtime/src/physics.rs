use anyhow::{bail, Context, Result};
use crate::physics_config::{Config, Filter, Query};
use rapier3d::prelude::*;
use rapier3d::parry::bounding_volume::{Aabb, BoundingVolume};
use rapier3d::parry::query::ShapeCastOptions;
use serde::Serialize;
use serde_json::{json, Value};
use shared::components::{BodyType, Collider as Shape};
use shared::scene::{Entity, Scene, Transform};
use std::collections::{BTreeMap, BTreeSet};
use crate::terrain_stream::{TerrainSources, SharedTerrain};

#[path = "physics_gameplay.rs"]
mod gameplay;

pub(crate) enum Force {
    Impulse(u64, [f32; 3]),
    Velocity(u64, [f32; 3]),
    Applied(u64, [f32; 3]),
    Ragdoll(u64, bool),
    Hit(u64, String, [f32; 3]),
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
    terrain: BTreeMap<u64, (String, SharedTerrain)>,
    pub(crate) terrain_sources: TerrainSources,
    terrain_handles: BTreeMap<(u64, u32, u32), ColliderHandle>,
    terrain_owners: std::collections::HashMap<ColliderHandle, u64>,
    query: QueryPipeline,
    owners: std::collections::HashMap<RigidBodyHandle, u64>,
    forced: Vec<RigidBodyHandle>,
    gameplay: gameplay::Gameplay,
}

#[derive(Serialize)]
pub(crate) struct RayHit {
    pub id: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub distance: f32,
    pub hitbox: Option<String>,
    pub damage_multiplier: f64,
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

fn terrain_tile_ranges(config: &engine_terrain::TerrainConfig, min: [f64; 2], max: [f64; 2]) -> [std::ops::RangeInclusive<i64>; 2] {
    let grid = config.grid();
    std::array::from_fn(|i| {
        let lower = ((min[i] / config.tile_m).ceil() - 1.0).clamp(0.0, grid[i] as f64) as i64;
        let upper = (max[i] / config.tile_m).floor().clamp(-1.0, grid[i] as f64 - 1.0) as i64;
        lower..=upper
    })
}

pub(crate) fn nearby_terrain_tiles(
    config: &engine_terrain::TerrainConfig,
    offset: [f32; 3],
    centers: &[[f64; 2]],
    radius: f64,
) -> BTreeSet<usize> {
    let grid = config.grid();
    let mut tiles = BTreeSet::new();
    for center in centers {
        let local = [center[0] - offset[0] as f64, center[1] - offset[2] as f64];
        let [xs, zs] = terrain_tile_ranges(config, local.map(|v| v - radius), local.map(|v| v + radius));
        for z in zs {
            for x in xs.clone() {
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
            terrain_sources: Default::default(),
            terrain_handles: BTreeMap::new(),
            terrain_owners: std::collections::HashMap::new(),
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
            forced: Vec::new(),
            gameplay: gameplay::Gameplay::default(),
        }
    }

    pub(crate) fn refresh_queries(&mut self) {
        self.query.update(&self.colliders);
    }

    pub(crate) fn prepare_nav_queries(&mut self) {
        let compounds: Vec<_> = self.colliders.iter().filter_map(|(handle, collider)| {
            let parent = collider.parent()?;
            let parts = collider.shape().as_compound()?.shapes().to_vec();
            Some((handle, parent, *collider.position_wrt_parent()?, parts))
        }).collect();
        for (handle, parent, pose, parts) in compounds {
            self.colliders.remove(handle, &mut self.islands, &mut self.bodies, false);
            for (local, shape) in parts {
                self.colliders.insert_with_parent(
                    ColliderBuilder::new(shape).position(pose * local).build(),
                    parent,
                    &mut self.bodies,
                );
            }
        }
        self.refresh_queries();
    }

    pub(crate) fn nav_bounds(&self) -> Vec<(u64, [f32; 3], [f32; 3])> {
        self.colliders.iter().filter(|(_, c)| !c.is_sensor()).map(|(_, c)| {
            let a=c.compute_aabb();
            let id=c.parent().and_then(|p|self.owners.get(&p).copied()).unwrap_or(0);
            (id,[a.mins.x,a.mins.y,a.mins.z],[a.maxs.x,a.maxs.y,a.maxs.z])
        }).collect()
    }

    pub(crate) fn nav_surfaces(&self, x:f32,z:f32,top:f32,bottom:f32,slope:f32) -> Vec<(f32,ColliderHandle)> {
        let ray=Ray::new(point![x,top,z],vector![0.0,-1.0,0.0]);
        let mut hits=vec![];
        self.query.intersections_with_ray(&self.bodies,&self.colliders,&ray,top-bottom,true,QueryFilter::new().exclude_sensors(),|handle,hit|{
            if hit.normal.y >= slope { hits.push((top-hit.time_of_impact,handle)); } true
        });
        hits.sort_by(|a,b|a.0.total_cmp(&b.0).then_with(||a.1.into_raw_parts().cmp(&b.1.into_raw_parts())));
        hits
    }

    pub(crate) fn nav_clear(&self,x:f32,y:f32,z:f32,half:f32,height:f32,supports:&[ColliderHandle]) -> bool {
        let shape=Cuboid::new(vector![half,height*0.5,half]);
        let pose=Isometry::translation(x,y+height*0.5+0.025,z);
        let allowed=|h:ColliderHandle,_:&Collider|!supports.contains(&h);
        let mut clear=true;
        self.query.intersections_with_shape(&self.bodies,&self.colliders,&pose,&shape,QueryFilter::new().exclude_sensors().predicate(&allowed),|_|{clear=false;false});
        clear
    }

    /// Casts a ray against enabled, non-sensor colliders. `id` 0 means terrain/unowned.
    pub(crate) fn raycast(&self, origin: [f32; 3], dir: [f32; 3], max_dist: f32, exclude: Option<u64>) -> Option<RayHit> {
        if !max_dist.is_finite() || max_dist <= 0.0 || origin.iter().chain(&dir).any(|v| !v.is_finite()) {
            return None;
        }
        let direction = Vector::from(dir).try_normalize(1e-6)?;
        self.cast_ray(Point::from(origin), direction, max_dist, Filter { exclude, ..Filter::default() })
    }

    fn query_filter<'a>(options: Filter, predicate: &'a impl Fn(ColliderHandle, &Collider) -> bool) -> QueryFilter<'a> {
        let mut filter = QueryFilter::new().predicate(predicate);
        if let Some(groups) = options.groups {
            filter = filter.groups(groups.into());
        }
        filter
    }

    fn allowed(&self, handle: ColliderHandle, collider: &Collider, options: Filter, ray: bool) -> bool {
        let owner = self.owner(handle);
        let region = self.gameplay.hitboxes.get(&handle);
        let ray_only = region.is_some_and(|region| region.ray_only);
        (ray || !ray_only)
            && (!ray || region.is_some() || !self.gameplay.named_owners.contains(&owner))
            && (options.include_sensors || !collider.is_sensor() || ray_only)
            && collider.is_enabled()
            && collider.parent().is_none_or(|parent| self.bodies.get(parent).is_some_and(RigidBody::is_enabled))
            && options.exclude != Some(owner)
    }

    fn owner(&self, collider: ColliderHandle) -> u64 {
        self.colliders.get(collider)
            .and_then(Collider::parent)
            .and_then(|parent| self.owners.get(&parent).copied())
            .or_else(|| self.terrain_owners.get(&collider).copied())
            .unwrap_or(0)
    }

    fn cast_ray(&self, origin: Point<Real>, direction: Vector<Real>, distance: f32, options: Filter) -> Option<RayHit> {
        let ray = Ray::new(origin, direction);
        let allowed = |handle, collider: &Collider| self.allowed(handle, collider, options, true);
        let (collider, hit) = self.query.cast_ray_and_get_normal(
            &self.bodies, &self.colliders, &ray, distance, true, Self::query_filter(options, &allowed))?;
        Some(self.hit(collider, ray.point_at(hit.time_of_impact), hit.normal, hit.time_of_impact))
    }

    fn hit(&self, collider: ColliderHandle, point: Point<Real>, normal: Vector<Real>, distance: f32) -> RayHit {
        let region = self.gameplay.hitboxes.get(&collider);
        RayHit {
            id: self.owner(collider), point: point.into(), normal: normal.into(), distance,
            hitbox: region.map(|r| r.name.clone()), damage_multiplier: region.map_or(1.0, |r| r.multiplier),
        }
    }

    pub(crate) fn query(&self, name: &str, args: Query) -> Result<Value> {
        let pose = args.pose()?;
        if name == "raycast" {
            let (direction, distance) = args.cast()?;
            return Ok(query_hit(self.cast_ray(Point::from(args.origin), direction, distance, args.filter)));
        }
        let shape = args.shape.as_ref().context("shape is required")?.build([1.0; 3], true)?;
        let allowed = |handle, collider: &Collider| self.allowed(handle, collider, args.filter, false);
        let filter = Self::query_filter(args.filter, &allowed);
        match name {
            "shape_cast" => {
                let (direction, distance) = args.cast()?;
                let hit = self.query.cast_shape(&self.bodies, &self.colliders, &pose, &direction, shape.as_ref(),
                    ShapeCastOptions::with_max_time_of_impact(distance), filter);
                Ok(query_hit(hit.map(|(collider, hit)| self.hit(collider, hit.witness1, hit.normal1.into_inner(), hit.time_of_impact))))
            }
            "overlap" => {
                let mut ids = BTreeSet::new();
                self.query.intersections_with_shape(&self.bodies, &self.colliders, &pose, shape.as_ref(), filter, |handle| {
                    ids.insert(self.owner(handle));
                    true
                });
                Ok(json!({"ids": ids.iter().take(256).collect::<Vec<_>>(), "truncated": ids.len() > 256}))
            }
            _ => bail!("unknown physics query {name}"),
        }
    }

    pub(crate) fn reconcile(&mut self, scene: &Scene) -> Result<()> {
        self.reconcile_terrain(scene)?;
        self.reconcile_bodies(scene)
    }

    pub(crate) fn reconcile_for_query(&mut self, scene: &Scene, name: &str, args: &Query) -> Result<()> {
        if scene.entities.iter().any(|e| e.props.get("ai_terrain").and_then(Value::as_str).is_some()) {
            let pose = args.pose()?;
            let start = Point::from(args.origin);
            let bounds = if name == "raycast" {
                let (direction, distance) = args.cast()?;
                Aabb::new(start, start).merged(&Aabb::new(start + direction * distance, start + direction * distance))
            } else {
                let shape = args.shape.as_ref().context("shape is required")?.build([1.0; 3], true)?;
                let bounds = shape.compute_aabb(&pose);
                if name == "shape_cast" {
                    let (direction, distance) = args.cast()?;
                    bounds.merged(&Aabb::new(bounds.mins + direction * distance, bounds.maxs + direction * distance))
                } else {
                    bounds
                }
            };
            self.reconcile_terrain_near(scene, &[], Some(bounds))?;
        }
        self.reconcile_bodies(scene)
    }

    fn reconcile_bodies(&mut self, scene: &Scene) -> Result<()> {
        let live: BTreeSet<_> = scene.entities.iter().map(|e| e.id).collect();
        let removed: Vec<_> = self
            .handles
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in removed {
            let handle = self.handles.remove(&id).unwrap();
            self.remove_gameplay(id);
            self.remove_body(handle);
        }
        for entity in &scene.entities {
            if self.handles.contains_key(&entity.id)
                || (entity.collider.is_none() && entity.rigidbody.is_none() && entity.props.get("ai_physics").is_none())
            {
                continue;
            }
            let config = entity.rigidbody.clone().unwrap_or_default();
            let extra = Config::from_entity(entity)?;
            extra.validate()?;
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
            let collider = if let Some(geometry) = &extra.collider {
                if config.kind == BodyType::Dynamic && matches!(geometry, crate::physics_config::Geometry::Trimesh { .. }) {
                    bail!("entity {}: trimesh is not supported on dynamic bodies", entity.name);
                }
                Some(ColliderBuilder::new(geometry.build(entity.transform.scale, false)?))
            } else { entity
                .collider
                .as_ref()
                .map(|shape| collider(entity, shape, &self.root))
                .transpose()? };
            let builder = if collider.is_none() {
                builder.additional_mass(config.mass)
            } else {
                builder
            };
            let builder = if entity.props["ai_lock_rotation"].as_bool() == Some(true) { builder.lock_rotations() } else { builder };
            let handle = self
                .bodies
                .insert(builder.position(pose(&entity.transform))
                    .linear_damping(extra.linear_damping)
                    .angular_damping(extra.angular_damping)
                    .ccd_enabled(extra.ccd)
                    .build());
            if let Some(collider) = collider {
                self.colliders.insert_with_parent(
                    collider
                        .mass(config.mass)
                        .friction(config.friction)
                        .friction_combine_rule(friction_rule)
                        .restitution(config.restitution)
                        .collision_groups(extra.groups.into())
                        .active_collision_types(if extra.sensor { ActiveCollisionTypes::all() } else { ActiveCollisionTypes::default() })
                        .sensor(extra.sensor)
                        .build(),
                    handle,
                    &mut self.bodies,
                );
            }
            self.handles.insert(entity.id, handle);
            self.owners.insert(handle, entity.id);
            self.attach_gameplay(entity, handle, extra, config)?;
        }
        self.reconcile_joints();
        Ok(())
    }

    fn remove_body(&mut self, handle: RigidBodyHandle) {
        self.owners.remove(&handle);
        self.bodies.remove(handle, &mut self.islands, &mut self.colliders, &mut self.joints, &mut self.multibody, true);
    }

    pub(crate) fn reconcile_terrain(&mut self, scene: &Scene) -> Result<()> {
        let centers: Vec<_> = scene
            .entities
            .iter()
            .filter(|e| {
                e.tags.iter().any(|t| t == "player")
                    || self.ragdoll_active(e.id)
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
        self.reconcile_terrain_near(scene, &centers, None)
    }

    fn reconcile_terrain_near(&mut self, scene: &Scene, centers: &[[f64; 2]], query_bounds: Option<Aabb>) -> Result<()> {
        let mut wanted = BTreeSet::new();
        let mut live = BTreeSet::new();
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
                    self.remove_terrain_collider(key);
                }
            }
            if !self.terrain.contains_key(&entity.id) {
                if !self.terrain_sources.borrow().contains_key(path) {
                    anyhow::ensure!(!self.root.as_os_str().is_empty(), "terrain {path} missing from bundled terrain sources");
                    let file = engine_core::project_path(&self.root, std::path::Path::new(path))?;
                    let terrain = engine_terrain::web::StreamedTerrain::from_terrain(&engine_terrain::Terrain::load(&file)?);
                    self.terrain_sources.borrow_mut().insert(path.to_owned(), std::rc::Rc::new(std::cell::RefCell::new(terrain)));
                }
                self.terrain.insert(
                    entity.id,
                    (path.to_owned(), self.terrain_sources.borrow()[path].clone()),
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
            let terrain = self.terrain[&entity.id].1.borrow();
            let config = &terrain.config;
            let offset = entity.transform.position;
            let indices = if let Some(bounds) = query_bounds {
                let local = |point: Point<Real>| [f64::from(point.x) - f64::from(offset[0]), f64::from(point.z) - f64::from(offset[2])];
                let [xs, zs] = terrain_tile_ranges(config, local(bounds.mins), local(bounds.maxs));
                zs.flat_map(|z| xs.clone().map(move |x| z as usize * config.grid()[0] as usize + x as usize)).collect()
            } else {
                nearby_terrain_tiles(config, offset, centers, radius)
            };
            for index in indices {
                let coord = (index as u32 % config.grid()[0], index as u32 / config.grid()[0]);
                let Some(tile) = terrain.tiles.get(&coord) else { continue; };
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
                let handle = self.colliders.insert(collider);
                self.terrain_handles.insert(key, handle);
                self.terrain_owners.insert(handle, entity.id);
            }
        }
        let removed: Vec<_> = self
            .terrain_handles
            .keys()
            .filter(|k| !wanted.contains(k))
            .copied()
            .collect();
        for key in removed {
            self.remove_terrain_collider(key);
        }
        self.terrain.retain(|id, _| live.contains(id));
        Ok(())
    }

    fn remove_terrain_collider(&mut self, key: (u64, u32, u32)) {
        let handle = self.terrain_handles.remove(&key).unwrap();
        self.terrain_owners.remove(&handle);
        self.colliders.remove(handle, &mut self.islands, &mut self.bodies, true);
    }

    /// Enables/disables non-fixed bodies for the active region; returns whether any changed.
    pub(crate) fn apply_active(&mut self, inactive: &BTreeSet<u64>) -> bool {
        let mut changed = false;
        for (id, handle) in &self.handles {
            let active_rig = self.ragdoll_active(*id);
            let body = &mut self.bodies[*handle];
            let enabled = !active_rig && (body.is_fixed() || !inactive.contains(id));
            if body.is_enabled() != enabled {
                body.set_enabled(enabled);
                if enabled { body.wake_up(true); }
                changed = true;
            }
        }
        self.apply_ragdoll_active(inactive) || changed
    }

    /// Copies script-set transforms of `dirty` entities onto their bodies.
    pub(crate) fn apply_poses(&mut self, scene: &Scene, dirty: &BTreeSet<u64>) {
        for entity in &scene.entities {
            if dirty.contains(&entity.id) {
                if self.ragdoll_active(entity.id) {
                    self.move_ragdoll(entity.id, pose(&entity.transform));
                } else if let Some(handle) = self.handles.get(&entity.id) {
                    self.bodies[*handle].set_position(pose(&entity.transform), true);
                }
            }
        }
        self.bodies.propagate_modified_body_positions_to_colliders(&mut self.colliders);
    }

    pub(crate) fn step(
        &mut self,
        scene: &mut Scene,
        dirty: &BTreeSet<u64>,
        forces: Vec<Force>,
        inactive: &BTreeSet<u64>,
    ) -> Result<Vec<Value>> {
        self.reconcile(scene)?;
        self.apply_poses(scene, dirty);
        for handle in self.forced.drain(..) {
            if let Some(body) = self.bodies.get_mut(handle) {
                body.reset_forces(false);
            }
        }
        for force in forces {
            enum Mode { Impulse, Velocity, Force }
            let (id, value, mode) = match force {
                Force::Impulse(id, v) => (id, v, Mode::Impulse),
                Force::Velocity(id, v) => (id, v, Mode::Velocity),
                Force::Applied(id, v) => (id, v, Mode::Force),
                Force::Ragdoll(id, active) => {
                    if self.handles.contains_key(&id) {
                        self.set_ragdoll(id, active)?;
                    }
                    continue;
                }
                Force::Hit(id, region, impulse) => {
                    if let Some(handle) = self.ragdoll_part(id, &region) {
                        self.bodies[handle].apply_impulse(Vector::from(impulse), true);
                    }
                    continue;
                }
            };
            let handle = self.ragdoll_part(id, "").or_else(|| self.handles.get(&id).copied());
            if let Some(handle) = handle {
                let body = &mut self.bodies[handle];
                let vector = vector![value[0], value[1], value[2]];
                match mode {
                    Mode::Impulse => body.apply_impulse(vector, true),
                    Mode::Velocity => body.set_linvel(vector, true),
                    Mode::Force => {
                        body.add_force(vector, true);
                        self.forced.push(handle);
                    }
                }
            } else if scene.entities.iter().any(|e| e.id == id) {
                bail!("entity {id} needs a rigidbody for impulse/velocity");
            }
        }
        self.apply_active(inactive);
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
            if let Some(handle) = self.ragdoll_part(entity.id, "").or_else(|| self.handles.get(&entity.id).copied()) {
                let body = &self.bodies[handle];
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
        Ok(self.update_collision_events())
    }

    pub(crate) fn motion(&self) -> (BTreeMap<u64,[f32;3]>, BTreeSet<u64>) {
        let mut velocities=BTreeMap::new(); let mut grounded=BTreeSet::new();
        for (id,handle) in &self.handles {
            let body=&self.bodies[self.ragdoll_part(*id, "").unwrap_or(*handle)]; let v=body.linvel();
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

fn query_hit(hit: Option<RayHit>) -> Value {
    if let Some(hit) = hit {
        let mut value = json!(hit);
        value["hit"] = json!(true);
        value
    } else {
        json!({"hit": false})
    }
}

fn collider(entity: &Entity, shape: &Shape, root: &std::path::Path) -> Result<ColliderBuilder> {
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
                    engine_geo::load_gltf(&root.join(path))?
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
