use crate::Engine;
use anyhow::{ensure, Context, Result};
use engine_net::simulate::{run, Options};
use serde_json::Value;
use shared::{
    components::Script,
    mcp_protocol::text_result,
    scene::{PrimitiveKind, Scene},
};
use std::{collections::BTreeMap, path::Path};

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
        let mut scene: Scene = match args.get("scene") {
            Some(Value::String(path)) => serde_json::from_str(&std::fs::read_to_string(
                engine_core::project_path(&root, Path::new(path))?,
            )?)?,
            Some(value) => serde_json::from_value(value.clone())?,
            None => self.world.scene.clone(),
        };
        engine_core::validate_scene_paths(&scene, &root)?;
        let mut sources = BTreeMap::new();
        for entity in &mut scene.entities {
            if let Some(Script::Path(path) | Script::File { path }) = &entity.script {
                if !sources.contains_key(path) {
                    let candidate = [root.join(path), root.join("assets").join(path)]
                        .into_iter()
                        .find(|p| p.is_file())
                        .with_context(|| {
                            format!("script {path}: file not found in project or assets")
                        })?;
                    let candidate = engine_core::project_path(&root, &candidate)?;
                    sources.insert(path.clone(), std::fs::read_to_string(candidate)?);
                }
            }
            if let PrimitiveKind::Gltf { path } = &mut entity.mesh {
                *path = engine_core::project_path(&root, Path::new(path))?
                    .to_string_lossy()
                    .into_owned();
            }
        }
        Ok(text_result(serde_json::to_string(&run(
            scene, sources, options,
        )?)?))
    }
}
