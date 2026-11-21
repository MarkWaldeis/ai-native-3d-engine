use crate::Engine;
use engine_core::{project_path, validate_scene_paths};
use serde_json::{json, Value};
use std::path::Path;

impl Engine {
    pub(crate) fn secure_arguments(&self, name: &str, args: &Value) -> Result<Value, String> {
        let result = (|| -> anyhow::Result<Value> {
            if !self.privileged_tools && matches!(name, "blender_exec" | "run_blender_script") {
                anyhow::bail!(
                    "privileged tool disabled in remote mode; requires --allow-privileged-tools"
                );
            }
            let root = self.assets_dir.parent().unwrap_or(Path::new("."));
            let mut args = args.clone();
            let key = match name {
                "save_scene" | "load_scene" => Some("path"),
                "export_web" => Some("out_dir"),
                "run_blender_script" => Some("script"),
                _ => None,
            };
            if let Some(key) = key {
                let path = match args.get(key) {
                    Some(value) => value
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("{key} must be a nonempty path string"))?,
                    None if key == "path" => "scene.json",
                    None => anyhow::bail!("missing path argument {key}"),
                };
                args[key] = json!(project_path(root, Path::new(path))?);
            }
            if name == "write_script" {
                if let Some(path) = args["path"].as_str() {
                    project_path(root, Path::new(path))?;
                }
            }
            if name == "spawn_primitive" && args["kind"] == "gltf" {
                if let Some(path) = args["path"].as_str() {
                    args["path"] = json!(project_path(root, &self.assets_dir.join(path))?);
                }
            }
            if matches!(name, "export_web" | "net_simulate") {
                if let Some(Value::String(path)) = args.get("scene") {
                    let path = project_path(root, Path::new(path))?;
                    args["scene"] = json!(path);
                    let scene = serde_json::from_str(&std::fs::read_to_string(path)?)?;
                    validate_scene_paths(&scene, root)?;
                } else if let Some(scene) = args.get("scene") {
                    validate_scene_paths(&serde_json::from_value(scene.clone())?, root)?;
                }
            }
            if matches!(
                name,
                "save_scene"
                    | "play"
                    | "simulate"
                    | "net_simulate"
                    | "get_screenshot"
                    | "validate_scene"
                    | "export_web"
            ) {
                validate_scene_paths(&self.world.scene, root)?;
            }
            if matches!(name, "blender_sync" | "list_assets" | "run_blender_script") {
                project_path(root, &self.assets_dir)?;
            }
            if name == "blender_sync" {
                project_path(root, &self.assets_dir.join("blender"))?;
            }
            Ok(args)
        })();
        result.map_err(|e| e.to_string())
    }
}
