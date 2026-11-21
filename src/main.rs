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
    let mut args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--net-serve") {
        let result = (|| -> anyhow::Result<()> {
            let scene_path=args.get(2).ok_or_else(|| anyhow::anyhow!("usage: ai-engine --net-serve <scene.json> [--addr 127.0.0.1:7790]"))?;
            let addr=match args.len() {
                3 => "127.0.0.1:7790".to_owned(),
                5 if args[3]=="--addr" => args[4].clone(),
                _ => anyhow::bail!("usage: ai-engine --net-serve <scene.json> [--addr 127.0.0.1:7790]"),
            };
            let root=std::env::current_dir()?.canonicalize()?;
            let path=engine_core::project_path(&root,std::path::Path::new(scene_path))?;
            let scene=serde_json::from_str(&std::fs::read_to_string(path)?)?;
            engine_core::validate_scene_paths(&scene,&root)?;
            std::thread::spawn(move || engine_net::websocket::serve(scene,engine_net::SimSource::ProjectRoot(root),&addr))
                .join().map_err(|_| anyhow::anyhow!("network server thread panicked"))?
        })();
        if let Err(error)=result { eprintln!("network server failed: {error:#}"); std::process::exit(1); }
        return;
    }
    if args.get(1).map(String::as_str) == Some("init") {
        let result = (|| -> anyhow::Result<Value> {
            let dir = args.get(2).ok_or_else(|| anyhow::anyhow!("usage: ai-engine init <dir> [--template coin_game|sky_garden|empty]"))?;
            let template = match args.len() {
                3 => "coin_game",
                5 if args[3] == "--template" => &args[4],
                _ => anyhow::bail!("usage: ai-engine init <dir> [--template coin_game|sky_garden|empty]"),
            };
            ai_engine::project::initialize(std::path::Path::new(dir), template)
        })();
        match result {
            Ok(value) => println!("{value}"),
            Err(error) => { eprintln!("project init failed: {error:#}"); std::process::exit(1); }
        }
        return;
    }
    let project = args.iter().position(|arg| arg == "--project").map(|pos| {
        let path = args.get(pos+1).ok_or_else(|| anyhow::anyhow!("--project requires a directory"))?;
        let path = std::path::Path::new(path).canonicalize()?;
        std::env::set_current_dir(&path)?;
        args.drain(pos..pos+2);
        Ok::<_, anyhow::Error>(path)
    }).transpose().unwrap_or_else(|error| { eprintln!("project failed: {error:#}"); std::process::exit(1); });

    if let Some(pos) = args.iter().position(|a| a == "--export-web") {
        let result = (|| -> anyhow::Result<()> {
            let scene = args.get(pos+1).ok_or_else(||anyhow::anyhow!("usage: ai-engine --export-web <scene.json> <out_dir>"))?;
            let out = args.get(pos+2).ok_or_else(||anyhow::anyhow!("usage: ai-engine --export-web <scene.json> <out_dir>"))?;
            let scene = engine_core::project_path(&std::env::current_dir()?,std::path::Path::new(scene))?;
            let scene = serde_json::from_str(&std::fs::read_to_string(scene)?)?;
            let report = ai_engine::web::export_web(&scene, &std::env::current_dir()?, std::path::Path::new(out))?;
            println!("{report}"); Ok(())
        })();
        if let Err(error) = result { eprintln!("web export failed: {error:#}"); std::process::exit(1); }
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--serve") {
        let result = (|| -> anyhow::Result<()> {
            let remote = args.iter().any(|arg|arg=="--allow-remote");
            let privileged = args.iter().any(|arg|arg=="--allow-privileged-tools");
            anyhow::ensure!(!privileged || remote,"--allow-privileged-tools requires --allow-remote");
            let options = if remote { ai_engine::http::HttpOptions::remote_from_env(privileged)? } else { ai_engine::http::HttpOptions::default() };
            let addr = args.get(pos+1).filter(|arg|!arg.starts_with("--")).map(String::as_str).unwrap_or("127.0.0.1:7777");
            ai_engine::http::serve_project_addr_with_options(addr,project,options)
        })();
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

    let mut engine = project.map(|path| Engine::from_project(&path)).unwrap_or_else(|| Ok(Engine::new()))
        .unwrap_or_else(|error| { eprintln!("project failed: {error:#}"); std::process::exit(1); });
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
    let root = std::env::current_dir()?;
    let out_path = engine_core::project_path(&root,std::path::Path::new(out_path))?;
    if let Some(p) = scene_path {
        engine.world.load_with_root(&engine_core::project_path(&root,std::path::Path::new(p))?,&root)?;
    } else {
        engine.world.scene = shared::mocks::mock_scene();
    }
    let scene: Scene = engine.world.scene.clone();
    let mut renderer = engine_render::Renderer::new()?;
    let png = renderer.screenshot_png(&scene, engine.world.revision, 960, 540)?;
    std::fs::write(&out_path, &png)?;
    eprintln!("[demo] wrote {} ({} bytes)", out_path.display(), png.len());
    Ok(())
}
