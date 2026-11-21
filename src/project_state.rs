use crate::{swarm::Swarm, Engine};
use anyhow::{bail, ensure, Context, Result};
use engine_core::{project_path, World};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[path = "checkpoint_journal.rs"]
mod journal;

pub(crate) struct Storage {
    root: PathBuf,
    _writer: File,
    journal: std::sync::Mutex<journal::Cache>,
}

impl Drop for Storage {
    fn drop(&mut self) {
        let _ = self._writer.unlock();
    }
}

#[cfg(test)]
mod storage_tests {
    #[test]
    fn drop_releases_writer_even_with_an_outliving_duplicate_descriptor() {
        let root=std::env::temp_dir().join(format!("storage-drop-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let storage=super::Storage::open(&root).unwrap();
        let duplicate=storage._writer.try_clone().unwrap();
        assert!(super::Storage::open(&root).is_err());
        drop(storage);
        let next=super::Storage::open(&root).unwrap();
        drop(next);
        drop(duplicate);
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn metadata_dir(root: &Path) -> Result<PathBuf> {
    let dir = project_path(root, Path::new(".ai-engine"))?;
    std::fs::create_dir_all(&dir)?;
    let ignore = project_path(root, &dir.join(".gitignore"))?;
    if !ignore.exists() {
        std::fs::write(ignore, "*\n")?;
    }
    Ok(dir)
}

fn atomic_write(root: &Path, name: &str, value: &Value) -> Result<()> {
    let destination = project_path(root, &Path::new(".ai-engine").join(name))?;
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce)
        .map_err(|_| anyhow::anyhow!("checkpoint nonce generation failed"))?;
    let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let temporary = project_path(
        root,
        &Path::new(".ai-engine").join(format!("{name}.{suffix}.tmp")),
    )?;
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(value)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, &destination)?;
        #[cfg(unix)]
        let _ =
            File::open(destination.parent().unwrap()).and_then(|directory| directory.sync_all());
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.with_context(|| format!("persist project {}", destination.display()))
}

pub(crate) fn configuration(root: &Path) -> Result<(String, u16)> {
    metadata_dir(root)?;
    let identity = root.to_string_lossy().into_owned();
    let path = project_path(root, Path::new(".ai-engine/config.json"))?;
    let config: Value = if path.exists() {
        serde_json::from_slice(&std::fs::read(&path)?).context("invalid project config")?
    } else {
        let hash = identity.bytes().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        let config = json!({"version":1,"project_id":identity,"blender_port":20000+hash%40000});
        atomic_write(root, "config.json", &config)?;
        config
    };
    ensure!(config["version"] == 1, "unsupported project config version");
    ensure!(config["project_id"].as_str() == Some(&identity), "project config identity mismatch; copied/moved projects must start with a fresh .ai-engine directory");
    let port = config["blender_port"]
        .as_u64()
        .filter(|p| (1..=65535).contains(p))
        .context("blender_port must be an integer in 1..=65535")? as u16;
    let port = match std::env::var("AI_ENGINE_BLENDER_PORT") {
        Ok(value) => value
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .context("AI_ENGINE_BLENDER_PORT must be in 1..=65535")?,
        Err(std::env::VarError::NotPresent) => port,
        Err(error) => bail!("invalid AI_ENGINE_BLENDER_PORT: {error}"),
    };
    Ok((identity, port))
}

fn milliseconds() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}

fn integer(value: &Value, key: &str) -> Result<u64> {
    value[key]
        .as_u64()
        .with_context(|| format!("invalid persisted {key}"))
}

impl Storage {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        metadata_dir(root)?;
        let path = project_path(root, Path::new(".ai-engine/writer.lock"))?;
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        writer
            .try_lock()
            .context("project already has a coordination writer; stop the other engine first")?;
        Ok(Self {
            root: root.into(),
            _writer: writer,
            journal: Default::default(),
        })
    }

