use crate::Engine;
use anyhow::{ensure, Result};
use engine_net::simulate::{run, Options};
use engine_net::SimSource;
use serde_json::Value;
use shared::{mcp_protocol::text_result, scene::Scene};
use std::path::Path;

impl Engine {
    pub(crate) fn net_simulate(&self, args: &Value) -> Result<Value> {
        ensure!(
            args.get("clients").is_some() && args.get("ticks").is_some(),
            "net_simulate requires clients and ticks"
        );
        let mut config = args.clone();
        for key in ["scene", "agent"] {
            config.as_object_mut().unwrap().remove(key);
        }
        let options: Options = serde_json::from_value(config)?;
        options.validate()?;
        let root = self
            .assets_dir
            .parent()
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let scene: Scene = match args.get("scene") {
            Some(Value::String(path)) => serde_json::from_str(&std::fs::read_to_string(
                engine_core::project_path(&root, Path::new(path))?,
            )?)?,
            Some(value) => serde_json::from_value(value.clone())?,
            None => self.world.scene.clone(),
        };
        engine_core::validate_scene_paths(&scene, &root)?;
        Ok(text_result(serde_json::to_string(&run(
            scene,
            SimSource::ProjectRoot(root),
            options,
        )?)?))
    }
}
