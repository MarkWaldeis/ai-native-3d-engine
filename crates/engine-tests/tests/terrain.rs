use engine_terrain::{FloatingOrigin, Terrain, TerrainConfig, WorldPos};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!(
                "terrain-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(path.join("assets")).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn config() -> TerrainConfig {
    TerrainConfig {
        size_m: [512.0; 2],
        samples: 9,
        ..Default::default()
    }
}

#[test]
fn streamed_tiles_enable_physics_and_rhai_height() {
    use engine_terrain::web::{encode_tile, StreamedTerrain};
    use std::collections::BTreeMap;
    let mut terrain=Terrain::generate(TerrainConfig {size_m:[512.0;2],samples:33,height_scale:0.0,..Default::default()}).unwrap();
    for t in &mut terrain.tiles {t.heights.fill(10.0);}
    terrain.manifest.height_range=[10.0;2];
    let mut streamed=StreamedTerrain::from_terrain(&terrain);
    streamed.tiles.clear();
    let scene: shared::scene::Scene=serde_json::from_value(json!({"entities":[
        {"id":1,"name":"terrain","kind":"box","size":[1,1,1],"transform":{"position":[0,2,0]},"props":{"ai_terrain":"map/terrain.json","ai_terrain_radius":32}},
        {"id":2,"name":"box","kind":"box","size":[1,1,1],"transform":{"position":[128,15,128]},"collider":{"shape":"box"},"rigidbody":{"type":"dynamic"},"script":{"source":"fn on_tick(ctx,dt) {ctx.state_set(\"h\",ctx.terrain_height(128.0,128.0));}"}}
    ]})).unwrap();
    assert!(engine_runtime::Simulation::from_sources(scene.clone(),BTreeMap::new()).err().unwrap().to_string().contains("missing from bundled terrain"));
    let sources=BTreeMap::from([("map/terrain.json".into(),streamed)]);
    let mut missing=engine_runtime::Simulation::from_sources_with_terrain(scene.clone(),BTreeMap::new(),sources.clone()).unwrap();
    missing.step(180,&[]).unwrap();
    assert!(missing.scene().entities[1].transform.position[1]<0.0);
    let mut sim=engine_runtime::Simulation::from_sources_with_terrain(scene,BTreeMap::new(),sources).unwrap();
    assert_eq!(sim.terrain_height(128.0,128.0),2.0);
    assert_eq!(sim.terrain_tiles_wanted(0.0),vec![("map/terrain.json".into(),0,0)]);
    sim.step(10,&[]).unwrap();
    assert!(sim.scene().entities[1].transform.position[1]<15.0);
    assert!(sim.insert_terrain_tile("missing",0,0,&[]).is_err());
    assert!(sim.insert_terrain_tile("map/terrain.json",0,0,&[]).is_err());
    sim.insert_terrain_tile("map/terrain.json",0,0,&encode_tile(&terrain.tiles[0],33,[10.0;2])).unwrap();
    assert!(sim.terrain_tiles_wanted(0.0).is_empty());
    sim.step(180,&[]).unwrap();
    assert!((sim.scene().entities[1].transform.position[1]-12.5).abs()<0.05);
    assert_eq!(sim.terrain_height(128.0,128.0),12.0);
    assert_eq!(sim.state_summary()["state"]["h"],12.0);
    let empty=engine_runtime::Simulation::from_sources(Default::default(),BTreeMap::new()).unwrap();
    assert_eq!(empty.terrain_height(0.0,0.0),0.0);
    assert!(empty.terrain_tiles_wanted(32.0).is_empty());
}

#[test]
fn deterministic_independent_edges_normals_and_roundtrip() {
    let c = config();
    let terrain = Terrain::generate(c.clone()).unwrap();
    let a = c.generate_tile(0, 0).unwrap();
    let b = c.generate_tile(1, 0).unwrap();
    assert_eq!(a.heights, terrain.tiles[0].heights);
    assert_eq!(a.heights, c.generate_tile(0, 0).unwrap().heights);
    let ma = a.mesh(&c);
    let mb = b.mesh(&c);
    for z in 0..c.samples {
        assert_eq!(a.height(&c, c.samples - 1, z), b.height(&c, 0, z));
        assert_eq!(
            ma.vertices[z * c.samples + c.samples - 1].normal,
            mb.vertices[z * c.samples].normal
        );
    }
    let d = c.generate_tile(0, 1).unwrap();
    let md = d.mesh(&c);
    for x in 0..c.samples {
        assert_eq!(
            ma.vertices[(c.samples - 1) * c.samples + x].normal,
            md.vertices[x].normal
        );
    }
    let p = Project::new();
    terrain.save(&p.0.join("map")).unwrap();
    let restored = Terrain::load(&p.0.join("map/terrain.json")).unwrap();
    assert_eq!(restored.manifest.config, c);
    for (a, b) in terrain.tiles.iter().zip(restored.tiles) {
        assert_eq!(a.heights, b.heights);
    }
    assert!(terrain.save(&p.0.join("map")).is_err());
    assert!(terrain.sample(-1.0, 0.0).is_err());
    assert!(terrain.sample(f64::NAN, 0.0).is_err());
    assert!(c.generate_tile(2, 0).is_err());
}

