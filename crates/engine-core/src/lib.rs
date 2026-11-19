//! engine-core: thread-safe-ish world state + command queue.
//! v0.1 keeps entities in a flat Vec (no archetypes yet) — correctness first.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use shared::scene::{Camera, Entity, EntityId, PrimitiveKind, Scene, Transform};
use std::collections::VecDeque;
use std::path::Path;

/// Mutations the outside world (MCP) can apply. Processed via World::apply.
#[derive(Debug)]
pub enum Command {
    SpawnPrimitive {
        name: String,
        kind: PrimitiveKind,
        transform: Transform,
        color: [f32; 4],
    },
    SetTransform {
        id: EntityId,
        transform: Transform,
    },
    SetColor {
        id: EntityId,
        color: [f32; 4],
    },
    Remove {
        id: EntityId,
    },
    SetCamera {
        camera: Camera,
    },
    SaveScene {
        path: String,
    },
    LoadScene {
        path: String,
    },
}

#[derive(Default)]
pub struct CommandQueue(VecDeque<Command>);

impl CommandQueue {
    pub fn push(&mut self, cmd: Command) {
        self.0.push_back(cmd);
    }
    pub fn drain(&mut self) -> impl Iterator<Item = Command> + '_ {
        self.0.drain(..)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub struct World {
    pub scene: Scene,
    next_id: EntityId,
    /// Bumped on every mutation — the renderer uses it as a dirty flag.
    pub revision: u64,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            scene: Scene::default(),
            next_id: 1,
            revision: 0,
        }
    }

    pub fn spawn(
        &mut self,
        name: impl Into<String>,
        kind: PrimitiveKind,
        transform: Transform,
        color: [f32; 4],
    ) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        self.scene.entities.push(Entity {
            id,
            name: name.into(),
            mesh: kind,
            transform,
            color,
            visible: true,
        });
        self.bump();
        id
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.scene.entities.iter().find(|e| e.id == id)
    }

    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.scene.entities.iter_mut().find(|e| e.id == id)
    }

    pub fn remove(&mut self, id: EntityId) -> bool {
        let n = self.scene.entities.len();
        self.scene.entities.retain(|e| e.id != id);
        let removed = self.scene.entities.len() != n;
        if removed {
            self.bump();
        }
        removed
    }

    pub fn apply(&mut self, cmd: Command) -> Result<Value> {
        match cmd {
            Command::SpawnPrimitive {
                name,
                kind,
                transform,
                color,
            } => {
                let id = self.spawn(name, kind, transform, color);
                Ok(json!({"id": id}))
            }
            Command::SetTransform { id, transform } => {
                let e = self.entity_mut(id).context(format!("no entity {id}"))?;
                e.transform = transform;
                self.bump();
                Ok(json!({"ok": true}))
            }
            Command::SetColor { id, color } => {
                let e = self.entity_mut(id).context(format!("no entity {id}"))?;
                e.color = color;
                self.bump();
                Ok(json!({"ok": true}))
            }
            Command::Remove { id } => {
                if !self.remove(id) {
                    bail!("no entity {id}");
                }
                Ok(json!({"ok": true}))
            }
            Command::SetCamera { camera } => {
                self.scene.camera = camera;
                self.bump();
                Ok(json!({"ok": true}))
            }
            Command::SaveScene { path } => {
                self.save(Path::new(&path))?;
                Ok(json!({"ok": true, "path": path}))
            }
            Command::LoadScene { path } => {
                self.load(Path::new(&path))?;
                Ok(json!({"ok": true, "path": path}))
            }
        }
    }

    pub fn drain_queue(&mut self, queue: &mut CommandQueue) -> Vec<Result<Value>> {
        queue.drain().map(|c| self.apply(c)).collect()
    }

    pub fn scene_json(&self) -> Value {
        serde_json::to_value(&self.scene).unwrap_or_else(|_| json!({"error": "serialize"}))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let s = serde_json::to_string_pretty(&self.scene)?;
        std::fs::write(path, s).with_context(|| format!("write {}", path.display()))
    }

    pub fn load(&mut self, path: &Path) -> Result<()> {
        let s = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        self.scene = serde_json::from_str(&s).with_context(|| "parse scene json")?;
        self.next_id = self.scene.entities.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        self.bump();
        Ok(())
    }

    fn bump(&mut self) {
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::scene::PrimitiveKind;

    #[test]
    fn spawn_transform_remove() {
        let mut w = World::new();
        let id = w.spawn(
            "a",
            PrimitiveKind::Box { size: [1.0; 3] },
            Transform::default(),
            [1.0; 4],
        );
        assert_eq!(w.scene.entities.len(), 1);
        let mut t = Transform::default();
        t.position = [3.0, 0.0, 0.0];
        w.apply(Command::SetTransform { id, transform: t }).unwrap();
        assert_eq!(w.entity(id).unwrap().transform.position[0], 3.0);
        assert!(w.remove(id));
        assert!(!w.remove(id));
    }

    #[test]
    fn scene_roundtrip() {
        let mut w = World::new();
        w.spawn("floor", PrimitiveKind::Plane { size: 5.0 }, Transform::default(), [0.5; 4]);
        let dir = std::env::temp_dir().join("ai_engine_core_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scene.json");
        w.save(&p).unwrap();

        let mut w2 = World::new();
        w2.load(&p).unwrap();
        assert_eq!(w2.scene.entities.len(), 1);
        assert_eq!(w2.scene.entities[0].name, "floor");
        // next spawn must not collide with loaded id
        let id = w2.spawn("b", PrimitiveKind::Plane { size: 1.0 }, Transform::default(), [0.5; 4]);
        assert_eq!(id, 2);
    }
}
