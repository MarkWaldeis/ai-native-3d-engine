use engine_terrain::{Terrain, TerrainConfig};
use serde_json::json;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = root.join("target/terrain-demo-source");
    std::fs::create_dir_all(&output)?;
    let terrain = Terrain::generate(TerrainConfig::preset_44km2())?;
    if !output.join("map").exists() {
        terrain.save(&output.join("map"))?;
    }
    for (name, coordinate) in [("origin", 128.0), ("far", 6400.0)] {
        let h = terrain.sample(coordinate, coordinate)? as f64;
        let scene = json!({
            "name": "Höhenland · 44 km²",
            "camera": {"position":[coordinate+12.0,h+12.0,coordinate+18.0],"target":[coordinate,h,coordinate],"fov_deg":55.0},
            "entities":[
                {"id":1,"name":"heightfield","kind":"box","size":[1,1,1],"props":{"ai_terrain":"target/terrain-demo-source/map/terrain.json","ai_terrain_radius":256}},
                {"id":2,"name":"wanderer","kind":"box","size":[0.8,1.8,0.8],"color":[0.95,0.36,0.06,1.0],
                 "tags":["player"],"transform":{"position":[coordinate,h+2.5,coordinate]},
                 "collider":{"shape":"box"},"rigidbody":{"type":"dynamic","friction":1.0},
                 "props":{"ai_lock_rotation":true,"ai_world":{"draw_distance":4500,"sim_radius":256},"ai_ui":{"title":"Höhenland · 44 km²","mission":"Gestreamtes Höhenfeld · Rust-Physik","intro_title":"Eine weite Landschaft","intro_text":"256-m-Kacheln · Höhen- und Neigungsmaterial · lokale Präzision"}},
                 "script":{"source":"fn on_tick(ctx,dt) { let v=ctx.velocity(); ctx.set_velocity([ctx.input(\"move_x\")*8.0,v[1],ctx.input(\"move_z\")*8.0]); }"}}
            ]
        });
        let scene: shared::scene::Scene = serde_json::from_value(scene)?;
        std::fs::write(output.join(format!("{name}.json")), serde_json::to_vec_pretty(&scene)?)?;
        println!("{}", output.join(format!("{name}.json")).display());
    }
    Ok(())
}
