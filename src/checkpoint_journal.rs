use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{io::Write, path::Path};

const COMPACT_AFTER: usize = 16;
#[derive(Default)]
pub(super) struct Cache {
    state: Option<Value>,
    sequence: u64,
    records: usize,
    legacy: bool,
    checksum: String,
}

struct Digest(u64);
impl Write for Digest {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn digest(state: &Value) -> Result<String> {
    let mut hash = Digest(0xcbf29ce484222325);
    serde_json::to_writer(&mut hash, state)?;
    Ok(format!("{:016x}", hash.0))
}
fn byte_digest(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}
fn checked_path(root: &Path, name: &str) -> Result<std::path::PathBuf> {
    ensure!(
        !root.join(".ai-engine").is_symlink() && !root.join(".ai-engine").join(name).is_symlink(),
        "checkpoint control paths must not be symlinks"
    );
    engine_core::project_path(root, &Path::new(".ai-engine").join(name))
}

fn diff(old: &Value, new: &Value, path: &mut Vec<String>, ops: &mut Vec<Value>, scene: &Value) {
    if old == new {
        return;
    }
    if path.as_slice() == ["world", "snapshots"] {
        if let (Some(a), Some(b)) = (old.as_array(), new.as_array()) {
            let dropped = a
                .iter()
                .position(|v| b.first().is_some_and(|first| first[0] == v[0]))
                .unwrap_or(a.len());
            let retained = a.len() - dropped;
            if b.len() == retained + 1
                && a[dropped..] == b[..retained]
                && b[retained]
                    .as_array()
                    .is_some_and(|v| v.len() == 2 && v[1] == *scene)
            {
                if dropped > 0 {
                    ops.push(
                        json!({"op":"splice","path":path,"index":0,"remove":dropped,"values":[]}),
                    );
                }
                for entry in &b[retained..] {
                    ops.push(json!({"op":"snapshot","revision":entry[0]}));
                }
                return;
            }
        }
    }
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().filter(|key| !b.contains_key(*key)) {
                path.push(key.clone());
                ops.push(json!({"op":"remove","path":path}));
                path.pop();
            }
            for (key, value) in b {
                path.push(key.clone());
                if let Some(old) = a.get(key) {
                    diff(old, value, path, ops, scene);
                } else {
                    ops.push(json!({"op":"set","path":path,"value":value}));
                }
                path.pop();
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (old, new)) in a.iter().zip(b).enumerate() {
                path.push(i.to_string());
                diff(old, new, path, ops, scene);
                path.pop();
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            let prefix = a.iter().zip(b).take_while(|(a, b)| a == b).count();
            let suffix = a[prefix..]
                .iter()
                .rev()
                .zip(b[prefix..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            ops.push(json!({"op":"splice","path":path,"index":prefix,"remove":a.len()-prefix-suffix,"values":&b[prefix..b.len()-suffix]}));
        }
        _ => ops.push(json!({"op":"set","path":path,"value":new})),
    }
}

fn at_mut<'a>(mut state: &'a mut Value, path: &[String]) -> Result<&'a mut Value> {
    for key in path {
        state = match state {
            Value::Array(a) => a.get_mut(key.parse::<usize>()?),
            Value::Object(o) => o.get_mut(key),
            _ => None,
        }
        .context("invalid delta path")?;
    }
    Ok(state)
}
fn apply(state: &mut Value, ops: &[Value]) -> Result<()> {
    for op in ops {
        if op["op"] == "snapshot" {
            let scene = state["world"]["scene"].clone();
            let revision = op["revision"]
                .as_u64()
                .context("invalid snapshot revision")?;
            ensure!(
                state["world"]["revision"] == revision,
                "snapshot revision mismatch"
            );
            at_mut(state, &["world".into(), "snapshots".into()])?
                .as_array_mut()
                .context("invalid snapshot ring")?
                .push(json!([revision, scene]));
            continue;
        }
        let path: Vec<String> = serde_json::from_value(op["path"].clone())?;
        match op["op"].as_str() {
            Some("set") | Some("remove") => {
                let (key, parent) = path.split_last().context("empty delta path")?;
                match at_mut(state, parent)? {
                    Value::Object(map) => {
                        if op["op"] == "remove" {
                            ensure!(map.remove(key).is_some(), "missing removed key");
                        } else {
                            ensure!(op.get("value").is_some(), "missing delta value");
                            map.insert(key.clone(), op["value"].clone());
                        }
                    }
                    Value::Array(array) => {
                        ensure!(op["op"] == "set", "array removal requires splice");
                        let slot = array
                            .get_mut(key.parse::<usize>()?)
                            .context("invalid array delta")?;
                        *slot = op.get("value").context("missing delta value")?.clone();
                    }
                    _ => anyhow::bail!("invalid delta parent"),
                }
            }
            Some("splice") => {
                let array = at_mut(state, &path)?
                    .as_array_mut()
                    .context("invalid splice target")?;
                let index = op["index"].as_u64().context("invalid splice index")?;
                let remove = op["remove"].as_u64().context("invalid splice count")?;
                ensure!(
                    index <= array.len() as u64 && remove <= array.len() as u64 - index,
                    "splice outside array"
                );
                array.splice(
                    index as usize..(index + remove) as usize,
                    op["values"]
                        .as_array()
                        .context("invalid splice values")?
                        .iter()
                        .cloned(),
                );
            }
            _ => anyhow::bail!("unknown checkpoint delta operation"),
        }
    }
    Ok(())
}

