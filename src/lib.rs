//! Engine facade: owns World + Renderer + AssetPipeline and executes MCP tool
//! calls. Lives in the root crate so it can coordinate across all sub-crates.

use asset_pipeline::AssetPipeline;
use engine_core::{Command, World};
use engine_mcp::ToolHandler;
use engine_render::Renderer;
use serde_json::Value;
use shared::mcp_protocol::{image_result, text_result};
use shared::scene::{Camera, PrimitiveKind, Transform};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Engine {
    pub world: World,
    renderer: Option<Renderer>,
    pub pipeline: AssetPipeline,
    pub assets_dir: PathBuf,
}

impl Engine {
    pub fn new() -> Self {
        let assets_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("assets");
        Self {
            world: World::new(),
            renderer: None,
            pipeline: AssetPipeline::new(&assets_dir),
            assets_dir,
        }
    }

    /// Renderer is created lazily — the MCP server works (spawn/save/load)
    /// even on a machine without a usable GPU.
    fn renderer(&mut self) -> Result<&mut Renderer, String> {
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new().map_err(|e| format!("renderer init: {e}"))?);
        }
        Ok(self.renderer.as_mut().unwrap())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

fn arr3(v: Option<&Value>, default: [f32; 3]) -> [f32; 3] {
    match v.and_then(|v| v.as_array()) {
        Some(a) if a.len() == 3 => [
            a[0].as_f64().unwrap_or(0.0) as f32,
            a[1].as_f64().unwrap_or(0.0) as f32,
            a[2].as_f64().unwrap_or(0.0) as f32,
        ],
        _ => default,
    }
}

fn arr4(v: Option<&Value>, default: [f32; 4]) -> [f32; 4] {
    match v.and_then(|v| v.as_array()) {
        Some(a) if a.len() == 4 => [
            a[0].as_f64().unwrap_or(0.0) as f32,
            a[1].as_f64().unwrap_or(0.0) as f32,
            a[2].as_f64().unwrap_or(0.0) as f32,
            a[3].as_f64().unwrap_or(1.0) as f32,
        ],
        _ => default,
    }
}

fn num(v: Option<&Value>, default: f32) -> f32 {
    v.and_then(|v| v.as_f64()).unwrap_or(default as f64) as f32
}

fn primitive_kind(args: &Value, assets_dir: &Path) -> Result<PrimitiveKind, String> {
    let kind = args.get("kind").and_then(|k| k.as_str()).unwrap_or("box");
    let k = match kind {
        "box" => {
            let size = match args.get("size") {
                Some(v) if v.is_number() => {
                    let s = num(Some(v), 1.0);
                    [s, s, s]
                }
                Some(v) => arr3(Some(v), [1.0; 3]),
                None => [1.0; 3],
            };
            PrimitiveKind::Box { size }
        }
        "plane" => PrimitiveKind::Plane {
            size: num(args.get("size"), 10.0),
        },
        "sphere" => PrimitiveKind::Sphere {
            radius: num(args.get("radius"), 0.5),
            segments: args.get("segments").and_then(|s| s.as_u64()).unwrap_or(24) as u32,
        },
        "cylinder" => PrimitiveKind::Cylinder {
            radius: num(args.get("radius"), 0.5),
            height: num(args.get("height"), 1.0),
            segments: args.get("segments").and_then(|s| s.as_u64()).unwrap_or(24) as u32,
        },
        "wall_with_door" => PrimitiveKind::WallWithDoor {
            width: num(args.get("width"), 4.0),
            height: num(args.get("height"), 3.0),
            door_width: num(args.get("door_width"), 1.0),
            door_height: num(args.get("door_height"), 2.1),
        },
        "gltf" => {
            let p = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "gltf kind needs string arg 'path'".to_string())?;
            let p = Path::new(p);
            // Relative paths resolve against assets/ so agents can write
            // just the file name.
            let full = if p.is_absolute() { p.to_path_buf() } else { assets_dir.join(p) };
            if !full.exists() {
                return Err(format!("asset not found: {}", full.display()));
            }
            PrimitiveKind::Gltf {
                path: full.to_string_lossy().replace('\\', "/"),
            }
        }
        other => return Err(format!("unknown primitive kind: {other}")),
    };
    Ok(k)
}

fn id_of(args: &Value) -> Result<u64, String> {
    args.get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing integer arg 'id'".to_string())
}

fn expected_rev(args: &Value) -> Option<u64> {
    args.get("expected_revision").and_then(|v| v.as_u64())
}

