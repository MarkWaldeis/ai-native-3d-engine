use anyhow::{ensure, Result};
use rapier3d::prelude::*;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    preset: Preset,
    pub start_active: bool,
    pub activate_on_death: bool,
    pub scale: f32,
    pub offset: [f32; 3],
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Preset {
    Humanoid,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            preset: Preset::Humanoid,
            start_active: false,
            activate_on_death: true,
            scale: 1.0,
            offset: [0.0; 3],
        }
    }
}

impl Config {
    pub fn parts(&self) -> Result<&'static [Part]> {
        ensure!(
            self.scale.is_finite() && (0.1..=10.0).contains(&self.scale),
            "ragdoll scale must be in [0.1, 10]"
        );
        ensure!(
            self.offset.into_iter().all(|value| value.is_finite() && value.abs() <= 1.0e6),
            "invalid ragdoll offset"
        );
        Ok(match self.preset {
            Preset::Humanoid => &HUMANOID,
        })
    }
}

pub(crate) struct Part {
    pub name: &'static str,
    pub center: [f32; 3],
    pub radius: f32,
    pub half_height: f32,
    pub mass_fraction: f32,
    pub multiplier: f64,
    joint: Option<Joint>,
}

struct Joint {
    parent: usize,
    pivot: [f32; 3],
    hinge_limits: Option<[f32; 2]>,
}

impl Part {
    pub fn pose(&self, root: Isometry<Real>, scale: f32) -> Isometry<Real> {
        root * Isometry::translation(
            self.center[0] * scale,
            self.center[1] * scale,
            self.center[2] * scale,
        )
    }

    pub fn joint(&self, parts: &[Self], scale: f32) -> Option<(usize, GenericJoint)> {
        let joint = self.joint.as_ref()?;
        let anchor = |center: [f32; 3]| {
            Point::from(std::array::from_fn(|i| {
                (joint.pivot[i] - center[i]) * scale
            }))
        };
        let mut builder = if let Some(limits) = joint.hinge_limits {
            let mut hinge: GenericJoint = RevoluteJointBuilder::new(Vector::x_axis())
                .limits(limits.map(f32::to_radians))
                .into();
            hinge.set_contacts_enabled(false);
            hinge
        } else {
            let limits = [-40.0_f32.to_radians(), 40.0_f32.to_radians()];
            SphericalJointBuilder::new()
                .contacts_enabled(false)
                .limits(JointAxis::AngX, limits)
                .limits(JointAxis::AngY, limits)
                .limits(JointAxis::AngZ, limits)
                .into()
        };
        builder.set_local_anchor1(anchor(parts[joint.parent].center));
        builder.set_local_anchor2(anchor(self.center));
        Some((joint.parent, builder))
    }
}

const fn part(
    name: &'static str,
    center: [f32; 3],
    size: [f32; 2],
    mass_fraction: f32,
    multiplier: f64,
    joint: Option<Joint>,
) -> Part {
    Part {
        name,
        center,
        radius: size[0],
        half_height: size[1],
        mass_fraction,
        multiplier,
        joint,
    }
}

const fn joint(parent: usize, pivot: [f32; 3], hinge_limits: Option<[f32; 2]>) -> Option<Joint> {
    Some(Joint {
        parent,
        pivot,
        hinge_limits,
    })
}

const HUMANOID: [Part; 11] = [
    part("pelvis", [0.0, 0.0, 0.0], [0.18, 0.08], 0.20, 1.0, None),
    part(
        "torso",
        [0.0, 0.34, 0.0],
        [0.22, 0.14],
        0.28,
        1.0,
        joint(0, [0.0, 0.15, 0.0], None),
    ),
    part(
        "head",
        [0.0, 0.75, 0.0],
        [0.15, 0.04],
        0.10,
        2.0,
        joint(1, [0.0, 0.58, 0.0], None),
    ),
    part(
        "left_upper_arm",
        [-0.32, 0.35, 0.0],
        [0.08, 0.13],
        0.045,
        0.75,
        joint(1, [-0.25, 0.49, 0.0], None),
    ),
    part(
        "left_forearm",
        [-0.34, 0.02, 0.0],
        [0.065, 0.13],
        0.025,
        0.75,
        joint(3, [-0.33, 0.19, 0.0], Some([-135.0, 5.0])),
    ),
    part(
        "right_upper_arm",
        [0.32, 0.35, 0.0],
        [0.08, 0.13],
        0.045,
        0.75,
        joint(1, [0.25, 0.49, 0.0], None),
    ),
    part(
        "right_forearm",
        [0.34, 0.02, 0.0],
        [0.065, 0.13],
        0.025,
        0.75,
        joint(5, [0.33, 0.19, 0.0], Some([-135.0, 5.0])),
    ),
    part(
        "left_thigh",
        [-0.11, -0.29, 0.0],
        [0.10, 0.16],
        0.10,
        0.75,
        joint(0, [-0.11, -0.12, 0.0], None),
    ),
    part(
        "left_shin",
        [-0.11, -0.71, 0.0],
        [0.08, 0.16],
        0.04,
        0.75,
        joint(7, [-0.11, -0.50, 0.0], Some([-5.0, 135.0])),
    ),
    part(
        "right_thigh",
        [0.11, -0.29, 0.0],
        [0.10, 0.16],
        0.10,
        0.75,
        joint(0, [0.11, -0.12, 0.0], None),
    ),
    part(
        "right_shin",
        [0.11, -0.71, 0.0],
        [0.08, 0.16],
        0.04,
        0.75,
        joint(9, [0.11, -0.50, 0.0], Some([-5.0, 135.0])),
    ),
];
