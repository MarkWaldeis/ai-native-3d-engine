//! Delayed snapshot rendering, independent of the prediction simulation.
use crate::{protocol::Snapshot, TICK_MS};
use engine_runtime::NetEntityState;
use std::collections::BTreeMap;

pub struct Interpolator {
    snapshots: BTreeMap<u64, Vec<NetEntityState>>,
    pub delay_ticks: f64,
    pub max_extrapolation_ticks: f64,
    capacity: usize,
    rendered_tick: std::cell::Cell<f64>,
}

impl Default for Interpolator {
    fn default() -> Self {
        Self {
            snapshots: BTreeMap::new(),
            delay_ticks: 6.0,
            max_extrapolation_ticks: 15.0,
            capacity: 128,
            rendered_tick: std::cell::Cell::new(f64::NEG_INFINITY),
        }
    }
}

impl Interpolator {
    /// Accepts full (already expanded) snapshots, including out-of-order arrivals.
    pub fn push(&mut self, snapshot: &Snapshot) {
        self.snapshots
            .insert(snapshot.server_tick, snapshot.entities.clone());
        while self.snapshots.len() > self.capacity {
            self.snapshots.pop_first();
        }
    }

    pub fn render(&self, now_tick_fraction: f64) -> Vec<NetEntityState> {
        let Some((&latest, _)) = self.snapshots.last_key_value() else {
            return vec![];
        };
        let tick = (latest as f64 - self.delay_ticks + now_tick_fraction.max(0.0))
            .max(self.rendered_tick.get());
        self.rendered_tick.set(tick);
        self.sample(tick)
    }

    pub fn sample(&self, tick: f64) -> Vec<NetEntityState> {
        let tick = tick.max(0.0);
        let before = self.snapshots.range(..=tick.floor() as u64).next_back();
        let after = self.snapshots.range(tick.ceil() as u64..).next();
        match (before, after) {
            (Some((&a, left)), Some((&b, right))) if b > a => {
                let fraction = ((tick - a as f64) / (b - a) as f64) as f32;
                left.iter()
                    .map(|state| {
                        right
                            .iter()
                            .find(|r| r.id == state.id)
                            .map_or_else(|| state.clone(), |r| interpolate(state, r, fraction))
                    })
                    .collect()
            }
            (Some((&at, states)), _) => {
                let dt = ((tick - at as f64).clamp(0.0, self.max_extrapolation_ticks.min(15.0))
                    * TICK_MS
                    / 1000.0) as f32;
                states
                    .iter()
                    .map(|state| {
                        let mut result = state.clone();
                        for axis in 0..3 {
                            result.position[axis] += state.linvel[axis] * dt;
                        }
                        result.rotation_deg = multiply(
                            angular_step(state.angvel, dt),
                            quaternion(state.rotation_deg),
                        )
                        .euler();
                        if let Some(rig) = &mut result.ragdoll {
                            if rig.enabled {
                                for bone in &mut rig.parts {
                                    if bone.sleeping {
                                        continue;
                                    }
                                    for axis in 0..3 {
                                        bone.position[axis] += bone.linvel[axis] * dt;
                                    }
                                    bone.rotation_deg = multiply(
                                        angular_step(bone.angvel, dt),
                                        quaternion(bone.rotation_deg),
                                    )
                                    .euler();
                                }
                            }
                        }
                        result
                    })
                    .collect()
            }
            (_, Some((_, states))) => states.clone(),
            _ => vec![],
        }
    }
}

fn interpolate(a: &NetEntityState, b: &NetEntityState, t: f32) -> NetEntityState {
    let mut result = a.clone();
    for i in 0..3 {
        result.position[i] += (b.position[i] - a.position[i]) * t;
        result.linvel[i] += (b.linvel[i] - a.linvel[i]) * t;
        result.angvel[i] += (b.angvel[i] - a.angvel[i]) * t;
    }
    result.rotation_deg = slerp(quaternion(a.rotation_deg), quaternion(b.rotation_deg), t).euler();
    if let (Some(left), Some(right)) = (&mut result.ragdoll, &b.ragdoll) {
        if left.active && right.active && left.parts.len() == right.parts.len() {
            for (l, r) in left.parts.iter_mut().zip(&right.parts) {
                if l.name != r.name {
                    continue;
                }
                for axis in 0..3 {
                    l.position[axis] += (r.position[axis] - l.position[axis]) * t;
                    l.linvel[axis] += (r.linvel[axis] - l.linvel[axis]) * t;
                    l.angvel[axis] += (r.angvel[axis] - l.angvel[axis]) * t;
                }
                l.rotation_deg =
                    slerp(quaternion(l.rotation_deg), quaternion(r.rotation_deg), t).euler();
            }
        }
    }
    result
}

