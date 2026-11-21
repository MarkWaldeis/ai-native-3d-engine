use anyhow::{ensure, Result};
use engine_runtime::Simulation;
use serde_json::{json, Value};
use shared::{
    navigation::{NavMesh, NavParams, NavPoly, NavTile},
    scene::Scene,
};
use std::{collections::BTreeMap, time::Instant};

fn scene(agents: usize, ramp: bool) -> Result<Scene> {
    ensure!(agents <= 128, "workload supports 0..=128 agents");
    let height = |x: f32| if ramp { x * 0.05 } else { 0.0 };
    let mut tiles: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for x in -16_i32..16 {
        for z in -16_i32..16 {
            if x >= -12 && z < 8 {
                continue;
            }
            tiles
                .entry([x.div_euclid(16), z.div_euclid(16)])
                .or_default()
                .push(NavPoly {
                    cell: [x, z],
                    y: height(x as f32 + 0.5),
                    corners: [
                        height(x as f32),
                        height(x as f32 + 1.0),
                        height(x as f32 + 1.0),
                        height(x as f32),
                    ],
                });
        }
    }
    let mesh = NavMesh {
        version: 1,
        params: NavParams {
            cell_size: 1.0,
            tile_size: 16.0,
            bounds: Some([-16.0, -16.0, 16.0, 16.0]),
            ..Default::default()
        },
        fingerprint: "qa-prebaked-l-corridor".into(),
        tiles: tiles
            .into_iter()
            .map(|(region, polygons)| NavTile {
                region,
                fingerprint: "qa-prebaked".into(),
                polygons,
            })
            .collect(),
    };
    engine_runtime::navigation::validate(&mesh)?;
    let mut scene = Scene::default();
    scene.navmesh = Some(mesh);
    for id in 1..=agents {
        let x = -14.5 + (id % 4) as f32 * 0.1;
        scene.entities.push(serde_json::from_value(json!({"id":id,"name":format!("agent-{id}"),"kind":"box","size":[0.2,0.4,0.2],"transform":{"position":[x,height(x)+0.2,-14.5]},"nav_agent":{"speed":8.0,"offset_y":0.2,"target":[14.5,height(14.5),13.5+(id%4) as f32*0.1]}}))?);
    }
    Ok(scene)
}

fn workload(agents: usize, ramp: bool, ticks: u64) -> Result<Value> {
    ensure!(
        (1..=3600).contains(&ticks),
        "workload ticks must be in 1..=3600"
    );
    let scene = scene(agents, ramp)?;
    let mut sim = Simulation::from_sources(scene.clone(), BTreeMap::new())?;
    let mut samples = Vec::new();
    let mut times = Vec::new();
    for tick in 1..=ticks {
        let start = Instant::now();
        sim.step(1, &[])?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        if [1, 60, 180, 360].contains(&tick) || tick == ticks {
            samples.push(json!({"tick":tick,"nav_agents":sim.nav_agent_states()}));
        }
    }
    if ticks >= 600 {
        ensure!(
            sim.nav_agent_states()
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["agent"]["status"] == "arrived"),
            "workload did not reach its targets"
        );
    }
    let first = times[0];
    times.sort_by(f64::total_cmp);
    Ok(
        json!({"name":format!("{}-{agents}",if ramp {"ramp_l"} else {"flat_l"}),"agents":agents,"ticks":ticks,"scene":scene,"samples":samples,"native_step_ms":{"first":first,"p95":times[(times.len()-1)*95/100],"max":times[times.len()-1]}}),
    )
}

#[cfg(not(test))]
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() <= 1,
        "usage: cargo run --example nav_scaling -- [MAX_NATIVE_STEP_MS]"
    );
    let budget = args.first().map(|v| v.parse::<f64>()).transpose()?;
    ensure!(
        budget.is_none_or(|v| v.is_finite() && v > 0.0),
        "step budget must be finite and positive"
    );
    let mut cases = Vec::new();
    for ramp in [false, true] {
        for agents in [0, 1, 16, 64, 128] {
            let case = workload(agents, ramp, 600)?;
            if let Some(budget) = budget {
                ensure!(
                    case["native_step_ms"]["max"].as_f64().unwrap() <= budget,
                    "{} exceeded native step budget {budget} ms: {}",
                    case["name"],
                    case["native_step_ms"]
                );
            }
            cases.push(case);
        }
    }
    println!(
        "{}",
        json!({"version":1,"scope":"pre-baked quad routes only; no geometry bake, avoidance, streaming or rendering; native timings exclude snapshot/JSON cost","cases":cases})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_turn_cross_negative_tiles_climb_and_arrive_deterministically() {
        for ramp in [false, true] {
            let first = workload(1, ramp, 600).unwrap();
            let second = workload(1, ramp, 600).unwrap();
            assert_eq!(first["samples"], second["samples"]);
            assert!(
                first["samples"][0]["nav_agents"][0]["agent"]["path"]
                    .as_array()
                    .unwrap()
                    .len()
                    > 2
            );
            assert_eq!(
                first["samples"][4]["nav_agents"][0]["agent"]["status"],
                "arrived"
            );
            assert_eq!(
                first["scene"]["navmesh"]["tiles"].as_array().unwrap().len(),
                3
            );
            assert_eq!(scene(128, ramp).unwrap().entities.len(), 128);
        }
    }
    #[test]
    fn empty_and_invalid_workloads_are_bounded() {
        let empty = workload(0, false, 1).unwrap();
        assert_eq!(empty["samples"][0]["nav_agents"], json!([]));
        assert!(scene(129, false).is_err());
        assert!(workload(1, false, 0).is_err());
        assert!(workload(1, false, 3601).is_err());
    }
}
