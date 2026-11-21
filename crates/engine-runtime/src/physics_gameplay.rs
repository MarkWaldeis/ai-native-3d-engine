use super::{pose, Physics};
use crate::{physics_config::Config, ragdoll};
use anyhow::{ensure, Context, Result};
use rapier3d::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};
use shared::{components::RigidBody as BodyConfig, scene::Entity};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const MAX_PHYSICS_EVENTS: usize = 256;
const MAX_ACTIVE_RAGDOLLS: usize = 128;

#[derive(Clone, Default)]
pub(super) struct Gameplay {
    pub hitboxes: HashMap<ColliderHandle, Region>,
    pub named_owners: BTreeSet<u64>,
    attached_regions: BTreeMap<u64, Vec<ColliderHandle>>,
    declared_joints: BTreeMap<u64, Vec<(u64, GenericJoint)>>,
    linked_joints: BTreeMap<(u64, usize), ImpulseJointHandle>,
    rigs: BTreeMap<u64, Rig>,
    active_rigs: usize,
    pairs: BTreeSet<Pair>,
    events: Vec<Collision>,
}

#[derive(Clone)]
pub(super) struct Region {
    pub name: String,
    pub multiplier: f64,
    pub ray_only: bool,
}

#[derive(Clone)]
struct Rig {
    config: ragdoll::Config,
    body: BodyConfig,
    groups: InteractionGroups,
    parts: Vec<(RigidBodyHandle, ColliderHandle)>,
    upright: Rotation<Real>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Pair {
    a: u64,
    b: u64,
    sensor: bool,
}

#[derive(Clone, Serialize)]
struct Collision {
    #[serde(rename = "type")]
    kind: &'static str,
    phase: &'static str,
    a: u64,
    b: u64,
}

impl Physics {
    pub(super) fn attach_gameplay(
        &mut self,
        entity: &Entity,
        handle: RigidBodyHandle,
        config: Config,
        body: BodyConfig,
    ) -> Result<()> {
        let id = entity.id;
        if !config.hitboxes.is_empty() {
            let mut names = BTreeSet::new();
            let mut attached = Vec::new();
            for region in &config.hitboxes {
                ensure!(
                    names.insert(&region.name),
                    "entity {} has duplicate hitbox {}",
                    entity.name,
                    region.name
                );
                let collider = region.build(entity.transform.scale, config.groups)?.build();
                let collider =
                    self.colliders
                        .insert_with_parent(collider, handle, &mut self.bodies);
                attached.push(collider);
                self.gameplay.hitboxes.insert(
                    collider,
                    Region {
                        name: region.name.clone(),
                        multiplier: region.damage_multiplier,
                        ray_only: true,
                    },
                );
            }
            self.gameplay.named_owners.insert(id);
            self.gameplay.attached_regions.insert(id, attached);
        }
        if !config.joints.is_empty() {
            let specs = config
                .joints
                .iter()
                .map(|joint| {
                    ensure!(
                        joint.target != id && joint.target != 0,
                        "entity {} has an invalid joint target",
                        entity.name
                    );
                    Ok((joint.target, joint.build()?))
                })
                .collect::<Result<Vec<_>>>()?;
            self.gameplay.declared_joints.insert(id, specs);
        }
        if let Some(rig) = config.ragdoll {
            rig.parts()?;
            let start_active = rig.start_active;
            self.gameplay.rigs.insert(
                id,
                Rig {
                    config: rig,
                    body,
                    groups: config.groups.into(),
                    parts: vec![],
                    upright: pose(&entity.transform).rotation,
                },
            );
            if start_active {
                self.set_ragdoll(id, true)?;
            }
        }
        Ok(())
    }

    pub(super) fn remove_gameplay(&mut self, id: u64) {
        if let Some(rig) = self.gameplay.rigs.remove(&id) {
            if !rig.parts.is_empty() {
                self.gameplay.active_rigs -= 1;
                for (body, collider) in rig.parts {
                    self.gameplay.hitboxes.remove(&collider);
                    self.remove_body(body);
                }
            }
        }
        if let Some(colliders) = self.gameplay.attached_regions.remove(&id) {
            for collider in colliders {
                self.gameplay.hitboxes.remove(&collider);
            }
        }
        self.gameplay.named_owners.remove(&id);
        self.gameplay.declared_joints.remove(&id);
    }

