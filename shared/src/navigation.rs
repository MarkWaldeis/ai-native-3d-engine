use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NavParams {
    pub radius: f32,
    pub height: f32,
    pub step: f32,
    pub max_slope_deg: f32,
    pub cell_size: f32,
    pub tile_size: f32,
    pub bounds: Option<[f32; 4]>,
}
impl Default for NavParams {
    fn default() -> Self {
        Self {
            radius: 0.2,
            height: 1.0,
            step: 0.25,
            max_slope_deg: 45.0,
            cell_size: 0.25,
            tile_size: 128.0,
            bounds: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavPoly {
    pub cell: [i32; 2],
    pub y: f32,
    pub corners: [f32; 4],
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavTile {
    pub region: [i32; 2],
    pub fingerprint: String,
    pub polygons: Vec<NavPoly>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavMesh {
    pub version: u32,
    pub params: NavParams,
    pub fingerprint: String,
    pub tiles: Vec<NavTile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NavAgent {
    pub speed: f32,
    pub target: Option<[f32; 3]>,
    pub offset_y: f32,
    pub path: Vec<[f32; 3]>,
    pub next: usize,
    pub planned_target: Option<[f32; 3]>,
    pub status: String,
}
impl Default for NavAgent {
    fn default() -> Self {
        Self {
            speed: 1.0,
            target: None,
            offset_y: 0.0,
            path: vec![],
            next: 0,
            planned_target: None,
            status: "idle".into(),
        }
    }
}
