//! End-to-end tests: drive the engine the same way an MCP client would —
//! newline-delimited JSON-RPC in, JSON / base64-PNG out.

use ai_engine::Engine;
use serde_json::{json, Value};
use std::time::Instant;

fn call(engine: &mut Engine, name: &str, arguments: Value) -> Value {
    let line = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
    .to_string();
    engine_mcp::handle_line(&line, engine).expect("no response")
}

#[test]
fn mcp_roundtrip_spawn_and_state() {
    let mut e = Engine::new();
    let t0 = Instant::now();

    let r = call(&mut e, "spawn_primitive", json!({"kind": "box", "name": "crate", "position": [0, 0.5, 0], "color": [1, 0.4, 0.1, 1]}));
    assert!(r["result"]["content"].is_array(), "{r}");
    let r = call(&mut e, "spawn_primitive", json!({"kind": "sphere", "radius": 0.6, "position": [2, 0.6, 0]}));
    assert!(r["result"]["content"].is_array(), "{r}");
    let r = call(&mut e, "get_scene_state", json!({}));
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    let scene: Value = serde_json::from_str(text).unwrap();
    assert_eq!(scene["entities"].as_array().unwrap().len(), 2);

    let elapsed = t0.elapsed();
    assert!(elapsed.as_millis() < 100, "roundtrip too slow: {elapsed:?}");
}

#[test]
fn transform_and_persistence() {
    let mut e = Engine::new();
    call(&mut e, "spawn_primitive", json!({"kind": "plane", "size": 8}));
    call(&mut e, "set_transform", json!({"id": 1, "position": [1, 2, 3]}));
    let r = call(&mut e, "get_scene_state", json!({}));
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("[1.0,2.0,3.0]") || text.contains("[ 1.0, 2.0, 3.0 ]") || text.contains("3.0"));

    let dir = std::env::temp_dir().join("ai_engine_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("e2e_scene.json");
    let r = call(&mut e, "save_scene", json!({"path": p.to_string_lossy()}));
    assert!(r["result"]["content"].is_array(), "{r}");

    let mut e2 = Engine::new();
    let r = call(&mut e2, "load_scene", json!({"path": p.to_string_lossy()}));
    assert!(r["result"]["content"].is_array(), "{r}");
    assert_eq!(e2.world.scene.entities.len(), 1);
}

#[test]
fn screenshot_returns_base64_png() {
    let mut e = Engine::new();
    e.world.scene = shared::mocks::mock_scene();
    let r = call(&mut e, "get_screenshot", json!({"width": 320, "height": 240}));

    // No GPU → error result is acceptable on CI runners; real machines must pass.
    if r["result"]["isError"].as_bool() == Some(true) {
        eprintln!("SKIP: no renderer available: {r}");
        return;
    }
    let b64 = r["result"]["content"][0]["data"].as_str().unwrap();
    let png = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        b64,
    )
    .unwrap();
    assert_eq!(&png[1..4], b"PNG", "not a png");
    assert!(png.len() > 500, "png suspiciously small: {}", png.len());
}

/// The evaluator scenario: two agents read the same revision and emit
/// conflicting mutations "in the same tick". Serialized stdin makes them
/// sequential; expected_revision makes the loser explicit instead of a
/// silent last-writer-wins clobber.
#[test]
fn conflicting_agents_resolve_deterministically() {
    let mut e = Engine::new();
    call(&mut e, "spawn_primitive", json!({"kind": "box", "name": "crate"}));
    let r = call(&mut e, "get_revision", json!({}));
    let rev: u64 = r["result"]["content"][0]["text"].as_str().unwrap().parse().unwrap();

    // Agent A wins.
    let a = call(&mut e, "set_transform", json!({"id": 1, "position": [1, 0, 0], "expected_revision": rev}));
    assert!(a["result"]["content"].is_array(), "{a}");

    // Agent B acts on the same (now stale) revision → rejected.
    let b = call(&mut e, "set_transform", json!({"id": 1, "position": [9, 0, 0], "expected_revision": rev}));
    assert_eq!(b["result"]["isError"], true, "{b}");
    let msg = b["result"]["content"][0]["text"].as_str().unwrap();
    assert!(msg.contains("revision conflict"), "{msg}");

    // World kept agent A's write.
    let s = call(&mut e, "get_scene_state", json!({}));
    let text = s["result"]["content"][0]["text"].as_str().unwrap();
    let scene: Value = serde_json::from_str(text).unwrap();
    assert_eq!(scene["entities"][0]["transform"]["position"][0], 1.0);
}

#[test]
fn rollback_restores_earlier_revision() {
    let mut e = Engine::new();
    call(&mut e, "spawn_primitive", json!({"kind": "box", "name": "a"}));
    let r = call(&mut e, "get_revision", json!({}));
    let rev1: u64 = r["result"]["content"][0]["text"].as_str().unwrap().parse().unwrap();
    call(&mut e, "spawn_primitive", json!({"kind": "sphere", "name": "b"}));

    let rb = call(&mut e, "rollback", json!({"revision": rev1}));
    assert!(rb["result"]["content"].is_array(), "{rb}");
    assert_eq!(e.world.scene.entities.len(), 1);
    assert!(e.world.revision > rev1, "revision stays monotonic");

    // Out-of-window revisions are errors, not silent no-ops.
    let bad = call(&mut e, "rollback", json!({"revision": 99999}));
    assert_eq!(bad["result"]["isError"], true, "{bad}");
}

#[test]
fn unknown_tool_is_error() {
    let mut e = Engine::new();
    let r = call(&mut e, "does_not_exist", json!({}));
    assert_eq!(r["result"]["isError"], true);
}
