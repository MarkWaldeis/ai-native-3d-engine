use engine_runtime::{Input, Simulation};
use serde_json::json;
use shared::scene::Scene;
use std::collections::{BTreeMap, BTreeSet};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Default)]
pub struct WebGame {
    simulation: Option<Simulation>,
    known: BTreeSet<u64>,
    transforms: BTreeMap<u64, shared::scene::Transform>,
    terrains: BTreeMap<String, engine_terrain::web::StreamedTerrain>,
}

#[wasm_bindgen]
impl WebGame {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_terrain(&mut self, path: &str, manifest_json: &str, coarse_bytes: &[u8]) -> Result<(), JsValue> {
        let terrain = serde_json::from_str::<engine_terrain::web::WebManifest>(manifest_json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let terrain = engine_terrain::web::StreamedTerrain::from_manifest(&terrain, coarse_bytes)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.terrains.insert(path.to_owned(), terrain);
        Ok(())
    }

    pub fn add_terrain_tile(&mut self, path: &str, x: u32, z: u32, bytes: &[u8]) -> Result<(), JsValue> {
        let terrain = self.terrains.get_mut(path).ok_or_else(|| JsValue::from_str("unknown terrain"))?;
        terrain.insert_tile(x,z,bytes).map_err(|e|JsValue::from_str(&e.to_string()))?;
        if let Some(sim) = &mut self.simulation {
            sim.insert_terrain_tile(path,x,z,bytes).map_err(|e|JsValue::from_str(&e.to_string()))?;
        }
        Ok(())
    }

    pub fn terrain_tiles_wanted(&self, margin: f64) -> String {
        let wanted: Vec<_> = self.simulation.as_ref().map(|s|s.terrain_tiles_wanted(margin)).unwrap_or_default().into_iter().map(|(path,x,z)|json!({"path":path,"x":x,"z":z})).collect();
        serde_json::to_string(&wanted).unwrap()
    }

    pub fn terrain_height(&self, x: f64, z: f64) -> f64 {
        self.simulation.as_ref().map_or(0.0,|s|s.terrain_height(x,z))
    }

