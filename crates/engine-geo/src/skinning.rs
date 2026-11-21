use crate::{normalize, Mat4};
use anyhow::{bail, ensure, Context, Result};
use gltf::accessor::{DataType, Dimensions};
use gltf::animation::{util::ReadOutputs, Interpolation, Property};
use shared::{
    animation::Animator,
    mesh::{MeshData, Vertex},
};
use std::{collections::BTreeMap, path::Path};

fn attribute(
    accessor: &gltf::Accessor<'_>,
    dimensions: Dimensions,
    types: &[DataType],
) -> Result<()> {
    ensure!(
        accessor.dimensions() == dimensions && types.contains(&accessor.data_type()),
        "unsupported accessor type"
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct Trs {
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}

impl Trs {
    fn matrix(self) -> Mat4 {
        let [x, y, z, w] = unit(self.rotation);
        let [sx, sy, sz] = self.scale;
        Mat4([
            [
                (1.0 - 2.0 * (y * y + z * z)) * sx,
                2.0 * (x * y + z * w) * sx,
                2.0 * (x * z - y * w) * sx,
                0.0,
            ],
            [
                2.0 * (x * y - z * w) * sy,
                (1.0 - 2.0 * (x * x + z * z)) * sy,
                2.0 * (y * z + x * w) * sy,
                0.0,
            ],
            [
                2.0 * (x * z + y * w) * sz,
                2.0 * (y * z - x * w) * sz,
                (1.0 - 2.0 * (x * x + y * y)) * sz,
                0.0,
            ],
            [
                self.translation[0],
                self.translation[1],
                self.translation[2],
                1.0,
            ],
        ])
    }
}

fn unit(q: [f32; 4]) -> [f32; 4] {
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length > 1e-12 {
        q.map(|v| v / length)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

fn mix<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
    std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t)
}

fn slerp(a: [f32; 4], mut b: [f32; 4], t: f32) -> [f32; 4] {
    let a = unit(a);
    b = unit(b);
    let mut dot = a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
    if dot < 0.0 {
        b = b.map(|v| -v);
        dot = -dot;
    }
    if dot > 0.9995 {
        return unit(mix(a, b, t));
    }
    let angle = dot.clamp(-1.0, 1.0).acos();
    let denominator = angle.sin();
    std::array::from_fn(|i| {
        (a[i] * ((1.0 - t) * angle).sin() + b[i] * (t * angle).sin()) / denominator
    })
}

struct Track {
    node: usize,
    property: Property,
    interpolation: Interpolation,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
}

impl Track {
    fn sample(&self, time: f64) -> [f32; 4] {
        let cubic = self.interpolation == Interpolation::CubicSpline;
        let key = |i: usize| self.values[if cubic { i * 3 + 1 } else { i }];
        let after = self.times.partition_point(|t| (*t as f64) <= time);
        if after == 0 {
            return key(0);
        }
        if after == self.times.len() {
            return key(after - 1);
        }
        let before = after - 1;
        if self.interpolation == Interpolation::Step {
            return key(before);
        }
        let span = self.times[after] - self.times[before];
        let t = ((time - self.times[before] as f64) / span as f64) as f32;
        if cubic {
            let (a, b) = (key(before), key(after));
            let outgoing = self.values[before * 3 + 2];
            let incoming = self.values[after * 3];
            let value = std::array::from_fn(|i| {
                (2.0 * t * t * t - 3.0 * t * t + 1.0) * a[i]
                    + (t * t * t - 2.0 * t * t + t) * span * outgoing[i]
                    + (-2.0 * t * t * t + 3.0 * t * t) * b[i]
                    + (t * t * t - t * t) * span * incoming[i]
            });
            return if self.property == Property::Rotation {
                unit(value)
            } else {
                value
            };
        }
        if self.property == Property::Rotation {
            slerp(key(before), key(after), t)
        } else {
            mix(key(before), key(after), t)
        }
    }
}

struct Skin {
    joints: Vec<usize>,
    inverse_bind: Vec<Mat4>,
}

struct Primitive {
    node: usize,
    skin: Option<usize>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

pub struct SkinPose {
    pub mesh: MeshData,
    pub node_matrices: Vec<[[f32; 4]; 4]>,
}

pub struct SkinnedAsset {
    bind: Vec<Trs>,
    parents: Vec<Option<usize>>,
    skins: Vec<Skin>,
    primitives: Vec<Primitive>,
    clips: BTreeMap<String, Vec<Track>>,
}

impl SkinnedAsset {
    pub fn load(path: &Path) -> Result<Self> {
        ensure!(
            std::fs::metadata(path)?.len() <= 64 * 1024 * 1024,
            "native animated GLB exceeds 64 MiB"
        );
        Self::from_slice(&std::fs::read(path)?)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "native animated GLB exceeds 64 MiB"
        );
        let gltf = gltf::Gltf::from_slice(bytes)?;
        let blob = gltf
            .blob
            .as_deref()
            .context("native animation requires a self-contained GLB")?;
        ensure!(
            gltf.buffers().count() == 1
                && gltf
                    .buffers()
                    .all(|b| matches!(b.source(), gltf::buffer::Source::Bin)),
            "external animation buffers are not allowed"
        );
        for view in gltf.views() {
            ensure!(
                view.offset()
                    .checked_add(view.length())
                    .is_some_and(|end| end <= blob.len()),
                "buffer view outside GLB"
            );
        }
        for accessor in gltf.accessors() {
            ensure!(
                accessor.count() > 0 && accessor.count() <= 4_000_000,
                "invalid accessor count"
            );
            ensure!(
                accessor.sparse().is_none(),
                "native sparse accessors are not supported"
            );
            if let Some(view) = accessor.view() {
                let stride = view.stride().unwrap_or(accessor.size());
                ensure!(
                    stride >= accessor.size(),
                    "accessor stride smaller than element"
                );
                let end = (accessor.count() - 1)
                    .checked_mul(stride)
                    .and_then(|n| n.checked_add(accessor.offset()))
                    .and_then(|n| n.checked_add(accessor.size()));
                ensure!(
                    end.is_some_and(|end| end <= view.length()),
                    "accessor outside buffer view"
                );
            }
        }
        let count = gltf.nodes().count();
        ensure!(count <= 4096, "native animation exceeds 4096 nodes");
        let mut bind = Vec::with_capacity(count);
        let mut parents = vec![None; count];
        let mut children = vec![Vec::new(); count];
        for node in gltf.nodes() {
            let (translation, rotation, scale) = node.transform().decomposed();
            ensure!(
                translation
                    .iter()
                    .chain(&rotation)
                    .chain(&scale)
                    .all(|v| v.is_finite()),
                "nonfinite node transform"
            );
            ensure!(
                rotation.iter().map(|v| v * v).sum::<f32>() > 1e-12,
                "zero node quaternion"
            );
            bind.push(Trs {
                translation,
                rotation,
                scale,
            });
            for child in node.children() {
                ensure!(
                    parents[child.index()].replace(node.index()).is_none(),
                    "node has multiple parents"
                );
                children[node.index()].push(child.index());
            }
        }
        world_matrices(&bind, &parents)?;
        let scene = gltf
            .default_scene()
            .or_else(|| gltf.scenes().next())
            .context("GLB has no scene")?;
        let mut order = Vec::new();
        let mut pending: Vec<_> = scene.nodes().map(|n| n.index()).collect();
        pending.reverse();
        while let Some(node) = pending.pop() {
            order.push(node);
            pending.extend(children[node].iter().rev().copied());
        }
        let active: std::collections::BTreeSet<_> = order.iter().copied().collect();
        ensure!(active.len() == order.len(), "duplicate scene nodes");
        let mut skins = Vec::new();
        for skin in gltf.skins() {
            if let Some(accessor) = skin.inverse_bind_matrices() {
                attribute(&accessor, Dimensions::Mat4, &[DataType::F32])?;
            }
            let joints: Vec<_> = skin.joints().map(|j| j.index()).collect();
            ensure!(
                !joints.is_empty() && joints.len() <= 256,
                "skin requires 1..256 joints"
            );
            let inverse_bind: Vec<_> = skin
                .reader(|_| Some(blob))
                .read_inverse_bind_matrices()
                .map(|m| m.map(Mat4).collect())
                .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
            ensure!(
                inverse_bind.len() == joints.len(),
                "inverse bind count differs from joint count"
            );
            ensure!(
                inverse_bind
                    .iter()
                    .all(|m| m.0.iter().flatten().all(|v| v.is_finite())),
                "nonfinite inverse bind matrix"
            );
            skins.push(Skin {
                joints,
                inverse_bind,
            });
        }
        let nodes: Vec<_> = gltf.nodes().collect();
        let mut primitives = Vec::new();
        let mut total_vertices = 0;
        for index in order {
            let node = &nodes[index];
            if let Some(mesh) = node.mesh() {
                for primitive in mesh.primitives() {
                    ensure!(
                        primitive.mode() == gltf::mesh::Mode::Triangles,
                        "native animated meshes require triangles"
                    );
                    ensure!(
                        primitive.morph_targets().len() == 0,
                        "native morph targets are not supported"
                    );
                    let reader = primitive.reader(|_| Some(blob));
                    let position = primitive
                        .get(&gltf::Semantic::Positions)
                        .context("missing positions")?;
                    attribute(&position, Dimensions::Vec3, &[DataType::F32])?;
                    ensure!(
                        position.count() <= 1_000_000,
                        "native animated asset exceeds one million vertices"
                    );
                    if let Some(normal) = primitive.get(&gltf::Semantic::Normals) {
                        attribute(&normal, Dimensions::Vec3, &[DataType::F32])?;
                    }
                    if let Some(uv) = primitive.get(&gltf::Semantic::TexCoords(0)) {
                        attribute(
                            &uv,
                            Dimensions::Vec2,
                            &[DataType::F32, DataType::U8, DataType::U16],
                        )?;
                    }
                    if let Some(indices) = primitive.indices() {
                        attribute(
                            &indices,
                            Dimensions::Scalar,
                            &[DataType::U8, DataType::U16, DataType::U32],
                        )?;
                    }
                    let positions: Vec<_> = reader
                        .read_positions()
                        .context("missing positions")?
                        .collect();
                    total_vertices += positions.len();
                    ensure!(
                        total_vertices <= 1_000_000,
                        "native animated asset exceeds one million vertices"
                    );
                    let normals: Vec<_> = reader
                        .read_normals()
                        .map(|it| it.collect())
                        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
                    let uvs: Vec<_> = reader
                        .read_tex_coords(0)
                        .map(|it| it.into_f32().collect())
                        .unwrap_or_else(|| vec![[0.0; 2]; positions.len()]);
                    ensure!(
                        normals.len() == positions.len() && uvs.len() == positions.len(),
                        "vertex attribute count mismatch"
                    );
                    let color = primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_factor();
                    let vertices: Vec<_> = positions
                        .into_iter()
                        .zip(normals)
                        .zip(uvs)
                        .map(|((p, n), uv)| {
                            let mut v = Vertex::new(p, n, uv);
                            v.color = color;
                            v
                        })
                        .collect();
                    ensure!(
                        vertices.iter().all(|v| v
                            .position
                            .iter()
                            .chain(&v.normal)
                            .chain(&v.uv)
                            .all(|x| x.is_finite())),
                        "nonfinite vertex attribute"
                    );
                    let indices: Vec<_> = reader
                        .read_indices()
                        .map(|it| it.into_u32().collect())
                        .unwrap_or_else(|| (0..vertices.len() as u32).collect());
                    ensure!(
                        indices.len() % 3 == 0
                            && indices.iter().all(|i| (*i as usize) < vertices.len()),
                        "invalid triangle indices"
                    );
                    let mut joints = Vec::new();
                    let mut weights = Vec::new();
                    let skin = node.skin().map(|s| s.index());
                    if let Some(skin) = skin {
                        ensure!(
                            skins[skin].joints.iter().all(|j| active.contains(j)),
                            "skin joint outside active scene"
                        );
                        ensure!(
                            primitive.get(&gltf::Semantic::Joints(1)).is_none()
                                && primitive.get(&gltf::Semantic::Weights(1)).is_none(),
                            "more than four skin influences are not supported"
                        );
                        attribute(
                            &primitive
                                .get(&gltf::Semantic::Joints(0))
                                .context("missing JOINTS_0")?,
                            Dimensions::Vec4,
                            &[DataType::U8, DataType::U16],
                        )?;
                        attribute(
                            &primitive
                                .get(&gltf::Semantic::Weights(0))
                                .context("missing WEIGHTS_0")?,
                            Dimensions::Vec4,
                            &[DataType::F32, DataType::U8, DataType::U16],
                        )?;
                        joints = reader
                            .read_joints(0)
                            .context("missing JOINTS_0")?
                            .into_u16()
                            .collect();
                        weights = reader
                            .read_weights(0)
                            .context("missing WEIGHTS_0")?
                            .into_f32()
                            .collect();
                        ensure!(
                            joints.len() == vertices.len() && weights.len() == vertices.len(),
                            "skin attribute count mismatch"
                        );
                        ensure!(
                            joints
                                .iter()
                                .flatten()
                                .all(|j| (*j as usize) < skins[skin].joints.len()),
                            "joint index outside skin"
                        );
                        for weights in &mut weights {
                            let sum = weights.iter().sum::<f32>();
                            ensure!(
                                weights.iter().all(|w| w.is_finite() && *w >= 0.0)
                                    && sum.is_finite()
                                    && sum > 0.0,
                                "invalid skin weights"
                            );
                            *weights = weights.map(|w| w / sum);
                        }
                    }
                    primitives.push(Primitive {
                        node: index,
                        skin,
                        vertices,
                        indices,
                        joints,
                        weights,
                    });
                }
            }
        }
        ensure!(total_vertices > 0, "GLB contains no mesh geometry");
        let mut clips = BTreeMap::new();
        for animation in gltf.animations() {
            let mut tracks = Vec::new();
            let mut targets = std::collections::BTreeSet::new();
            for channel in animation.channels() {
                let property = channel.target().property();
                let dimensions = match property {
                    Property::Translation | Property::Scale => Dimensions::Vec3,
                    Property::Rotation => Dimensions::Vec4,
                    Property::MorphTargetWeights => {
                        bail!("native morph animation is not supported")
                    }
                };
                attribute(
                    &channel.sampler().input(),
                    Dimensions::Scalar,
                    &[DataType::F32],
                )?;
                attribute(&channel.sampler().output(), dimensions, &[DataType::F32])?;
                ensure!(
                    targets.insert((channel.target().node().index(), format!("{property:?}"))),
                    "duplicate animation channel"
                );
                let reader = channel.reader(|_| Some(blob));
                let times: Vec<_> = reader
                    .read_inputs()
                    .context("missing animation times")?
                    .collect();
                ensure!(
                    !times.is_empty()
                        && times.iter().all(|t| t.is_finite() && *t >= 0.0)
                        && times.windows(2).all(|p| p[1] > p[0]),
                    "invalid animation times"
                );
                let values: Vec<[f32; 4]> =
                    match reader.read_outputs().context("missing animation values")? {
                        ReadOutputs::Translations(values) | ReadOutputs::Scales(values) => {
                            values.map(|v| [v[0], v[1], v[2], 0.0]).collect()
                        }
                        ReadOutputs::Rotations(values) => values.into_f32().collect(),
                        ReadOutputs::MorphTargetWeights(_) => {
                            bail!("native morph animation is not supported")
                        }
                    };
                let interpolation = channel.sampler().interpolation();
                ensure!(
                    values.len()
                        == times.len()
                            * if interpolation == Interpolation::CubicSpline {
                                3
                            } else {
                                1
                            },
                    "animation output count mismatch"
                );
                ensure!(
                    values.iter().flatten().all(|v| v.is_finite()),
                    "nonfinite animation value"
                );
                if property == Property::Rotation {
                    let (offset, stride) = if interpolation == Interpolation::CubicSpline {
                        (1, 3)
                    } else {
                        (0, 1)
                    };
                    ensure!(
                        values.iter().skip(offset).step_by(stride).all(|v| v
                            .iter()
                            .map(|x| x * x)
                            .sum::<f32>()
                            > 1e-12),
                        "zero animation quaternion"
                    );
                }
                tracks.push(Track {
                    node: channel.target().node().index(),
                    property,
                    interpolation,
                    times,
                    values,
                });
            }
            let name = animation
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("animation_{}", animation.index()));
            ensure!(clips.insert(name, tracks).is_none(), "duplicate clip name");
        }
        Ok(Self {
            bind,
            parents,
            skins,
            primitives,
            clips,
        })
    }

    fn clip_pose(&self, animator: &Animator, name: &str, time: f64) -> Result<Vec<Trs>> {
        ensure!(
            time.is_finite() && time >= 0.0,
            "invalid animation sample time"
        );
        let state = animator
            .machine
            .states
            .get(name)
            .context("unknown animation state")?;
        let tracks = self
            .clips
            .get(&state.clip)
            .with_context(|| format!("missing GLB clip {}", state.clip))?;
        let mut pose = self.bind.clone();
        for track in tracks {
            let value = track.sample(time);
            match track.property {
                Property::Translation => {
                    pose[track.node].translation = [value[0], value[1], value[2]]
                }
                Property::Scale => pose[track.node].scale = [value[0], value[1], value[2]],
                Property::Rotation => pose[track.node].rotation = unit(value),
                Property::MorphTargetWeights => unreachable!(),
            }
        }
        Ok(pose)
    }

    pub fn pose(&self, animator: Option<&Animator>) -> Result<SkinPose> {
        let local = if let Some(animator) = animator {
            let name = if animator.current_state.is_empty() {
                &animator.machine.initial
            } else {
                &animator.current_state
            };
            let mut current = self.clip_pose(animator, name, animator.time)?;
            ensure!(
                animator.blend_time.is_finite()
                    && animator.blend_time >= 0.0
                    && animator.blend_elapsed.is_finite()
                    && animator.blend_elapsed >= 0.0,
                "invalid animation blend"
            );
            if let Some(previous) = &animator.previous_state {
                let weight = if animator.blend_time > 0.0 {
                    (animator.blend_elapsed / animator.blend_time).min(1.0) as f32
                } else {
                    1.0
                };
                if weight < 1.0 {
                    ensure!(
                        previous != name,
                        "animation cannot crossfade a state into itself"
                    );
                    let previous = self.clip_pose(animator, previous, animator.previous_time)?;
                    for (current, previous) in current.iter_mut().zip(previous) {
                        current.translation =
                            mix(previous.translation, current.translation, weight);
                        current.scale = mix(previous.scale, current.scale, weight);
                        current.rotation = slerp(previous.rotation, current.rotation, weight);
                    }
                }
            }
            current
        } else {
            self.bind.clone()
        };
        let world = world_matrices(&local, &self.parents)?;
        let palettes: Vec<Vec<_>> = self
            .skins
            .iter()
            .map(|s| {
                s.joints
                    .iter()
                    .zip(&s.inverse_bind)
                    .map(|(j, ib)| world[*j].mul(*ib))
                    .collect()
            })
            .collect();
        let mut mesh = MeshData::new();
        for primitive in &self.primitives {
            let base = mesh.vertices.len() as u32;
            for (i, source) in primitive.vertices.iter().enumerate() {
                let transform = if let Some(skin) = primitive.skin {
                    let mut matrix = [[0.0; 4]; 4];
                    for (joint, weight) in primitive.joints[i].iter().zip(primitive.weights[i]) {
                        for (column, source) in
                            matrix.iter_mut().zip(palettes[skin][*joint as usize].0)
                        {
                            for (value, source) in column.iter_mut().zip(source) {
                                *value += source * weight;
                            }
                        }
                    }
                    Mat4(matrix)
                } else {
                    world[primitive.node]
                };
                let mut vertex = *source;
                vertex.position = transform.xform_point(source.position);
                vertex.normal = normalize(transform.xform_dir(source.normal));
                ensure!(
                    vertex
                        .position
                        .iter()
                        .chain(&vertex.normal)
                        .all(|v| v.is_finite()),
                    "nonfinite posed vertex"
                );
                mesh.vertices.push(vertex);
            }
            mesh.indices
                .extend(primitive.indices.iter().map(|i| i + base));
        }
        Ok(SkinPose {
            mesh,
            node_matrices: world.into_iter().map(|m| m.0).collect(),
        })
    }
}

fn world_matrices(pose: &[Trs], parents: &[Option<usize>]) -> Result<Vec<Mat4>> {
    let mut result = vec![Mat4::IDENTITY; pose.len()];
    let mut marks = vec![0; pose.len()];
    let mut chain = Vec::new();
    for i in 0..pose.len() {
        let mut node = Some(i);
        while let Some(index) = node {
            if marks[index] == 2 {
                break;
            }
            ensure!(marks[index] != 1, "cyclic node hierarchy");
            marks[index] = 1;
            chain.push(index);
            node = parents[index];
        }
        while let Some(index) = chain.pop() {
            let parent = parents[index].map(|p| result[p]).unwrap_or(Mat4::IDENTITY);
            result[index] = parent.mul(pose[index].matrix());
            marks[index] = 2;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_clamps_steps_slerps_and_uses_cubic_tangents() {
        let mut track = Track {
            node: 0,
            property: Property::Translation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 2.0],
            values: vec![[0.0; 4], [2.0; 4]],
        };
        assert_eq!(track.sample(1.0), [1.0; 4]);
        assert_eq!(track.sample(-1.0), [0.0; 4]);
        assert_eq!(track.sample(3.0), [2.0; 4]);
        track.interpolation = Interpolation::Step;
        assert_eq!(track.sample(1.9), [0.0; 4]);
        assert_eq!(track.sample(2.0), [2.0; 4]);
        track.interpolation = Interpolation::CubicSpline;
        track.values = vec![[0.0; 4], [0.0; 4], [2.0; 4], [0.0; 4], [2.0; 4], [0.0; 4]];
        assert_eq!(track.sample(1.0), [1.5; 4]);
        let q = slerp([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0], 0.5);
        assert!((q[2].abs() - 0.5_f32.sqrt()).abs() < 1e-6);
        assert_eq!(
            slerp([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, -1.0], 0.5),
            [0.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn empty_invalid_and_cyclic_assets_fail_without_panicking() {
        assert!(SkinnedAsset::from_slice(&[]).is_err());
        let pose = vec![
            Trs {
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3]
            };
            2
        ];
        assert!(world_matrices(&pose, &[Some(1), Some(0)]).is_err());
        let deep = vec![pose[0]; 4096];
        let parents: Vec<_> = (0..4096_usize).map(|i| i.checked_sub(1)).collect();
        assert_eq!(world_matrices(&deep, &parents).unwrap().len(), 4096);
    }
}
