use engine_core::World;
use serde_json::json;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| {
        anyhow::anyhow!("usage: cargo run --example authoring_index -- scene.json")
    })?;
    let bytes = std::fs::read(path)?;
    let mut world = World::new();
    world.replace_authoring_scene(serde_json::from_slice(&bytes)?)?;
    let start = Instant::now();
    let index = world.authoring_index();
    let build = start.elapsed();
    let start = Instant::now();
    for _ in 0..200 {
        std::hint::black_box(world.authoring_index());
    }
    let warm = start.elapsed();
    println!(
        "{}",
        json!({"entities":world.scene.entities.len(),"scene_json_bytes":bytes.len(),"index_payload_bytes":index.payload_bytes(),"cold_build_ms":build.as_secs_f64()*1000.0,"warm_validation_ms":warm.as_secs_f64()*1000.0/200.0,"memory_scope":"owned vector/string capacities only; excludes BTreeMap node, Arc and allocator overhead; not process RSS"})
    );
    Ok(())
}
