//! Deterministic, isolated gameplay runtime.
mod physics;
pub mod report;
mod script;

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

pub const DT: f32 = 1.0 / 60.0;
pub const MAX_ENTITIES: usize = 1024;
pub const MAX_TICKS: u64 = 36_000;

struct Data {
    scene: Scene,
    inputs: BTreeMap<String, f64>,
    state: BTreeMap<String, Value>,
    logs: VecDeque<String>,
    forces: Vec<Force>,
    dirty: BTreeSet<u64>,
    velocities: BTreeMap<u64, [f32;3]>,
    grounded: BTreeSet<u64>,
    next_id: u64,
}
impl Data {
    fn log(&mut self, message: String) {
        if self.logs.len() == 1024 {
            self.logs.pop_front();
        }
        self.logs.push_back(message.chars().take(4096).collect());
    }
}

struct ScriptInstance {
    ast: AST,
    label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub tick: u64,
    pub action: String,
    pub value: f64,
}

pub struct Simulation {
    data: Rc<RefCell<Data>>,
    engine: rhai::Engine,
    scripts: BTreeMap<u64, ScriptInstance>,
    physics: Physics,
    root: PathBuf,
    sources: Option<BTreeMap<String, String>>,
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
        Self::create(scene, root.canonicalize()?, None)
    }

    pub fn from_sources(scene: Scene, sources: BTreeMap<String, String>) -> Result<Self> {
        Self::create(scene, PathBuf::new(), Some(sources))
    }

    fn create(
        mut scene: Scene,
        root: PathBuf,
        sources: Option<BTreeMap<String, String>>,
    ) -> Result<Self> {
        if scene.entities.len() > MAX_ENTITIES {
            bail!("simulation supports at most {MAX_ENTITIES} entities");
        }
        scene.entities.sort_by_key(|e| e.id);
        for entity in &mut scene.entities {
            if sources.is_none() {
                if let shared::scene::PrimitiveKind::Gltf{path}=&mut entity.mesh {
                    if Path::new(path).is_relative() {*path=root.join(&*path).to_string_lossy().into_owned();}
                }
            }
            validate_entity(entity)?;
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
        let data = Rc::new(RefCell::new(Data {
            scene,
            inputs: BTreeMap::new(),
            state: BTreeMap::new(),
            logs: VecDeque::new(),
            forces: vec![],
            dirty: BTreeSet::new(),
            velocities: BTreeMap::new(),
            grounded: BTreeSet::new(),
            next_id,
        }));
        let engine = script::engine(data.clone());
        let mut simulation = Self {
            data,
            engine,
            scripts: BTreeMap::new(),
            physics: Physics::new(),
            root,
            sources,
            tick: 0,
            failed: false,
        };
        simulation.prepare_scripts()?;
        simulation
            .physics
            .reconcile(&simulation.data.borrow().scene)?;
        Ok(simulation)
    }

    pub fn scene(&self) -> Scene {
        self.data.borrow().scene.clone()
    }

    pub fn state(&self) -> Value {
        let data = self.data.borrow();
        json!({"tick":self.tick, "failed":self.failed,
            "entities": data.scene.entities.iter().map(|e| json!({"id":e.id, "name":e.name, "tags":e.tags, "transform":e.transform})).collect::<Vec<_>>(),
            "state":data.state, "logs":data.logs})
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
        let entities = self.scene().entities;
        self.scripts
            .retain(|id, _| entities.iter().any(|e| e.id == *id));
        for entity in entities {
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
            if self.scripts.contains_key(&entity.id) {
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
                self.hook(entity.id, "on_start")?;
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
        };
        let options = CallFnOptions::new().eval_ast(false);
        let result = if hook == "on_start" {
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
        let result = (|| {
            while self.tick < end {
                for input in inputs.iter().filter(|i| i.tick == self.tick) {
                    self.data
                        .borrow_mut()
                        .inputs
                        .insert(input.action.clone(), input.value);
                }
                self.prepare_scripts()?;
                let ids: Vec<_> = self.scripts.keys().copied().collect();
                for id in ids {
                    if self.data.borrow().scene.entities.iter().any(|e| e.id == id) {
                        self.hook(id, "on_tick")?;
                    }
                }
                let mut data = self.data.borrow_mut();
                let forces = std::mem::take(&mut data.forces);
                let dirty = std::mem::take(&mut data.dirty);
                self.physics.step(&mut data.scene, &dirty, forces)?;
                let (velocities, grounded) = self.physics.motion();
                data.velocities=velocities; data.grounded=grounded;
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
    use shared::scene::PrimitiveKind;
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
