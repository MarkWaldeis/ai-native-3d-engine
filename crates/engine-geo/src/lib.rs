//! Procedural mesh generators. Pure CPU code — takes a PrimitiveKind recipe,
//! returns vertex/index buffers. No GPU, no dependencies beyond `shared`.

use shared::mesh::{MeshData, Vertex};
use shared::scene::PrimitiveKind;
use std::f32::consts::{PI, TAU};

pub fn generate(kind: &PrimitiveKind) -> MeshData {
    match kind {
        PrimitiveKind::Box { size } => box_mesh(*size),
        PrimitiveKind::Plane { size } => plane_mesh(*size),
        PrimitiveKind::Sphere { radius, segments } => sphere_mesh(*radius, *segments),
        PrimitiveKind::Cylinder {
            radius,
            height,
            segments,
        } => cylinder_mesh(*radius, *height, *segments),
        PrimitiveKind::WallWithDoor {
            width,
            height,
            door_width,
            door_height,
        } => wall_with_door(*width, *height, *door_width, *door_height),
    }
}

fn v(p: [f32; 3], n: [f32; 3], uv: [f32; 2]) -> Vertex {
    Vertex::new(p, n, uv)
}

/// Axis-aligned box centered at origin, per-face normals.
pub fn box_mesh(size: [f32; 3]) -> MeshData {
    let [w, h, d] = [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0];
    let mut m = MeshData::new();
    // +Y top
    m.push_quad([
        v([-w, h, -d], [0.0, 1.0, 0.0], [0.0, 0.0]),
        v([-w, h, d], [0.0, 1.0, 0.0], [0.0, 1.0]),
        v([w, h, d], [0.0, 1.0, 0.0], [1.0, 1.0]),
        v([w, h, -d], [0.0, 1.0, 0.0], [1.0, 0.0]),
    ]);
    // -Y bottom
    m.push_quad([
        v([-w, -h, d], [0.0, -1.0, 0.0], [0.0, 0.0]),
        v([-w, -h, -d], [0.0, -1.0, 0.0], [0.0, 1.0]),
        v([w, -h, -d], [0.0, -1.0, 0.0], [1.0, 1.0]),
        v([w, -h, d], [0.0, -1.0, 0.0], [1.0, 0.0]),
    ]);
    // +Z front
    m.push_quad([
        v([-w, -h, d], [0.0, 0.0, 1.0], [0.0, 0.0]),
        v([w, -h, d], [0.0, 0.0, 1.0], [1.0, 0.0]),
        v([w, h, d], [0.0, 0.0, 1.0], [1.0, 1.0]),
        v([-w, h, d], [0.0, 0.0, 1.0], [0.0, 1.0]),
    ]);
    // -Z back
    m.push_quad([
        v([w, -h, -d], [0.0, 0.0, -1.0], [0.0, 0.0]),
        v([-w, -h, -d], [0.0, 0.0, -1.0], [1.0, 0.0]),
        v([-w, h, -d], [0.0, 0.0, -1.0], [1.0, 1.0]),
        v([w, h, -d], [0.0, 0.0, -1.0], [0.0, 1.0]),
    ]);
    // +X right
    m.push_quad([
        v([w, -h, d], [1.0, 0.0, 0.0], [0.0, 0.0]),
        v([w, -h, -d], [1.0, 0.0, 0.0], [1.0, 0.0]),
        v([w, h, -d], [1.0, 0.0, 0.0], [1.0, 1.0]),
        v([w, h, d], [1.0, 0.0, 0.0], [0.0, 1.0]),
    ]);
    // -X left
    m.push_quad([
        v([-w, -h, -d], [-1.0, 0.0, 0.0], [0.0, 0.0]),
        v([-w, -h, d], [-1.0, 0.0, 0.0], [1.0, 0.0]),
        v([-w, h, d], [-1.0, 0.0, 0.0], [1.0, 1.0]),
        v([-w, h, -d], [-1.0, 0.0, 0.0], [0.0, 1.0]),
    ]);
    m
}

