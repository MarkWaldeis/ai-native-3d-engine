use engine_terrain::{Terrain, TerrainConfig};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let directory = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("usage: bench OUT_DIR [SEED]"))?,
    );
    let config = TerrainConfig {
        seed: args.next().map(|s| s.parse()).transpose()?.unwrap_or(1),
        ..TerrainConfig::preset_44km2()
    };
    let terrain = Terrain::generate(config)?;
    terrain.save(&directory)?;
    println!(
        "Terrain: {} tiles; {}",
        terrain.tiles.len(),
        directory.join("terrain.json").display()
    );
    Ok(())
}
