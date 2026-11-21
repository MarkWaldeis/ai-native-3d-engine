use anyhow::{bail, ensure, Context, Result};
use rapier3d::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use shared::scene::Entity;

#[derive(Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Groups {
    memberships: u32,
    filter: u32,
}

impl Default for Groups {
    fn default() -> Self {
        Self {
            memberships: u32::MAX,
            filter: u32::MAX,
        }
    }
}

impl From<Groups> for InteractionGroups {
    fn from(value: Groups) -> Self {
        Self::new(
            Group::from_bits_retain(value.memberships),
            Group::from_bits_retain(value.filter),
        )
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub collider: Option<Geometry>,
    pub groups: Groups,
    pub sensor: bool,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub ccd: bool,
    pub hitboxes: Vec<Hitbox>,
    pub joints: Vec<Joint>,
    pub ragdoll: Option<crate::ragdoll::Config>,
}

impl Config {
    pub fn from_entity(entity: &Entity) -> Result<Self> {
        entity
            .props
            .get("ai_physics")
            .map(|value| match value {
                Value::String(text) => serde_json::from_str(text),
                value => serde_json::from_value(value.clone()),
            })
            .transpose()
            .map(Option::unwrap_or_default)
            .with_context(|| format!("entity {}: invalid ai_physics", entity.name))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            [self.linear_damping, self.angular_damping]
                .into_iter()
                .all(|v| v.is_finite() && v >= 0.0),
            "physics damping must be finite and nonnegative"
        );
        ensure!(
            self.hitboxes.len() <= 32 && self.joints.len() <= 16,
            "an entity supports up to 32 hitboxes and 16 joints"
        );
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hitbox {
    pub name: String,
    pub shape: Geometry,
    #[serde(default)]
    position: [f32; 3],
    #[serde(default)]
    rotation_deg: [f32; 3],
    #[serde(default = "one")]
    pub damage_multiplier: f64,
}

fn one() -> f64 {
    1.0
}

impl Hitbox {
    pub fn build(&self, scale: [f32; 3], groups: Groups) -> Result<ColliderBuilder> {
        ensure!(
            !self.name.is_empty()
                && self.name.len() <= 64
                && (0.0..=100.0).contains(&self.damage_multiplier),
            "invalid hitbox name or damage_multiplier (expected [0,100])"
        );
        let (pose, shape) = self
            .shape
            .build_part(scale, self.position, self.rotation_deg)?;
        Ok(ColliderBuilder::new(shape)
            .position(pose)
            .sensor(true)
            .active_collision_types(ActiveCollisionTypes::empty())
            .collision_groups(groups.into())
            .mass(0.0))
    }
}

#[derive(Deserialize)]
pub(crate) struct Joint {
    pub target: u64,
    #[serde(flatten)]
    kind: JointKind,
    #[serde(default)]
    anchor: [f32; 3],
    #[serde(default)]
    target_anchor: [f32; 3],
    #[serde(default)]
    contacts_enabled: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum JointKind {
    Fixed,
    Spherical,
    Revolute {
        axis: [f32; 3],
        limits_deg: Option<[f32; 2]>,
    },
}

impl Joint {
    pub fn build(&self) -> Result<GenericJoint> {
        ensure!(
            self.anchor
                .into_iter()
                .chain(self.target_anchor)
                .all(finite),
            "invalid joint anchors"
        );
        let mut joint: GenericJoint = match self.kind {
            JointKind::Fixed => FixedJointBuilder::new().into(),
            JointKind::Spherical => SphericalJointBuilder::new().into(),
            JointKind::Revolute { axis, limits_deg } => {
                ensure!(axis.into_iter().all(finite), "invalid joint axis");
                let axis = UnitVector::try_new(Vector::from(axis), 1e-6)
                    .ok_or_else(|| anyhow::anyhow!("joint axis cannot be zero"))?;
                let mut builder = RevoluteJointBuilder::new(axis);
                if let Some([min, max]) = limits_deg {
                    ensure!(
                        min.is_finite()
                            && max.is_finite()
                            && min >= -180.0
                            && min <= max
                            && max <= 180.0,
                        "invalid joint angle limits"
                    );
                    builder = builder.limits([min.to_radians(), max.to_radians()]);
                }
                builder.into()
            }
        };
        joint.set_contacts_enabled(self.contacts_enabled);
        joint.set_local_anchor1(Point::from(self.anchor));
        joint.set_local_anchor2(Point::from(self.target_anchor));
        Ok(joint)
    }
}

#[derive(Deserialize, serde::Serialize)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Geometry {
    Box {
        half_extents: [f32; 3],
    },
    Sphere {
        radius: f32,
    },
    Capsule {
        radius: f32,
        half_height: f32,
    },
    ConvexHull {
        vertices: Vec<[f32; 3]>,
    },
    Trimesh {
        vertices: Vec<[f32; 3]>,
        triangles: Vec<[u32; 3]>,
    },
    Compound {
        parts: Vec<Part>,
    },
}

#[derive(Deserialize, serde::Serialize)]
pub(crate) struct Part {
    #[serde(default)]
    position: [f32; 3],
    #[serde(default)]
    rotation_deg: [f32; 3],
    #[serde(flatten)]
    geometry: Geometry,
}

pub(crate) fn isometry(position: [f32; 3], rotation_deg: [f32; 3]) -> Isometry<Real> {
    let [x, y, z] = rotation_deg.map(f32::to_radians);
    Isometry::from_parts(
        Translation::from(Vector::from(position)),
        Rotation::from_euler_angles(x, y, z),
    )
}

fn finite(v: f32) -> bool {
    v.is_finite() && v.abs() <= 1.0e6
}

fn positive(v: f32) -> bool {
    finite(v) && v > 0.0
}

impl Geometry {
    fn build_part(
        &self,
        scale: [f32; 3],
        position: [f32; 3],
        rotation_deg: [f32; 3],
    ) -> Result<(Isometry<Real>, SharedShape)> {
        let position = std::array::from_fn(|i| position[i] * scale[i]);
        ensure!(
            position.into_iter().chain(rotation_deg).all(finite),
            "invalid collider part transform"
        );
        let pose = isometry(position, rotation_deg);
        if rotation_deg == [0.0; 3]
            || scale.into_iter().all(|s| s == scale[0])
            || matches!(self, Self::Sphere { .. })
        {
            return Ok((pose, self.build(scale, true)?));
        }

        let shape = self.build([1.0; 3], true)?;
        let transform = |v: Vector<Real>| (pose.rotation * v).component_mul(&Vector::from(scale));
        let shape = if let Some(capsule) = shape.as_capsule() {
            let x = transform(Vector::x());
            let z = transform(Vector::z());
            let (xx, zz) = (x.norm_squared(), z.norm_squared());
            let radius =
                capsule.radius * ((xx + zz + (xx - zz).hypot(2.0 * x.dot(&z))) * 0.5).sqrt();
            let end = transform(capsule.segment.b.coords);
            ensure!(
                positive(radius) && end.iter().copied().all(finite),
                "invalid capsule dimensions"
            );
            SharedShape::capsule(Point::from(-end), Point::from(end), radius)
        } else {
            let vertices = if let Some(cuboid) = shape.as_cuboid() {
                cuboid.to_trimesh().0
            } else {
                shape
                    .as_convex_polyhedron()
                    .context("expected convex collider part")?
                    .points()
                    .to_vec()
            };
            let vertices: Vec<_> = vertices
                .into_iter()
                .map(|p| Point::from(transform(p.coords)))
                .collect();
            ensure!(
                vertices
                    .iter()
                    .all(|p| p.coords.iter().copied().all(finite)),
                "invalid physics vertex"
            );
            SharedShape::convex_hull(&vertices).context("degenerate scaled collider part")?
        };
        Ok((isometry(position, [0.0; 3]), shape))
    }