#[test]
fn full_area_fast_and_origin_precision() {
    let c = TerrainConfig {
        samples: 3,
        ..TerrainConfig::preset_44km2()
    };
    let start = std::time::Instant::now();
    let terrain = Terrain::generate(c).unwrap();
    assert_eq!(terrain.tiles.len(), 26 * 26);
    assert!(start.elapsed().as_secs() < 10);
    let p = WorldPos([6600.123456789, 120.123456789, 6600.987654321]);
    let mut origin = FloatingOrigin {
        origin: WorldPos::default(),
        threshold: 1024.0,
        tile_m: 256.0,
    };
    assert!(origin.rebase(p));
    assert!(!origin.rebase(p));
    let restored = WorldPos::from_relative(p.relative_to(origin.origin), origin.origin);
    let error = (restored.0[0] - p.0[0]).abs();
    let naive = (p.0[0] as f32 as f64 - p.0[0]).abs();
    assert!(
        error < 0.001 && error < naive / 10.0,
        "rebased={error}, naive={naive}"
    );
}

#[test]
fn imports_png_and_r16_and_rejects_invalid_data() {
    let p = Project::new();
    let values = [0u16, 65535, 32768, 16384];
    let raw = p.0.join("map.r16");
    std::fs::write(
        &raw,
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let png = p.0.join("map.png");
    let mut encoder = png::Encoder::new(std::fs::File::create(&png).unwrap(), 2, 2);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(
            &values
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let c = config();
    let a = Terrain::import(&raw, c.clone(), Some([2, 2])).unwrap();
    let b = Terrain::import(&png, c.clone(), None).unwrap();
    for (a, b) in a.tiles.iter().zip(&b.tiles) {
        assert_eq!(a.heights, b.heights);
    }
    assert_eq!(a.sample(0.0, 0.0).unwrap(), 0.0);
    assert_eq!(a.sample(512.0, 0.0).unwrap(), 200.0);
    assert!((a.sample(256.0, 0.0).unwrap() - 100.0).abs() < 0.001);
    assert!(Terrain::import(&raw, c.clone(), None).is_err());
    assert!(Terrain::import(&raw, c.clone(), Some([3, 2])).is_err());
    assert!(Terrain::generate(TerrainConfig { samples: 1, ..c }).is_err());
    let empty = p.0.join("empty.r16");
    std::fs::write(&empty, []).unwrap();
    assert!(Terrain::import(&empty, config(), Some([0, 0])).is_err());
    a.save(&p.0.join("tiles")).unwrap();
    std::fs::write(p.0.join("tiles/tile_0_0.f32"), []).unwrap();
    assert!(Terrain::load(&p.0.join("tiles/terrain.json")).is_err());
}

#[test]
fn terrain_mcp_tools_and_path_confinement() {
    let p = Project::new();
    let mut engine = ai_engine::Engine::new();
    engine.assets_dir = p.0.join("assets");
    let call = |engine: &mut ai_engine::Engine, name: &str, args: Value| {
        engine_mcp::handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string(),engine).unwrap()["result"].clone()
    };
    let list = engine_mcp::handle_line(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        &mut engine,
    )
    .unwrap();
    for name in engine_mcp::terrain::NAMES {
        assert!(list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == *name));
    }
    let result = call(
        &mut engine,
        "terrain_generate",
        json!({"out_dir":"map","size_m":[512,512],"samples":3}),
    );
    assert_ne!(result["isError"], true, "{result}");
    for (name, args) in [
        ("terrain_info", json!({"path":"map/terrain.json"})),
        (
            "terrain_sample",
            json!({"path":"map/terrain.json","x":100,"z":100}),
        ),
    ] {
        let r = call(&mut engine, name, args);
        assert_ne!(r["isError"], true, "{r}");
    }
    std::fs::write(p.0.join("input.r16"), [0; 8]).unwrap();
    let r = call(
        &mut engine,
        "terrain_import",
        json!({"heightmap":"input.r16","out_dir":"imported","width":2,"height":2,"size_m":[256,256],"samples":3}),
    );
    assert_ne!(r["isError"], true, "{r}");
    for (name, args) in [
        ("terrain_generate", json!({"out_dir":"../escape"})),
        (
            "terrain_import",
            json!({"heightmap":"../outside.r16","out_dir":"nope"}),
        ),
        ("terrain_info", json!({"path":"../outside.json"})),
        (
            "terrain_sample",
            json!({"path":"map/terrain.json","x":-1,"z":0}),
        ),
        ("terrain_generate", json!({})),
    ] {
        let r = call(&mut engine, name, args);
        assert_eq!(r["isError"], true, "{r}");
    }
}

