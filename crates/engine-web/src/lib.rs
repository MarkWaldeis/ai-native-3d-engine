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
}

#[wasm_bindgen]
impl WebGame {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
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
        let simulation = Simulation::from_sources(scene, scripts).map_err(|e| e.to_string())?;
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
        let result = json!({"tick":state["tick"],"transforms":changed,"state":state["state"],"logs":state["logs"],"spawned":spawned,"removed":removed});
        self.known = current;
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
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
