//! engine-render: wgpu headless renderer.
//! Renders the shared Scene into an offscreen texture and returns it as
//! RGBA bytes / PNG / base64 PNG — the "eyes" of the AI feedback loop.
//! No window/surface needed; works on Vulkan/DX12/Metal.

mod math;
pub mod inspection;
mod qa;
mod skinning;

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use shared::scene::{EntityId, Scene};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,   // xyz = direction TO light
    ambient: vec4<f32>,     // x = ambient intensity
};
@group(0) @binding(0) var<uniform> g: Globals;

struct Model { m: mat4x4<f32>, color: vec4<f32> };
@group(1) @binding(0) var<uniform> mdl: Model;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(i: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = g.view_proj * mdl.m * vec4<f32>(i.pos, 1.0);
    // uniform-scale approximation is fine for a prototype
    o.nrm = normalize((mdl.m * vec4<f32>(i.nrm, 0.0)).xyz);
    o.color = mdl.color * i.color;
    return o;
}

@fragment
fn fs(i: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(i.nrm);
    let l = max(dot(n, normalize(g.light_dir.xyz)), 0.0);
    let shade = g.ambient.x + (1.0 - g.ambient.x) * l;
    return vec4<f32>(i.color.rgb * shade, i.color.a);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: math::Mat4,
    light_dir: [f32; 4],
    ambient: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ModelUniform {
    m: math::Mat4,
    color: [f32; 4],
}

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    n_indices: u32,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    model_layout: wgpu::BindGroupLayout,
    color_tex: Option<wgpu::Texture>,
    color_view: Option<wgpu::TextureView>,
    depth_view: Option<wgpu::TextureView>,
    meshes: HashMap<EntityId, GpuMesh>,
    cached_revision: u64,
    cached_animators: Vec<(EntityId, shared::animation::Animator)>,
    skins: skinning::SkinCache,
    size: (u32, u32),
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

impl Renderer {
    /// Creates adapter/device/pipeline. Err if no usable GPU backend exists.
    pub fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| anyhow::anyhow!("request_adapter failed: {e:?}"))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .context("request_device")?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lit-unlit"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pl"),
            bind_group_layouts: &[&globals_layout, &model_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pbr-lite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<shared::mesh::Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {offset:32,shader_location:3,format:wgpu::VertexFormat::Float32x4},
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                        wgpu::VertexAttribute {
                            offset: 12,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            globals_buf,
            globals_bg,
            model_layout,
            color_tex: None,
            color_view: None,
            depth_view: None,
            meshes: HashMap::new(),
            cached_revision: u64::MAX,
            cached_animators: Vec::new(),
            skins: skinning::SkinCache::default(),
            size: (0, 0),
        })
    }

    fn ensure_targets(&mut self, w: u32, h: u32) {
        if self.size == (w, h) && self.color_view.is_some() {
            return;
        }
        self.size = (w, h);
        let desc = wgpu::TextureDescriptor {
            label: Some("color"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let tex = self.device.create_texture(&desc);
        self.color_view = Some(tex.create_view(&Default::default()));
        self.color_tex = Some(tex);
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            label: Some("depth"),
            ..desc
        });
        self.depth_view = Some(depth.create_view(&Default::default()));
    }

    fn sync_meshes(&mut self, scene: &Scene, revision: u64) -> Result<()> {
        let animators: Vec<_> = scene.entities.iter().filter_map(|e| e.animator.as_ref().map(|a| (e.id,a.clone()))).collect();
        if self.cached_revision == revision && self.cached_animators == animators {
            return Ok(());
        }
        self.meshes.clear();
        self.skins.retain(scene);
        for e in &scene.entities {
            let mesh = match &e.mesh {
                shared::scene::PrimitiveKind::Gltf { path } => {
                    if let Some(animator) = &e.animator {
                        self.skins.mesh(std::path::Path::new(path),animator)
                            .with_context(|| format!("native animation {} ({path})",e.name))?
                    } else {
                    match engine_geo::load_gltf(std::path::Path::new(path)) {
                        Ok(m) => m,
                        Err(err) => {
                            eprintln!("[render] gltf load failed ({path}): {err:#}");
                            continue;
                        }
                    }
                    }
                }
                other => engine_geo::generate(other),
            };
            if mesh.vertices.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            let vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&e.name),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ibuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&e.name),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let model = ModelUniform {
                m: math::trs(e.transform.position, e.transform.rotation_deg, e.transform.scale),
                color: e.color,
            };
            let uniform = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&e.name),
                contents: bytemuck::bytes_of(&model),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&e.name),
                layout: &self.model_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            self.meshes.insert(
                e.id,
                GpuMesh {
                    vbuf,
                    ibuf,
                    n_indices: mesh.indices.len() as u32,
                    uniform,
                    bind_group,
                },
            );
        }
        self.cached_revision = revision;
        self.cached_animators = animators;
        Ok(())
    }

    /// Renders the scene to an offscreen texture, returns unpadded RGBA bytes.
    pub fn render_rgba(&mut self, scene: &Scene, revision: u64, w: u32, h: u32) -> Result<Vec<u8>> {
        self.render_rgba_camera(scene, revision, w, h, 0.05, 500.0, false)
    }

    fn render_rgba_camera(&mut self, scene: &Scene, revision: u64, w: u32, h: u32, near: f32, far: f32, safe_up: bool) -> Result<Vec<u8>> {
        if w == 0 || h == 0 || w > 8192 || h > 8192 {
            bail!("invalid size {w}x{h}");
        }
        self.ensure_targets(w, h);
        self.sync_meshes(scene, revision)?;

        // update per-entity transforms (cheap enough without full re-sync)
        for e in &scene.entities {
            if let Some(g) = self.meshes.get(&e.id) {
                let model = ModelUniform {
                    m: math::trs(e.transform.position, e.transform.rotation_deg, e.transform.scale),
                    color: e.color,
                };
                self.queue.write_buffer(&g.uniform, 0, bytemuck::bytes_of(&model));
            }
        }

        let cam = &scene.camera;
        let aspect = w as f32 / h as f32;
        let view = math::look_at(cam.position, cam.target, inspection::up(cam, safe_up));
        let proj = math::perspective(cam.fov_deg, aspect, near, far);
        let globals = Globals {
            view_proj: math::mul(proj, view),
            light_dir: [scene.light_dir[0], scene.light_dir[1], scene.light_dir[2], 0.0],
            ambient: [scene.ambient, 0.0, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.color_view.as_ref().unwrap(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.32,
                            g: 0.48,
                            b: 0.68,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.depth_view.as_ref().unwrap(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.globals_bg, &[]);
            for e in &scene.entities {
                if !e.visible {
                    continue;
                }
                if let Some(g) = self.meshes.get(&e.id) {
                    pass.set_bind_group(1, &g.bind_group, &[]);
                    pass.set_vertex_buffer(0, g.vbuf.slice(..));
                    pass.set_index_buffer(g.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..g.n_indices, 0, 0..1);
                }
            }
        }

        // readback
        let unpadded_row = w * 4;
        let padded_row = unpadded_row.div_ceil(256) * 256;
        let buf_size = (padded_row * h) as u64;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: buf_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: self.color_tex.as_ref().unwrap(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::Wait);
        rx.recv().context("map channel")?.context("map_async")?;
        let data = slice.get_mapped_range();
        let mut rgba = vec![0u8; (unpadded_row * h) as usize];
        for y in 0..h as usize {
            let src = y * padded_row as usize;
            let dst = y * unpadded_row as usize;
            rgba[dst..dst + unpadded_row as usize]
                .copy_from_slice(&data[src..src + unpadded_row as usize]);
        }
        drop(data);
        staging.unmap();
        Ok(rgba)
    }

    /// Full pipeline: render → PNG encode → base64 string.
    pub fn screenshot_base64(&mut self, scene: &Scene, revision: u64, w: u32, h: u32) -> Result<String> {
        let rgba = self.render_rgba(scene, revision, w, h)?;
        Ok(B64.encode(encode_png(&rgba, w, h)?))
    }

    /// Render + PNG bytes (for tests / file output).
    pub fn screenshot_png(&mut self, scene: &Scene, revision: u64, w: u32, h: u32) -> Result<Vec<u8>> {
        let rgba = self.render_rgba(scene, revision, w, h)?;
        encode_png(&rgba, w, h)
    }
}

fn encode_png(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    image::ImageEncoder::write_image(enc, rgba, w, h, image::ExtendedColorType::Rgba8.into())
        .context("png encode")?;
    Ok(out)
}
