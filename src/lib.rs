//! Engine facade: owns World + Renderer + AssetPipeline and executes MCP tool
//! calls. Lives in the root crate so it can coordinate across all sub-crates.

use asset_pipeline::AssetPipeline;
use engine_core::{Command, World};
use engine_mcp::ToolHandler;
use engine_render::Renderer;
use serde_json::{json, Value};
use shared::mcp_protocol::{image_result, text_result};
use shared::scene::{Camera, PrimitiveKind, Transform};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod swarm;
pub mod http;
mod blender;
mod runtime;
mod qa;
mod authoring;
mod batch;
mod security;
mod tasks;
pub mod web;
pub mod project;
mod project_state;
mod prefabs;
mod authoring_query;
mod asset_bundle;
mod animation;
mod navigation;

pub struct Engine {
    pub world: World,
    renderer: Option<Renderer>,
    pub pipeline: AssetPipeline,
    pub assets_dir: PathBuf,
    swarm: swarm::Swarm,
    pub blender: asset_pipeline::bridge::BlenderBridge,
    simulation: Option<engine_runtime::Simulation>,
    render_revision: u64,
    privileged_tools: bool,
    storage: Option<project_state::Storage>,
    coordination_depth: usize,
}

impl Engine {
    pub fn from_project(path: &Path) -> anyhow::Result<Self> {
        let root = path.canonicalize()?;
        let storage = project_state::Storage::open(&root)?;
        let (identity, port) = project_state::configuration(&root)?;
        let mut engine = Self::with_assets(engine_core::project_path(&root, &root.join("assets"))?);
        engine.blender = asset_pipeline::bridge::BlenderBridge::for_project(port, identity);
        if let Some((world,swarm)) = storage.restore()? {
            engine.world=world;
            engine.swarm=swarm;
        } else {
            engine.world.load_with_root(&root.join("scene.json"), &root)?;
            storage.save(&engine.world,&engine.swarm)?;
        }
        engine.storage=Some(storage);
        Ok(engine)
    }

    pub fn new() -> Self {
        let assets_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("assets");
        Self::with_assets(assets_dir)
    }

