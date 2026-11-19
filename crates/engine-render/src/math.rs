//! Minimal column-major mat4 helpers — enough for view/proj/model.
//! Deliberately dependency-free (no glam) to keep the prototype lean.

pub type Mat4 = [[f32; 4]; 4]; // column-major: m[col][row]

pub fn mul(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            out[c][r] = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

/// Translation * Rotation(euler XYZ, degrees) * Scale
pub fn trs(position: [f32; 3], rot_deg: [f32; 3], scale: [f32; 3]) -> Mat4 {
    let (sx, cx) = rot_deg[0].to_radians().sin_cos();
    let (sy, cy) = rot_deg[1].to_radians().sin_cos();
    let (sz, cz) = rot_deg[2].to_radians().sin_cos();
    // R = Rz * Ry * Rx (apply X first, like most engines' XYZ euler)
    let r: Mat4 = [
        [cy * cz, cy * sz, -sy, 0.0],
        [sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy, 0.0],
        [cx * sy * cz + sx * sz, cx * sy * sz - sx * cz, cx * cy, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut m = r;
    for c in 0..3 {
        m[c][0] *= scale[c];
        m[c][1] *= scale[c];
        m[c][2] *= scale[c];
    }
    m[3] = [position[0], position[1], position[2], 1.0];
    m
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Right-handed look-at view matrix.
pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// Right-handed perspective with wgpu clip z in [0,1].
pub fn perspective(fov_y_deg: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y_deg.to_radians() * 0.5).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, (far * near) / (near - far), 0.0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trs_moves_origin() {
        let m = trs([2.0, 3.0, 4.0], [0.0; 3], [1.0; 3]);
        // transform of origin = last column xyz
        assert_eq!([m[3][0], m[3][1], m[3][2]], [2.0, 3.0, 4.0]);
    }

    #[test]
    fn perspective_maps_near_to_zero() {
        let p = perspective(60.0, 1.0, 0.1, 100.0);
        // z_eye = -near should map to clip z = 0
        let clip_w = -p[2][3] * 0.1; // w = -z_eye
        let clip_z = p[2][2] * -0.1 + p[3][2];
        assert!((clip_z / clip_w).abs() < 1e-4);
    }
}
