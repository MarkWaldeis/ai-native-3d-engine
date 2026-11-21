use ai_engine::Engine;
use engine_mcp::ToolHandler;
use engine_runtime::{
    navigation::{self, bake, query},
    Simulation,
};
use serde_json::{json, Value};
use shared::{navigation::NavParams, scene::Scene};
use std::{collections::BTreeMap, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn scene() -> Scene {
    serde_json::from_value(json!({"entities":[
    {"id":1,"name":"floor","kind":"box","size":[10,0.2,10],"transform":{"position":[0,-0.1,0]},"collider":{"shape":"box"}},
    {"id":2,"name":"wall","kind":"box","size":[0.4,2,6],"transform":{"position":[0,1,0]},"collider":{"shape":"box"}}
]})).unwrap()
}
fn call(e: &mut Engine, name: &str, args: Value) -> Value {
    let r = e
        .handle(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn tiled_bake_paths_clearance_incremental_changes_and_errors() {
    let mut s = scene();
    let p = NavParams {
        tile_size: 4.0,
        ..Default::default()
    };
    let (m, stats) = bake(&s, &root(), p.clone()).unwrap();
    assert!(stats["polygons"].as_u64().unwrap() > 100);
    assert!(m.tiles.iter().any(|t| t.region[0] < 0));
    let path = query(&m, [-3., 0., 0.], [3., 0., 0.]).unwrap();
    assert!(path.len() > 2, "wall must force corners");
    assert!(path.iter().any(|p| p[2].abs() > 3.0));
    assert_eq!(path, query(&m, [-3., 0., 0.], [3., 0., 0.]).unwrap());
    assert_eq!(
        query(&m, [-3., 0., -3.8], [3., 0., -3.8]).unwrap().len(),
        2,
        "straight corridor must string-pull"
    );
    s.navmesh = Some(m.clone());
    let (same, stats) = bake(&s, &root(), p.clone()).unwrap();
    assert_eq!(same, m);
    assert_eq!(stats["baked_tiles"], 0);
    s.entities[1].transform.position[2] = 0.5;
    let (_, stats) = bake(&s, &root(), p.clone()).unwrap();
    assert!(stats["reused_tiles"].as_u64().unwrap() > 0);
    assert!(stats["baked_tiles"].as_u64().unwrap() > 0);
    s.entities[1].transform.scale[2] = 3.0;
    let (blocked, _) = bake(&s, &root(), p.clone()).unwrap();
    assert!(query(&blocked, [-3., 0., 0.], [3., 0., 0.]).is_err());
    assert!(query(&m, [100., 0., 0.], [3., 0., 0.]).is_err());
    assert!(query(&m, [f32::NAN, 0., 0.], [3., 0., 0.]).is_err());
    assert!(bake(
        &s,
        &root(),
        NavParams {
            radius: -1.,
            ..p.clone()
        }
    )
    .is_err());
    assert!(bake(
        &s,
        &root(),
        NavParams {
            bounds: Some([-1000., -1000., 1000., 1000.]),
            ..p
        }
    )
    .is_err());
    let (empty, stats) = bake(&Scene::default(), &root(), NavParams::default()).unwrap();
    assert_eq!(stats["polygons"], 0);
    assert!(query(&empty, [0.; 3], [1.; 3]).is_err());
    let mut bad = m;
    bad.tiles[0].polygons[0].cell = [i32::MIN, 0];
    assert!(navigation::validate(&bad).is_err());
}

#[test]
fn step_slope_height_and_radius_are_real_constraints() {
    let mut s:Scene=serde_json::from_value(json!({"entities":[
        {"id":1,"name":"lower","kind":"box","size":[4,0.2,4],"transform":{"position":[-2,-0.1,0]},"collider":{"shape":"box"}},
        {"id":2,"name":"upper","kind":"box","size":[4,0.2,4],"transform":{"position":[2,0.1,0]},"collider":{"shape":"box"}}
    ]})).unwrap();
    let p = NavParams::default();
    let (m, _) = bake(&s, &root(), p.clone()).unwrap();
    assert!(query(&m, [-2., 0., 0.], [2., 0.2, 0.]).is_ok());
    let (m, _) = bake(
        &s,
        &root(),
        NavParams {
            step: 0.05,
            ..p.clone()
        },
    )
    .unwrap();
    assert!(query(&m, [-2., 0., 0.], [2., 0.2, 0.]).is_err());
    s.entities.truncate(1);
    s.entities[0].transform.rotation_deg[2] = 25.;
    let (flat, _) = bake(
        &s,
        &root(),
        NavParams {
            max_slope_deg: 5.,
            ..p.clone()
        },
    )
    .unwrap();
    let (ramp, _) = bake(&s, &root(), p.clone()).unwrap();
    assert!(
        ramp.tiles.iter().map(|t| t.polygons.len()).sum::<usize>()
            > flat.tiles.iter().map(|t| t.polygons.len()).sum::<usize>()
    );
    s = scene();
    s.entities[1].transform.position = [0., 0.75, 0.];
    s.entities[1].mesh = serde_json::from_value(json!({"kind":"box","size":[3.,0.2,3.]})).unwrap();
    let (low, _) = bake(
        &s,
        &root(),
        NavParams {
            height: 0.5,
            ..p.clone()
        },
    )
    .unwrap();
    let (tall, _) = bake(
        &s,
        &root(),
        NavParams {
            height: 1.5,
            ..p.clone()
        },
    )
    .unwrap();
    assert!(query(&low, [-1., 0., 0.], [1., 0., 0.]).is_ok());
    assert!(query(&tall, [-1., 0., 0.], [1., 0., 0.]).is_err());
    let (small, _) = bake(&scene(), &root(), p.clone()).unwrap();
    let (wide, _) = bake(
        &scene(),
        &root(),
        NavParams {
            radius: 0.7,
            height: 1.5,
            ..p
        },
    )
    .unwrap();
    assert!(
        small.tiles.iter().map(|t| t.polygons.len()).sum::<usize>()
            > wide.tiles.iter().map(|t| t.polygons.len()).sum::<usize>()
    );
}

#[test]
fn fixed_ticks_rhai_navigation_pause_regions_and_unreachable_targets() {
    let mut s = scene();
    s.navmesh = Some(bake(&s, &root(), NavParams::default()).unwrap().0);
    s.entities.push(serde_json::from_value(json!({"id":3,"name":"walker","kind":"sphere","radius":0.1,"segments":8,"transform":{"position":[-3,0.2,0]},"nav_agent":{"speed":3,"offset_y":0.2},"script":{"source":"fn on_start(ctx) { ctx.nav_set_target([3.0,0.0,0.0]); } fn on_tick(ctx,dt) { ctx.state_set(\"corners\",ctx.nav_path().len); }"}})).unwrap());
    let mut batch = Simulation::from_sources(s.clone(), BTreeMap::new()).unwrap();
    let mut ticks = Simulation::from_sources(s.clone(), BTreeMap::new()).unwrap();
    batch.step(500, &[]).unwrap();
    for _ in 0..500 {
        ticks.step(1, &[]).unwrap();
    }
    assert_eq!(batch.nav_agent_states(), ticks.nav_agent_states());
    let state = batch.nav_agent_states();
    assert_eq!(state[0]["agent"]["status"], "arrived");
    assert_eq!(state[0]["position"], json!([3.0f32, 0.2f32, 0.0f32]));
    let frozen = batch.nav_agent_states();
    batch.step(0, &[]).unwrap();
    assert_eq!(batch.nav_agent_states(), frozen);
    let mut inactive = Simulation::from_sources(s.clone(), BTreeMap::new()).unwrap();
    inactive.set_active_region(100., 100., 1., 1.).unwrap();
    let before = inactive.nav_agent_states();
    inactive.step(60, &[]).unwrap();
    assert_eq!(before, inactive.nav_agent_states());
    s.entities.last_mut().unwrap().script = None;
    s.entities
        .last_mut()
        .unwrap()
        .nav_agent
        .as_mut()
        .unwrap()
        .target = Some([100., 0., 0.]);
    let mut sim = Simulation::from_sources(s.clone(), BTreeMap::new()).unwrap();
    sim.step(1, &[]).unwrap();
    assert_eq!(sim.nav_agent_states()[0]["agent"]["status"], "unreachable");
    s.entities
        .last_mut()
        .unwrap()
        .nav_agent
        .as_mut()
        .unwrap()
        .speed = -1.;
    assert!(Simulation::from_sources(s, BTreeMap::new()).is_err());
}

#[test]
fn mcp_asset_lint_locks_cas_atomic_rejection_rollback_and_restart() {
    let project = root()
        .join("target")
        .join(format!("nav-project-{}", std::process::id()));
    ai_engine::project::initialize(&project, "empty").unwrap();
    let mut e = Engine::from_project(&project).unwrap();
    e.world.replace_authoring_scene(scene()).unwrap();
    call(
        &mut e,
        "claim_entities",
        json!({"ids":[1],"agent":"owner","ttl_s":60}),
    );
    assert!(e
        .handle("nav_bake", &json!({"agent":"other"}))
        .unwrap_err()
        .contains("locked"));
    assert!(e
        .handle(
            "nav_bake",
            &json!({"agent":"owner","expected_revision":9999})
        )
        .unwrap_err()
        .contains("revision"));
    let rev = e.world.revision;
    let result = call(&mut e, "nav_bake", json!({"agent":"owner"}));
    assert!(project.join(result["asset"].as_str().unwrap()).is_file());
    let saved = e.world.scene.navmesh.clone();
    let path = call(
        &mut e,
        "nav_query_path",
        json!({"start":[-3,0,0],"goal":[3,0,0]}),
    );
    assert!(path["path"].as_array().unwrap().len() > 2);
    assert!(e.handle("apply_batch",&json!({"mode":"atomic","agent":"owner","commands":[{"tool":"nav_bake","arguments":{}}]})).is_err());
    call(&mut e, "rollback", json!({"revision":rev,"agent":"owner"}));
    assert!(e.world.scene.navmesh.is_none());
    call(&mut e, "nav_bake", json!({"agent":"owner"}));
    drop(e);
    let mut e = Engine::from_project(&project).unwrap();
    assert_eq!(e.world.scene.navmesh, saved);
    e.world.scene.entities.push(serde_json::from_value(json!({"id":99,"name":"lost goal","kind":"sphere","radius":0.1,"segments":8,"tags":["goal"],"transform":{"position":[100,0,0]}})).unwrap());
    let lint = call(&mut e, "nav_lint", json!({"start":[-3,0,0]}));
    assert_eq!(lint["findings"][0]["code"], "nav_unreachable");
    e.world
        .scene
        .entities
        .last_mut()
        .unwrap()
        .transform
        .position = [3., 0., 0.];
    assert!(
        call(&mut e, "nav_lint", json!({"start":[-3,0,0]}))["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    e.world.scene.entities[1].transform.scale[2] = 3.0;
    assert_eq!(
        call(&mut e, "nav_lint", json!({"start":[-3,0,0]}))["findings"][0]["code"],
        "nav_unreachable"
    );
    call(&mut e, "play", json!({}));
    assert!(e
        .handle("nav_bake", &json!({"agent":"owner"}))
        .unwrap_err()
        .contains("during play"));
    drop(e);
    std::fs::remove_dir_all(project).unwrap();
}

#[test]
fn rhai_empty_unreachable_missing_component_and_native_incremental_refresh() {
    let mut s = scene();
    s.navmesh = Some(bake(&s, &root(), NavParams::default()).unwrap().0);
    s.entities.push(serde_json::from_value(json!({"id":3,"name":"agent","kind":"sphere","radius":0.1,"segments":8,"transform":{"position":[-3,0,0]},"nav_agent":{},"script":{"source":"fn on_start(ctx) { ctx.state_set(\"empty\",ctx.nav_path().len); ctx.state_set(\"reachable\",ctx.nav_set_target([100.0,0.0,0.0])); }"}})).unwrap());
    let sim = Simulation::from_sources(s.clone(), BTreeMap::new()).unwrap();
    assert_eq!(sim.state()["state"]["empty"], 0);
    assert_eq!(sim.state()["state"]["reachable"], false);
    s.entities[2].nav_agent = None;
    assert!(Simulation::from_sources(s.clone(), BTreeMap::new())
        .err()
        .unwrap()
        .to_string()
        .contains("nav_agent"));
    s.entities[2].nav_agent = Some(Default::default());
    s.entities[2].script = None;
    s.entities[2].nav_agent.as_mut().unwrap().target = Some([3., 0., 0.]);
    s.entities[1].transform.scale[2] = 3.0;
    let mut fresh = Simulation::new(s, &root()).unwrap();
    fresh.step(1, &[]).unwrap();
    assert_eq!(
        fresh.nav_agent_states()[0]["agent"]["status"],
        "unreachable"
    );
}