#[derive(Clone, Copy)]
struct Quaternion([f32; 4]);
impl Quaternion {
    fn normalized(self) -> Self {
        let norm = self.0.iter().map(|v| v * v).sum::<f32>().sqrt();
        Self(self.0.map(|v| v / norm))
    }
    fn euler(self) -> [f32; 3] {
        let [w, x, y, z] = self.normalized().0;
        [
            ((2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y))).to_degrees(),
            (2.0 * (w * y - z * x)).clamp(-1.0, 1.0).asin().to_degrees(),
            ((2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z))).to_degrees(),
        ]
    }
}

// Rapier/nalgebra from_euler_angles: extrinsic X then Y then Z (qz*qy*qx).
fn quaternion(degrees: [f32; 3]) -> Quaternion {
    let [x, y, z] = degrees.map(|v| (v.to_radians() * 0.5).sin_cos());
    Quaternion([
        x.1 * y.1 * z.1 + x.0 * y.0 * z.0,
        x.0 * y.1 * z.1 - x.1 * y.0 * z.0,
        x.1 * y.0 * z.1 + x.0 * y.1 * z.0,
        x.1 * y.1 * z.0 - x.0 * y.0 * z.1,
    ])
}

fn slerp(a: Quaternion, mut b: Quaternion, t: f32) -> Quaternion {
    let mut dot = a.0.iter().zip(b.0).map(|(a, b)| a * b).sum::<f32>();
    if dot < 0.0 {
        b.0 = b.0.map(|v| -v);
        dot = -dot;
    }
    let (left, right) = if dot > 0.9995 {
        (1.0 - t, t)
    } else {
        let theta = dot.clamp(-1.0, 1.0).acos();
        (
            ((1.0 - t) * theta).sin() / theta.sin(),
            (t * theta).sin() / theta.sin(),
        )
    };
    Quaternion(std::array::from_fn(|i| a.0[i] * left + b.0[i] * right)).normalized()
}

fn multiply(a: Quaternion, b: Quaternion) -> Quaternion {
    let [w, x, y, z] = a.0;
    let [v, i, j, k] = b.0;
    Quaternion([
        w * v - x * i - y * j - z * k,
        w * i + x * v + y * k - z * j,
        w * j - x * k + y * v + z * i,
        w * k + x * j - y * i + z * v,
    ])
}

fn angular_step(velocity: [f32; 3], dt: f32) -> Quaternion {
    let speed = velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
    if speed < 1e-8 {
        return Quaternion([1.0, 0.0, 0.0, 0.0]);
    }
    let (sin, cos) = (speed * dt * 0.5).sin_cos();
    Quaternion([
        cos,
        velocity[0] / speed * sin,
        velocity[1] / speed * sin,
        velocity[2] / speed * sin,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotations_follow_short_arc_and_match_xyz_convention() {
        for angles in [[20.0, 30.0, 40.0], [-80.0, -10.0, 170.0]] {
            let roundtrip = quaternion(angles).euler();
            for i in 0..3 {
                assert!((angles[i] - roundtrip[i]).abs() < 1e-4);
            }
        }
        let middle = slerp(
            quaternion([0.0, 170.0, 0.0]),
            quaternion([0.0, -170.0, 0.0]),
            0.5,
        );
        let expected = quaternion([0.0, 180.0, 0.0]);
        assert!(
            (middle
                .0
                .iter()
                .zip(expected.0)
                .map(|(a, b)| a * b)
                .sum::<f32>()
                .abs()
                - 1.0)
                .abs()
                < 1e-5
        );
    }
}
