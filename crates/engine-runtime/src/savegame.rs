//! Versioned, base-relative persistent game state.
use super::*;
use serde::{Deserialize, Serialize};

pub const MAX_SAVE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Body {
    id: u64,
    linvel: [f32; 3],
    angvel: [f32; 3],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delta {
    modified: Vec<Value>,
    removed: Vec<u64>,
    spawned: Vec<shared::scene::Entity>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CombatState {
    health: BTreeMap<u64, f64>,
    vars: BTreeMap<u64, BTreeMap<String, Value>>,
    hits: BTreeMap<u64, Vec<Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    format: String,
    version: u32,
    tick: u64,
    base_fingerprint: String,
    state: BTreeMap<String, Value>,
    flags: BTreeMap<String, Value>,
    inventory: BTreeMap<String, i64>,
    next_id: u64,
    delta: Delta,
    bodies: Vec<Body>,
    #[serde(default)]
    inputs: BTreeMap<String, f64>,
    #[serde(default)]
    combat: CombatState,
    #[serde(default)]
    pending_starts: BTreeSet<u64>,
}

fn fingerprint(scene: &Scene) -> String {
    fn canonical(value: Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: BTreeMap<_, _> = map.into_iter().map(|(k, v)| (k, canonical(v))).collect();
                serde_json::to_value(sorted).unwrap()
            }
            Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
            value => value,
        }
    }
    let bytes = canonical(serde_json::to_value(scene).unwrap()).to_string();
    let hash = bytes.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("fnv1a64:{hash:016x}")
}

fn validate_map(map: &BTreeMap<String, Value>) -> Result<()> {
    if map.len() > 256 || map.iter().any(|(k, v)| k.len() > 256 || v.to_string().len() > 65_536) {
        bail!("saved global state limit exceeded");
    }
    Ok(())
}

