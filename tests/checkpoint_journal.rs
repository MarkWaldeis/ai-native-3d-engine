use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "checkpoint-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        ai_engine::project::initialize(&path, "empty").unwrap();
        let entities: Vec<_> = (1..=100)
            .map(|id| json!({"id":id,"name":format!("box-{id}"),"kind":"box","size":[1,1,1]}))
            .collect();
        std::fs::write(
            path.join("scene.json"),
            json!({"entities":entities}).to_string(),
        )
        .unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(".ai-engine").join(name)
    }
    fn records(&self) -> Vec<PathBuf> {
        let mut entries: Vec<_> = std::fs::read_dir(self.file("journal"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|s| s == "json"))
            .collect();
        entries.sort();
        entries
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn color(engine: &mut Engine, n: u32) {
    engine
        .handle(
            "set_color",
            &json!({"id":100,"color":[n as f32/100.0,0.5,0.4,1.0]}),
        )
        .unwrap();
}

#[test]
fn delta_recovery_compaction_and_snapshot_rollover() {
    let p = Project::new();
    let mut e = Engine::from_project(&p.0).unwrap();
    let base = std::fs::read(p.file("state.json")).unwrap();
    let base_bytes = base.len();
    color(&mut e, 1);
    assert_eq!(std::fs::read(p.file("state.json")).unwrap(), base);
    let size = std::fs::metadata(p.records().last().unwrap())
        .unwrap()
        .len();
    assert!(
        size < base.len() as u64 / 4,
        "delta={size}, base={}",
        base.len()
    );
    let mut compacted = 0;
    let mut previous = base;
    for n in 2..=82 {
        color(&mut e, n);
        let current = std::fs::read(p.file("state.json")).unwrap();
        if current != previous {
            compacted += 1;
        }
        previous = current;
        if n % 9 == 0 {
            let expected = e.world.checkpoint();
            drop(e);
            e = Engine::from_project(&p.0).unwrap();
            assert_eq!(e.world.checkpoint(), expected);
        }
    }
    assert!(compacted >= 5);
    assert!(p.records().len() < 16);
    let expected = e.world.checkpoint();
    drop(e);
    let e = Engine::from_project(&p.0).unwrap();
    assert_eq!(e.world.checkpoint(), expected);
    println!("PASS delta checkpoints: replay across 82 edits, snapshot rollover and {compacted} compactions; first delta {size} bytes vs base {base_bytes} bytes");
}

#[test]
fn legacy_migration_and_uncommitted_tail() {
    let p = Project::new();
    let e = Engine::from_project(&p.0).unwrap();
    drop(e);
    let envelope: Value =
        serde_json::from_slice(&std::fs::read(p.file("state.json")).unwrap()).unwrap();
    std::fs::write(p.file("state.json"), envelope["state"].to_string()).unwrap();
    let mut e = Engine::from_project(&p.0).unwrap();
    color(&mut e, 1);
    drop(e);
    let envelope: Value =
        serde_json::from_slice(&std::fs::read(p.file("state.json")).unwrap()).unwrap();
    assert_eq!(envelope["version"], 2);
    let mut e = Engine::from_project(&p.0).unwrap();
    color(&mut e, 2);
    let expected = e.world.checkpoint();
    std::fs::rename(p.file("head.json"), p.file("head.backup")).unwrap();
    std::fs::create_dir(p.file("head.json")).unwrap();
    assert!(e
        .handle("set_color", &json!({"id":100,"color":[1,1,1,1]}))
        .is_err());
    assert_eq!(e.world.checkpoint(), expected);
    drop(e);
    std::fs::remove_dir(p.file("head.json")).unwrap();
    std::fs::rename(p.file("head.backup"), p.file("head.json")).unwrap();
    std::fs::write(p.file("journal/interrupted.tmp"), "{").unwrap();
    let mut e = Engine::from_project(&p.0).unwrap();
    assert_eq!(e.world.checkpoint(), expected);
    color(&mut e, 3);
    drop(e);
    assert!(Engine::from_project(&p.0).is_ok());
    println!("PASS legacy migration, failed head commit rolls back memory, orphan tail ignored/replaced and torn temp ignored");
}

#[test]
fn corrupt_and_missing_committed_records_fail_closed() {
    for corruption in ["corrupt", "missing", "missing-head", "hash", "unicode-name"] {
        let p = Project::new();
        let mut e = Engine::from_project(&p.0).unwrap();
        color(&mut e, 1);
        drop(e);
        let path = p.records().pop().unwrap();
        match corruption {
            "corrupt" => std::fs::write(&path, "{").unwrap(),
            "missing" => std::fs::remove_file(&path).unwrap(),
            "missing-head" => std::fs::remove_file(p.file("head.json")).unwrap(),
            "unicode-name" => {
                std::fs::write(p.file("journal/0000000000000000000éjson"), "{}").unwrap();
            }
            _ => {
                let mut record: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                record["after"] = json!("corrupt");
                std::fs::write(&path, record.to_string()).unwrap();
            }
        }
        assert!(Engine::from_project(&p.0).is_err(), "accepted {corruption}");
    }
    println!("PASS malformed record, missing committed tail/head and mismatched hash fail closed");
}