    pub fn build(&self, scale: [f32; 3], convex_only: bool) -> Result<SharedShape> {
        let abs = scale.map(f32::abs);
        let scaled = |p: [f32; 3]| std::array::from_fn(|i| p[i] * scale[i]);
        let points = |vertices: &[[f32; 3]]| -> Result<Vec<Point<Real>>> {
            ensure!(
                (3..=16_384).contains(&vertices.len()),
                "physics mesh requires 3..=16384 vertices"
            );
            vertices
                .iter()
                .map(|&p| {
                    let p = scaled(p);
                    ensure!(p.into_iter().all(finite), "invalid physics vertex");
                    Ok(Point::from(p))
                })
                .collect()
        };
        Ok(match self {
            Self::Box { half_extents } => {
                let [x, y, z] = std::array::from_fn(|i| half_extents[i] * abs[i]);
                ensure!(
                    [x, y, z].into_iter().all(positive),
                    "invalid box half_extents"
                );
                SharedShape::cuboid(x, y, z)
            }
            Self::Sphere { radius } => {
                let radius = radius * abs.into_iter().fold(0.0, f32::max);
                ensure!(positive(radius), "invalid sphere radius");
                SharedShape::ball(radius)
            }
            Self::Capsule {
                radius,
                half_height,
            } => {
                let (radius, height) = (radius * abs[0].max(abs[2]), half_height * abs[1]);
                ensure!(
                    positive(radius) && finite(height) && height >= 0.0,
                    "invalid capsule dimensions"
                );
                SharedShape::capsule_y(height, radius)
            }
            Self::ConvexHull { vertices } => SharedShape::convex_hull(&points(vertices)?)
                .ok_or_else(|| anyhow::anyhow!("convex_hull requires noncoplanar vertices"))?,
            Self::Trimesh {
                vertices,
                triangles,
            } if !convex_only => {
                let vertices = points(vertices)?;
                ensure!(
                    !triangles.is_empty() && triangles.len() <= 32_768,
                    "trimesh requires 1..=32768 triangles"
                );
                ensure!(
                    triangles.iter().all(|&[a, b, c]| {
                        [a, b, c].into_iter().all(|i| (i as usize) < vertices.len())
                            && (vertices[b as usize] - vertices[a as usize])
                                .cross(&(vertices[c as usize] - vertices[a as usize]))
                                .norm_squared()
                                > 0.0
                    }),
                    "trimesh has invalid indices or a degenerate triangle"
                );
                SharedShape::trimesh(vertices, triangles.clone())
            }
            Self::Compound { parts } if !convex_only => {
                ensure!(
                    (1..=64).contains(&parts.len()),
                    "compound requires 1..=64 convex parts"
                );
                SharedShape::compound(
                    parts
                        .iter()
                        .map(|part| {
                            part.geometry
                                .build_part(scale, part.position, part.rotation_deg)
                        })
                        .collect::<Result<Vec<_>>>()?,
                )
            }
            _ => bail!("only convex shapes are allowed here"),
        })
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Filter {
    pub exclude: Option<u64>,
    pub include_sensors: bool,
    pub groups: Option<Groups>,
}

#[derive(Deserialize)]
pub(crate) struct Query {
    pub origin: [f32; 3],
    pub direction: Option<[f32; 3]>,
    pub distance: Option<f32>,
    pub shape: Option<Geometry>,
    #[serde(default)]
    pub rotation_deg: [f32; 3],
    #[serde(default)]
    pub filter: Filter,
}

impl Query {
    pub fn pose(&self) -> Result<Isometry<Real>> {
        ensure!(
            self.origin.into_iter().chain(self.rotation_deg).all(finite),
            "invalid query pose"
        );
        Ok(isometry(self.origin, self.rotation_deg))
    }

    pub fn cast(&self) -> Result<(Vector<Real>, f32)> {
        let dir = self
            .direction
            .ok_or_else(|| anyhow::anyhow!("direction is required"))?;
        let distance = self
            .distance
            .ok_or_else(|| anyhow::anyhow!("distance is required"))?;
        ensure!(
            dir.into_iter().all(finite)
                && distance.is_finite()
                && distance > 0.0
                && distance <= 10_000.0,
            "cast requires finite direction and distance in (0, 10000]"
        );
        let dir = Vector::from(dir)
            .try_normalize(1e-6)
            .ok_or_else(|| anyhow::anyhow!("direction cannot be zero"))?;
        Ok((dir, distance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rotated_compound_boxes_and_hitboxes_scale_in_entity_axes() {
        let scale = [3.0, 1.0, 1.0];
        let compound: Geometry = serde_json::from_value(json!({
            "shape":"compound","parts":[{"shape":"box","half_extents":[0.25,1,0.5],
                "position":[2,0,0],"rotation_deg":[0,0,90]}]
        }))
        .unwrap();
        let hitbox: Hitbox = serde_json::from_value(json!({
            "name":"rotated","shape":{"shape":"box","half_extents":[0.25,1,0.5]},
            "position":[2,0,0],"rotation_deg":[0,0,90]
        }))
        .unwrap();
        for bounds in [
            compound.build(scale, false).unwrap().compute_local_aabb(),
            hitbox
                .build(scale, Groups::default())
                .unwrap()
                .build()
                .compute_aabb(),
        ] {
            assert!((bounds.mins - point![3.0, -0.25, -0.5]).norm() < 1e-5);
            assert!((bounds.maxs - point![9.0, 0.25, 0.5]).norm() < 1e-5);
        }
    }

    #[test]
    fn rejects_invalid_mesh_indices_and_nested_compounds() {
        let mesh: Geometry = serde_json::from_value(json!({
            "shape":"trimesh", "vertices":[[0,0,0],[1,0,0],[0,1,0]], "triangles":[[0,1,3]],
        }))
        .unwrap();
        assert!(mesh.build([1.0; 3], false).is_err());
        let nested: Geometry = serde_json::from_value(json!({
            "shape":"compound", "parts":[{"shape":"compound","parts":[]}],
        }))
        .unwrap();
        assert!(nested.build([1.0; 3], false).is_err());
    }
}
