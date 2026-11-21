use crate::{script, Data, Simulation};
use anyhow::Result;
use engine_terrain::web::StreamedTerrain;
use shared::{components::BodyType, scene::Scene};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub(crate) type SharedTerrain = Rc<RefCell<StreamedTerrain>>;
pub(crate) type TerrainSources = Rc<RefCell<BTreeMap<String, SharedTerrain>>>;

fn height(scene: &Scene, sources: &TerrainSources, x: f64, z: f64) -> f64 {
    let sources = sources.borrow();
    let mut first = None;
    let mut entities: Vec<_> = scene.entities.iter().collect();
    entities.sort_by_key(|e| e.id);
    for e in entities {
        let Some(t) = e
            .props
            .get("ai_terrain")
            .and_then(|v| v.as_str())
            .and_then(|p| sources.get(p))
        else {
            continue;
        };
        let t = t.borrow();
        let lx = x - e.transform.position[0] as f64;
        let lz = z - e.transform.position[2] as f64;
        let h = e.transform.position[1] as f64 + t.sample(lx, lz) as f64;
        first.get_or_insert(h);
        if lx >= 0.0 && lz >= 0.0 && lx <= t.config.size_m[0] && lz <= t.config.size_m[1] {
            return h;
        }
    }
    first.unwrap_or(0.0)
}
pub(crate) fn register(
    engine: &mut rhai::Engine,
    data: Rc<RefCell<Data>>,
    sources: TerrainSources,
) {
    engine.register_fn(
        "terrain_height",
        move |_: &mut script::Context, x: f64, z: f64| -> f64 {
            height(&data.borrow().scene, &sources, x, z)
        },
    );
}
impl Simulation {
    pub fn from_sources_with_terrain(
        scene: Scene,
        scripts: BTreeMap<String, String>,
        terrains: BTreeMap<String, StreamedTerrain>,
    ) -> Result<Self> {
        Self::create(scene, Default::default(), Some(scripts), terrains)
    }
    pub fn insert_terrain_tile(&mut self, path: &str, x: u32, z: u32, bytes: &[u8]) -> Result<()> {
        let sources = self.physics.borrow().terrain_sources.clone();
        let sources = sources.borrow();
        let t = sources
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("unknown terrain: {path}"))?;
        let result = t.borrow_mut().insert_tile(x, z, bytes);
        result
    }
    pub fn terrain_height(&self, x: f64, z: f64) -> f64 {
        height(
            &self.data.borrow().scene,
            &self.physics.borrow().terrain_sources,
            x,
            z,
        )
    }
    pub fn terrain_tiles_wanted(&self, margin_m: f64) -> Vec<(String, u32, u32)> {
        let data = self.data.borrow();
        let physics = self.physics.borrow();
        let sources = physics.terrain_sources.clone();
        let sources = sources.borrow();
        let centers: Vec<_> = data
            .scene
            .entities
            .iter()
            .filter(|e| {
                e.tags.iter().any(|t| t == "player")
                    || physics.has_ragdoll(e.id)
                    || e.rigidbody
                        .as_ref()
                        .is_some_and(|r| r.kind == BodyType::Dynamic)
            })
            .map(|e| {
                [
                    e.transform.position[0] as f64,
                    e.transform.position[2] as f64,
                ]
            })
            .collect();
        let mut wanted = BTreeSet::new();
        for e in &data.scene.entities {
            let Some(path) = e.props.get("ai_terrain").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(t) = sources.get(path) else {
                continue;
            };
            let t = t.borrow();
            let radius = e
                .props
                .get("ai_terrain_radius")
                .and_then(|v| v.as_f64())
                .unwrap_or(512.0)
                + if margin_m.is_finite() {
                    margin_m.max(0.0)
                } else {
                    0.0
                };
            for index in crate::physics::nearby_terrain_tiles(
                &t.config,
                e.transform.position,
                &centers,
                radius,
            ) {
                let x = index as u32 % t.config.grid()[0];
                let z = index as u32 / t.config.grid()[0];
                if !t.tiles.contains_key(&(x, z)) {
                    wanted.insert((path.to_owned(), x, z));
                }
            }
        }
        wanted.into_iter().collect()
    }
}
