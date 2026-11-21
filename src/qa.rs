use crate::Engine;
use anyhow::{bail, Result};
use engine_render::inspection::{self, Bounds};
use serde_json::{json, Value};
use shared::{
    components::BodyType,
    mcp_protocol::image_result,
    scene::{Entity, PrimitiveKind, Scene},
};
use std::{collections::BTreeMap, path::Path};

impl Engine {
    pub(crate) fn qa_screenshot(&mut self, args: &Value) -> Result<Value> {
        let views: Option<Vec<String>> = args
            .get("views")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?;
        if views.as_ref().is_some_and(|v| v.is_empty() || v.len() > 7) {
            bail!("views must contain 1..=7 view names");
        }
        if let Some(views) = &views {
            for view in views {
                if !["current", "front", "back", "left", "right", "top", "iso"]
                    .contains(&view.as_str())
                {
                    bail!("unknown view '{view}'; valid views: current, front, back, left, right, top, iso");
                }
            }
        }
        let overlay = args
            .get("overlay_ids")
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| anyhow::anyhow!("overlay_ids must be boolean"))
            })
            .transpose()?
            .unwrap_or(false);
        let dimension = |key: &str, default: u64| -> Result<u32> {
            Ok(args
                .get(key)
                .map(|v| {
                    v.as_u64()
                        .filter(|n| *n <= 2048)
                        .ok_or_else(|| anyhow::anyhow!("{key} must be an integer <=2048"))
                })
                .transpose()?
                .unwrap_or(default) as u32)
        };
        let w = dimension("width", if views.is_some() { 480 } else { 960 })?;
        let h = dimension("height", if views.is_some() { 320 } else { 540 })?;
        let scene = self
            .simulation
            .as_ref()
            .map(|s| s.scene())
            .unwrap_or_else(|| self.world.scene.clone());
        if views.is_none() && !overlay {
            return Ok(image_result(self.render_scene_base64(&scene, w, h)?));
        }
        self.render_revision = self.render_revision.wrapping_add(1);
        let revision = self.render_revision;
        let png = self.renderer().map_err(anyhow::Error::msg)?.screenshot_qa(
            &scene,
            revision,
            w,
            h,
            views.as_deref(),
            overlay,
        )?;
        Ok(image_result(png))
    }
}

fn exempt(entity: &Entity) -> bool {
    entity
        .tags
        .iter()
        .any(|t| t == "floating" || t == "trigger")
        || entity.props["ai_floating"] == true
        || entity.props["ai_trigger"] == true
        || (entity.script.is_some() && entity.collider.is_none())
}
fn solid(entity: &Entity) -> bool {
    entity.visible && !exempt(entity) && entity.transform.scale.iter().all(|v| v.is_finite() && *v>0.0)
}

