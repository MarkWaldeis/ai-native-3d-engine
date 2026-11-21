use crate::math;
use anyhow::{bail, Result};
use shared::scene::{Camera, Entity, PrimitiveKind};

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn center(self) -> [f32; 3] {
        std::array::from_fn(|i| (self.min[i] + self.max[i]) * 0.5)
    }
    pub fn corners(self) -> [[f32; 3]; 8] {
        std::array::from_fn(|n| {
            std::array::from_fn(|i| {
                if n & (1 << i) == 0 {
                    self.min[i]
                } else {
                    self.max[i]
                }
            })
        })
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }
}

pub fn bounds(entity: &Entity) -> Result<Bounds> {
    let mesh = match &entity.mesh {
        PrimitiveKind::Gltf { path } => engine_geo::load_gltf(std::path::Path::new(path))?,
        PrimitiveKind::Sphere { segments, .. } | PrimitiveKind::Cylinder { segments, .. }
            if *segments > 256 =>
        {
            bail!("segments exceed QA limit of 256")
        }
        kind => engine_geo::generate(kind),
    };
    let model = math::trs(
        entity.transform.position,
        entity.transform.rotation_deg,
        entity.transform.scale,
    );
    let mut result = Bounds {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };
    for vertex in mesh.vertices {
        let point = transform(model, vertex.position);
        for i in 0..3 {
            if !point[i].is_finite() {
                bail!("nonfinite entity bounds");
            }
            result.min[i] = result.min[i].min(point[i]);
            result.max[i] = result.max[i].max(point[i]);
        }
    }
    if !result.min[0].is_finite() {
        bail!("empty entity mesh");
    }
    Ok(result)
}

pub(crate) fn transform(matrix: math::Mat4, point: [f32; 3]) -> [f32; 4] {
    std::array::from_fn(|r| matrix[3][r] + (0..3).map(|c| matrix[c][r] * point[c]).sum::<f32>())
}

pub(crate) fn up(camera: &Camera, safe: bool) -> [f32; 3] {
    let d: [f32; 3] = std::array::from_fn(|i| camera.position[i] - camera.target[i]);
    if safe && d[0].abs() + d[2].abs() < 0.001 * d[1].abs() {
        [0.0, 0.0, -1.0]
    } else {
        [0.0, 1.0, 0.0]
    }
}

pub(crate) fn matrix(camera: &Camera, aspect: f32, near: f32, far: f32, safe: bool) -> math::Mat4 {
    math::mul(
        math::perspective(camera.fov_deg, aspect, near, far),
        math::look_at(camera.position, camera.target, up(camera, safe)),
    )
}

pub fn outside_frustum(bounds: Bounds, camera: &Camera, aspect: f32) -> bool {
    let matrix = matrix(camera, aspect, 0.05, 500.0, false);
    let points = bounds.corners().map(|p| transform(matrix, p));
    (0..6).any(|plane| {
        points.iter().all(|p| match plane {
            0 => p[0] < -p[3],
            1 => p[0] > p[3],
            2 => p[1] < -p[3],
            3 => p[1] > p[3],
            4 => p[2] < 0.0,
            _ => p[2] > p[3],
        })
    })
}

pub fn framed_camera(bounds: Bounds, current: Camera, view: &str, aspect: f32) -> Result<Camera> {
    let mut direction = match view {
        "current" => std::array::from_fn(|i| current.position[i] - current.target[i]),
        "front" => [0.0, 0.0, 1.0],
        "back" => [0.0, 0.0, -1.0],
        "left" => [-1.0, 0.0, 0.0],
        "right" => [1.0, 0.0, 0.0],
        "top" => [0.0, 1.0, 0.0],
        "iso" => [1.0, 0.8, 1.0],
        _ => {
            bail!("unknown view '{view}'; valid views: current, front, back, left, right, top, iso")
        }
    };
    let mut length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length < 0.0001 {
        direction = [1.0, 0.8, 1.0];
        length = 2.64_f32.sqrt();
    }
    let center = bounds.center();
    let diagonal = bounds
        .max
        .iter()
        .zip(bounds.min)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
        .max(0.1);
    let fov = if current.fov_deg.is_finite() {
        current.fov_deg.clamp(10.0, 120.0)
    } else {
        60.0
    };
    let vertical = (fov.to_radians() * 0.5).tan();
    let unit = direction.map(|v| v / length);
    let orientation = Camera {
        position: unit,
        target: [0.0; 3],
        fov_deg: fov,
    };
    let rotation = math::look_at(unit, [0.0; 3], up(&orientation, true));
    let distance = bounds
        .corners()
        .iter()
        .map(|p| {
            let offset = std::array::from_fn(|i| p[i] - center[i]);
            let view = transform(rotation, offset);
            let toward_camera = -view[2] - 1.0;
            toward_camera
                + (view[0].abs() / (vertical * aspect)).max(view[1].abs() / vertical) * 1.15
                + diagonal * 0.025
        })
        .fold(0.1, f32::max);
    Ok(Camera {
        position: std::array::from_fn(|i| center[i] + direction[i] / length * distance),
        target: center,
        fov_deg: fov,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_corner_has_margin_in_every_auto_view() {
        let bounds = Bounds {
            min: [-5.5, -0.4, -2.5],
            max: [5.5, 1.7, 2.5],
        };
        for name in ["current", "front", "back", "left", "right", "top", "iso"] {
            for aspect in [0.5, 1.5, 3.0] {
                let cam = framed_camera(bounds, Camera::default(), name, aspect).unwrap();
                let matrix = matrix(&cam, aspect, 0.0001, 1000.0, true);
                for p in bounds.corners().map(|p| transform(matrix, p)) {
                    assert!(
                        p[3] > 0.0
                            && p[0].abs() < p[3]
                            && p[1].abs() < p[3]
                            && p[2] > 0.0
                            && p[2] < p[3],
                        "{name} {aspect}: {p:?}"
                    );
                }
            }
        }
    }
}
