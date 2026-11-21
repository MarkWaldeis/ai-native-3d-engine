//! Deterministic, isolated gameplay runtime.
mod combat;
pub mod animation;
pub mod navigation;
mod physics;
mod physics_config;
mod ragdoll;
mod netcode;
pub use netcode::{NetEntityState, SimState};
mod terrain_stream;
pub mod report;
mod script;
pub mod savegame;

use anyhow::{bail, Context as _, Result};
use physics::{Force, Physics};
use rhai::{CallFnOptions, Scope, AST};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::components::Script;
use shared::scene::Scene;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[cfg(test)]
mod region_tests;

pub const DT: f32 = 1.0 / 60.0;
pub const MAX_ENTITIES: usize = 32768;

#[cfg(test)]
mod entity_limit_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn large_scenes_load_but_loading_and_scripted_spawns_remain_bounded() {
        assert_eq!(MAX_ENTITIES, 32768);
        let mut scene = Scene::default();
        for id in 1..=1025 {
            scene.entities.push(serde_json::from_value(json!({
                "id": id, "name": format!("entity-{id}"), "kind": "box", "size": [1,1,1]
            })).unwrap());
        }
        let mut simulation = Simulation::from_sources(scene, BTreeMap::new()).unwrap();
        simulation.step(1, &[]).unwrap();
        let entity = simulation.scene().entities[0].clone();
        let mut oversized = Scene::default();
        oversized.entities = vec![entity.clone(); MAX_ENTITIES + 1];
        let error = Simulation::from_sources(oversized, BTreeMap::new()).err().unwrap();
        assert!(error.to_string().contains("at most 32768 entities"));

        let scene: Scene = serde_json::from_value(json!({"entities": [{
            "id": 1, "name": "spawner", "kind": "box", "size": [1,1,1],
            "script": {"source": "fn on_tick(ctx, dt) { ctx.spawn_entity(#{kind: \"box\", size: [1,1,1]}); }"}
        }]})).unwrap();
        let simulation = Simulation::from_sources(scene, BTreeMap::new()).unwrap();
        {
            let mut data = simulation.data.borrow_mut();
            for id in 2..MAX_ENTITIES as u64 {
                let mut next = entity.clone();
                next.id = id;
                data.scene.entities.push(next);
            }
            data.next_id = MAX_ENTITIES as u64;
        }
        simulation.hook(1, "on_tick").unwrap();
        assert_eq!(simulation.data.borrow().scene.entities.len(), MAX_ENTITIES);
        let error = simulation.hook(1, "on_tick").unwrap_err();
        assert!(error.to_string().contains("simulation entity limit exceeded"));
        assert_eq!(simulation.data.borrow().scene.entities.len(), MAX_ENTITIES);
        assert!(Simulation::from_sources(Scene::default(), BTreeMap::new()).is_ok());
    }
}
pub const MAX_TICKS: u64 = 36_000;

#[derive(Clone)]
struct Data {
    scene: Scene,
    inputs: BTreeMap<String, f64>,
    entity_inputs: BTreeMap<u64, BTreeMap<String, f64>>,
    state: BTreeMap<String, Value>,
    flags: BTreeMap<String, Value>,
    inventory: BTreeMap<String, i64>,
    logs: VecDeque<String>,
    forces: Vec<Force>,
    dirty: BTreeSet<u64>,
    velocities: BTreeMap<u64, [f32;3]>,
    grounded: BTreeSet<u64>,
    next_id: u64,
    combat: combat::Combat,
}
impl Data {
    fn input(&self, id: u64, name: &str) -> f64 {
        self.entity_inputs.get(&id).and_then(|inputs| inputs.get(name))
            .or_else(|| self.inputs.get(name)).copied().unwrap_or(0.0)
    }
    fn refresh_player(&mut self) {
        self.combat.player = self
            .scene
            .entities
            .iter()
            .find(|e| e.tags.iter().any(|t| t == "player"))
            .map(|e| e.id);
        let combat = &mut self.combat;
        let stale = combat.vars.len() + combat.health.len() + combat.hits.len() > self.scene.entities.len() + 64;
        if stale {
            let live: BTreeSet<u64> = self.scene.entities.iter().map(|e| e.id).collect();
            combat.vars.retain(|id, _| live.contains(id));
            combat.health.retain(|id, _| live.contains(id));
            combat.hits.retain(|id, _| live.contains(id));
        }
    }
    fn entity_index(&self, id: u64) -> Option<usize> {
        let entities = &self.scene.entities;
        match entities.binary_search_by_key(&id, |e| e.id) {
            Ok(index) => Some(index),
            Err(_) => entities.iter().position(|e| e.id == id),
        }
    }
    fn log(&mut self, message: String) {
        if self.logs.len() == 1024 {
            self.logs.pop_front();
        }
        self.logs.push_back(message.chars().take(4096).collect());
    }
}

