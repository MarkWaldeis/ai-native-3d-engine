use shared::scene::{Entity, Scene};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct IndexCache(Mutex<Option<Arc<EntityIndex>>>);

impl Clone for IndexCache {
    fn clone(&self) -> Self {
        Self(Mutex::new(
            self.0.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        ))
    }
}

impl IndexCache {
    pub fn get(&self, scene: &Scene) -> Arc<EntityIndex> {
        let mut cached = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = cached.as_ref().filter(|index| index.matches(scene)) {
            return index.clone();
        }
        let index = Arc::new(EntityIndex::new(scene));
        *cached = Some(index.clone());
        index
    }

    pub fn clear(&mut self) {
        self.0.get_mut().unwrap_or_else(|e| e.into_inner()).take();
    }
}

pub const COMPONENTS: [&str; 7] = [
    "mesh",
    "collider",
    "rigidbody",
    "script",
    "animator",
    "nav_agent",
    "prefab",
];

fn flags(entity: &Entity) -> u8 {
    1 | ((entity.collider.is_some() as u8) << 1)
        | ((entity.rigidbody.is_some() as u8) << 2)
        | ((entity.script.is_some() as u8) << 3)
        | ((entity.animator.is_some() as u8) << 4)
        | ((entity.nav_agent.is_some() as u8) << 5)
        | ((entity.props.get("_prefab").is_some() as u8) << 6)
}

pub fn components(entity: &Entity) -> Vec<&'static str> {
    let mask = flags(entity);
    COMPONENTS
        .into_iter()
        .enumerate()
        .filter_map(|(i, name)| (mask & (1 << i) != 0).then_some(name))
        .collect()
}

fn prefab(entity: &Entity) -> Option<&str> {
    entity
        .props
        .get("_prefab")
        .and_then(|p| p.get("name"))
        .and_then(|p| p.as_str())
}

struct Key {
    id: u64,
    position: [u32; 3],
    flags: u8,
    tags: Vec<String>,
    prefab: Option<String>,
}

impl Key {
    fn matches(&self, entity: &Entity) -> bool {
        self.id == entity.id
            && self.position == entity.transform.position.map(f32::to_bits)
            && self.flags == flags(entity)
            && self.tags == entity.tags
            && self.prefab.as_deref() == prefab(entity)
    }
}

pub struct EntityIndex {
    keys: Vec<Key>,
    all: Vec<usize>,
    tags: BTreeMap<String, Vec<usize>>,
    components: BTreeMap<&'static str, Vec<usize>>,
    prefabs: BTreeMap<String, Vec<usize>>,
    regions: BTreeMap<(i64, i64), Vec<usize>>,
}

impl EntityIndex {
    pub fn new(scene: &Scene) -> Self {
        let keys: Vec<_> = scene
            .entities
            .iter()
            .map(|e| Key {
                id: e.id,
                position: e.transform.position.map(f32::to_bits),
                flags: flags(e),
                tags: e.tags.clone(),
                prefab: prefab(e).map(str::to_owned),
            })
            .collect();
        let mut all: Vec<_> = (0..keys.len()).collect();
        all.sort_unstable_by_key(|i| (keys[*i].id, *i));
        let mut result = Self {
            keys,
            all,
            tags: BTreeMap::new(),
            components: BTreeMap::new(),
            prefabs: BTreeMap::new(),
            regions: BTreeMap::new(),
        };
        for &row in &result.all {
            let key = &result.keys[row];
            for tag in key.tags.iter().collect::<BTreeSet<_>>() {
                result.tags.entry(tag.clone()).or_default().push(row);
            }
            for (i, name) in COMPONENTS.iter().enumerate() {
                if key.flags & (1 << i) != 0 {
                    result.components.entry(name).or_default().push(row);
                }
            }
            if let Some(name) = &key.prefab {
                result.prefabs.entry(name.clone()).or_default().push(row);
            }
            let p = scene.entities[row].transform.position;
            result
                .regions
                .entry((
                    (f64::from(p[0]) / 128.0).floor() as i64,
                    (f64::from(p[2]) / 128.0).floor() as i64,
                ))
                .or_default()
                .push(row);
        }
        result
    }

    pub fn matches(&self, scene: &Scene) -> bool {
        self.keys.len() == scene.entities.len()
            && self
                .keys
                .iter()
                .zip(&scene.entities)
                .all(|(key, e)| key.matches(e))
    }

    pub fn candidates(
        &self,
        tag: Option<&str>,
        component: Option<&str>,
        prefab: Option<&str>,
        region: Option<(i64, i64)>,
    ) -> Vec<usize> {
        let mut lists: Vec<&[usize]> = Vec::with_capacity(4);
        for list in [
            tag.map(|t| self.tags.get(t)),
            component.map(|c| self.components.get(c)),
            prefab.map(|p| self.prefabs.get(p)),
            region.map(|r| self.regions.get(&r)),
        ]
        .into_iter()
        .flatten()
        {
            let Some(list) = list else { return Vec::new() };
            lists.push(list);
        }
        lists.sort_unstable_by_key(|list| list.len());
        let Some((first, rest)) = lists.split_first() else {
            return self.all.clone();
        };
        first
            .iter()
            .copied()
            .filter(|row| {
                rest.iter().all(|list| {
                    list.binary_search_by_key(&(self.keys[*row].id, *row), |other| {
                        (self.keys[*other].id, *other)
                    })
                    .is_ok()
                })
            })
            .collect()
    }

    pub fn payload_bytes(&self) -> usize {
        let rows = |rows: &Vec<usize>| rows.capacity() * std::mem::size_of::<usize>();
        self.keys.capacity() * std::mem::size_of::<Key>()
            + rows(&self.all)
            + self
                .keys
                .iter()
                .map(|k| {
                    k.tags.capacity() * std::mem::size_of::<String>()
                        + k.tags.iter().map(String::capacity).sum::<usize>()
                        + k.prefab.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>()
            + self
                .tags
                .iter()
                .chain(&self.prefabs)
                .map(|(k, v)| k.capacity() + rows(v))
                .sum::<usize>()
            + self
                .components
                .values()
                .chain(self.regions.values())
                .map(rows)
                .sum::<usize>()
    }
}
