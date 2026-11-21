use shared::scene::Scene;
use std::path::Path;

#[allow(dead_code)]
#[path = "../examples/waldwaechter_parity.rs"]
mod parity;

#[test]
fn showcase_follow_framing_survives_blender_round_trip() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/waldwaechter/scene.json");
    let scene: Scene = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let player = scene
        .entities
        .iter()
        .find(|e| e.tags.iter().any(|t| t == "player"))
        .unwrap();
    for props in [&player.props, &player.props["_blender_props"]] {
        let config: serde_json::Value =
            serde_json::from_str(props["ai_world"].as_str().unwrap()).unwrap();
        assert_eq!(config["camera_distance"], 18);
    }
}

#[test]
fn showcase_victory_capture_and_checkpoint_preserve_collections() {
    let cases = parity::run().unwrap();
    assert!(cases["forest_win"]["win_tick"].is_u64());
    assert!(cases["forest_caught"]["win_tick"].is_null());
    assert!(cases["forest_checkpoint"]["win_tick"].is_null());
    let samples = cases["forest_win"]["samples"].as_array().unwrap();
    assert_ne!(
        samples.first().unwrap()["nav_agents"][0]["position"],
        samples.last().unwrap()["nav_agents"][0]["position"]
    );
}

#[test]
fn showcase_authored_assets_and_invalid_script_fail_closed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/waldwaechter");
    let mut scene: Scene =
        serde_json::from_slice(&std::fs::read(root.join("scene.json")).unwrap()).unwrap();
    assert_eq!(
        scene
            .entities
            .iter()
            .filter(|e| e.tags.iter().any(|t| t == "guard"))
            .count(),
        3
    );
    assert_eq!(
        scene
            .entities
            .iter()
            .filter(|e| e.animator.is_some())
            .count(),
        4
    );
    assert_eq!(scene.prefabs.len(), 2);
    assert!(scene.navmesh.as_ref().unwrap().tiles.len() >= 16);
    for entity in scene.entities.iter().filter(|e| e.animator.is_some()) {
        let names: Vec<_> = entity
            .animator
            .as_ref()
            .unwrap()
            .clips
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, vec!["idle", "jump", "walk"]);
        let encoded = serde_json::to_value(entity).unwrap();
        let bytes = std::fs::read(root.join(encoded["path"].as_str().unwrap())).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let gltf: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
        assert!(!gltf["skins"][0]["joints"].as_array().unwrap().is_empty());
        assert!(gltf["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mesh| mesh["primitives"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["attributes"]["JOINTS_0"].is_number()
                    && p["attributes"]["WEIGHTS_0"].is_number())));
    }
    let mut no_relics = scene.clone();
    no_relics
        .entities
        .retain(|e| !e.tags.iter().any(|t| t == "relic"));
    let mut no_relics = engine_runtime::Simulation::new(no_relics, &root).unwrap();
    no_relics.step(1, &[]).unwrap();
    assert_eq!(no_relics.state_summary()["state"]["score"], 0);
    assert_eq!(no_relics.state_summary()["state"]["won"], false);
    let player = scene
        .entities
        .iter_mut()
        .find(|e| e.tags.iter().any(|t| t == "player"))
        .unwrap();
    player.script = Some(shared::components::Script::Inline {
        source: "fn on_tick(ctx,dt) { throw \"expected QA error\"; }".into(),
    });
    let mut sim = engine_runtime::Simulation::new(scene, &root).unwrap();
    assert!(sim.step(1, &[]).is_err() || sim.state_summary()["failed"] == true);
    let empty =
        engine_runtime::Simulation::from_sources(Scene::default(), Default::default()).unwrap();
    assert_eq!(empty.state()["entities"].as_array().unwrap().len(), 0);
}