pub(crate) fn validate_scene(scene: &Scene, root: &Path) -> Value {
    let mut findings = vec![];
    let mut names = BTreeMap::new();
    let mut bounds = vec![];
    let mut add = |severity: &str, entity: u64, code: &str, message: String, fix: Value| {
        findings.push(json!({"severity":severity,"entity":entity,"code":code,"message":message,"suggested_fix":fix}))
    };
    for entity in &scene.entities {
        if entity.name.trim().is_empty() {
            add(
                "warning",
                entity.id,
                "empty_name",
                "Entity name is empty; choose a unique descriptive name in scene JSON or Blender."
                    .into(),
                Value::Null,
            );
        } else if let Some(previous) = names.insert(entity.name.trim(), entity.id) {
            add("warning",entity.id,"duplicate_name",format!("Name '{}' is also used by entity {previous}; rename it in scene JSON or Blender.",entity.name),Value::Null);
        }
        if entity
            .transform
            .scale
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            add(
                "error",
                entity.id,
                "invalid_scale",
                "Scale must be finite and strictly positive on every axis.".into(),
                json!({"tool":"set_transform","arguments":{"id":entity.id,"scale":[1,1,1]}}),
            );
        }
        if entity
            .rigidbody
            .as_ref()
            .is_some_and(|b| b.kind == BodyType::Dynamic)
            && entity.collider.is_none()
        {
            add("warning",entity.id,"dynamic_without_collider","Dynamic body has no collider; add a collider in scene JSON or Blender ai_collider.".into(),Value::Null);
        }
        if let Some(script) = &entity.script {
            if let Err(error) = engine_runtime::Simulation::check_script(script, &entity.name, root)
            {
                add(
                    "error",
                    entity.id,
                    "script_compile",
                    error.to_string(),
                    Value::Null,
                );
            }
        }
        if let PrimitiveKind::Gltf { path } = &entity.mesh {
            if !Path::new(path).is_file() {
                add(
                    "error",
                    entity.id,
                    "missing_asset",
                    format!("Asset file does not exist: {path}"),
                    json!({"tool":"list_assets","arguments":{}}),
                );
                continue;
            }
        }
        match inspection::bounds(entity) {
            Ok(b) => {
                if entity.visible && inspection::outside_frustum(b, &scene.camera, 16.0 / 9.0) {
                    add("info",entity.id,"outside_camera","Entity bounds are outside the authoring camera frustum (16:9). Reframe the camera.".into(),Value::Null);
                }
                bounds.push((entity, b));
            }
            Err(error) => add(
                "error",
                entity.id,
                "invalid_bounds",
                error.to_string(),
                Value::Null,
            ),
        }
    }
    for (entity, b) in &bounds {
        if !solid(entity)
            || entity
                .rigidbody
                .as_ref()
                .is_some_and(|body| body.kind == BodyType::Dynamic)
        {
            continue;
        }
        let support = bounds
            .iter()
            .filter(|(other, s)| {
                other.id != entity.id
                    && solid(other)
                    && s.min[1] < b.min[1]
                    && s.max[1] <= b.min[1] + 0.1
                    && (0..3)
                        .filter(|i| *i != 1)
                        .all(|i| s.min[i] < b.max[i] - 0.01 && s.max[i] > b.min[i] + 0.01)
            })
            .map(|(_, s)| s.max[1])
            .max_by(f32::total_cmp);
        if let Some(top) = support {
            let gap = b.min[1] - top;
            if gap > 0.1_f32.max((b.max[1] - b.min[1]) * 0.05) {
                let mut position = entity.transform.position;
                position[1] -= gap;
                add("warning",entity.id,"floating",format!("Bottom is {gap:.3} units above the closest support. Tag floating or set ai_floating:true if intentional."),json!({"tool":"set_transform","arguments":{"id":entity.id,"position":position}}));
            }
        }
    }
    for (i, (a, ba)) in bounds.iter().enumerate() {
        if !solid(a) {
            continue;
        }
        for (b, bb) in &bounds[i + 1..] {
            if !solid(b) {
                continue;
            }
            let depth: [f32; 3] = std::array::from_fn(|axis| {
                ba.max[axis].min(bb.max[axis]) - ba.min[axis].max(bb.min[axis])
            });
            let volume = |b: &Bounds| {
                (0..3)
                    .map(|axis| (b.max[axis] - b.min[axis]).max(0.0))
                    .product::<f32>()
            };
            if depth.iter().all(|d| *d > 0.05)
                && depth.iter().product::<f32>() > volume(ba).min(volume(bb)) * 0.3
            {
                let axis = (0..3)
                    .min_by(|i, j| depth[*i].total_cmp(&depth[*j]))
                    .unwrap();
                let mut position = b.transform.position;
                position[axis] += if bb.center()[axis] >= ba.center()[axis] {
                    depth[axis] + 0.02
                } else {
                    -depth[axis] - 0.02
                };
                add("warning",b.id,"heavy_overlap",format!("Bounds heavily interpenetrate entity {} ({}); conservative world-AABB check, review rotated/concave geometry.",a.id,a.name),json!({"tool":"set_transform","arguments":{"id":b.id,"position":position}}));
            }
        }
    }
    let combined = bounds
        .iter()
        .filter(|(e, _)| e.visible)
        .map(|(_, b)| *b)
        .reduce(Bounds::union);
    drop(add);
    if let Some(bounds) = combined {
        if let Ok(camera) = inspection::framed_camera(bounds, scene.camera, "current", 16.0 / 9.0) {
            for finding in &mut findings {
                if finding["code"] == "outside_camera" {
                    finding["suggested_fix"] = json!({"tool":"set_camera","arguments":{"position":camera.position,"target":camera.target,"fov_deg":camera.fov_deg}});
                }
            }
        }
    }
    json!(findings)
}
