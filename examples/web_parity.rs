use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::{json, Map, Value};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let traces: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(
        root.join("tools/web_traces.json"),
    )?)?;
    let mut cases = Map::new();
    anyhow::ensure!(!traces.is_empty(), "parity input traces must not be empty");
    for trace in traces {
        let name = trace["name"].as_str().unwrap();
        let ticks = trace["ticks"].as_u64().unwrap();
        let keys = trace["keys"].as_array().unwrap();
        let project = trace["project"]
            .as_str()
            .map(|path| root.join(path))
            .unwrap_or_else(|| root.to_path_buf());
        let mut engine = if trace["project"].is_string() {
            Engine::from_project(&project)?
        } else {
            Engine::new()
        };
        engine.assets_dir = project.join("assets");
        engine.world.load_with_root(
            &root.join(format!(
                "examples/{}/scene.json",
                trace["game"].as_str().unwrap()
            )),
            &project,
        )?;
        let mut assertions: Vec<Value> = (0..=ticks)
            .map(|tick| json!({"state":"won","equals":true,"at_tick":tick}))
            .collect();
        let samples: Vec<_> = (0..=ticks)
            .filter(|tick| tick % 30 == 0 || *tick == ticks)
            .collect();
        for tick in &samples {
            for key in keys {
                assertions.push(json!({"state":key,"equals":null,"at_tick":tick}));
            }
        }
        let result = engine
            .handle(
                "simulate",
                &json!({"ticks":ticks,"inputs":trace["inputs"],"asserts":assertions}),
            )
            .map_err(anyhow::Error::msg)?;
        let report: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap())?;
        let state = &report["final_state"]["state"];
        for (key, expected) in trace["expected"].as_object().unwrap() {
            anyhow::ensure!(
                state[key] == *expected
                    || state[key]
                        .as_f64()
                        .zip(expected.as_f64())
                        .is_some_and(|(a, b)| a == b),
                "{name}: expected {key}={expected}, got {}",
                state[key]
            );
        }
        if trace["checkpoint_required"] == true {
            anyhow::ensure!(
                state["_checkpoint"].as_f64().unwrap_or(0.0) > 0.0,
                "{name}: checkpoint never advanced"
            );
        }
        let results = report["asserts"].as_array().unwrap();
        let win_tick = results[..=ticks as usize]
            .iter()
            .find(|r| r["passed"] == true)
            .map(|r| r["at_tick"].clone());
        let checkpoints: Vec<_> = samples
            .iter()
            .enumerate()
            .map(|(i, tick)| {
                let state: Map<_, _> = keys
                    .iter()
                    .enumerate()
                    .map(|(j, key)| {
                        (
                            key.as_str().unwrap().into(),
                            results[ticks as usize + 1 + i * keys.len() + j]["actual"].clone(),
                        )
                    })
                    .collect();
                json!({"tick":tick,"state":state})
            })
            .collect();
        let mut animation = engine_runtime::Simulation::new(engine.world.scene.clone(), &project)?;
        let inputs: Vec<engine_runtime::Input> = serde_json::from_value(trace["inputs"].clone())?;
        let mut animation_samples = Vec::new();
        loop {
            if animation.tick % 30 == 0 || animation.tick == ticks {
                animation_samples.push(json!({"tick":animation.tick,"animators":animation.animator_states()}));
            }
            if animation.tick == ticks { break; }
            let events: Vec<_> = inputs.iter().filter(|i| i.tick == animation.tick).cloned().collect();
            animation.step(1, &events)?;
        }
        let summary = json!({"win_tick":win_tick,"checkpoints":checkpoints,"animation_samples":animation_samples});
        eprintln!(
            "PASS native simulate {name}: win_tick={}, final={state}",
            summary["win_tick"]
        );
        cases.insert(name.into(), summary);
    }
    println!("{}", serde_json::to_string_pretty(&cases)?);
    Ok(())
}
