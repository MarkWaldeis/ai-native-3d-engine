use engine_runtime::{Input, Simulation};
use shared::scene::Scene;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn game() -> Simulation {
    let scene: Scene = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/coin_game/scene.json")).unwrap(),
    )
    .unwrap();
    Simulation::new(scene, &root()).unwrap()
}

#[test]
fn leaving_either_platform_axis_respawns_and_clears_velocity() {
    for (axis, direction) in [
        ("move_x", -1.0),
        ("move_x", 1.0),
        ("move_z", -1.0),
        ("move_z", 1.0),
    ] {
        let mut sim = game();
        while sim.state()["state"]["_respawns"].as_i64().unwrap() == 0 {
            assert!(sim.tick < 300, "respawn must happen after leaving {axis}");
            sim.step(
                1,
                &[Input { entity: None,
                    tick: sim.tick,
                    action: axis.into(),
                    value: direction,
                }],
            )
            .unwrap();
        }
        assert_eq!(
            sim.scene()
                .entities
                .iter()
                .find(|e| e.tags.iter().any(|tag| tag == "player"))
                .unwrap()
                .transform
                .position,
            [-3.0, 0.45, 0.0]
        );
        let score = sim.state()["state"]["score"].clone();
        sim.step(
            5,
            &[Input { entity: None,
                tick: sim.tick,
                action: axis.into(),
                value: 0.0,
            }],
        )
        .unwrap();
        assert_eq!(sim.state()["state"]["score"], score);
        assert_eq!(sim.state()["state"]["_respawns"], 1);
        assert_eq!(
            sim.scene()
                .entities
                .iter()
                .find(|e| e.tags.iter().any(|tag| tag == "player"))
                .unwrap()
                .transform
                .position,
            [-3.0, 0.45, 0.0]
        );
    }
}

#[test]
fn normal_coin_route_still_collects_and_wins() {
    let mut sim = game();
    sim.step(
        180,
        &[Input { entity: None,
            tick: 0,
            action: "move_x".into(),
            value: 1.0,
        }],
    )
    .unwrap();
    assert_eq!(sim.state()["state"]["score"], 3);
    assert_eq!(sim.state()["state"]["won"], true);
    assert_eq!(sim.state()["state"]["_respawns"], 0);
}