fn validate_restored_paths(entity: &shared::scene::Entity, root: Option<&Path>) -> Result<()> {
    let check = |path: &str| -> Result<()> {
        let requested = Path::new(path);
        if path.is_empty() || requested.is_absolute() || path.contains('\\') || path.contains(':')
            || requested.components().any(|part| !matches!(part, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
            bail!("saved entity paths must be project-relative without .. or absolute paths");
        }
        if let Some(root) = root { engine_core::project_path(root, requested)?; }
        Ok(())
    };
    if let shared::scene::PrimitiveKind::Gltf { path } = &entity.mesh { check(path)?; }
    if let Some(path) = entity.script.as_ref().and_then(Script::path) { check(path)?; }
    if let Some(lods) = entity.props.get("web_lods") {
        for path in lods.as_object().context("web_lods must be an object")?.values() {
            check(path.as_str().context("LOD path must be a string")?)?;
        }
    }
    Ok(())
}

impl Simulation {
    pub fn save_game(&self) -> Value {
        let data = self.data.borrow();
        let base: BTreeMap<_, _> = self.base_scene.entities.iter().map(|e| (e.id, e)).collect();
        let live: BTreeMap<_, _> = data.scene.entities.iter().map(|e| (e.id, e)).collect();
        let removed: Vec<_> = base.keys().filter(|id| !live.contains_key(id)).copied().collect();
        let mut modified = Vec::new();
        let mut spawned = Vec::new();
        for (id, entity) in live {
            if let Some(original) = base.get(&id) {
                let original = serde_json::to_value(original).unwrap();
                let current = serde_json::to_value(entity).unwrap();
                let mut fields = serde_json::Map::new();
                for (key, value) in current.as_object().unwrap() {
                    if original.get(key) != Some(value) { fields.insert(key.clone(), value.clone()); }
                }
                for key in original.as_object().unwrap().keys() {
                    if current.get(key).is_none() { fields.insert(key.clone(), Value::Null); }
                }
                if !fields.is_empty() {
                    fields.insert("id".into(), json!(id));
                    modified.push(Value::Object(fields));
                }
            } else { spawned.push(entity.clone()); }
        }
        let physics = self.physics.borrow();
        let bodies: Vec<_> = physics.handles.iter().filter_map(|(id, handle)| {
            let body = &physics.bodies[*handle];
            (!body.is_fixed()).then(|| Body { id: *id, linvel: (*body.linvel()).into(), angvel: (*body.angvel()).into() })
        }).collect();
        let pending_starts: BTreeSet<_> = data.scene.entities.iter()
            .filter(|e| e.script.is_some() && !self.scripts.contains_key(&e.id)).map(|e| e.id).collect();
        json!({"format":"ai-engine-save","version":1,"tick":self.tick,
            "base_fingerprint":fingerprint(&self.base_scene),"state":data.state,"flags":data.flags,
            "inventory":data.inventory,"next_id":data.next_id,"inputs":data.inputs,
            "combat":{"health":data.combat.health,"vars":data.combat.vars,"hits":data.combat.hits},
            "pending_starts":pending_starts,
            "delta":{"modified":modified,"removed":removed,"spawned":spawned},"bodies":bodies})
    }

    pub fn load_game(base_scene: Scene, root: &Path, save: Value) -> Result<Self> {
        Self::load_game_inner(base_scene, root.canonicalize()?, None, BTreeMap::new(), save)
    }

    pub fn load_game_from_sources(base_scene: Scene, sources: BTreeMap<String, String>, save: Value) -> Result<Self> {
        Self::load_game_from_sources_with_terrain(base_scene, sources, BTreeMap::new(), save)
    }

    pub fn load_game_from_sources_with_terrain(base_scene: Scene, sources: BTreeMap<String, String>, terrains: BTreeMap<String, engine_terrain::web::StreamedTerrain>, save: Value) -> Result<Self> {
        Self::load_game_inner(base_scene, PathBuf::new(), Some(sources), terrains, save)
    }

    fn load_game_inner(base_scene: Scene, root: PathBuf, sources: Option<BTreeMap<String, String>>, terrains: BTreeMap<String, engine_terrain::web::StreamedTerrain>, value: Value) -> Result<Self> {
        if serde_json::to_vec(&value)?.len() > MAX_SAVE_BYTES { bail!("save exceeds 4 MB limit"); }
        let save: Save = serde_json::from_value(value).context("malformed savegame")?;
        if save.format != "ai-engine-save" || save.version != 1 { bail!("unsupported savegame format/version"); }
        validate_map(&save.state)?;
        validate_map(&save.flags)?;
        if save.inventory.len() > 256 || save.inventory.iter().any(|(k,v)| k.len() > 256 || *v < 0) { bail!("invalid saved inventory"); }
        if save.inputs.len() > 256 || save.inputs.iter().any(|(k,v)| k.len()>256 || !v.is_finite()) { bail!("invalid saved inputs"); }
        if save.combat.health.len() > MAX_ENTITIES || save.combat.vars.len() > MAX_ENTITIES || save.combat.hits.len() > MAX_ENTITIES
            || save.combat.health.values().any(|v| !v.is_finite() || *v < 0.0)
            || save.combat.vars.values().any(|vars| vars.len() > 32 || vars.iter().any(|(k,v)| k.len() > 64 || v.to_string().len() > 4096))
            || save.combat.hits.values().any(|hits| hits.len() > 64) {
            bail!("invalid saved combat state");
        }
        if save.delta.modified.len() > MAX_ENTITIES || save.delta.spawned.len() > MAX_ENTITIES || save.delta.removed.len() > MAX_ENTITIES || save.bodies.len() > MAX_ENTITIES { bail!("save entity limit exceeded"); }
        if save.pending_starts.len() > MAX_ENTITIES { bail!("save pending script limit exceeded"); }
        let mut sim = Self::create_mode(base_scene, root, sources, terrains, true)?;
        sim.base_changed = fingerprint(&sim.base_scene) != save.base_fingerprint;
        let mut entities: BTreeMap<_, _> = sim.base_scene.entities.iter().map(|e| (e.id, e.clone())).collect();
        if sim.sources.is_none() {
            for entity in entities.values_mut() {
                if let shared::scene::PrimitiveKind::Gltf { path } = &mut entity.mesh {
                    *path = Path::new(path).strip_prefix(&sim.root).context("base asset path escapes project root")?.to_string_lossy().into_owned();
                }
            }
        }
        for id in save.delta.removed { entities.remove(&id); }
        let mut seen = BTreeSet::new();
        for patch in save.delta.modified {
            let fields = patch.as_object().context("modified entity must be an object")?;
            let id = patch["id"].as_u64().context("modified entity requires integer id")?;
            if !seen.insert(id) { bail!("duplicate modified entity id"); }
            if let Some(entity) = entities.get_mut(&id) {
                let mut value = serde_json::to_value(&*entity)?;
                for (key, field) in fields {
                    if !value.as_object().unwrap().contains_key(key) && !matches!(key.as_str(), "animator" | "nav_agent" | "size" | "radius" | "height" | "segments" | "width" | "door_width" | "door_height" | "path") { bail!("unknown modified entity field {key}"); }
                    value[key] = field.clone();
                }
                *entity = serde_json::from_value(value).context("invalid modified entity")?;
            }
        }
        for entity in save.delta.spawned {
            if entities.insert(entity.id, entity).is_some() { bail!("spawned entity id already exists"); }
        }
        if entities.len() > MAX_ENTITIES { bail!("save entity limit exceeded"); }
        let minimum_next = entities.keys().next_back().copied().unwrap_or(0).checked_add(1).context("entity id overflow")?;
        if save.next_id == 0 || (!sim.base_changed && save.next_id < minimum_next) { bail!("invalid saved next_id"); }
        for entity in entities.values_mut() {
            validate_entity(entity)?;
            validate_restored_paths(entity, sim.sources.is_none().then_some(sim.root.as_path()))?;
            if sim.sources.is_none() {
                if let shared::scene::PrimitiveKind::Gltf { path } = &mut entity.mesh {
                    *path = engine_core::project_path(&sim.root, Path::new(path))?.to_string_lossy().into_owned();
                }
            }
        }
        {
            let mut data = sim.data.borrow_mut();
            data.scene.entities = entities.into_values().collect();
            data.state = save.state;
            data.flags = save.flags;
            data.inventory = save.inventory;
            data.inputs = save.inputs;
            data.next_id = save.next_id.max(minimum_next);
            data.combat.health = save.combat.health;
            data.combat.vars = save.combat.vars.into_iter().map(|(id, vars)| (id, vars.into_iter().collect())).collect();
            data.combat.hits = save.combat.hits;
            data.dirty = data.scene.entities.iter().map(|e| e.id).collect();
            data.refresh_player();
        }
        sim.scripts.clear();
        sim.prepare_scripts()?;
        let terrain_sources = sim.physics.borrow().terrain_sources.clone();
        let mut physics = Physics::new(sim.root.clone());
        physics.terrain_sources = terrain_sources;
        sim.physics = Rc::new(RefCell::new(physics));
        sim.physics.borrow_mut().reconcile_terrain(&sim.data.borrow().scene)?;
        sim.sync_physics()?;
        {
            let mut physics = sim.physics.borrow_mut();
            let mut ids = BTreeSet::new();
            for body in save.bodies {
                if !ids.insert(body.id) || body.linvel.iter().chain(&body.angvel).any(|v| !v.is_finite()) { bail!("invalid saved body velocity"); }
                if let Some(handle) = physics.handles.get(&body.id).copied() {
                    let target = &mut physics.bodies[handle];
                    if target.is_fixed() { bail!("saved velocity for static body"); }
                    target.set_linvel(body.linvel.into(), true);
                    target.set_angvel(body.angvel.into(), true);
                    sim.data.borrow_mut().velocities.insert(body.id, body.linvel);
                }
            }
        }
        sim.tick = save.tick;
        sim.loading = false;
        for id in sim.scripts.keys().copied().collect::<Vec<_>>() {
            if sim.data.borrow().entity_index(id).is_some() {
                sim.hook(id, if save.pending_starts.contains(&id) { "on_start" } else { "on_load" })?;
            }
        }
        sim.sync_physics()?;
        Ok(sim)
    }
}
