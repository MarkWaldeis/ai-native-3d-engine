use engine_terrain::{Terrain, TerrainConfig};
fn main() -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let terrain = Terrain::generate(TerrainConfig::preset_44km2())?;
    println!(
        "44 km² default generation: {} tiles, {} samples/edge, {:.3} seconds",
        terrain.tiles.len(),
        terrain.manifest.config.samples,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