#[test]
fn far_tile_physics_lands_on_height_and_seam() {
    let p = Project::new();
    // Imported planar ramp catches transposition/scale errors in the heightfield.
    std::fs::write(
        p.0.join("ramp.r16"),
        [0u16, 65535, 0, 65535]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let c = TerrainConfig {
        samples: 5,
        height_scale: 20.0,
        ..Default::default()
    };
    let terrain = Terrain::import(&p.0.join("ramp.r16"), c, Some([2, 2])).unwrap();
    terrain.save(&p.0.join("map")).unwrap();
    let scene=serde_json::from_value(json!({"entities":[
        {"id":1,"name":"terrain","kind":"plane","size":1.0,"props":{"ai_terrain":"map/terrain.json","ai_terrain_radius":300}},
        {"id":2,"name":"ball","kind":"sphere","radius":0.5,"segments":12,"transform":{"position":[6400.0,30.0,6400.0]},"collider":{"shape":"sphere","radius":0.5},"rigidbody":{"type":"dynamic","friction":1.0}}
    ]})).unwrap();
    let mut simulation = engine_runtime::Simulation::new(scene, &p.0).unwrap();
    simulation.step(300, &[]).unwrap();
    let landed = simulation
        .scene()
        .entities
        .into_iter()
        .find(|e| e.id == 2)
        .unwrap()
        .transform
        .position;
    let height = terrain.sample(landed[0] as f64, landed[2] as f64).unwrap();
    assert!(
        (landed[1] - height - 0.5).abs() < 0.08,
        "landed={landed:?}, height={height}"
    );
    // Travel across multiple seams while colliders enter/leave a smaller radius.
    let scene=serde_json::from_value(json!({"entities":[
        {"id":1,"name":"terrain","kind":"plane","size":1.0,"props":{"ai_terrain":"map/terrain.json","ai_terrain_radius":80}},
        {"id":2,"name":"ball","kind":"sphere","radius":0.5,"segments":12,"transform":{"position":[6100.0,25.0,6400.0]},"collider":{"shape":"sphere","radius":0.5},"rigidbody":{"type":"dynamic"},"script":{"source":"fn on_tick(ctx, dt) { let v=ctx.velocity(); ctx.set_velocity([50.0,v[1],0.0]); }"}}
    ]})).unwrap();
    let mut simulation = engine_runtime::Simulation::new(scene, &p.0).unwrap();
    simulation.step(480, &[]).unwrap();
    let pos = simulation
        .scene()
        .entities
        .into_iter()
        .find(|e| e.id == 2)
        .unwrap()
        .transform
        .position;
    assert!(pos[0] > 6450.0);
    assert!(
        (pos[1] - terrain.sample(pos[0] as f64, pos[2] as f64).unwrap() - 0.5).abs() < 0.1,
        "{pos:?}"
    );
}

#[test]
fn real_blender_bake_import_and_sample() {
    let Some(blender) = asset_pipeline::find_blender() else {
        eprintln!("SKIP: Blender not found; terrain bake requires real Blender");
        return;
    };
    let project = Project::new();
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../blender/terrain_heightmap.py");
    let output = std::process::Command::new(blender)
        .args([
            "-b",
            "--factory-startup",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(script)
        .arg("--")
        .arg("--out")
        .arg(project.0.join("baked.png"))
        .args(["--resolution", "33", "--grid", "33"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("; 0 uncovered samples"), "{stdout}");
    eprintln!("{stdout}");
    let mut args: Value =
        serde_json::from_slice(&std::fs::read(project.0.join("baked.import.json")).unwrap())
            .unwrap();
    args["samples"] = json!(5);
    let mut engine = ai_engine::Engine::new();
    engine.assets_dir = project.0.join("assets");
    let call = |engine: &mut ai_engine::Engine, name: &str, args: Value| {
        let response = engine_mcp::handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string(), engine).unwrap();
        assert_ne!(response["result"]["isError"], true, "{response}");
        serde_json::from_str::<Value>(response["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap()
    };
    call(&mut engine, "terrain_import", args);
    for (x, z) in [
        (0.0, 0.0),
        (3300.0, 3300.0),
        (44000000.0_f64.sqrt(), 44000000.0_f64.sqrt()),
    ] {
        let sample = call(
            &mut engine,
            "terrain_sample",
            json!({"path":"baked/terrain.json","x":x,"z":z}),
        );
        let height = sample["height"].as_f64().unwrap();
        assert!(height > 0.0 && height < 200.0, "{sample}");
        eprintln!(
            "Blender PNG16 -> terrain_import -> terrain_sample ({x:.3}, {z:.3}): {height:.6} m"
        );
    }
}