#[derive(Clone)]
struct ScriptInstance {
    ast: AST,
    label: String,
}

#[derive(Clone, Copy)]
struct ActiveRegion {
    x: f32,
    z: f32,
    radius: f32,
    hysteresis: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(default)]
    pub entity: Option<u64>,
    pub tick: u64,
    pub action: String,
    pub value: f64,
}

pub struct Simulation {
    base_scene: Scene,
    loading: bool,
    pub base_changed: bool,
    data: Rc<RefCell<Data>>,
    engine: rhai::Engine,
    scripts: BTreeMap<u64, ScriptInstance>,
    physics: Rc<RefCell<Physics>>,
    root: PathBuf,
    sources: Option<BTreeMap<String, String>>,
    active_region: Option<ActiveRegion>,
    inactive: BTreeSet<u64>,
    pub tick: u64,
    pub failed: bool,
}

impl Simulation {
    pub fn check_script(script: &Script, name: &str, root: &Path) -> Result<()> {
        let simulation = Self::new(Scene::default(), root)?;
        let (label, source) = simulation.source(script, name)?;
        if source.len() > 65_536 {
            bail!("{label}:1: script exceeds 65536 bytes");
        }
        simulation
            .engine
            .compile(&source)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{label}:{}: {e}", e.position().line().unwrap_or(1)))
    }
    pub fn new(scene: Scene, root: &Path) -> Result<Self> {
        Self::create(scene, root.canonicalize()?, None, BTreeMap::new())
    }

    pub fn from_sources(scene: Scene, sources: BTreeMap<String, String>) -> Result<Self> {
        Self::from_sources_with_terrain(scene, sources, BTreeMap::new())
    }

    fn create(
        scene: Scene,
        root: PathBuf,
        sources: Option<BTreeMap<String, String>>,
        terrains: BTreeMap<String, engine_terrain::web::StreamedTerrain>,
    ) -> Result<Self> {
        Self::create_mode(scene, root, sources, terrains, false)
    }

