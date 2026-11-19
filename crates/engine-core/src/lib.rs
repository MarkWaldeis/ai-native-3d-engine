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
    /// Restore the scene to an earlier revision (snapshot ring).
    Rollback {
        revision: u64,
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
    /// Bumped on every mutation — the renderer uses it as a dirty flag and
    /// callers use it as an optimistic-concurrency token (`apply_expected`).
    pub revision: u64,
    /// Ring of per-revision snapshots: (revision, scene_at_that_revision).
    /// Written on every mutation — a complete deterministic history.
    snapshots: VecDeque<(u64, Scene)>,
}

/// Max retained snapshots — older revisions fall out of the rollback window.
pub const SNAPSHOT_CAP: usize = 64;

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
            snapshots: VecDeque::new(),
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

    /// Optimistic-concurrency gate: apply `cmd` only if the caller's view of
    /// the world is still current. When two agents both read revision R and
    /// both send a mutation `expected_revision = R`, the first wins and the
    /// second is rejected deterministically — it must re-read and retry.
    pub fn apply_expected(&mut self, expected_revision: u64, cmd: Command) -> Result<Value> {
        if self.revision != expected_revision {
            bail!(
                "revision conflict: caller expected {expected_revision}, world is at {} — re-read scene state and retry",
                self.revision
            );
        }
        self.apply(cmd)
    }

    /// Restore the scene captured at `revision`. The restore itself bumps the
    /// revision (kept monotonic), so stale `expected_revision` holders still
    /// conflict afterwards instead of resurrecting abandoned states.
    pub fn rollback_to(&mut self, revision: u64) -> Result<()> {
        let pos = self
            .snapshots
            .iter()
            .position(|(r, _)| *r == revision)
            .with_context(|| {
                let oldest = self.snapshots.front().map(|(r, _)| *r);
                match oldest {
                    Some(o) => format!("revision {revision} not in snapshot window [{o}..{}]", self.revision),
                    None => format!("revision {revision} not in snapshot window (empty)"),
                }
            })?;
        self.scene = self.snapshots[pos].1.clone();
        self.bump();
        Ok(())
    }

    /// Scene revision captured by the oldest retained snapshot (rollback floor).
    pub fn oldest_snapshot_revision(&self) -> Option<u64> {
        self.snapshots.front().map(|(r, _)| *r)
    }

    fn push_snapshot(&mut self) {
        self.snapshots.push_back((self.revision, self.scene.clone()));
        while self.snapshots.len() > SNAPSHOT_CAP {
            self.snapshots.pop_front();
        }
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
                Ok(json!({"id": id, "revision": self.revision}))
            }
            Command::SetTransform { id, transform } => {
                let e = self.entity_mut(id).context(format!("no entity {id}"))?;
                e.transform = transform;
                self.bump();
                Ok(json!({"ok": true, "revision": self.revision}))
            }
            Command::SetColor { id, color } => {
                let e = self.entity_mut(id).context(format!("no entity {id}"))?;
                e.color = color;
                self.bump();
                Ok(json!({"ok": true, "revision": self.revision}))
            }
            Command::Remove { id } => {
                if !self.remove(id) {
                    bail!("no entity {id}");
                }
                Ok(json!({"ok": true, "revision": self.revision}))
            }
            Command::SetCamera { camera } => {
                self.scene.camera = camera;
                self.bump();
                Ok(json!({"ok": true, "revision": self.revision}))
            }
            Command::SaveScene { path } => {
                self.save(Path::new(&path))?;
                Ok(json!({"ok": true, "path": path}))
            }
            Command::LoadScene { path } => {
                self.load(Path::new(&path))?;
                Ok(json!({"ok": true, "path": path, "revision": self.revision}))
            }
            Command::Rollback { revision } => {
                self.rollback_to(revision)?;
                Ok(json!({"ok": true, "revision": self.revision}))
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
        // Every mutation records the resulting scene keyed by its new
        // revision — the snapshot ring is a complete, deterministic history.
        self.push_snapshot();
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

    #[test]
    fn expected_revision_serializes_conflicting_writers() {
        // Two agents both read revision R and both try to mutate: first wins,
        // second is rejected and must re-read.
        let mut w = World::new();
        let id = w.spawn("a", PrimitiveKind::Box { size: [1.0; 3] }, Transform::default(), [1.0; 4]);
        let r = w.revision;

        let mut t1 = Transform::default();
        t1.position = [1.0, 0.0, 0.0];
        w.apply_expected(r, Command::SetTransform { id, transform: t1 }).unwrap();

        let mut t2 = Transform::default();
        t2.position = [9.0, 0.0, 0.0];
        let err = w
            .apply_expected(r, Command::SetTransform { id, transform: t2 })
            .unwrap_err();
        assert!(err.to_string().contains("revision conflict"), "{err}");
        assert_eq!(w.entity(id).unwrap().transform.position[0], 1.0);

        // retry with the fresh revision succeeds
        w.apply_expected(w.revision, Command::SetTransform { id, transform: t2 }).unwrap();
        assert_eq!(w.entity(id).unwrap().transform.position[0], 9.0);
    }

    #[test]
    fn rollback_restores_scene_and_keeps_revision_monotonic() {
        let mut w = World::new();
        w.spawn("a", PrimitiveKind::Box { size: [1.0; 3] }, Transform::default(), [1.0; 4]);
        let r1 = w.revision;
        w.spawn("b", PrimitiveKind::Sphere { radius: 1.0, segments: 8 }, Transform::default(), [1.0; 4]);
        assert_eq!(w.scene.entities.len(), 2);

        w.apply(Command::Rollback { revision: r1 }).unwrap();
        assert_eq!(w.scene.entities.len(), 1);
        assert_eq!(w.scene.entities[0].name, "a");
        assert!(w.revision > r1, "revision stays monotonic after rollback");

        // rollback is itself snapshot/undo-able
        w.apply(Command::Rollback { revision: w.revision - 1 }).unwrap();
        assert_eq!(w.scene.entities.len(), 2);

        // revisions outside the window error instead of silently passing
        assert!(w.apply(Command::Rollback { revision: 9999 }).is_err());
    }
}