/// Horizontal quad in the XZ plane, size = edge length, facing +Y.
pub fn plane_mesh(size: f32) -> MeshData {
    let s = size / 2.0;
    let mut m = MeshData::new();
    m.push_quad([
        v([-s, 0.0, -s], [0.0, 1.0, 0.0], [0.0, 0.0]),
        v([-s, 0.0, s], [0.0, 1.0, 0.0], [0.0, size]),
        v([s, 0.0, s], [0.0, 1.0, 0.0], [size, size]),
        v([s, 0.0, -s], [0.0, 1.0, 0.0], [size, 0.0]),
    ]);
    m
}

/// UV sphere centered at origin. segments = columns, rings implied.
pub fn sphere_mesh(radius: f32, segments: u32) -> MeshData {
    let segs = segments.max(8);
    let rings = (segments.max(8) / 2).max(4);
    let mut m = MeshData::new();
    for r in 0..=rings {
        let theta = r as f32 / rings as f32 * PI; // 0..PI
        let (st, ct) = theta.sin_cos();
        for c in 0..=segs {
            let phi = c as f32 / segs as f32 * TAU;
            let (sp, cp) = phi.sin_cos();
            let n = [st * cp, ct, st * sp];
            m.vertices.push(v(
                [radius * n[0], radius * n[1], radius * n[2]],
                n,
                [c as f32 / segs as f32, r as f32 / rings as f32],
            ));
        }
    }
    let row = segs + 1;
    for r in 0..rings {
        for c in 0..segs {
            let a = r * row + c;
            let b = a + row;
            m.indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    m
}

/// Cylinder centered at origin, axis = Y, flat-shaded caps.
pub fn cylinder_mesh(radius: f32, height: f32, segments: u32) -> MeshData {
    let segs = segments.max(8);
    let h = height / 2.0;
    let mut m = MeshData::new();
    // side
    for i in 0..=segs {
        let phi = i as f32 / segs as f32 * TAU;
        let (sp, cp) = phi.sin_cos();
        let n = [cp, 0.0, sp];
        m.vertices.push(v([radius * cp, -h, radius * sp], n, [i as f32 / segs as f32, 0.0]));
        m.vertices.push(v([radius * cp, h, radius * sp], n, [i as f32 / segs as f32, 1.0]));
    }
    for i in 0..segs {
        let a = i * 2;
        m.indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
    // caps
    for (y, n, flip) in [(h, 1.0f32, false), (-h, -1.0f32, true)] {
        let center = m.vertices.len() as u32;
        m.vertices.push(v([0.0, y, 0.0], [0.0, n, 0.0], [0.5, 0.5]));
        for i in 0..=segs {
            let phi = i as f32 / segs as f32 * TAU;
            let (sp, cp) = phi.sin_cos();
            m.vertices.push(v(
                [radius * cp, y, radius * sp],
                [0.0, n, 0.0],
                [0.5 + 0.5 * cp, 0.5 + 0.5 * sp],
            ));
        }
        for i in 0..segs {
            let (a, b) = (center + 1 + i, center + 1 + i + 1);
            if flip {
                m.indices.extend_from_slice(&[center, b, a]);
            } else {
                m.indices.extend_from_slice(&[center, a, b]);
            }
        }
    }
    m
}

/// Wall in the XY plane (front faces +Z), standing on y=0, centered on x.
/// Rectangular door hole centered on x, bottom at y=0. Includes a back face
/// (flipped winding) and inner door jamb faces with fixed thickness.
pub fn wall_with_door(width: f32, height: f32, door_width: f32, door_height: f32) -> MeshData {
    let t = 0.15; // wall thickness
    let hw = width / 2.0;
    let hd = (door_width / 2.0).min(hw);
    let dh = door_height.min(height);
    let zf = t / 2.0;
    let zb = -t / 2.0;
    let mut m = MeshData::new();
    let n = [0.0, 0.0, 1.0];
    let nb = [0.0, 0.0, -1.0];

    // front face strips (+Z): left, right, above-door lintel
    let front = [
        ([-hw, 0.0], [-hd, height]),
        ([hd, 0.0], [hw, height]),
        ([-hd, dh], [hd, height]),
    ];
    for ([x0, y0], [x1, y1]) in front {
        m.push_quad([
            v([x0, y0, zf], n, [0.0, 0.0]),
            v([x1, y0, zf], n, [1.0, 0.0]),
            v([x1, y1, zf], n, [1.0, 1.0]),
            v([x0, y1, zf], n, [0.0, 1.0]),
        ]);
        m.push_quad([
            v([x1, y0, zb], nb, [0.0, 0.0]),
            v([x0, y0, zb], nb, [1.0, 0.0]),
            v([x0, y1, zb], nb, [1.0, 1.0]),
            v([x1, y1, zb], nb, [0.0, 1.0]),
        ]);
    }

    // door jamb: left inner wall (-x side faces +X)
    m.push_quad([
        v([-hd, 0.0, zb], [1.0, 0.0, 0.0], [0.0, 0.0]),
        v([-hd, 0.0, zf], [1.0, 0.0, 0.0], [1.0, 0.0]),
        v([-hd, dh, zf], [1.0, 0.0, 0.0], [1.0, 1.0]),
        v([-hd, dh, zb], [1.0, 0.0, 0.0], [0.0, 1.0]),
    ]);
    // right jamb (+x side faces -X)
    m.push_quad([
        v([hd, 0.0, zf], [-1.0, 0.0, 0.0], [0.0, 0.0]),
        v([hd, 0.0, zb], [-1.0, 0.0, 0.0], [1.0, 0.0]),
        v([hd, dh, zb], [-1.0, 0.0, 0.0], [1.0, 1.0]),
        v([hd, dh, zf], [-1.0, 0.0, 0.0], [0.0, 1.0]),
    ]);
    // lintel underside (faces -Y)
    m.push_quad([
        v([-hd, dh, zf], [0.0, -1.0, 0.0], [0.0, 0.0]),
        v([-hd, dh, zb], [0.0, -1.0, 0.0], [0.0, 1.0]),
        v([hd, dh, zb], [0.0, -1.0, 0.0], [1.0, 1.0]),
        v([hd, dh, zf], [0.0, -1.0, 0.0], [1.0, 0.0]),
    ]);
    m
}

/// Convenience: rotate a wall (built in XY plane, bottom at y=0) so it can be
/// used as a room side — not needed for v0.1 but cheap to keep.
pub fn rotate_y(m: &mut MeshData, angle_deg: f32) {
    let (s, c) = angle_deg.to_radians().sin_cos();
    for vtx in &mut m.vertices {
        let [x, y, z] = vtx.position;
        vtx.position = [x * c + z * s, y, -x * s + z * c];
        let [nx, ny, nz] = vtx.normal;
        vtx.normal = [nx * c + nz * s, ny, -nx * s + nz * c];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid(m: &MeshData) {
        assert!(!m.vertices.is_empty());
        assert!(!m.indices.is_empty());
        assert!(m.indices.iter().all(|i| (*i as usize) < m.vertices.len()));
        assert_eq!(m.indices.len() % 3, 0);
    }

    #[test]
    fn all_primitives_valid() {
        for k in [
            PrimitiveKind::Box { size: [1.0; 3] },
            PrimitiveKind::Plane { size: 10.0 },
            PrimitiveKind::Sphere { radius: 1.0, segments: 16 },
            PrimitiveKind::Cylinder { radius: 1.0, height: 2.0, segments: 16 },
            PrimitiveKind::WallWithDoor { width: 6.0, height: 3.0, door_width: 1.2, door_height: 2.1 },
        ] {
            assert_valid(&generate(&k));
        }
    }

    #[test]
    fn box_has_24_verts_36_idx() {
        let m = box_mesh([1.0; 3]);
        assert_eq!(m.vertices.len(), 24);
        assert_eq!(m.indices.len(), 36);
    }
}