impl Engine {
    /// Route a mutation through the optimistic-concurrency gate when the
    /// caller supplied `expected_revision`, otherwise apply directly.
    fn apply_cmd(&mut self, cmd: Command, expected: Option<u64>) -> anyhow::Result<Value> {
        match expected {
            Some(rev) => self.world.apply_expected(rev, cmd),
            None => self.world.apply(cmd),
        }
    }
}

impl ToolHandler for Engine {
    fn handle(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let expected = expected_rev(args);
        let r = (|| -> anyhow::Result<Value> {
            match name {
                "get_scene_state" => Ok(text_result(
                    serde_json::to_string_pretty(&self.world.scene)?,
                )),
                "spawn_primitive" => {
                    let kind = primitive_kind(args, &self.assets_dir).map_err(anyhow::Error::msg)?;
                    let t = Transform {
                        position: arr3(args.get("position"), [0.0; 3]),
                        rotation_deg: arr3(args.get("rotation_deg"), [0.0; 3]),
                        scale: arr3(args.get("scale"), [1.0; 3]),
                    };
                    let color = arr4(args.get("color"), [0.8, 0.8, 0.8, 1.0]);
                    let name = args
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("entity")
                        .to_string();
                    let v = self.apply_cmd(Command::SpawnPrimitive {
                        name,
                        kind,
                        transform: t,
                        color,
                    }, expected)?;
                    Ok(text_result(v.to_string()))
                }
                "set_transform" => {
                    let id = id_of(args).map_err(anyhow::Error::msg)?;
                    let cur = self
                        .world
                        .entity(id)
                        .map(|e| e.transform)
                        .unwrap_or_default();
                    let t = Transform {
                        position: arr3(args.get("position"), cur.position),
                        rotation_deg: arr3(args.get("rotation_deg"), cur.rotation_deg),
                        scale: arr3(args.get("scale"), cur.scale),
                    };
                    Ok(text_result(
                        self.apply_cmd(Command::SetTransform { id, transform: t }, expected)?.to_string(),
                    ))
                }
                "set_color" => {
                    let id = id_of(args).map_err(anyhow::Error::msg)?;
                    let color = arr4(args.get("color"), [0.8; 4]);
                    Ok(text_result(
                        self.apply_cmd(Command::SetColor { id, color }, expected)?.to_string(),
                    ))
                }
                "remove_entity" => {
                    let id = id_of(args).map_err(anyhow::Error::msg)?;
                    Ok(text_result(self.apply_cmd(Command::Remove { id }, expected)?.to_string()))
                }
                "set_camera" => {
                    let c = self.world.scene.camera;
                    let cam = Camera {
                        position: arr3(args.get("position"), c.position),
                        target: arr3(args.get("target"), c.target),
                        fov_deg: num(args.get("fov_deg"), c.fov_deg),
                    };
                    Ok(text_result(
                        self.apply_cmd(Command::SetCamera { camera: cam }, expected)?.to_string(),
                    ))
                }
                "get_screenshot" => {
                    let w = args.get("width").and_then(|v| v.as_u64()).unwrap_or(960) as u32;
                    let h = args.get("height").and_then(|v| v.as_u64()).unwrap_or(540) as u32;
                    let rev = self.world.revision;
                    let scene = self.world.scene.clone();
                    let b64 = self
                        .renderer()
                        .map_err(anyhow::Error::msg)?
                        .screenshot_base64(&scene, rev, w, h)?;
                    Ok(image_result(b64))
                }
                "save_scene" => {
                    let p = args.get("path").and_then(|p| p.as_str()).unwrap_or("scene.json");
                    Ok(text_result(
                        self.world.apply(Command::SaveScene { path: p.into() })?.to_string(),
                    ))
                }
                "load_scene" => {
                    let p = args.get("path").and_then(|p| p.as_str()).unwrap_or("scene.json");
                    Ok(text_result(
                        self.apply_cmd(Command::LoadScene { path: p.into() }, expected)?.to_string(),
                    ))
                }
                "get_revision" => Ok(text_result(self.world.revision.to_string())),
                "rollback" => {
                    let rev = args
                        .get("revision")
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| anyhow::Error::msg("missing integer arg 'revision'"))?;
                    Ok(text_result(
                        self.apply_cmd(Command::Rollback { revision: rev }, expected)?.to_string(),
                    ))
                }
                "list_assets" => Ok(text_result(self.pipeline.list_assets().join("\n"))),
                "run_blender_script" => {
                    let s = args.get("script").and_then(|v| v.as_str()).unwrap_or("");
                    let out = self
                        .pipeline
                        .run_blender_script(Path::new(s), Duration::from_secs(120))?;
                    Ok(text_result(out))
                }
                other => anyhow::bail!("unknown tool: {other}"),
            }
        })();
        r.map_err(|e| e.to_string())
    }
}
