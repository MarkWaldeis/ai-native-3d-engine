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
fn net_arena_export_bundles_multiplayer_transport_and_spawn_script() {
    let fixture=Fixture::new();
    let root=root();
    let scene:Scene=serde_json::from_str(&std::fs::read_to_string(root.join("examples/net_arena/scene.json")).unwrap()).unwrap();
    let out=fixture.0.join("net-arena");
    export_bundle(&scene,&root,&out,&fixture.0.join("pkg")).unwrap();
    let player=std::fs::read_to_string(out.join("player.js")).unwrap();
    assert!(player.contains("new NetGame()"));
    assert!(out.join("net.js").is_file());
    let scripts:Value=serde_json::from_str(&std::fs::read_to_string(out.join("scripts.json")).unwrap()).unwrap();
    assert!(scripts.as_object().unwrap().values().any(|source|source.as_str().unwrap().contains("spawn_entity")));
}

#[test]
fn web_export_bundles_compact_terrain_once_and_rewrites_paths() {
    let fixture=Fixture::new();
    let terrain=engine_terrain::Terrain::generate(engine_terrain::TerrainConfig {size_m:[512.0;2],samples:33,..Default::default()}).unwrap();
    terrain.save(&fixture.0.join("map")).unwrap();
    let scene:Scene=serde_json::from_value(json!({"entities":[
        {"id":1,"name":"ground","kind":"box","size":[1,1,1],"props":{"ai_terrain":"map/terrain.json","ai_terrain_radius":64}},
        {"id":2,"name":"ground2","kind":"box","size":[1,1,1],"props":{"ai_terrain":"map/terrain.json"}}
    ]})).unwrap();
    let out=fixture.0.join("site");
    let report=export_bundle(&scene,&fixture.0,&out,&fixture.0.join("pkg")).unwrap();
    assert_eq!(report["terrain"],1);
    assert!(out.join("terrain/0/terrain.json").is_file());
    assert!(out.join("terrain/0/coarse.bin").is_file());
    for z in 0..2 {for x in 0..2 {assert_eq!(std::fs::metadata(out.join(format!("terrain/0/tiles/{x}_{z}.bin"))).unwrap().len(),(35*35*2+12) as u64);}}
    let exported:Scene=serde_json::from_slice(&std::fs::read(out.join("scene.json")).unwrap()).unwrap();
    for e in &exported.entities {assert_eq!(e.props["ai_terrain"],"terrain/0/terrain.json");}
    assert_eq!(exported.entities[0].props["ai_terrain_radius"],64);
    let manifest:engine_terrain::web::WebManifest=serde_json::from_slice(&std::fs::read(out.join("terrain/0/terrain.json")).unwrap()).unwrap();
    let streamed=engine_terrain::web::StreamedTerrain::from_manifest(&manifest,&std::fs::read(out.join("terrain/0/coarse.bin")).unwrap()).unwrap();
    assert!((streamed.sample(128.0,128.0)-terrain.sample(128.0,128.0).unwrap()).abs()<0.01);
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
        "favicon.svg",
        "player.js",
        "input.js",
        "input-settings.js",
        "nature.js",
        "nature-settings.js",
        "streaming.js",
        "world.js",
        "hlod.js",
        "occlusion.js",
        "effects.js",
        "presentation.js",
        "graphics-settings.js",
        "gpu-timer.js",
        "hdr.js",
        "sky.js",
        "materials.js",
        "surfaces.js",
        "atmosphere.js",
        "local-lights.js",
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
            &[engine_runtime::Input { entity: None,
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
        &[engine_runtime::Input { entity: None,
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
fn web_export_bundles_authored_low_lods_and_rejects_invalid_paths() {
    let fixture = Fixture::new();
    let root = root();
    let mut scene: Scene = serde_json::from_value(json!({"entities":[
        {"id":1,"name":"lod-model","kind":"gltf","path":"assets/suzanne.glb",
         "props":{"web_lods":{"1":"assets/suzanne.glb","2":"assets/suzanne.glb"}}},
        {"id":2,"name":"no-lod","kind":"gltf","path":"assets/suzanne.glb"}
    ]}))
    .unwrap();
    let out = fixture.0.join("lod-site");
    let report = export_bundle(&scene, &root, &out, &fixture.0.join("pkg")).unwrap();
    assert_eq!(report["assets"], 1);
    let exported: Value =
        serde_json::from_slice(&std::fs::read(out.join("scene.json")).unwrap()).unwrap();
    assert_eq!(
        exported["entities"][0]["props"]["web_lods"]["1"],
        "assets/0.glb"
    );
    assert!(exported["entities"][1]["props"].get("web_lods").is_none());
    scene.entities[0].props["web_lods"]["1"] = json!("examples/open_world_shooter/assets/tree_palm_low.glb");
    let distinct = fixture.0.join("distinct-lod-site");
    let report = export_bundle(&scene, &root, &distinct, &fixture.0.join("pkg")).unwrap();
    assert_eq!(report["assets"], 2);
    assert!(distinct.join("assets/1.glb").is_file());
    for (index, low) in [
        json!("missing.glb"),
        json!("Cargo.toml"),
        json!("../escape.glb"),
        json!(12),
    ]
    .into_iter()
    .enumerate()
    {
        scene.entities[0].props["web_lods"]["1"] = low;
        assert!(export_bundle(
            &scene,
            &root,
            &fixture.0.join(format!("invalid-lod-{index}")),
            &fixture.0.join("pkg")
        )
        .is_err());
    }
}

#[test]
fn web_export_bundles_local_primitive_material_maps_once() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let mut png_bytes = Vec::new();
    {
        let mut png = png::Encoder::new(&mut png_bytes, 2, 2);
        png.set_color(png::ColorType::Rgb);
        png.write_header().unwrap().write_image_data(&[128; 12]).unwrap();
    }
    std::fs::write(root.join("surface.PNG"), &png_bytes).unwrap();
    std::fs::write(root.join("invalid.png"), b"not an image").unwrap();
    let mut scene: Scene = serde_json::from_value(json!({"entities":[
        {"id":1,"name":"floor","kind":"plane","size":4,
         "props":{"ai_material":{"base_color_map":"surface.PNG","uv_repeat":[2,3]}}},
        {"id":2,"name":"box","kind":"box","size":[1,1,1],
         "props":{"ai_material":"{\"normal_map\":\"./surface.PNG\",\"orm_map\":\"surface.PNG\"}"}}
    ]})).unwrap();
    let original = scene.clone();
    let out = root.join("material-site");
    let pkg = root.join("pkg");
    let report = export_bundle(&scene, root, &out, &pkg).unwrap();
    assert_eq!(scene, original);
    assert_eq!(report["assets"], 1);
    let bundled = "assets/material/0.png";
    assert_eq!(std::fs::read(out.join(bundled)).unwrap(), png_bytes);
    let exported: Scene =
        serde_json::from_slice(&std::fs::read(out.join("scene.json")).unwrap()).unwrap();
    assert_eq!(exported.entities[0].props["ai_material"]["base_color_map"], bundled);
    assert_eq!(exported.entities[0].props["ai_material"]["uv_repeat"], json!([2,3]));
    let parsed: Value = serde_json::from_str(
        exported.entities[1].props["ai_material"].as_str().unwrap()
    ).unwrap();
    assert_eq!(parsed, json!({"normal_map":bundled,"orm_map":bundled}));
    for (i, bad) in [
        json!("https://example.invalid/color.png"), json!("../outside.png"),
        json!(root.join("surface.PNG")), json!("missing.png"), json!("invalid.png"), json!(42),
    ].into_iter().enumerate() {
        scene.entities[0].props["ai_material"]["base_color_map"] = bad;
        let destination = root.join(format!("bad-material-{i}"));
        assert!(export_bundle(&scene, root, &destination, &pkg).is_err());
        assert!(!destination.exists());
    }
}

#[test]
fn web_export_bundles_authored_hdri_and_rejects_invalid_assets() {
    let fixture = Fixture::new();
    let repo = root();
    let mut scene: Scene = serde_json::from_slice(
        &std::fs::read(repo.join("examples/material_studio/scene.json")).unwrap(),
    )
    .unwrap();
    let pkg = fixture.0.join("pkg");
    let out = fixture.0.join("site");
    let original = scene.clone();
    let report = export_bundle(&scene, &repo, &out, &pkg).unwrap();
    assert_eq!(scene, original);
    assert_eq!(report["assets"], 5);
    assert!(out.join("vendor/addons/loaders/RGBELoader.js").is_file());
    assert!(out.join("contact-occlusion.js").is_file());
    let hdr = std::fs::read(repo.join("examples/material_studio/studio.hdr")).unwrap();
    assert_eq!(
        std::fs::read(out.join("assets/environment.hdr")).unwrap(),
        hdr
    );
    let exported: Scene =
        serde_json::from_slice(&std::fs::read(out.join("scene.json")).unwrap()).unwrap();
    let config = &exported.entities[1].props["ai_graphics"];
    assert_eq!(config["environment_map"], "assets/environment.hdr");
    assert_eq!(config["environment_background"], false);
    assert_eq!(config["exposure"], 0.93);

    // The JSON-string form is used by Blender custom properties.
    scene.entities[1].props["ai_graphics"] =
        json!(r#"{"environment_map":"examples/material_studio/studio.hdr"}"#);
    assert_eq!(
        export_bundle(&scene, &repo, &fixture.0.join("string"), &pkg).unwrap()["assets"],
        5
    );

    let assets = &fixture.0;
    let broken = assets.join("broken.hdr");
    std::fs::write(&broken, &hdr[..hdr.len() - 1]).unwrap();
    scene
        .entities
        .retain(|entity| entity.tags.iter().any(|tag| tag == "player"));
    let player = scene.entities.first_mut().unwrap();
    player.script = None;
    for (path, expected) in [
        (json!(42), "project-relative .hdr path"),
        (json!("photo.jpg"), "project-relative .hdr path"),
        (
            json!("../escaped.hdr"),
            "invalid ai_graphics.environment_map path",
        ),
        (json!("absent.hdr"), "environment map not found"),
        (json!("broken.hdr"), "truncated RGBE scanline packet"),
    ] {
        scene.entities[0].props["ai_graphics"] = json!({"environment_map":path});
        let invalid = assets.join("invalid");
        let err = export_bundle(&scene, assets, &invalid, &pkg).unwrap_err();
        assert!(format!("{err:#}").contains(expected), "{err:#}");
        assert!(!invalid.exists());
    }
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