fn records(root: &Path) -> Result<Vec<(u64, std::path::PathBuf)>> {
    let dir = checked_path(root, "journal")?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    ensure!(
        !dir.is_symlink() && dir.is_dir(),
        "invalid checkpoint journal directory"
    );
    let mut result = vec![];
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid journal filename"))?;
        if name.ends_with(".tmp") {
            continue;
        }
        ensure!(
            entry.file_type()?.is_file()
                && name.len() == 25
                && name.ends_with(".json")
                && name.as_bytes()[..20].iter().all(|c| c.is_ascii_digit()),
            "invalid checkpoint journal entry"
        );
        result.push((name[..20].parse()?, entry.path()));
    }
    result.sort_by_key(|(sequence, _)| *sequence);
    Ok(result)
}

impl Cache {
    pub fn load(&mut self, root: &Path) -> Result<Option<&Value>> {
        let path = checked_path(root, "state.json")?;
        let mut entries = records(root)?;
        let head_path = checked_path(root, "head.json")?;
        let head = match std::fs::read(head_path) {
            Ok(bytes) => {
                let v: Value = serde_json::from_slice(&bytes).context("invalid checkpoint head")?;
                ensure!(
                    v["version"] == 1 && v["format"] == "ai-engine-checkpoint-head",
                    "invalid checkpoint head format"
                );
                Some((
                    v["sequence"]
                        .as_u64()
                        .context("invalid checkpoint head sequence")?,
                    v["checksum"]
                        .as_str()
                        .context("invalid checkpoint head checksum")?
                        .to_owned(),
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        ensure!(
            head.is_some() || !root.join(".ai-engine/journal").exists(),
            "checkpoint journal head is missing"
        );
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                ensure!(
                    entries.is_empty() && head.is_none(),
                    "journal without base checkpoint"
                );
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        let base: Value = serde_json::from_slice(&bytes)
            .context("invalid coordination checkpoint; refusing to reset project state")?;
        self.checksum = byte_digest(&bytes);
        self.legacy = base["version"] == 1;
        let mut state = if self.legacy {
            ensure!(
                entries.is_empty() && head.is_none(),
                "legacy checkpoint cannot have a delta journal"
            );
            self.sequence = 0;
            base
        } else {
            ensure!(
                base["version"] == 2 && base["format"] == "ai-engine-checkpoint",
                "unsupported coordination checkpoint version"
            );
            self.sequence = base["sequence"]
                .as_u64()
                .context("invalid checkpoint sequence")?;
            if let Some((seq, checksum)) = &head {
                if *seq < self.sequence {
                    ensure!(
                        base["previous_sequence"] == *seq && base["previous_checksum"] == *checksum,
                        "stale checkpoint head mismatch"
                    );
                }
            }
            base.get("state")
                .context("missing checkpoint state")?
                .clone()
        };
        let base_sequence = self.sequence;
        let committed = head
            .as_ref()
            .map(|h| h.0)
            .unwrap_or(self.sequence)
            .max(self.sequence);
        entries.retain(|(seq, _)| *seq > self.sequence && *seq <= committed);
        self.records = entries.len();
        ensure!(
            self.records < COMPACT_AFTER,
            "checkpoint compaction interval exceeded"
        );
        for (seq, path) in entries {
            ensure!(
                self.sequence.checked_add(1) == Some(seq),
                "checkpoint journal sequence gap"
            );
            let record: Value = serde_json::from_slice(&std::fs::read(path)?)
                .context("invalid checkpoint journal")?;
            ensure!(
                record["version"] == 2
                    && record["sequence"] == seq
                    && record["before"] == self.checksum,
                "checkpoint journal chain mismatch"
            );
            let payload = record["payload"]
                .as_str()
                .context("missing checkpoint payload")?;
            let checksum = byte_digest(payload.as_bytes());
            ensure!(
                record["after"] == checksum,
                "checkpoint payload checksum mismatch"
            );
            let ops: Vec<Value> = serde_json::from_str(payload)?;
            apply(&mut state, &ops)?;
            self.checksum = checksum;
            self.sequence = seq;
        }
        ensure!(
            self.sequence == committed,
            "checkpoint journal tail is missing"
        );
        if let Some((seq, checksum)) = head {
            if seq >= base_sequence {
                ensure!(
                    self.checksum == checksum,
                    "checkpoint head checksum mismatch"
                );
            }
        }
        self.state = Some(state);
        Ok(self.state.as_ref())
    }

    pub fn save(&mut self, root: &Path, state: Value) -> Result<()> {
        let base = checked_path(root, "state.json")?;
        checked_path(root, "head.json")?;
        checked_path(root, "journal")?;
        let sequence = self
            .sequence
            .checked_add(1)
            .context("checkpoint sequence exhausted")?;
        let mut ops = vec![];
        if let Some(old) = &self.state {
            ensure!(base.is_file(), "checkpoint base is missing or not a file");
            diff(old, &state, &mut vec![], &mut ops, &state["world"]["scene"]);
        }
        let compact = self.state.is_none() || self.legacy || self.records + 1 >= COMPACT_AFTER;
        if compact {
            let base = json!({"format":"ai-engine-checkpoint","version":2,"sequence":sequence,"previous_sequence":self.sequence,"previous_checksum":self.checksum,"state":state});
            let checksum = digest(&base)?;
            super::atomic_write(root, "state.json", &base)?;
            self.checksum = checksum;
            self.records = 0;
            self.legacy = false;
            if let Ok(entries) = records(root) {
                for (seq, path) in entries {
                    if seq <= sequence {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        } else {
            if !root.join(".ai-engine/head.json").exists() {
                super::atomic_write(
                    root,
                    "head.json",
                    &json!({"format":"ai-engine-checkpoint-head","version":1,"sequence":self.sequence,"checksum":self.checksum}),
                )?;
            }
            let dir = checked_path(root, "journal")?;
            std::fs::create_dir_all(&dir)?;
            let name = format!("journal/{sequence:020}.json");
            let orphan = root.join(".ai-engine").join(&name);
            if let Ok(metadata) = std::fs::symlink_metadata(&orphan) {
                ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "invalid uncommitted journal entry"
                );
                std::fs::remove_file(orphan)?;
            }
            let payload = serde_json::to_string(&ops)?;
            let checksum = byte_digest(payload.as_bytes());
            let record = json!({"version":2,"sequence":sequence,"before":self.checksum,"after":checksum,"payload":payload});
            super::atomic_write(root, &name, &record)?;
            super::atomic_write(
                root,
                "head.json",
                &json!({"format":"ai-engine-checkpoint-head","version":1,"sequence":sequence,"checksum":checksum}),
            )?;
            self.checksum = checksum;
            self.records += 1;
        }
        self.state = Some(state);
        self.sequence = sequence;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_integrity_is_independent_of_float_parser_roundoff() {
        let values: Vec<_> = (1..100).map(|n| json!(n as f32 / 100.0)).collect();
        for value in values {
            let wire = serde_json::to_string(&value).unwrap();
            let restored: Value = serde_json::from_str(&wire).unwrap();
            assert_eq!(
                value.as_f64().unwrap() as f32,
                restored.as_f64().unwrap() as f32
            );
            let envelope = json!({"payload":wire,"checksum":byte_digest(wire.as_bytes())});
            let restored: Value = serde_json::from_str(&envelope.to_string()).unwrap();
            assert_eq!(restored["payload"], wire);
            assert_eq!(
                restored["checksum"],
                byte_digest(restored["payload"].as_str().unwrap().as_bytes())
            );
            let mut ops = vec![];
            diff(
                &json!({"a":0}),
                &json!({"a":value}),
                &mut vec![],
                &mut ops,
                &Value::Null,
            );
            let mut state = json!({"a":0});
            apply(&mut state, &ops).unwrap();
            assert_eq!(state, json!({"a":value}));
        }
    }
    #[test]
    fn patches_preserve_arrays_objects_and_empty_values() {
        for (old, new) in [
            (json!({"a":[1,2,3],"remove":0}), json!({"a":[1,3],"new":[]})),
            (json!({"a":[1,2]}), json!({"a":[1,4,2]})),
            (json!({"a":{}}), json!({"a":[null]})),
        ] {
            let mut ops = vec![];
            diff(&old, &new, &mut vec![], &mut ops, &new["world"]["scene"]);
            let mut restored = old;
            apply(&mut restored, &ops).unwrap();
            assert_eq!(restored, new);
        }
        assert!(apply(
            &mut json!({}),
            &[json!({"op":"splice","path":[],"index":0,"remove":1,"values":[]})]
        )
        .is_err());
    }
}
