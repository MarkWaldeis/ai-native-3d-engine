use serde::{Deserialize, Serialize};

pub type EntityId = u64;

/// Transform stored as position + euler rotation (degrees, XYZ) + scale.
/// Euler degrees are friendlier for LLM callers than quaternions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Transform {
    #[serde(default)]
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation_deg: [f32; 3],
    #[serde(default = "one3")]
    pub scale: [f32; 3],
}

fn one3() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation_deg: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl Transform {
    pub fn at(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: [x, y, z],
            ..Default::default()
        }
    }
}

/// Procedural mesh recipes. Serialized flat into the entity:
/// `{"kind":"box","size":[1,1,1], ...}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrimitiveKind {
    Box { size: [f32; 3] },
    Plane { size: f32 },
    Sphere { radius: f32, segments: u32 },
    Cylinder { radius: f32, height: f32, segments: u32 },
    /// Flat wall with a rectangular door opening (door bottom sits at y=0,
    /// centered on x). Cheap CSG substitute for room blockouts.
    WallWithDoor {
        width: f32,
        height: f32,
        door_width: f32,
        door_height: f32,
    },
    /// Mesh loaded from a glTF/GLB asset file (produced by the Blender
    /// pipeline). `path` is absolute or relative to the process CWD.
    Gltf { path: String },
}

impl Default for PrimitiveKind {
    fn default() -> Self {
        Self::Box { size: [1.0; 3] }
    }
}

fn default_color() -> [f32; 4] {
    [0.8, 0.8, 0.8, 1.0]
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    #[serde(flatten)]
    pub mesh: PrimitiveKind,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    #[serde(default = "default_true")]
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Camera {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub fov_deg: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: [7.0, 5.0, 7.0],
            target: [0.0, 0.5, 0.0],
            fov_deg: 60.0,
        }
    }
}

fn default_light_dir() -> [f32; 3] {
    [0.5, -1.0, 0.35]
}

/// Root scene document — the entire game state lives here as plain JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub camera: Camera,
    /// Direction TO the light (normalized-ish), used for lambert shading.
    #[serde(default = "default_light_dir")]
    pub light_dir: [f32; 3],
    #[serde(default = "default_ambient")]
    pub ambient: f32,
}

fn default_name() -> String {
    "untitled".to_string()
}
fn default_ambient() -> f32 {
    0.25
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            name: default_name(),
            entities: Vec::new(),
            camera: Camera::default(),
            light_dir: default_light_dir(),
            ambient: default_ambient(),
        }
    }
}
