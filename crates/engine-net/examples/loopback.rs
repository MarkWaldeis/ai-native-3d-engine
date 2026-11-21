use anyhow::{bail, ensure, Context, Result};
use engine_net::simulate::{run, InputEvent, Options};
use engine_net::SimSource;
use shared::scene::Scene;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut options = Options::default();
    let mut seconds = 10.0_f64;
    let mut scene_path = PathBuf::from("examples/net_arena/scene.json");
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--scene" => scene_path = value.into(),
            "--rtt-ms" => options.rtt_ms = value.parse()?,
            "--jitter-ms" => options.jitter_ms = value.parse()?,
            "--loss" => options.loss = value.parse()?,
            "--seconds" => seconds = value.parse()?,
            _ => bail!("unknown argument {flag}"),
        }
    }
    ensure!(
        seconds.is_finite() && (0.0..=600.0).contains(&seconds),
        "seconds must be in 0..=600"
    );
    options.ticks = (seconds * 60.0) as u64;
    let moving_ticks = (options.ticks as f64 * 0.6) as u64;
    if options.ticks > 0 {
        options.inputs = (0..options.clients)
            .map(|player| {
                let direction = if player == 0 { 1.0 } else { -0.7 };
                let mut events = vec![InputEvent {
                    tick: 0,
                    action: "move_x".into(),
                    value: direction,
                }];
                if moving_ticks > 0 {
                    events.push(InputEvent {
                        tick: moving_ticks,
                        action: "move_x".into(),
                        value: 0.0,
                    });
                }
                events
            })
            .collect();
    }
    let scene: Scene = serde_json::from_str(&std::fs::read_to_string(&scene_path)?)?;
    // Like the engine, example scene references are relative to the project cwd.
    let sources = SimSource::ProjectRoot(std::env::current_dir()?);
    println!(
        "{}",
        serde_json::to_string_pretty(&run(scene, sources, options)?)?
    );
    Ok(())
}