    pub fn load(&mut self, scene_json: &str, scripts_json: &str) -> Result<String, JsValue> {
        self.load_inner(scene_json, scripts_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn tick(&mut self, inputs_json: &str) -> Result<String, JsValue> {
        self.tick_inner(inputs_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn set_active_region(&mut self, x: f32, z: f32, radius: f32, hysteresis: f32) -> Result<(), JsValue> {
        self.simulation.as_mut().ok_or_else(|| JsValue::from_str("load a scene before setting a region"))?
            .set_active_region(x,z,radius,hysteresis).map_err(|error| JsValue::from_str(&error.to_string()))
    }

    pub fn clear_active_region(&mut self) {
        if let Some(simulation) = self.simulation.as_mut() { simulation.clear_active_region(); }
    }
}

impl WebGame {
    fn load_inner(&mut self, scene_json: &str, scripts_json: &str) -> Result<String, String> {
        let scene: Scene = serde_json::from_str(scene_json).map_err(|e| e.to_string())?;
        let scripts: BTreeMap<String, String> =
            serde_json::from_str(scripts_json).map_err(|e| e.to_string())?;
        let simulation = Simulation::from_sources_with_terrain(scene, scripts, self.terrains.clone()).map_err(|e| e.to_string())?;
        self.simulation = Some(simulation);
        self.known.clear();
        self.transforms.clear();
        self.snapshot()
    }

    fn tick_inner(&mut self, inputs_json: &str) -> Result<String, String> {
        let values: BTreeMap<String, f64> =
            serde_json::from_str(inputs_json).map_err(|e| e.to_string())?;
        let simulation = self.simulation.as_mut().ok_or("load a scene before tick")?;
        let inputs: Vec<_> = values
            .into_iter()
            .map(|(action, value)| Input {
                tick: simulation.tick,
                action,
                value,
            })
            .collect();
        simulation.step(1, &inputs).map_err(|e| e.to_string())?;
        self.snapshot()
    }

    fn snapshot(&mut self) -> Result<String, String> {
        let simulation = self.simulation.as_ref().ok_or("no simulation")?;
        let mut current = BTreeSet::new();
        let mut spawned = Vec::new();
        let mut changed = Vec::new();
        simulation.visit_entities(|entity| {
            current.insert(entity.id);
            if !self.known.contains(&entity.id) {
                spawned.push(entity.clone());
            }
            if simulation.entity_active(entity.id) && self.transforms.get(&entity.id) != Some(&entity.transform) {
                changed.push(json!({"id":entity.id,"name":entity.name,"tags":entity.tags,"transform":entity.transform}));
                self.transforms.insert(entity.id, entity.transform);
            }
        });
        let removed: Vec<_> = self.known.difference(&current).copied().collect();
        let state = simulation.state_summary();
        self.transforms.retain(|id, _| current.contains(id));
        let combat = simulation.take_events();
        let result = json!({"tick":state["tick"],"transforms":changed,"state":state["state"],"logs":state["logs"],"spawned":spawned,"removed":removed,"fx":combat["fx"],"events":combat["events"],"animators":simulation.animator_states(),"nav_agents":simulation.nav_agent_states(),"ragdolls":simulation.ragdoll_states()});
        self.known = current;
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn bridge_reports_collision_events_and_ragdoll_poses() {
        let mut game = WebGame::new();
        let scene = json!({"entities":[
            {"id":1,"name":"trigger","kind":"box","size":[5,5,5],
                "collider":{"shape":"box"},"props":{"ai_physics":{"sensor":true}}},
            {"id":2,"name":"ragdoll","kind":"box","size":[1,1,1],
                "props":{"ai_physics":{"ragdoll":{"start_active":true}}}}
        ]});
        let initial: Value = serde_json::from_str(&game.load_inner(&scene.to_string(), "{}").unwrap()).unwrap();
        let tick: Value = serde_json::from_str(&game.tick_inner("{}").unwrap()).unwrap();
        assert_eq!(initial["ragdolls"][0]["parts"].as_array().unwrap().len(), 11);
        assert_ne!(initial["ragdolls"][0]["parts"], tick["ragdolls"][0]["parts"]);
        assert_eq!(tick["events"], json!([{"type":"sensor","phase":"started","a":1,"b":2}]));
    }
    #[test]
    fn terrain_bridge_loads_streams_and_preserves_tiles_on_reload() {
        use engine_terrain::{Terrain, TerrainConfig, web::*};
        let terrain=Terrain::generate(TerrainConfig {size_m:[512.0;2],samples:33,..Default::default()}).unwrap();
        let coarse=CoarseGrid::from_terrain(&terrain,16.0).unwrap();
        let manifest=WebManifest {version:1,config:terrain.manifest.config.clone(),grid:[2;2],height_range:terrain.manifest.height_range,coarse:CoarseInfo {step:16.0,dims:coarse.dims,file:"coarse.bin".into()},tiles:"tiles/{x}_{z}.bin".into()};
        let mut game=WebGame::new();
        assert_eq!(game.terrain_height(0.0,0.0),0.0);
        assert_eq!(game.terrain_tiles_wanted(0.0),"[]");
        game.set_terrain("terrain/0/terrain.json",&serde_json::to_string(&manifest).unwrap(),&coarse.encode(manifest.height_range)).unwrap();
        let scene=json!({"entities":[{"id":1,"name":"terrain","kind":"box","size":[1,1,1],"tags":["player"],"props":{"ai_terrain":"terrain/0/terrain.json","ai_terrain_radius":1}}]}).to_string();
        game.load_inner(&scene,"{}").unwrap();
        assert_eq!(serde_json::from_str::<Value>(&game.terrain_tiles_wanted(0.0)).unwrap(),json!([{"path":"terrain/0/terrain.json","x":0,"z":0}]));
        game.add_terrain_tile("terrain/0/terrain.json",0,0,&encode_tile(&terrain.tiles[0],33,manifest.height_range)).unwrap();
        assert_eq!(game.terrain_tiles_wanted(0.0),"[]");
        assert!((game.terrain_height(11.0,12.0)-terrain.sample(11.0,12.0).unwrap() as f64).abs()<0.01);
        game.load_inner(&scene,"{}").unwrap();
        assert_eq!(game.terrain_tiles_wanted(0.0),"[]");
    }
    #[test]
    fn web_preloads_tiles_for_static_and_kinematic_ragdoll_roots() {
        use engine_terrain::{web::*, Terrain, TerrainConfig};
        let path = "terrain.json";
        let terrain = Terrain::generate(TerrainConfig {
            size_m: [512.0; 2], samples: 5, height_scale: 0.0, ..Default::default()
        }).unwrap();
        let coarse = CoarseGrid::from_terrain(&terrain, 256.0).unwrap();
        let manifest = WebManifest {
            version: 1, config: terrain.manifest.config.clone(), grid: [2; 2],
            height_range: terrain.manifest.height_range,
            coarse: CoarseInfo { step: 256.0, dims: coarse.dims, file: "coarse.bin".into() },
            tiles: "tiles/{x}_{z}.bin".into(),
        };
        for (kind, start_active) in [("static", true), ("kinematic", false)] {
            let mut game = WebGame::new();
            game.set_terrain(path, &serde_json::to_string(&manifest).unwrap(),
                &coarse.encode(manifest.height_range)).unwrap();
            let scene = json!({"entities":[
                {"id":1,"name":"terrain","kind":"plane","size":1.0,"props":{"ai_terrain":path,"ai_terrain_radius":8}},
                {"id":2,"name":"target","kind":"box","size":[1,1,1],"transform":{"position":[384,3,384]},
                 "rigidbody":{"type":kind},"props":{"ai_physics":{"ragdoll":{"preset":"humanoid","start_active":start_active}}},
                 "script":{"source":"fn on_tick(ctx,dt) { if ctx.input(\"activate\") > 0.0 { ctx.set_ragdoll(true); } }"}}
            ]});
            game.load_inner(&scene.to_string(), "{}").unwrap();
            let wanted: Value = serde_json::from_str(&game.terrain_tiles_wanted(0.0)).unwrap();
            assert_eq!(wanted, json!([{"path":path,"x":1,"z":1}]), "{kind}");
            let tile = terrain.tiles.iter().find(|tile| (tile.x, tile.z) == (1, 1)).unwrap();
            game.add_terrain_tile(path, 1, 1, &encode_tile(tile, 5, manifest.height_range)).unwrap();
            game.tick_inner(r#"{"activate":1}"#).unwrap();
            for _ in 0..119 {
                game.tick_inner(r#"{"activate":0}"#).unwrap();
            }
            let rigs = game.simulation.as_ref().unwrap().ragdoll_states();
            let y = rigs[0]["parts"][0]["position"][1].as_f64().unwrap();
            assert!((0.0..1.0).contains(&y), "{kind}: pelvis y={y}");
        }
    }

    #[test]
    fn preloaded_tiles_make_on_start_height_identical_on_first_load_and_restart() {
        use engine_terrain::{web::*, Terrain, TerrainConfig};
        let mut terrain = Terrain::generate(TerrainConfig {
            size_m: [256.0; 2], samples: 3, ..Default::default()
        }).unwrap();
        terrain.tiles[0].heights[12] = 300.0;
        let range = [0.0, 400.0];
        let coarse = CoarseGrid::from_terrain(&terrain, 256.0).unwrap();
        let manifest = WebManifest {version:1, config:terrain.manifest.config.clone(), grid:[1;2], height_range:range,
            coarse:CoarseInfo {step:256.0,dims:coarse.dims,file:"coarse.bin".into()},tiles:"tiles/{x}_{z}.bin".into()};
        let mut game = WebGame::new();
        game.set_terrain("terrain.json", &serde_json::to_string(&manifest).unwrap(), &coarse.encode(range)).unwrap();
        game.add_terrain_tile("terrain.json", 0, 0, &encode_tile(&terrain.tiles[0],3,range)).unwrap();
        let scene = json!({"entities":[
            {"id":1,"name":"terrain","kind":"box","size":[1,1,1],"transform":{"position":[1000,7,2000]},"props":{"ai_terrain":"terrain.json"}},
            {"id":2,"name":"player","kind":"box","size":[1,1,1],"tags":["player"],"transform":{"position":[1128,0,2128]},
             "script":{"source":"fn on_start(ctx) {ctx.set_position([1128.0,ctx.terrain_height(1128.0,2128.0),2128.0]);}"}}
        ]}).to_string();
        let first: Value = serde_json::from_str(&game.load_inner(&scene,"{}").unwrap()).unwrap();
        let restart: Value = serde_json::from_str(&game.load_inner(&scene,"{}").unwrap()).unwrap();
        assert_eq!(first["transforms"],restart["transforms"]);
        let y = first["transforms"][1]["transform"]["position"][1].as_f64().unwrap();
        assert!((y-307.0).abs()<0.01, "on_start height: {y}");
    }
    #[test]
    fn unchanged_transforms_are_omitted_and_reload_is_full() {
        let scene =
            json!({"entities":[{"id":1,"name":"static","kind":"box","size":[1,1,1]}]}).to_string();
        let mut game = WebGame::new();
        for _ in 0..2 {
            let initial: Value =
                serde_json::from_str(&game.load_inner(&scene, "{}").unwrap()).unwrap();
            assert_eq!(initial["transforms"].as_array().unwrap().len(), 1);
            let tick: Value = serde_json::from_str(&game.tick_inner("{}").unwrap()).unwrap();
            assert_eq!(tick["transforms"], json!([]));
        }
    }
    #[test]
    fn static_world_loads_and_ticks_without_transform_payload() {
        let entities: Vec<_> = (1..=1000)
            .map(|id| json!({"id":id,"name":"prop","kind":"box","size":[1,1,1]}))
            .collect();
        let mut game = WebGame::new();
        let initial: Value = serde_json::from_str(&game.load_inner(&json!({"entities":entities}).to_string(), "{}").unwrap()).unwrap();
        assert_eq!(initial["transforms"].as_array().unwrap().len(), 1000);
        let tick: Value = serde_json::from_str(&game.tick_inner("{}").unwrap()).unwrap();
        assert_eq!(tick["transforms"], json!([]));
    }
    #[test]
    fn region_tick_omits_far_transforms_and_reactivation_reports_changes() {
        let scene = json!({"entities":[{"id":1,"name":"far","kind":"box","size":[1,1,1],"transform":{"position":[1000,0,0]},"script":{"source":"fn on_tick(ctx,dt) {let p=ctx.position();p[0]+=1.0;ctx.set_position(p);}"}}]}).to_string();
        let mut game = WebGame::new();
        let initial: Value = serde_json::from_str(&game.load_inner(&scene,"{}").unwrap()).unwrap();
        assert_eq!(initial["transforms"].as_array().unwrap().len(),1);
        game.simulation.as_mut().unwrap().set_active_region(0.,0.,100.,10.).unwrap();
        let far: Value = serde_json::from_str(&game.tick_inner("{}").unwrap()).unwrap();
        assert_eq!(far["transforms"],json!([]));
        game.simulation.as_mut().unwrap().set_active_region(1000.,0.,100.,10.).unwrap();
        let near: Value = serde_json::from_str(&game.tick_inner("{}").unwrap()).unwrap();
        assert_eq!(near["transforms"][0]["transform"]["position"][0],1001.0);
    }
    #[test]
    fn empty_scene_ticks_and_load_is_transactional_on_errors() {
        let mut game = WebGame::new();
        let scene = serde_json::to_string(&Scene::default()).unwrap();
        let state: Value = serde_json::from_str(&game.load_inner(&scene, "{}").unwrap()).unwrap();
        assert_eq!(state["spawned"], json!([]));
        assert!(game.load_inner("not json", "{}").is_err());
        let state: Value = serde_json::from_str(&game.tick_inner("{}").unwrap()).unwrap();
        assert_eq!(state["tick"], 1);
        assert_eq!(state["removed"], json!([]));
        assert!(state["transforms"].as_array().unwrap().is_empty());
    }
    #[test]
    fn bridge_uses_native_runtime_and_reports_spawn_remove_and_state() {
        let scene = json!({"entities":[{"id":1,"name":"player","kind":"box","size":[1,1,1],"script":"player.rhai"}]});
        let scripts = json!({"player.rhai":"fn on_start(ctx) {ctx.state_set(\"score\",0);ctx.spawn_entity(#{name:\"coin\",kind:\"box\",size:[1,1,1]});} fn on_tick(ctx,dt) {let p=ctx.position();p[0]+=ctx.input(\"move_x\")*dt;ctx.set_position(p);ctx.remove(ctx.find_by_name(\"coin\"));ctx.state_set(\"score\",1);}"});
        let mut game = WebGame::new();
        let initial: Value = serde_json::from_str(
            &game
                .load_inner(&scene.to_string(), &scripts.to_string())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(initial["spawned"].as_array().unwrap().len(), 2);
        let tick: Value =
            serde_json::from_str(&game.tick_inner("{\"move_x\":1}").unwrap()).unwrap();
        assert_eq!(tick["removed"], json!([2]));
        assert_eq!(tick["state"]["score"], 1);
        assert!(
            tick["transforms"][0]["transform"]["position"][0]
                .as_f64()
                .unwrap()
                > 0.0
        );
        assert!(game
            .load_inner(&scene.to_string(), "{}")
            .unwrap_err()
            .contains("missing from bundled"));
        assert!(WebGame::new().tick_inner("{}").is_err());
        assert!(game.tick_inner("[]").is_err());
    }
}