    pub(crate) fn restore(&self) -> Result<Option<(World, Swarm)>> {
        let mut journal = self.journal.lock().map_err(|_|anyhow::anyhow!("checkpoint cache poisoned"))?;
        let Some(state) = journal.load(&self.root)? else {return Ok(None)};
        ensure!(
            state["version"] == 1,
            "unsupported coordination checkpoint version"
        );
        ensure!(
            state["project_id"].as_str() == self.root.to_str(),
            "coordination checkpoint project mismatch"
        );
        let world = World::from_checkpoint(&state["world"], &self.root)?;
        let saved = integer(&state, "saved_at_ms")?;
        let now = milliseconds()?.max(saved);
        let instant = Instant::now();
        let deadline = |entry: &Value| -> Result<Instant> {
            let expiry = integer(entry, "expires_at_ms")?;
            ensure!(
                expiry.saturating_sub(saved) <= 86_400_000,
                "persisted expiry exceeds maximum TTL"
            );
            instant
                .checked_add(Duration::from_millis(expiry.saturating_sub(now)))
                .context("invalid persisted expiry")
        };
        let mut swarm = Swarm::default();
        swarm.tasks = serde_json::from_value(state["tasks"].clone())?;
        for (i, task) in swarm.tasks.iter().enumerate() {
            ensure!(task["id"] == (i + 1) as u64, "invalid persisted task ID");
            ensure!(
                integer(task, "lease")? < u64::MAX,
                "persisted task lease exhausted"
            );
            ensure!(
                task["title"].as_str().is_some_and(|s| !s.trim().is_empty())
                    && task["description"].is_string(),
                "invalid persisted task text"
            );
            ensure!(
                ["pending", "claimed", "completed"]
                    .iter()
                    .any(|s| task["status"] == *s),
                "invalid persisted task status"
            );
            ensure!(
                if task["status"] == "pending" {
                    task["agent"].is_null()
                } else {
                    task["agent"].as_str().is_some_and(|s| !s.is_empty())
                        && task["lease"].as_u64().unwrap() > 0
                },
                "invalid persisted task owner/lease"
            );
            ensure!(
                task["depends_on"]
                    .as_array()
                    .context("invalid persisted dependencies")?
                    .iter()
                    .all(|v| v.as_u64().is_some_and(|id| id > 0 && id < (i + 1) as u64)),
                "invalid persisted dependencies"
            );
        }
        for lease in state["task_leases"]
            .as_array()
            .context("invalid persisted leases")?
        {
            let id = integer(lease, "id")?;
            ensure!(
                swarm
                    .tasks
                    .iter()
                    .any(|t| t["id"] == id && t["status"] == "claimed"),
                "lease refers to unclaimed task"
            );
            ensure!(
                swarm.task_leases.insert(id, deadline(lease)?).is_none(),
                "duplicate persisted lease"
            );
        }
        ensure!(
            swarm
                .tasks
                .iter()
                .filter(|t| t["status"] == "claimed")
                .all(|t| swarm.task_leases.contains_key(&t["id"].as_u64().unwrap())),
            "missing persisted lease"
        );
        for lock in state["locks"]
            .as_array()
            .context("invalid persisted locks")?
        {
            let owner = lock["agent"]
                .as_str()
                .filter(|s| !s.is_empty())
                .context("invalid persisted lock owner")?;
            ensure!(
                swarm
                    .locks
                    .insert(integer(lock, "id")?, (owner.into(), deadline(lock)?))
                    .is_none(),
                "duplicate persisted lock"
            );
        }
        swarm.journal = serde_json::from_value(state["scene_journal"].clone())?;
        swarm.task_journal = serde_json::from_value(state["task_journal"].clone())?;
        swarm.task_sequence = integer(&state, "task_sequence")?;
        ensure!(
            swarm.task_sequence < u64::MAX,
            "persisted task sequence exhausted"
        );
        for (entries, key, maximum) in [
            (&swarm.journal, "revision", world.revision),
            (&swarm.task_journal, "sequence", swarm.task_sequence),
        ] {
            ensure!(entries.len() <= 1024, "persisted journal limit exceeded");
            let mut previous = 0;
            for entry in entries {
                let cursor = integer(entry, key)?;
                ensure!(
                    cursor > previous && cursor <= maximum,
                    "invalid persisted journal cursor"
                );
                previous = cursor;
            }
        }
        ensure!(
            swarm
                .task_journal
                .back()
                .map(|e| e["sequence"].as_u64().unwrap())
                .unwrap_or(0)
                == swarm.task_sequence,
            "task journal sequence mismatch"
        );
        Ok(Some((world, swarm)))
    }

    pub(crate) fn save(&self, world: &World, swarm: &Swarm) -> Result<()> {
        let wall = milliseconds()?;
        let instant = Instant::now();
        let deadline =
            |expiry: Instant| wall + expiry.saturating_duration_since(instant).as_millis() as u64;
        self.journal.lock().map_err(|_|anyhow::anyhow!("checkpoint cache poisoned"))?.save(
            &self.root,
            json!({"version":1,"project_id":self.root,"saved_at_ms":wall,
                "world":world.checkpoint(),"tasks":swarm.tasks,"task_sequence":swarm.task_sequence,
                "task_journal":swarm.task_journal,"scene_journal":swarm.journal,
                "locks":swarm.locks.iter().map(|(id,(agent,expiry))|json!({"id":id,"agent":agent,"expires_at_ms":deadline(*expiry)})).collect::<Vec<_>>(),
                "task_leases":swarm.task_leases.iter().map(|(id,expiry)|json!({"id":id,"expires_at_ms":deadline(*expiry)})).collect::<Vec<_>>()
            }),
        )
    }
}

impl Engine {
    pub(crate) fn durable_handle(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        if self.coordination_depth > 0
            || self.storage.is_none()
            || !matches!(
                name,
                "claim_entities"
                    | "nav_bake"
                    | "prefab_create" | "prefab_update" | "prefab_instantiate" | "prefab_set_overrides" | "prefab_remove_instance"
                    | "release_entities"
                    | "task_create"
                    | "task_claim"
                    | "task_renew"
                    | "task_release"
                    | "task_complete"
                    | "spawn_primitive"
                    | "set_transform"
                    | "set_color"
                    | "remove_entity"
                    | "set_camera"
                    | "load_scene"
                    | "rollback"
                    | "blender_sync"
                    | "write_script"
                    | "apply_batch"
            )
        {
            return self.handle_agent(name, args);
        }
        let world = self.world.clone();
        let swarm = self.swarm.clone();
        self.coordination_depth += 1;
        let result = self.handle_agent(name, args);
        self.coordination_depth -= 1;
        if self.world.revision != world.revision || self.swarm != swarm {
            if let Err(error) = self
                .storage
                .as_ref()
                .unwrap()
                .save(&self.world, &self.swarm)
            {
                self.world = world;
                self.swarm = swarm;
                return Err(format!(
                    "coordination checkpoint failed; change was not acknowledged: {error:#}"
                ));
            }
        }
        result
    }
}
