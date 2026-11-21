use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// Create a self-contained authoring project without overwriting existing files.
pub fn initialize(dir: &Path, template: &str) -> Result<Value> {
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let source = match template {
        "coin_game" => include_str!("../examples/coin_game/scene.json"),
        "sky_garden" => include_str!("../examples/sky_garden/scene.json"),
        "empty" => "{}",
        _ => bail!("unknown template {template}; choose coin_game, sky_garden, or empty"),
    };
    let mut scene: Value = if template == "empty" {
        serde_json::to_value(shared::scene::Scene::default())?
    } else {
        serde_json::from_str(source)?
    };
    for entity in scene["entities"]
        .as_array_mut()
        .context("template has no entities")?
    {
        if let Some(script) = entity["script"].as_str().map(str::to_owned) {
            let path = format!(
                "scripts/{}",
                Path::new(&script).file_name().unwrap().to_string_lossy()
            );
            entity["script"] = json!(path);
            if entity["props"].get("ai_script").is_some() {
                entity["props"]["ai_script"] = json!(path);
            }
        }
        if entity["kind"] == "gltf" {
            let name = Path::new(entity["path"].as_str().unwrap())
                .file_name()
                .unwrap();
            entity["path"] = json!(format!("assets/{}", name.to_string_lossy()));
        }
    }
    match template {
        "coin_game" => {
            files.insert(
                "scripts/player.rhai".into(),
                include_bytes!("../examples/coin_game/scripts/player.rhai").to_vec(),
            );
            files.insert(
                "scripts/coin.rhai".into(),
                include_bytes!("../examples/coin_game/scripts/coin.rhai").to_vec(),
            );
            files.insert(
                "validation.json".into(),
                include_bytes!("../examples/coin_game/validation.json").to_vec(),
            );
        }
        "sky_garden" => {
            files.insert("assets/milo_rig.glb".into(), include_bytes!("../examples/sky_garden/assets/milo_rig.glb").to_vec());
            files.insert("milo_rig.blend".into(), include_bytes!("../examples/sky_garden/milo_rig.blend").to_vec());
            files.insert("rigged_character.py".into(), include_bytes!("../tools/sky_garden/rigged_character.py").to_vec());
            files.insert(
                "scripts/player.rhai".into(),
                include_bytes!("../examples/sky_garden/scripts/player.rhai").to_vec(),
            );
            files.insert(
                "scripts/crystal.rhai".into(),
                include_bytes!("../examples/sky_garden/scripts/crystal.rhai").to_vec(),
            );
            files.insert(
                "validation.json".into(),
                include_bytes!("../examples/sky_garden/validation.json").to_vec(),
            );
            files.insert(
                "blender_scene.py".into(),
                include_str!("../tools/sky_garden/artist.py")
                    .replace("examples/sky_garden/", "")
                    .into_bytes(),
            );
            macro_rules! assets {
                ($($id:literal),*) => {
                    $(files.insert(concat!("assets/", stringify!($id), ".glb").into(), include_bytes!(concat!("../examples/sky_garden/assets/", stringify!($id), ".glb")).to_vec());)*
                };
            }
            assets!(
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24, 25
            );
        }
        _ => {}
    }
    if dir.symlink_metadata().is_ok()
        && (!dir.is_dir()
            || dir.symlink_metadata()?.file_type().is_symlink()
            || dir.read_dir()?.next().is_some())
    {
        bail!(
            "project directory must be new or empty (no symlinks): {}",
            dir.display()
        );
    }
    std::fs::create_dir_all(dir)?;
    let root = dir.canonicalize()?;
    let command = std::env::current_exe()?;
    let server = json!({"command":command,"args":["--project",root]});
    files.insert(
        ".mcp.json".into(),
        serde_json::to_vec_pretty(&json!({"mcpServers":{"ai-engine":server}}))?,
    );
    files.insert(
        ".cursor/mcp.json".into(),
        serde_json::to_vec_pretty(&json!({"mcpServers":{"ai-engine":server}}))?,
    );
    files.insert("scene.json".into(), serde_json::to_vec_pretty(&scene)?);
    files.insert(".gitignore".into(), b".ai-engine/\n".to_vec());
    let recipe = if template == "sky_garden" {
        format!("For the Sky Garden recipe, read blender_scene.py and execute its contents via\nblender_exec, prefixed with `from pathlib import Path` and `PROJECT_ROOT = {:?}`.\nIt replaces Blender's active scene and saves sky_garden.blend here.\nUpdate the recipe or saved Blender source when changing models.\n\n", root.to_string_lossy())
    } else {
        String::new()
    };
    files.insert("AGENTS.md".into(), format!("# Game project / Spielprojekt\n\nTemplate: `{template}`. Start the configured MCP server, then read `engine://guide`\nand `engine://scene`. The server loads scene.json and uses this project as its cwd.\n\nBlender-first: model with materials, ai_uid and gameplay props; blender_sync;\nwrite_script; simulate assertions; validate_scene; inspect screenshots;\nsave_scene with bundle_assets:true; export_web into a new/empty directory.\nUse project-relative scripts/ and assets/ paths. Preserve Blender source.\n\n{recipe}Swarm: unique agent IDs; task dependencies; task_claim and claim_entities before\nedits; expected_revision / get_changes_since on conflicts; release claims when done.\nAuthoring is blocked during play: stop before editing. Check win, fall/respawn and\nempty/error paths. Never claim visual QA without opening the screenshot.\n\nOnly trusted local clients: Blender Python has local machine access. Sessions\nare not authentication. Configs pin absolute executable and project paths; update\n.mcp.json and .cursor/mcp.json if either moves. Web export needs the engine source\ncheckout, Rust wasm32 target and wasm-bindgen CLI installed.\n").into_bytes());
    for folder in ["scripts", "assets", ".cursor"] {
        std::fs::create_dir(root.join(folder))?;
    }
    for (name, bytes) in &files {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(name))?
            .write_all(bytes)?;
    }
    crate::project_state::configuration(&root)?;
    Ok(
        json!({"project":root,"template":template,"files":files.keys().collect::<Vec<_>>(),"next":"Connect your MCP client; read engine://guide"}),
    )
}
