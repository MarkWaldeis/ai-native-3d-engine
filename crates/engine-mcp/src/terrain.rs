//! Project-confined terrain authoring tools; no shared scene contract changes.
use anyhow::{Context, Result};
use engine_terrain::{Terrain, TerrainConfig};
use serde_json::{json, Value};
use std::path::Path;

pub const NAMES: &[&str] = &[
    "terrain_generate",
    "terrain_import",
    "terrain_info",
    "terrain_sample",
];
pub fn tools() -> Vec<Value> {
    NAMES.iter().map(|name| {
        let required = match *name { "terrain_generate"=>vec!["out_dir"], "terrain_import"=>vec!["heightmap","out_dir"], "terrain_sample"=>vec!["path","x","z"], _=>vec!["path"] };
        json!({"name":name,"description":"CPU tiled terrain: generate/import to an empty directory, inspect manifest or sample meters. size_m is [x,z]; PNG grayscale16, raw r16 little endian requires width/height.","inputSchema":{"type":"object","required":required,"properties":{
            "out_dir":{"type":"string"},"path":{"type":"string"},"heightmap":{"type":"string"},
            "size_m":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
            "tile_m":{"type":"number","exclusiveMinimum":0},"samples":{"type":"integer","minimum":2,"maximum":4097},
            "height_scale":{"type":"number","minimum":0},"seed":{"type":"integer","minimum":0},
            "width":{"type":"integer","minimum":2},"height":{"type":"integer","minimum":2},"x":{"type":"number"},"z":{"type":"number"}
        }}})
    }).collect()
}
pub fn execute(name: &str, args: &Value, root: &Path) -> Result<Value> {
    let path = |key: &str| -> Result<std::path::PathBuf> {
        engine_core::project_path(
            root,
            Path::new(
                args[key]
                    .as_str()
                    .with_context(|| format!("missing {key}"))?,
            ),
        )
    };
    let result = match name {
        "terrain_generate" | "terrain_import" => {
            let config: TerrainConfig = serde_json::from_value(args.clone())?;
            let out = path("out_dir")?;
            let terrain = if name == "terrain_generate" {
                Terrain::generate(config)?
            } else {
                let dimensions = match (args["width"].as_u64(), args["height"].as_u64()) {
                    (Some(w), Some(h)) => Some([w as usize, h as usize]),
                    _ => None,
                };
                Terrain::import(&path("heightmap")?, config, dimensions)?
            };
            terrain.save(&out)?;
            json!({"path":out.join("terrain.json"),"manifest":terrain.manifest})
        }
        "terrain_info" => serde_json::to_value(Terrain::load(&path("path")?)?.manifest)?,
        "terrain_sample" => {
            let terrain = Terrain::load(&path("path")?)?;
            json!({"height":terrain.sample(args["x"].as_f64().context("missing x")?,args["z"].as_f64().context("missing z")?)?})
        }
        _ => anyhow::bail!("unknown terrain tool"),
    };
    Ok(shared::mcp_protocol::text_result(result.to_string()))
}