    fn create_mode(
        mut scene: Scene,
        root: PathBuf,
        sources: Option<BTreeMap<String, String>>,
        terrains: BTreeMap<String, engine_terrain::web::StreamedTerrain>,
        loading: bool,
    ) -> Result<Self> {
        if scene.entities.len() > MAX_ENTITIES {
            bail!("simulation supports at most {MAX_ENTITIES} entities");
        }
        scene.entities.sort_by_key(|e| e.id);
        if let Some(mesh)=&scene.navmesh {
            navigation::validate(mesh)?;
            if sources.is_none(){scene.navmesh=Some(navigation::bake(&scene,&root,mesh.params.clone())?.0);}
        }
        if scene.entities.iter().filter(|e|e.nav_agent.is_some()).count()>128 { bail!("at most 128 navigation agents are supported"); }
        for entity in &mut scene.entities {
            if sources.is_none() {
                if let shared::scene::PrimitiveKind::Gltf{path}=&mut entity.mesh {
                    if Path::new(path).is_relative() {*path=root.join(&*path).to_string_lossy().into_owned();}
                }
            }
            validate_entity(entity)?;
            if let Some(agent)=&mut entity.nav_agent {
                if scene.navmesh.is_none(){bail!("nav_agent requires a baked scene navmesh");}
                agent.path.clear();agent.next=0;agent.planned_target=None;agent.status="idle".into();
            }
            if let Some(animator) = &mut entity.animator {
                animation::reset(animator);
            }
        }
        if scene.entities.windows(2).any(|e| e[0].id == e[1].id) {
            bail!("scene contains duplicate entity ids");
        }
        let next_id = scene
            .entities
            .iter()
            .map(|e| e.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("entity id overflow")?;
        let base_scene = scene.clone();
        let data = Rc::new(RefCell::new(Data {
            scene,
            inputs: BTreeMap::new(),
            entity_inputs: BTreeMap::new(),
            state: BTreeMap::new(),
            flags: BTreeMap::new(),
            inventory: BTreeMap::new(),
            logs: VecDeque::new(),
            forces: vec![],
            dirty: BTreeSet::new(),
            velocities: BTreeMap::new(),
            grounded: BTreeSet::new(),
            next_id,
            combat: combat::Combat::default(),
        }));
        let terrain_sources: terrain_stream::TerrainSources = Rc::new(RefCell::new(terrains.into_iter().map(|(p,t)| (p,Rc::new(RefCell::new(t)))).collect()));
        let mut engine = script::engine(data.clone());
        terrain_stream::register(&mut engine, data.clone(), terrain_sources.clone());
        let mut physics = Physics::new(root.clone());
        physics.terrain_sources = terrain_sources;
        let physics = Rc::new(RefCell::new(physics));
        let mut simulation = Self {
            base_scene,
            loading,
            base_changed: false,
            data,
            engine,
            scripts: BTreeMap::new(),
            physics,
            root,
            sources,
            active_region: None,
            inactive: BTreeSet::new(),
            tick: 0,
            failed: false,
        };
        simulation.data.borrow_mut().refresh_player();
        simulation.physics.borrow_mut().reconcile_terrain(&simulation.data.borrow().scene)?;
        simulation.sync_physics()?;
        simulation.prepare_scripts()?;
        simulation.sync_physics()?;
        Ok(simulation)
    }

    fn sync_physics(&self) -> Result<()> {
        let mut physics = self.physics.borrow_mut();
        let data = self.data.borrow();
        physics.reconcile(&data.scene)?;
        physics.apply_poses(&data.scene, &data.dirty);
        physics.refresh_queries();
        Ok(())
    }

    pub fn scene(&self) -> Scene {
        self.data.borrow().scene.clone()
    }

    pub fn physics_query(&self, name: &str, args: &Value) -> Result<Value> {
        self.physics.borrow().query(name, serde_json::from_value(args.clone())?)
    }

    pub fn ragdoll_states(&self) -> Value {
        self.physics.borrow().ragdoll_states()
    }

    pub fn authoring_physics_query(mut scene: Scene, root: &Path, name: &str, args: &Value) -> Result<Value> {
        anyhow::ensure!(scene.entities.len() <= MAX_ENTITIES, "physics queries support at most {MAX_ENTITIES} entities");
        let query = serde_json::from_value(args.clone())?;
        scene.entities.sort_by_key(|entity| entity.id);
        let mut physics = Physics::new(root.to_path_buf());
        physics.reconcile_for_query(&scene, name, &query)?;
        physics.refresh_queries();
        physics.query(name, query)
    }

    pub fn visit_entities(&self, mut visitor: impl FnMut(&shared::scene::Entity)) {
        for entity in &self.data.borrow().scene.entities {
            visitor(entity);
        }
    }

    pub fn state_summary(&self) -> Value {
        let data = self.data.borrow();
        json!({"tick":self.tick,"failed":self.failed,"state":data.state,"flags":data.flags,"inventory":data.inventory,"logs":data.logs,"base_changed":self.base_changed})
    }

    pub fn animator_states(&self) -> Value {
        json!(self.data.borrow().scene.entities.iter().filter_map(|e| e.animator.as_ref().map(|a| json!({"id":e.id,"state":{
            "current_state":a.current_state,"time":a.time,"previous_state":a.previous_state,"previous_time":a.previous_time,
            "blend_time":a.blend_time,"blend_elapsed":a.blend_elapsed,"sequence":a.sequence,"values":a.values
        }}))).collect::<Vec<_>>())
    }

    pub fn nav_agent_states(&self)->Value {
        json!(self.data.borrow().scene.entities.iter().filter_map(|e|e.nav_agent.as_ref().map(|a|json!({"id":e.id,"position":e.transform.position,"agent":a}))).collect::<Vec<_>>())
    }

    pub fn set_active_region(&mut self, x: f32, z: f32, radius: f32, hysteresis: f32) -> Result<()> {
        if ![x,z,radius,hysteresis].iter().all(|v| v.is_finite()) || radius <= 0.0 || hysteresis < 0.0 {
            bail!("active region needs finite coordinates, positive radius and nonnegative hysteresis");
        }
        self.active_region = Some(ActiveRegion { x,z,radius,hysteresis });
        Ok(())
    }

    pub fn clear_active_region(&mut self) {
        self.active_region = None;
        self.inactive.clear();
    }

    pub fn entity_active(&self, id: u64) -> bool {
        !self.inactive.contains(&id)
    }

    fn refresh_active_region(&mut self) {
        let Some(region) = self.active_region else { return; };
        let data = self.data.borrow();
        let physics = self.physics.borrow();
        let moving = |e: &shared::scene::Entity| physics.ragdoll_active(e.id) || e.rigidbody.as_ref().is_some_and(|body| body.kind != shared::components::BodyType::Static);
        let candidates: Vec<_> = data.scene.entities.iter().filter(|entity| entity.script.is_some() || moving(entity)).collect();
        let live: BTreeSet<_> = candidates.iter().map(|e| e.id).collect();
        self.inactive.retain(|id| live.contains(id));
        for entity in candidates {
            let always = entity.tags.iter().any(|t| t == "player" || t == "always_active");
            if always {
                self.inactive.remove(&entity.id);
                continue;
            }
            let position = entity.transform.position;
            let threshold = region.radius + if self.inactive.contains(&entity.id) { 0.0 } else { region.hysteresis };
            if (position[0]-region.x).hypot(position[2]-region.z) > threshold {
                self.inactive.insert(entity.id);
            } else {
                self.inactive.remove(&entity.id);
            }
        }
    }

    /// Drains `{fx, events}` produced by physics and scripts since the last `step` call began.
    pub fn take_events(&self) -> Value {
        self.data.borrow_mut().combat.take_output()
    }

    pub fn state(&self) -> Value {
        let data = self.data.borrow();
        json!({"tick":self.tick, "failed":self.failed,
            "entities": data.scene.entities.iter().map(|e| json!({"id":e.id, "name":e.name, "tags":e.tags, "transform":e.transform})).collect::<Vec<_>>(),
            "state":data.state, "flags":data.flags,"inventory":data.inventory,"base_changed":self.base_changed,"logs":data.logs, "animators":self.animator_states(),"nav_agents":self.nav_agent_states(),"ragdolls":self.ragdoll_states()})
    }

    fn source(&self, script: &Script, name: &str) -> Result<(String, String)> {
        let path = match script {
            Script::Versioned { path, source } => return Ok((path.clone(), source.clone())),
            Script::Inline { source } => return Ok((format!("<{name}:inline>"), source.clone())),
            Script::Path(path) | Script::File { path } => path,
        };
        let requested = Path::new(path);
        if requested.is_absolute() || requested.extension().and_then(|s| s.to_str()) != Some("rhai")
        {
            bail!("script {path}: use a relative .rhai path inside project/assets");
        }
        if let Some(sources) = &self.sources {
            if requested
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                bail!("script {path}: path escapes source map");
            }
            let source = sources
                .get(path)
                .or_else(|| sources.get(&format!("assets/{path}")))
                .with_context(|| format!("script {path}: missing from bundled scripts map"))?;
            return Ok((path.clone(), source.clone()));
        }
        let file = [
            self.root.join(requested),
            self.root.join("assets").join(requested),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .with_context(|| format!("script {path}: file not found in project or assets"))?
        .canonicalize()?;
        if !file.starts_with(&self.root) {
            bail!("script {path}: path escapes project root");
        }
        let source = std::fs::read_to_string(&file)?;
        Ok((file.display().to_string(), source))
    }

    fn prepare_scripts(&mut self) -> Result<()> {
        let entities: Vec<_> = self.data.borrow().scene.entities.iter()
            .filter(|entity| entity.script.is_some()).cloned().collect();
        self.scripts
            .retain(|id, _| entities.iter().any(|e| e.id == *id));
        for entity in entities {
            if entity.script.is_none() || self.scripts.contains_key(&entity.id) {
                continue;
            }
            if !self
                .data
                .borrow()
                .scene
                .entities
                .iter()
                .any(|e| e.id == entity.id)
            {
                continue;
            }
            if let Some(script) = &entity.script {
                let (label, source) = self.source(script, &entity.name)?;
                if source.len() > 65_536 {
                    bail!("{label}:1: script exceeds 65536 bytes");
                }
                let mut ast = self.engine.compile(&source).map_err(|e| {
                    anyhow::anyhow!("{label}:{}: {e}", e.position().line().unwrap_or(1))
                })?;
                ast.set_source(&label);
                self.scripts
                    .insert(entity.id, ScriptInstance { ast, label });
                if !self.loading { self.hook(entity.id, "on_start")?; }
            }
        }
        Ok(())
    }

    fn hook(&self, id: u64, hook: &str) -> Result<()> {
        let script = &self.scripts[&id];
        if !script.ast.iter_functions().any(|f| f.name == hook) {
            return Ok(());
        }
        let ctx = script::Context {
            id,
            data: self.data.clone(),
            physics: self.physics.clone(),
        };
        let options = CallFnOptions::new().eval_ast(false);
        let result = if hook == "on_start" || hook == "on_load" {
            self.engine.call_fn_with_options::<rhai::Dynamic>(
                options,
                &mut Scope::new(),
                &script.ast,
                hook,
                (ctx,),
            )
        } else {
            self.engine.call_fn_with_options::<rhai::Dynamic>(
                options,
                &mut Scope::new(),
                &script.ast,
                hook,
                (ctx, 1.0_f64 / 60.0),
            )
        };
        result
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{}:{}: {e}", script.label, script_error_line(&e)))
    }

    pub fn validate_step(&self, ticks: u64, inputs: &[Input]) -> Result<()> {
        if self.failed {
            bail!("simulation stopped after script/physics error; stop and play again");
        }
        if ticks > MAX_TICKS || self.tick.checked_add(ticks).is_none() {
            bail!("ticks must be in 0..={MAX_TICKS}");
        }
        let end = self.tick + ticks;
        if inputs.len() > 4096 {
            bail!("at most 4096 input events per call");
        }
        for input in inputs {
            if input.tick < self.tick
                || input.tick >= end
                || input.action.is_empty()
                || input.action.len() > 128
                || !input.value.is_finite()
            {
                bail!("input events need tick in [{}..{end}), nonempty action (max 128 chars), and finite value", self.tick);
            }
        }
        Ok(())
    }

    pub fn step(&mut self, ticks: u64, inputs: &[Input]) -> Result<()> {
        self.validate_step(ticks, inputs)?;
        let end = self.tick + ticks;
        self.data.borrow_mut().combat.clear_output();
        let result = (|| {
            while self.tick < end {
                for input in inputs.iter().filter(|i| i.tick == self.tick) {
                    let mut data = self.data.borrow_mut();
                    let inputs = match input.entity {
                        Some(id) => data.entity_inputs.entry(id).or_default(),
                        None => &mut data.inputs,
                    };
                    inputs.insert(input.action.clone(), input.value);
                }
                {
                    let mut data = self.data.borrow_mut();
                    data.refresh_player();
                    data.combat.raycasts = 0;
                }
                self.prepare_scripts()?;
                self.refresh_active_region();
                {
                    let mut physics = self.physics.borrow_mut();
                    if physics.apply_active(&self.inactive) { physics.refresh_queries(); }
                }
                let ids: Vec<_> = self.scripts.keys().copied().collect();
                for id in ids {
                    if !self.entity_active(id) { continue; }
                    if self.data.borrow().scene.entities.iter().any(|e| e.id == id) {
                        self.hook(id, "on_tick")?;
                    }
                }
                let mut data = self.data.borrow_mut();
                let forces = std::mem::take(&mut data.forces);
                let dirty = std::mem::take(&mut data.dirty);
                let mut physics = self.physics.borrow_mut();
                for event in physics.step(&mut data.scene, &dirty, forces, &self.inactive)? {
                    data.combat.push_event(event);
                }
                let (velocities, grounded) = physics.motion();
                data.velocities=velocities; data.grounded=grounded;
                for entity in &mut data.scene.entities {
                    if !self.inactive.contains(&entity.id) {
                        if let Some(animator) = &mut entity.animator { animation::step(animator, DT as f64); }
                    }
                }
                let nav_dirty = {
                    let mut ragdolls = physics.active_ragdoll_ids().peekable();
                    if ragdolls.peek().is_some() {
                        let mut nav_inactive = self.inactive.clone();
                        nav_inactive.extend(ragdolls);
                        navigation::step(&mut data.scene, &nav_inactive, DT)?
                    } else {
                        navigation::step(&mut data.scene, &self.inactive, DT)?
                    }
                };
                if !nav_dirty.is_empty(){physics.apply_poses(&data.scene,&nav_dirty);physics.refresh_queries();}
                self.tick += 1;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}

fn script_error_line(error: &rhai::EvalAltResult) -> usize {
    match error {
        rhai::EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => script_error_line(inner),
        _ => error.position().line().unwrap_or(1),
    }
}

fn validate_entity(entity: &shared::scene::Entity) -> Result<()> {
    if let Some(a)=&entity.nav_agent {
        if !a.speed.is_finite() || !(0.0..=100.0).contains(&a.speed) || !a.offset_y.is_finite() || a.offset_y.abs()>100.0 || a.target.is_some_and(|v|v.iter().any(|n|!n.is_finite() || n.abs()>1e6)) {bail!("invalid nav_agent speed, offset or target");}
        if entity.rigidbody.as_ref().is_some_and(|b|b.kind!=shared::components::BodyType::Kinematic){bail!("nav_agent requires a kinematic rigidbody or no rigidbody");}
    }
    use shared::scene::PrimitiveKind;
    if let Some(animator) = &entity.animator {
        anyhow::ensure!(matches!(entity.mesh, PrimitiveKind::Gltf { .. }), "animator requires a gltf entity");
        animation::validate(animator)?;
    }
    if entity.id > i64::MAX as u64
        || entity
            .transform
            .position
            .iter()
            .chain(entity.transform.rotation_deg.iter())
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        || entity
            .transform
            .scale
            .iter()
            .any(|v| !v.is_finite() || v.abs() < 0.0001 || v.abs() > 10_000.0)
    {
        bail!(
            "entity {}: invalid transform/id for simulation",
            entity.name
        );
    }
    let positive = |v: f32| v.is_finite() && v > 0.0 && v <= 10_000.0;
    let valid = match &entity.mesh {
        PrimitiveKind::Box { size } => size.iter().all(|v| positive(*v)),
        PrimitiveKind::Plane { size } => positive(*size),
        PrimitiveKind::Sphere { radius, segments } => {
            positive(*radius) && (3..=256).contains(segments)
        }
        PrimitiveKind::Cylinder {
            radius,
            height,
            segments,
        } => positive(*radius) && positive(*height) && (3..=256).contains(segments),
        PrimitiveKind::WallWithDoor {
            width,
            height,
            door_width,
            door_height,
        } => [width, height, door_width, door_height]
            .iter()
            .all(|v| positive(**v)),
        PrimitiveKind::Gltf { .. } => true,
    };
    if !valid {
        bail!("entity {}: primitive dimensions must be positive and <=10000; segments must be 3..=256", entity.name);
    }
    Ok(())
}
