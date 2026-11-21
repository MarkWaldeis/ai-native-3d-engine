use anyhow::{ensure, Result};
use shared::{navigation::NavParams, scene::Scene};
use std::path::PathBuf;

fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("project path required"))?,
    );
    let path = root.join("navigation-input.json");
    let mut scene: Scene = serde_json::from_slice(&std::fs::read(&path)?)?;
    let params = NavParams {
        cell_size: 4.0,
        tile_size: 128.0,
        radius: 0.35,
        height: 1.5,
        step: 0.4,
        bounds: Some([-256.0, -256.0, 256.0, 256.0]),
        ..Default::default()
    };
    let (mesh, stats) = engine_runtime::navigation::bake(&scene, &root, params)?;
    for x in [8.0, 16.0, 24.0, 32.0, 40.0] {
        ensure!(
            !engine_runtime::navigation::query(&mesh, [0.0, 0.0, 0.0], [x, 0.0, 0.0])?.is_empty(),
            "unreachable relic"
        );
    }
    std::fs::write(
        root.join("assets/forest.nav.json"),
        serde_json::to_vec(&mesh)?,
    )?;
    scene.navmesh = Some(mesh);
    std::fs::write(&path, serde_json::to_vec_pretty(&scene)?)?;
    println!("{}", serde_json::to_string(&stats)?);
    Ok(())
}
