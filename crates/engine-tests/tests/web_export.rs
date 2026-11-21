use ai_engine::{web::export_bundle, Engine};
use serde_json::{json, Value};
use shared::scene::Scene;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "engine-web-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(p.join("pkg")).unwrap();
        std::fs::write(
            p.join("pkg/engine_web.js"),
            "export default function init() {};",
        )
        .unwrap();
        std::fs::write(p.join("pkg/engine_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn web_export_writes_offline_bundle_scripts_and_portable_glb_bounds() {
    let fixture = Fixture::new();
    let root = root();
    let mut scene: Scene = serde_json::from_str(
        &std::fs::read_to_string(root.join("examples/coin_game/scene.json")).unwrap(),
    )
    .unwrap();
    scene.entities.push(serde_json::from_value(json!({"id":99,"name":"model","kind":"gltf","path":root.join("assets/suzanne.glb"),"collider":{"shape":"mesh"}})).unwrap());
    let original = scene.clone();
    let out = fixture.0.join("site");
    let report = export_bundle(&scene, &root, &out, &fixture.0.join("pkg")).unwrap();
    assert_eq!(scene, original);
    assert_eq!(report["assets"], 1);
    assert_eq!(report["scripts"], 2);
    for path in [
        "index.html",
        "player.js",
        "streaming.js",
        "world.js",
        "effects.js",
        "presentation.js",
        "graphics-settings.js",
        "hdr.js",
        "sky.js",
        "materials.js",
        "style.css",
        "scene.json",
        "scripts.json",
        "scripts/0.rhai",
        "pkg/engine_web.js",
        "pkg/engine_web_bg.wasm",
        "vendor/three.module.js",
        "vendor/LICENSE.three",
        "assets/0.glb",
    ] {
        assert!(out.join(path).is_file(), "missing {path}");
    }
    let scene: Scene =
        serde_json::from_slice(&std::fs::read(out.join("scene.json")).unwrap()).unwrap();
    assert_eq!(
        scene.entities.last().unwrap().props["web_mesh_bounds"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let scripts: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&std::fs::read(out.join("scripts.json")).unwrap()).unwrap();
    let mut lateral =
        engine_runtime::Simulation::from_sources(scene.clone(), scripts.clone()).unwrap();
    lateral
        .step(
            30,
            &[engine_runtime::Input {
                tick: 0,
                action: "move_z".into(),
                value: 1.0,
            }],
        )
        .unwrap();
    let position = lateral
        .scene()
        .entities
        .iter()
        .find(|e| e.name == "player")
        .unwrap()
        .transform
        .position;
    assert!((position[2] - 1.0).abs() < 0.01);
    assert_eq!(position[0], -3.0);
    let mut sim = engine_runtime::Simulation::from_sources(scene, scripts).unwrap();
    sim.step(
        180,
        &[engine_runtime::Input {
            tick: 0,
            action: "move_x".into(),
            value: 1.0,
        }],
    )
    .unwrap();
    assert_eq!(sim.state()["state"]["score"], 3);
    assert_eq!(sim.state()["state"]["won"], true);
    assert!(
        export_bundle(&original, &root, &out, &fixture.0.join("pkg"))
            .unwrap_err()
            .to_string()
            .contains("empty")
    );
}

#[test]
fn web_export_empty_scene_and_missing_sources_are_safe() {
    let fixture = Fixture::new();
    let root = root();
    export_bundle(
        &Scene::default(),
        &root,
        &fixture.0.join("empty"),
        &fixture.0.join("pkg"),
    )
    .unwrap();
    let scene:Scene=serde_json::from_value(json!({"entities":[{"id":1,"name":"bad","kind":"box","size":[1,1,1],"script":"missing-web.rhai"}]})).unwrap();
    assert!(export_bundle(
        &scene,
        &root,
        &fixture.0.join("bad"),
        &fixture.0.join("pkg")
    )
    .is_err());
    assert!(!fixture.0.join("bad").exists());
    let scene: Scene = serde_json::from_value(
        json!({"entities":[{"id":1,"name":"bad","kind":"gltf","path":"missing-web.glb"}]}),
    )
    .unwrap();
    assert!(export_bundle(
        &scene,
        &root,
        &fixture.0.join("bad-asset"),
        &fixture.0.join("pkg")
    )
    .is_err());
    assert!(!fixture.0.join("bad-asset").exists());
    let mut engine = Engine::new();
    let result=engine_mcp::handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"export_web","arguments":{}}}).to_string(),&mut engine).unwrap();
    assert_eq!(result["result"]["isError"], true);
    let tools: Value = engine_mcp::handle_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        &mut engine,
    )
    .unwrap();
    assert!(tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "export_web"));
}

#[test]
fn web_export_bundles_ai_viewmodel_glb() {
    let fixture = Fixture::new();
    let root = root();
    let scene: Scene = serde_json::from_value(json!({"entities":[
        {"id":1,"name":"player","kind":"box","size":[1,1,1],"tags":["player"],"props":{"ai_fps":{},"ai_viewmodel":"assets/suzanne.glb"}}
    ]})).unwrap();
    let out = fixture.0.join("site");
    let report = export_bundle(&scene, &root, &out, &fixture.0.join("pkg")).unwrap();
    assert_eq!(report["assets"], 1);
    let exported: Value = serde_json::from_slice(&std::fs::read(out.join("scene.json")).unwrap()).unwrap();
    let path = exported["entities"][0]["props"]["ai_viewmodel"].as_str().unwrap().to_string();
    assert_eq!(path, "assets/0.glb");
    assert!(out.join(&path).is_file());
    let mut empty = scene.clone();
    empty.entities[0].props["ai_viewmodel"] = json!("");
    export_bundle(&empty, &root, &fixture.0.join("site3"), &fixture.0.join("pkg")).unwrap();
    let mut escape = scene.clone();
    escape.entities[0].props["ai_viewmodel"] = json!("../outside.glb");
    assert!(export_bundle(&escape, &root, &fixture.0.join("site2"), &fixture.0.join("pkg")).is_err());
}