    pub(super) fn reconcile_joints(&mut self) {
        self.gameplay
            .linked_joints
            .retain(|_, handle| self.joints.get(*handle).is_some());
        for (&owner, specs) in &self.gameplay.declared_joints {
            for (index, (target, spec)) in specs.iter().enumerate() {
                if let Some(&target) = self.handles.get(target) {
                    self.gameplay
                        .linked_joints
                        .entry((owner, index))
                        .or_insert_with(|| {
                            self.joints
                                .insert(self.handles[&owner], target, *spec, true)
                        });
                }
            }
        }
    }

    pub(crate) fn has_ragdoll(&self, id: u64) -> bool {
        self.gameplay.rigs.contains_key(&id)
    }

    pub(crate) fn ragdoll_active(&self, id: u64) -> bool {
        self.gameplay
            .rigs
            .get(&id)
            .is_some_and(|rig| !rig.parts.is_empty())
    }

    pub(crate) fn ragdoll_activates_on_death(&self, id: u64) -> bool {
        self.gameplay
            .rigs
            .get(&id)
            .is_some_and(|rig| rig.config.activate_on_death)
    }

    pub(super) fn set_ragdoll(&mut self, id: u64, active: bool) -> Result<()> {
        let rig = self
            .gameplay
            .rigs
            .get_mut(&id)
            .context("entity has no ragdoll preset")?;
        if active != rig.parts.is_empty() {
            return Ok(());
        }
        let root = self.handles[&id];
        if !active {
            let upright = rig.upright;
            let position = self.bodies[rig.parts[0].0].translation()
                - upright * Vector::from(rig.config.offset);
            for (body, collider) in std::mem::take(&mut rig.parts) {
                self.gameplay.hitboxes.remove(&collider);
                self.remove_body(body);
            }
            self.gameplay.active_rigs -= 1;
            let root = &mut self.bodies[root];
            root.set_position(
                Isometry::from_parts(Translation::from(position), upright),
                true,
            );
            root.set_linvel(Vector::zeros(), true);
            root.set_angvel(Vector::zeros(), true);
            root.set_enabled(true);
            return Ok(());
        }
        ensure!(
            self.gameplay.active_rigs < MAX_ACTIVE_RAGDOLLS,
            "at most {MAX_ACTIVE_RAGDOLLS} ragdolls may be active"
        );
        let parts = rig.config.parts()?;
        let root_body = &mut self.bodies[root];
        let (position, velocity) = (*root_body.position(), *root_body.linvel());
        rig.upright = position.rotation;
        let position = position * Isometry::translation(
            rig.config.offset[0], rig.config.offset[1], rig.config.offset[2],
        );
        root_body.set_enabled(false);
        for part in parts {
            let body = self.bodies.insert(
                RigidBodyBuilder::dynamic()
                    .position(part.pose(position, rig.config.scale))
                    .linvel(velocity)
                    .linear_damping(0.1)
                    .angular_damping(0.3)
                    .ccd_enabled(true),
            );
            let collider = self.colliders.insert_with_parent(
                ColliderBuilder::capsule_y(
                    part.half_height * rig.config.scale,
                    part.radius * rig.config.scale,
                )
                .mass(rig.body.mass * part.mass_fraction)
                .friction(rig.body.friction)
                .restitution(rig.body.restitution)
                .collision_groups(rig.groups),
                body,
                &mut self.bodies,
            );
            self.owners.insert(body, id);
            self.gameplay.hitboxes.insert(
                collider,
                Region {
                    name: part.name.into(),
                    multiplier: part.multiplier,
                    ray_only: false,
                },
            );
            if let Some((parent, joint)) = part.joint(parts, rig.config.scale) {
                self.joints.insert(rig.parts[parent].0, body, joint, true);
            }
            rig.parts.push((body, collider));
        }
        self.gameplay.active_rigs += 1;
        Ok(())
    }

    pub(super) fn apply_ragdoll_active(&mut self, inactive: &BTreeSet<u64>) -> bool {
        let mut changed = false;
        for (&id, rig) in &self.gameplay.rigs {
            for (handle, _) in &rig.parts {
                let body = &mut self.bodies[*handle];
                let enabled = !inactive.contains(&id);
                if body.is_enabled() != enabled {
                    body.set_enabled(enabled);
                    if enabled {
                        body.wake_up(true);
                    }
                    changed = true;
                }
            }
        }
        changed
    }

