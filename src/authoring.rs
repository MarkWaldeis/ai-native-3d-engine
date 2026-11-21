use crate::Engine;
use anyhow::{bail, Context, Result};
use engine_core::Command;
use serde_json::{json, Value};
use shared::{
    components::Script,
    scene::{PrimitiveKind, Scene},
};
use std::path::{Component, Path};

impl Engine {
    pub(crate) fn write_script(&mut self, args: &Value) -> Result<Value> {
        let path = args["path"]
            .as_str()
            .context("write_script requires path")?;
        let source = args["source"]
            .as_str()
            .context("write_script requires source")?;
        let ids: Vec<u64> = serde_json::from_value(args["ids"].clone())?;
        if ids.is_empty() {
            bail!("write_script ids must not be empty");
        }
        for id in &ids {
            if self.world.entity(*id).is_none() {
                bail!("no entity {id}");
            }
        }
        if let Some(expected) = crate::expected_rev(args) {
            if expected != self.world.revision {
                bail!(
                    "revision conflict: expected {expected}, world is at {}",
                    self.world.revision
                );
            }
        }
        let root = self
            .assets_dir
            .parent()
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let requested = Path::new(path);
        if requested
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
            || requested.extension().and_then(|s| s.to_str()) != Some("rhai")
        {
            bail!("script path must be a relative .rhai file inside the project");
        }
        let file = root.join(requested);
        if !file
            .parent()
            .context("script parent missing")?
            .canonicalize()?
            .starts_with(&root)
            || file.is_symlink()
        {
            bail!("script path escapes project root");
        }
        engine_runtime::Simulation::check_script(
            &Script::Inline {
                source: source.into(),
            },
            path,
            &root,
        )?;
        let staging = file.with_extension("rhai.pending");
        if staging.is_symlink() {
            bail!("script staging path must not be a symlink");
        }
        std::fs::write(&staging, source)?;
        std::fs::rename(staging, &file)?;
        let result = self.apply_cmd(
            Command::SetScripts {
                ids,
                script: Script::Path(path.replace('\\', "/")),
            },
            crate::expected_rev(args),
        )?;
        Ok(shared::mcp_protocol::text_result(result.to_string()))
    }
}

pub(crate) fn save_bundle(scene: &Scene, root: &Path, path: &Path) -> Result<Value> {
    let root = root.canonicalize()?;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let parent = path
        .parent()
        .context("scene path has no parent")?
        .canonicalize()?;
    if path.is_symlink() {
        bail!("bundled scene path must not be a symlink");
    }
    if !parent.starts_with(&root) {
        bail!("bundled scene must be inside the project");
    }
    let assets = parent.join("assets");
    std::fs::create_dir_all(&assets)?;
    let assets = assets.canonicalize()?;
    if !assets.starts_with(&root) {
        bail!("asset directory escapes project root");
    }
    let mut bundled = scene.clone();
    for entity in &mut bundled.entities {
        if let PrimitiveKind::Gltf { path } = &mut entity.mesh {
            let from = root.join(&*path);
            let to = assets.join(format!("{}.glb", entity.id));
            if to.is_symlink() {
                bail!("bundled asset path must not be a symlink");
            }
            if from.canonicalize()? != to.canonicalize().unwrap_or(to.clone()) {
                std::fs::copy(from, &to)?;
            }
            *path = to.strip_prefix(&root)?.to_string_lossy().replace('\\', "/");
        }
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&bundled)?)?;
    Ok(json!({"ok":true,"path":path,"bundled_assets":true}))
}
