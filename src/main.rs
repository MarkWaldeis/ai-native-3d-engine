//! ai-engine entry point.
//!
//!   ai-engine                            → MCP stdio server (newline-delimited JSON-RPC)
//!   ai-engine --demo out.png [scene.json] → render a scene once (mock scene
//!                                           when no file is given), write PNG, exit
//!
//! Logs go to stderr — stdout is reserved for protocol frames.

use ai_engine::Engine;
use serde_json::Value;
use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--serve") {
        let result = args.get(pos + 1).ok_or_else(|| anyhow::anyhow!("usage: ai-engine --serve 127.0.0.1:7777"))
            .and_then(|addr| ai_engine::http::serve_addr(addr));
        if let Err(e) = result { eprintln!("HTTP server failed: {e:#}"); std::process::exit(1); }
        return;
    }

    if args.iter().any(|a| a == "--demo") {
        let pos = args.iter().position(|a| a == "--demo").unwrap();
        let out = args.get(pos + 1).cloned().unwrap_or_else(|| "demo.png".into());
        let scene = args.get(pos + 2).cloned();
        std::process::exit(match demo(&out, scene.as_deref()) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("demo failed: {e:#}");
                1
            }
        });
    }

    let mut engine = Engine::new();
    eprintln!(
        "[ai-engine] MCP stdio server ready (blender: {:?}, assets: {})",
        engine.pipeline.blender_path(),
        engine.assets_dir.display()
    );
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if let Some(resp) = engine_mcp::handle_line(&line, &mut engine) {
            if resp != Value::Null {
                let mut lock = stdout.lock();
                let _ = writeln!(lock, "{}", serde_json::to_string(&resp).unwrap_or_default());
                let _ = lock.flush();
            }
        }
    }
}

fn demo(out_path: &str, scene_path: Option<&str>) -> anyhow::Result<()> {
    use shared::scene::Scene;
    let mut engine = Engine::new();
    if let Some(p) = scene_path {
        engine.world.load(std::path::Path::new(p))?;
    } else {
        engine.world.scene = shared::mocks::mock_scene();
    }
    let scene: Scene = engine.world.scene.clone();
    let mut renderer = engine_render::Renderer::new()?;
    let png = renderer.screenshot_png(&scene, engine.world.revision, 960, 540)?;
    std::fs::write(out_path, &png)?;
    eprintln!("[demo] wrote {} ({} bytes)", out_path, png.len());
    Ok(())
}