    fn with_assets(assets_dir: PathBuf) -> Self {
        Self {
            world: World::new(),
            renderer: None,
            pipeline: AssetPipeline::new(&assets_dir),
            assets_dir,
            swarm: swarm::Swarm::default(),
            blender: asset_pipeline::bridge::BlenderBridge::default(),
            simulation: None,
            render_revision: 0,
            privileged_tools: true,
            storage: None,
            coordination_depth: 0,
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
        other => return Err(format!("unknown primitive kind: {other}; valid kinds: box, plane, sphere, cylinder, wall_with_door, gltf{}", shared::suggestions::hint(other, ["box", "plane", "sphere", "cylinder", "wall_with_door", "gltf"]))),
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
    fn scene(&self) -> Result<Value, String> {
        serde_json::to_value(&self.world.scene).map_err(|error| error.to_string())
    }
    fn handle(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        self.durable_handle(name, args)
    }
}

impl Engine {
    fn handle_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let expected = expected_rev(args);
        let r = (|| -> anyhow::Result<Value> {
            match name {
                "animation_clips" | "animator_validate" | "animator_set" => self.animation_tool(name,args),
                "nav_bake" | "nav_query_path" | "nav_lint" => self.navigation_tool(name,args),
                "query_entities" | "get_scene_summary" => Ok(text_result(self.authoring_query(name,args)?.to_string())),
                "terrain_generate" | "terrain_import" | "terrain_info" | "terrain_sample" => engine_mcp::terrain::execute(name, args, self.assets_dir.parent().unwrap_or(Path::new("."))),
                "write_script" => self.write_script(args),
                "play" | "step" | "stop" | "get_sim_state" | "simulate" | "query" => self.runtime_tool(name, args),
                "blender_status" => {
                    Ok(text_result(match self.blender.request(json!({"command":"ping"})) {
                        Ok(info) => json!({"connected":true, "info":info}),
                        Err(error) => json!({"connected":false, "help":error.to_string()}),
                    }.to_string()))
                }
                "blender_start" => {
                    let executable = self.pipeline.blender_path().ok_or_else(|| anyhow::anyhow!("Blender not found; install Blender or add blender to PATH"))?;
                    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("blender/ai_engine_bridge.py");
                    Ok(text_result(self.blender.start(executable, &script)?.to_string()))
                }
                "blender_exec" => {
                    let code = args["code"].as_str().ok_or_else(|| anyhow::anyhow!("blender_exec requires string 'code'"))?;
                    Ok(text_result(self.blender.request(json!({"command":"exec", "code":code}))?.to_string()))
                }
                "blender_scene_info" => Ok(text_result(self.blender.request(json!({"command":"scene_info"}))?.to_string())),
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
                    if args.get("views").is_some() || args.get("overlay_ids").is_some() || args.get("overlay_navigation").is_some() { return self.qa_screenshot(args); }
                    let w = args.get("width").and_then(|v| v.as_u64()).unwrap_or(960) as u32;
                    let h = args.get("height").and_then(|v| v.as_u64()).unwrap_or(540) as u32;
                    let scene = self.simulation.as_ref().map(|s| s.scene()).unwrap_or_else(|| self.world.scene.clone());
                    let b64 = self.render_scene_base64(&scene, w, h)?;
                    Ok(image_result(b64))
                }
                "save_scene" => {
                    let p = args.get("path").and_then(|p| p.as_str()).unwrap_or("scene.json");
                    if args.get("bundle_assets").map(|v|v.as_bool().ok_or_else(||anyhow::anyhow!("bundle_assets must be boolean"))).transpose()?.unwrap_or(false) {
                        return Ok(text_result(authoring::save_bundle(&self.world.scene, self.assets_dir.parent().unwrap_or(Path::new(".")), Path::new(p))?.to_string()));
                    }
                    Ok(text_result(
                        self.world.apply(Command::SaveScene { path: p.into() })?.to_string(),
                    ))
                }
                "load_scene" => {
                    let p = args.get("path").and_then(|p| p.as_str()).unwrap_or("scene.json");
                    self.world.load_with_root(Path::new(p), self.assets_dir.parent().unwrap_or(Path::new(".")))?;
                    Ok(text_result(json!({"ok":true,"path":p,"revision":self.world.revision}).to_string()))
                }
                "apply_batch" => {
                    match args.get("mode").and_then(Value::as_str).unwrap_or("partial") {
                        "atomic" => return self.atomic_batch(args).map_err(anyhow::Error::msg),
                        "partial" => {},
                        _ => anyhow::bail!("batch mode must be partial or atomic"),
                    }
                    if args.get("mode").is_some_and(|v| !v.is_string()) { anyhow::bail!("batch mode must be partial or atomic"); }
                    let cmds = args
                        .get("commands")
                        .and_then(|c| c.as_array())
                        .ok_or_else(|| anyhow::Error::msg("missing array arg 'commands'"))?;
                    let mut results = Vec::with_capacity(cmds.len());
                    for c in cmds {
                        let tool = c.get("tool").and_then(|t| t.as_str()).unwrap_or("");
                        if tool == "apply_batch" {
                            anyhow::bail!("nested apply_batch is not allowed");
                        }
                        let mut sub_args = c.get("arguments").cloned().unwrap_or(json!({}));
                        if !sub_args.is_object() { anyhow::bail!("batch command arguments must be a JSON object"); }
                        sub_args["agent"] = args.get("agent").cloned().unwrap_or(json!("anonymous"));
                        match self.handle(tool, &sub_args) {
                            Ok(v) => results.push(v),
                            Err(e) => {
                                return Ok(json!({
                                    "content": [{"type": "text", "text": format!("batch stopped at command {} ({tool}): {e}", results.len())}],
                                    "isError": true,
                                    "applied": results.len(), "mode":"partial",
                                }));
                            }
                        }
                    }
                    Ok(json!({"mode":"partial","content": [{"type": "text", "text": serde_json::to_string(&results)?}]}))
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
                "validate_scene" => {
                    let scene = self.simulation.as_ref().map(|s| s.scene()).unwrap_or_else(|| self.world.scene.clone());
                    let project = self.assets_dir.parent().unwrap_or_else(|| Path::new("."));
                    Ok(text_result(qa::validate_scene(&scene, project).to_string()))
                }
                "export_web" => {
                    let out = args["out_dir"].as_str().filter(|s|!s.is_empty()).ok_or_else(||anyhow::anyhow!("export_web requires nonempty out_dir"))?;
                    let scene = match args.get("scene") {
                        None => self.world.scene.clone(),
                        Some(Value::String(path)) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
                        Some(scene) => serde_json::from_value(scene.clone())?,
                    };
                    let root = self.assets_dir.parent().unwrap_or_else(||Path::new("."));
                    Ok(text_result(web::export_web(&scene, root, Path::new(out))?.to_string()))
                }
                other => anyhow::bail!("unknown tool: {other}{}", shared::suggestions::hint(other, shared::mcp_protocol::builtin_tools().iter().map(|t| t.name))),
            }
        })();
        r.map_err(|e| e.to_string())
    }
}