    pub(crate) fn active_ragdoll_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.gameplay
            .rigs
            .iter()
            .filter_map(|(&id, rig)| (!rig.parts.is_empty()).then_some(id))
    }

    pub(super) fn move_ragdoll(&mut self, id: u64, position: Isometry<Real>) {
        let rig = &self.gameplay.rigs[&id];
        let delta = position * self.bodies[rig.parts[0].0].position().inverse();
        for (handle, _) in &rig.parts {
            let body = &mut self.bodies[*handle];
            body.set_position(delta * body.position(), true);
        }
    }

    pub(super) fn ragdoll_part(&self, id: u64, region: &str) -> Option<RigidBodyHandle> {
        let rig = self.gameplay.rigs.get(&id)?;
        rig.parts
            .iter()
            .find(|(_, collider)| self.gameplay.hitboxes[collider].name == region)
            .or_else(|| rig.parts.first())
            .map(|part| part.0)
    }

    pub(crate) fn hitbox_multiplier(&self, id: u64, name: &str) -> Option<f64> {
        let authored = self
            .gameplay
            .attached_regions
            .get(&id)
            .into_iter()
            .flatten()
            .copied();
        let limbs = self
            .gameplay
            .rigs
            .get(&id)
            .into_iter()
            .flat_map(|rig| rig.parts.iter().map(|part| part.1));
        limbs
            .chain(authored)
            .filter_map(|handle| self.gameplay.hitboxes.get(&handle))
            .find(|region| region.name == name)
            .map(|region| region.multiplier)
    }

    pub(crate) fn ragdoll_states(&self) -> Value {
        Value::Array(self.gameplay.rigs.iter().filter(|(_, rig)| !rig.parts.is_empty()).map(|(id, rig)| {
            let parts: Vec<_> = rig.config.parts().expect("validated rig").iter().zip(&rig.parts).map(|(part, (handle, _))| {
                let body = &self.bodies[*handle];
                let (x, y, z) = body.rotation().euler_angles();
                let position: [f32; 3] = (*body.translation()).into();
                json!({"name":part.name,"position":position,"rotation_deg":[x.to_degrees(),y.to_degrees(),z.to_degrees()],
                    "radius":part.radius * rig.config.scale,"half_height":part.half_height * rig.config.scale})
            }).collect();
            json!({"id":id,"enabled":self.bodies[rig.parts[0].0].is_enabled(),"parts":parts})
        }).collect())
    }

    pub(super) fn update_collision_events(&mut self) -> Vec<Value> {
        let pair = |first, second, sensor| {
            if [first, second].into_iter().any(|handle| {
                self.gameplay.hitboxes.get(&handle).is_some_and(|region| region.ray_only)
            }) {
                return None;
            }
            let (a, b) = (self.owner(first), self.owner(second));
            (a != b).then_some(Pair {
                a: a.min(b),
                b: a.max(b),
                sensor,
            })
        };
        let contacts = self
            .narrow
            .contact_pairs()
            .filter(|p| p.has_any_active_contact)
            .filter_map(|p| pair(p.collider1, p.collider2, false));
        let sensors = self
            .narrow
            .intersection_pairs()
            .filter(|p| p.2)
            .filter_map(|(a, b, _)| pair(a, b, true));
        let current: BTreeSet<_> = contacts.chain(sensors).collect();
        let event = |p: &Pair, phase| Collision {
            kind: if p.sensor { "sensor" } else { "contact" },
            phase,
            a: p.a,
            b: p.b,
        };
        self.gameplay.events = self
            .gameplay
            .pairs
            .difference(&current)
            .map(|p| event(p, "stopped"))
            .chain(
                current
                    .difference(&self.gameplay.pairs)
                    .map(|p| event(p, "started")),
            )
            .take(MAX_PHYSICS_EVENTS)
            .collect();
        self.gameplay.pairs = current;
        self.gameplay
            .events
            .iter()
            .map(|event| json!(event))
            .collect()
    }

    pub(crate) fn collision_events(&self, id: u64) -> Vec<Value> {
        self.gameplay
            .events
            .iter()
            .filter(|event| event.a == id || event.b == id)
            .map(|event| json!(event))
            .collect()
    }
}
