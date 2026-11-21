use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/lantern_maze");
    let mut engine = Engine::from_project(&root)?;
    engine
        .world
        .load_with_root(&root.join("scene.json"), &root)?;
    let mut scene = engine.world.scene.clone();
    scene.entities.retain(|e| e.name != "Laternenwart");
    let script = std::fs::read_to_string(root.join("scripts/patrol.rhai"))?;
    scene.entities.push(serde_json::from_value(json!({"id":22,"name":"Laternenwart","kind":"sphere","radius":0.2,"segments":12,
        "transform":{"position":[-4.5,0.22,2.5]},"color":[0.95,0.58,0.18,1.0],"tags":["npc","floating"],
        "nav_agent":{"speed":1.5,"offset_y":0.22},"script":{"path":"scripts/patrol.rhai","source":script}}))?);
    engine.world.replace_authoring_scene(scene)?;
    let response = engine
        .handle("nav_bake", &json!({}))
        .map_err(anyhow::Error::msg)?;
    let baked: Value = serde_json::from_str(response["content"][0]["text"].as_str().unwrap())?;
    println!("{}", serde_json::to_string_pretty(&baked)?);
    engine
        .handle(
            "save_scene",
            &json!({"path":"scene.json","bundle_assets":true}),
        )
        .map_err(anyhow::Error::msg)?;
    Ok(())
}
