use crate::Engine;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use shared::{mcp_protocol::text_result, navigation::NavParams, scene::Scene};
use std::{io::Write, path::Path};

pub(crate) fn lint(scene: &Scene, start: Option<[f32; 3]>) -> Value {
    let Some(mesh) = &scene.navmesh else {
        return json!({"available":false,"findings":[]});
    };
    let floor = |mut p: [f32; 3]| {
        let cell = [
            (p[0] / mesh.params.cell_size).floor() as i32,
            (p[2] / mesh.params.cell_size).floor() as i32,
        ];
        if let Some(y) = mesh
            .tiles
            .iter()
            .flat_map(|t| &t.polygons)
            .filter(|n| {
                n.cell == cell
                    && n.y <= p[1] + mesh.params.step
                    && p[1] - n.y <= 2.0 * mesh.params.height
            })
            .map(|n| n.y)
            .max_by(f32::total_cmp)
        {
            p[1] = y;
        }
        p
    };
    let start = start.or_else(|| {
        scene
            .entities
            .iter()
            .find(|e| e.tags.iter().any(|t| t == "player"))
            .map(|e| e.transform.position)
    });
    let Some(start) = start else {
        return json!({"available":true,"findings":[],"warning":"no player/start for reachability lint"});
    };
    let start = floor(start);
    let findings:Vec<_>=scene.entities.iter().filter(|e|e.tags.iter().any(|t|["goal","collectible","crystal","coin","lantern"].contains(&t.as_str())))
        .filter_map(|e|engine_runtime::navigation::query(mesh,start,floor(e.transform.position)).err().map(|error|json!({"entity":e.id,"code":"nav_unreachable","severity":"warning","message":format!("{}: {error}",e.name)}))).collect();
    json!({"available":true,"findings":findings})
}

impl Engine {
    pub(crate) fn navigation_tool(&mut self, name: &str, args: &Value) -> Result<Value> {
        if name == "nav_lint" {
            let start = args
                .get("start")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?;
            let mut scene = self.world.scene.clone();
            if let Some(mesh) = &scene.navmesh {
                let root = self.assets_dir.parent().unwrap_or(Path::new("."));
                scene.navmesh =
                    Some(engine_runtime::navigation::bake(&scene, root, mesh.params.clone())?.0);
            }
            return Ok(text_result(lint(&scene, start).to_string()));
        }
        if name == "nav_query_path" {
            let mesh = self
                .world
                .scene
                .navmesh
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("scene has no navmesh; call nav_bake"))?;
            let root = self.assets_dir.parent().unwrap_or(Path::new("."));
            let (fresh, _) =
                engine_runtime::navigation::bake(&self.world.scene, root, mesh.params.clone())?;
            let mesh = &fresh;
            let start = serde_json::from_value(args["start"].clone())?;
            let goal = serde_json::from_value(args["goal"].clone())?;
            let path = engine_runtime::navigation::query(mesh, start, goal)?;
            return Ok(text_result(
                json!({"path":path,"fingerprint":mesh.fingerprint}).to_string(),
            ));
        }
        if let Some(revision) = args["expected_revision"].as_u64() {
            ensure!(revision == self.world.revision, "revision conflict");
        }
        let mut params: NavParams = args
            .get("params")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or_else(|| {
                self.world
                    .scene
                    .navmesh
                    .as_ref()
                    .map_or_else(NavParams::default, |m| m.params.clone())
            });
        if self.world.scene.navmesh.is_none()
            && args
                .get("params")
                .is_none_or(|v| v.get("tile_size").is_none())
        {
            if let Some(world) = self
                .world
                .scene
                .entities
                .iter()
                .find(|e| e.tags.iter().any(|t| t == "player"))
                .and_then(|e| e.props.get("ai_world"))
            {
                let world = if let Some(s) = world.as_str() {
                    serde_json::from_str(s)?
                } else {
                    world.clone()
                };
                if let Some(size) = world["chunk_size"].as_f64() {
                    params.tile_size = size as f32;
                }
            }
        }
        let root = self.assets_dir.parent().unwrap_or(Path::new("."));
        let (mesh, stats) = engine_runtime::navigation::bake(&self.world.scene, root, params)?;
        let asset = format!("assets/navigation-{}.nav.json", mesh.fingerprint);
        let path = engine_core::project_path(root, Path::new(&asset))?;
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec(&mesh)?;
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temp, &path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result?;
        let mut scene = self.world.scene.clone();
        scene.navmesh = Some(mesh);
        let ids: Vec<_> = scene.entities.iter().map(|e| e.id).collect();
        if scene != self.world.scene {
            self.world.replace_authoring_scene(scene)?;
        }
        Ok(text_result(json!({"asset":asset,"stats":stats,"revision":self.world.revision,"ids":ids,"lint":lint(&self.world.scene,None)}).to_string()))
    }
}
