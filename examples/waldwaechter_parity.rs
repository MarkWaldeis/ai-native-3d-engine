use anyhow::{ensure, Result};
use engine_runtime::{Input, Simulation};
use serde_json::{json, Map, Value};
use shared::scene::Scene;
use std::{path::Path, time::Instant};

pub fn run() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = root.join("examples/waldwaechter");
    let scene: Scene = serde_json::from_slice(&std::fs::read(project.join("scene.json"))?)?;
    let traces: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("tools/waldwaechter/traces.json"))?)?;
    ensure!(!traces.is_empty(), "empty showcase trace list");
    let mut cases = Map::new();
    for trace in traces {
        let name = trace["name"].as_str().unwrap();
        ensure!(!cases.contains_key(name), "duplicate trace name");
        let ticks = trace["ticks"].as_u64().unwrap();
        let inputs: Vec<Input> = serde_json::from_value(trace["inputs"].clone())?;
        let mut sim = Simulation::new(scene.clone(), &project)?;
        let mut samples = vec![];
        let mut win_tick = None;
        let mut timings = vec![];
        loop {
            let summary = sim.state_summary();
            if summary["state"]["won"] == true && win_tick.is_none() {
                win_tick = Some(sim.tick);
            }
            if sim.tick % 30 == 0 || sim.tick == ticks {
                let snapshot = sim.state();
                let actors: Vec<_> = snapshot["entities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| {
                        e["tags"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|t| t == "player" || t == "guard")
                    })
                    .map(|e| json!({"id":e["id"],"position":e["transform"]["position"]}))
                    .collect();
                let state: Map<_, _> = [
                    "score",
                    "won",
                    "Erwischt",
                    "_checkpoint",
                    "_shield",
                    "_patrol_visits",
                ]
                .iter()
                .map(|k| ((*k).into(), summary["state"][k].clone()))
                .collect();
                samples.push(json!({"tick":sim.tick,"state":state,"actors":actors,"animators":sim.animator_states(),"nav_agents":sim.nav_agent_states()}));
            }
            if sim.tick == ticks {
                break;
            }
            let start = Instant::now();
            let events: Vec<_> = inputs
                .iter()
                .filter(|input| input.tick == sim.tick)
                .cloned()
                .collect();
            sim.step(1, &events)?;
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        ensure!(
            sim.state_summary()["failed"] == false,
            "{name}: script failed"
        );
        for (key, expected) in trace["expected"].as_object().unwrap() {
            let actual = sim.state_summary()["state"][key].clone();
            ensure!(
                actual == *expected
                    || actual
                        .as_f64()
                        .zip(expected.as_f64())
                        .is_some_and(|(a, b)| a == b),
                "{name}: expected {key}={expected}, got {actual}"
            );
        }
        timings.sort_by(f64::total_cmp);
        eprintln!(
            "PASS native {name}: win_tick={win_tick:?}; p95={:.3}ms; max={:.3}ms",
            timings[(timings.len() - 1) * 95 / 100],
            timings[timings.len() - 1]
        );
        cases.insert(name.into(), json!({"win_tick":win_tick,"samples":samples}));
    }
    Ok(Value::Object(cases))
}

fn main() -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&run()?)?);
    Ok(())
}
