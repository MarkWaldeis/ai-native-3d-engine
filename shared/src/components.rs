use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Collider {
    Box {
        #[serde(default)]
        half_extents: Option<[f32; 3]>,
    },
    Sphere {
        #[serde(default)]
        radius: Option<f32>,
    },
    Capsule {
        #[serde(default)]
        radius: Option<f32>,
        #[serde(default)]
        half_height: Option<f32>,
    },
    #[serde(alias = "mesh-from-entity-bounds")]
    Mesh,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BodyType {
    #[default]
    Static,
    Dynamic,
    Kinematic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RigidBody {
    #[serde(rename = "type", default)]
    pub kind: BodyType,
    #[serde(default = "mass")]
    pub mass: f32,
    #[serde(default = "friction")]
    pub friction: f32,
    #[serde(default)]
    pub restitution: f32,
}
fn mass() -> f32 {
    1.0
}
fn friction() -> f32 {
    0.5
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            kind: BodyType::Static,
            mass: mass(),
            friction: friction(),
            restitution: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Script {
    Path(String),
    Inline { source: String },
    File { path: String },
}
