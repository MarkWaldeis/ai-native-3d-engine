//! Procedural mesh generators. Pure CPU code — takes a PrimitiveKind recipe,
//! returns vertex/index buffers. No GPU, no dependencies beyond `shared`.

use shared::mesh::{MeshData, Vertex};
use shared::scene::PrimitiveKind;
use std::f32::consts::{PI, TAU};

pub mod skinning;

pub fn animation_clips(path: &std::path::Path) -> anyhow::Result<Vec<shared::animation::AnimationClip>> {
    let document = gltf::Gltf::open(path)?;
    let blob = document.blob.as_deref().ok_or_else(|| anyhow::anyhow!("animation inspection requires a self-contained GLB"))?;
    anyhow::ensure!(document.buffers().all(|b| matches!(b.source(), gltf::buffer::Source::Bin)), "external animation buffers are not allowed");
    let mut clips = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for animation in document.animations() {
        let name = animation.name().map(str::to_owned).unwrap_or_else(|| format!("animation_{}", animation.index()));
        anyhow::ensure!(names.insert(name.clone()), "duplicate animation clip name {name}");
        let mut duration = 0.0_f64;
        for channel in animation.channels() {
            let reader = channel.reader(|_| Some(blob));
            for time in reader.read_inputs().ok_or_else(|| anyhow::anyhow!("missing animation input"))? {
                anyhow::ensure!(time.is_finite() && time >= 0.0, "invalid animation timestamp");
                duration = duration.max(time as f64);
            }
        }
        anyhow::ensure!(duration > 0.0, "clip {name} has no positive duration");
        clips.push(shared::animation::AnimationClip { name, duration });
    }
    Ok(clips)
}

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
        // GLB assets are loaded via `load_gltf` (fallible IO), not generated.
        PrimitiveKind::Gltf { .. } => MeshData::new(),
    }
}

/// Load a `.glb`/`.gltf` file into a single fused `MeshData`: all primitives of
/// all nodes, with node transforms baked into vertex positions. Normals get
/// the same matrix (fine for rotation/uniform-scale node transforms — typical
/// Blender exports). Missing normals/uvs fall back to up/[0,0].
pub fn load_gltf(path: &std::path::Path) -> anyhow::Result<MeshData> {
    use anyhow::{bail, Context};
    let (doc, buffers, _images) =
        gltf::import(path).with_context(|| format!("gltf import {}", path.display()))?;

    let scene = doc
        .default_scene()
        .or_else(|| doc.scenes().next())
        .context("gltf has no scene")?;

    let mut out = MeshData::new();
    for node in scene.nodes() {
        walk_node(&node, Mat4::IDENTITY, &buffers, &mut out);
    }
    if out.vertices.is_empty() {
        bail!("gltf {} contains no mesh geometry", path.display());
    }
    Ok(out)
}

/// Minimal 4x4 for baking node transforms (column-major, glTF convention).
#[derive(Clone, Copy)]
struct Mat4([[f32; 4]; 4]);

impl Mat4 {
    const IDENTITY: Self = Mat4([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    fn from_gltf(m: [[f32; 4]; 4]) -> Self {
        Mat4(m)
    }

    fn mul(self, o: Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for (c, oc) in r.iter_mut().enumerate() {
            for (ri, e) in oc.iter_mut().enumerate() {
                *e = (0..4).map(|k| self.0[k][ri] * o.0[c][k]).sum();
            }
        }
        Mat4(r)
    }

    fn xform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let (x, y, z) = (p[0], p[1], p[2]);
        [
            self.0[0][0] * x + self.0[1][0] * y + self.0[2][0] * z + self.0[3][0],
            self.0[0][1] * x + self.0[1][1] * y + self.0[2][1] * z + self.0[3][1],
            self.0[0][2] * x + self.0[1][2] * y + self.0[2][2] * z + self.0[3][2],
        ]
    }

    fn xform_dir(&self, p: [f32; 3]) -> [f32; 3] {
        let (x, y, z) = (p[0], p[1], p[2]);
        [
            self.0[0][0] * x + self.0[1][0] * y + self.0[2][0] * z,
            self.0[0][1] * x + self.0[1][1] * y + self.0[2][1] * z,
            self.0[0][2] * x + self.0[1][2] * y + self.0[2][2] * z,
        ]
    }
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-8 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn walk_node(node: &gltf::Node, parent: Mat4, buffers: &[gltf::buffer::Data], out: &mut MeshData) {
    let world = parent.mul(Mat4::from_gltf(node.transform().matrix()));
    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            let reader = prim.reader(|buf| buffers.get(buf.index()).map(|d| &d.0[..]));
            let positions: Vec<[f32; 3]> = match reader.read_positions() {
                Some(it) => it.collect(),
                None => continue,
            };
            let normals: Vec<[f32; 3]> = reader
                .read_normals()
                .map(|it| it.collect())
                .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(0)
                .map(|t| t.into_f32().collect())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
            let indices: Vec<u32> = match reader.read_indices() {
                Some(it) => it.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            if !matches!(prim.mode(), gltf::mesh::Mode::Triangles) {
                continue;
            }
            let base = out.vertices.len() as u32;
            for i in 0..positions.len() {
                let mut vertex=Vertex::new(
                    world.xform_point(positions[i]),
                    normalize(world.xform_dir(normals[i])),
                    uvs[i],
                );
                vertex.color=prim.material().pbr_metallic_roughness().base_color_factor();
                out.vertices.push(vertex);
            }
            out.indices.extend(indices.iter().map(|i| i + base));
        }
    }
    for child in node.children() {
        walk_node(&child, world, buffers, out);
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
