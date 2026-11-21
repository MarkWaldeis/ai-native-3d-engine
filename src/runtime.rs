use crate::Engine;
use engine_runtime::{report, Input, Simulation, MAX_TICKS};
use serde_json::{json, Value};
use shared::mcp_protocol::{image_result, text_result};
use shared::scene::Scene;

fn ticks(args: &Value) -> anyhow::Result<u64> {
    args["ticks"]
        .as_u64()
        .filter(|n| *n <= MAX_TICKS)
        .ok_or_else(|| anyhow::anyhow!("ticks must be an integer in 0..={MAX_TICKS}"))
}
fn inputs(args: &Value) -> anyhow::Result<Vec<Input>> {
    Ok(args
        .get("inputs")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default())
}

impl Engine {
    pub(crate) fn render_scene_base64(
        &mut self,
        scene: &Scene,
        width: u32,
        height: u32,
    ) -> anyhow::Result<String> {
        self.render_revision = self.render_revision.wrapping_add(1);
        let revision = self.render_revision;
        self.renderer()
            .map_err(anyhow::Error::msg)?
            .screenshot_base64(scene, revision, width, height)
    }

    pub(crate) fn runtime_tool(&mut self, name: &str, args: &Value) -> anyhow::Result<Value> {
        let project = self
            .assets_dir
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let value = match name {
            "play" => {
                if self.simulation.is_some() {
                    anyhow::bail!("already playing; call stop before play");
                }
                self.simulation = Some(Simulation::new(self.world.scene.clone(), project)?);
                self.simulation.as_ref().unwrap().state()
            }
            "step" => {
                let ticks = ticks(args)?;
                let inputs = inputs(args)?;
                let simulation = self
                    .simulation
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("no simulation; call play before step"))?;
                simulation.step(ticks, &inputs)?;
                simulation.state()
            }
            "stop" => {
                self.simulation = None;
                json!({"playing":false, "authoring_revision":self.world.revision})
            }
            "get_sim_state" => self
                .simulation
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no simulation; call play first"))?
                .state(),
            "query" => {
                let scene = self
                    .simulation
                    .as_ref()
                    .map(|s| s.scene())
                    .unwrap_or_else(|| self.world.scene.clone());
                report::query(&scene, args)?
            }
            "simulate" => {
                let ticks = ticks(args)?;
                let inputs = inputs(args)?;
                let assertions = args
                    .get("asserts")
                    .map(|v| {
                        v.as_array()
                            .ok_or_else(|| anyhow::anyhow!("asserts must be an array"))
                    })
                    .transpose()?
                    .cloned()
                    .unwrap_or_default();
                let screenshot = args
                    .get("screenshot")
                    .map(|v| {
                        v.as_bool()
                            .ok_or_else(|| anyhow::anyhow!("screenshot must be boolean"))
                    })
                    .transpose()?
                    .unwrap_or(false);
                report::validate(&assertions, ticks)?;
                let mut simulation = Simulation::new(self.world.scene.clone(), project)?;
                simulation.validate_step(ticks, &inputs)?;
                let mut results: Vec<Option<Value>> = vec![None; assertions.len()];
                loop {
                    for (index, assertion) in assertions.iter().enumerate() {
                        let at = assertion
                            .get("at_tick")
                            .and_then(Value::as_u64)
                            .unwrap_or(ticks);
                        if at == simulation.tick {
                            results[index] = Some(report::evaluate(&simulation, assertion));
                        }
                    }
                    if simulation.tick == ticks {
                        break;
                    }
                    let events: Vec<_> = inputs
                        .iter()
                        .filter(|e| e.tick == simulation.tick)
                        .cloned()
                        .collect();
                    simulation.step(1, &events)?;
                }
                let results: Vec<_> = results.into_iter().map(Option::unwrap).collect();
                let report = json!({"ticks":ticks, "passed":results.iter().all(|r| r["passed"] == true), "asserts":results, "final_state":simulation.state()});
                let mut result = text_result(report.to_string());
                if screenshot {
                    let png = self.render_scene_base64(&simulation.scene(), 960, 540)?;
                    result["content"]
                        .as_array_mut()
                        .unwrap()
                        .push(image_result(png)["content"][0].clone());
                }
                return Ok(result);
            }
            _ => unreachable!(),
        };
        Ok(text_result(value.to_string()))
    }
}
