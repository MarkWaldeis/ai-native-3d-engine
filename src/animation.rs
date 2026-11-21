use crate::Engine;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use shared::{
    animation::{AnimationMachine, Animator},
    mcp_protocol::text_result,
    scene::PrimitiveKind,
};

impl Engine {
    pub(crate) fn animation_tool(&mut self, name: &str, args: &Value) -> Result<Value> {
        let entity = args["id"]
            .as_u64()
            .map(|id| {
                self.world
                    .entity(id)
                    .with_context(|| format!("entity {id} not found"))
            })
            .transpose()?;
        let path = if let Some(path) = args["path"].as_str() {
            ensure!(
                name == "animation_clips",
                "path is only accepted by animation_clips"
            );
            path
        } else {
            match &entity.context("supply an entity id or GLB path")?.mesh {
                PrimitiveKind::Gltf { path } => path,
                _ => anyhow::bail!("animator requires a gltf entity"),
            }
        };
        let root = self
            .assets_dir
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let path = engine_core::project_path(root, std::path::Path::new(path))?;
        let clips = engine_geo::animation_clips(&path)?;
        if name == "animation_clips" {
            return Ok(text_result(json!({"clips":clips}).to_string()));
        }
        let entity = entity.context("missing entity id")?;
        let machine: AnimationMachine = serde_json::from_value(
            args.get("machine")
                .cloned()
                .or_else(|| entity.animator.as_ref().map(|a| json!(a.machine)))
                .context("missing machine")?,
        )?;
        let mut animator: Animator = serde_json::from_value(
            json!({"clips":clips,"machine":machine,
            "speed":args.get("speed").cloned().unwrap_or_else(|| json!(entity.animator.as_ref().map_or(1.0,|a|a.speed))),
            "blend_time":args.get("blend_time").cloned().unwrap_or_else(||json!(entity.animator.as_ref().map_or(0.15,|a|a.blend_time)))}),
        )?;
        engine_runtime::animation::validate(&animator)?;
        engine_runtime::animation::reset(&mut animator);
        if name == "animator_validate" {
            return Ok(text_result(
                json!({"valid":true,"clips":animator.clips}).to_string(),
            ));
        }
        if let Some(expected) = args["expected_revision"].as_u64() {
            ensure!(
                expected == self.world.revision,
                "revision conflict: expected {expected}, world is at {}",
                self.world.revision
            );
        }
        let id = entity.id;
        let binding = entity.props.get("_prefab").cloned();
        let mut ids = vec![id];
        let mut scene = self.world.scene.clone();
        scene
            .entities
            .iter_mut()
            .find(|e| e.id == id)
            .unwrap()
            .animator = Some(animator.clone());
        if let Some(binding) = binding {
            if let (Some(root), Some(node)) =
                (binding["instance"].as_u64(), binding["node"].as_str())
            {
                self.check_entity_locks(&[root], args["agent"].as_str().unwrap_or("anonymous"))
                    .map_err(anyhow::Error::msg)?;
                let root_entity = scene
                    .entities
                    .iter_mut()
                    .find(|e| e.id == root)
                    .context("prefab root missing")?;
                let overrides = &mut root_entity.props["_prefab"]["overrides"];
                ensure!(overrides.is_object(), "invalid prefab overrides");
                ensure!(
                    overrides.get(node).is_none_or(|v| v.is_object()),
                    "invalid prefab node overrides"
                );
                overrides[node]["animator"] = json!(animator);
                ids.push(root);
            }
        }
        self.world.replace_authoring_scene(scene)?;
        ids.sort_unstable();
        ids.dedup();
        Ok(text_result(
            json!({"ids":ids,"revision":self.world.revision,"animator":animator}).to_string(),
        ))
    }
}
