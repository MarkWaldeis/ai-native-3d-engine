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
            "save_game" => {
                let save = self.simulation.as_ref().ok_or_else(|| anyhow::anyhow!("no simulation; call play before save_game"))?.save_game()?;
                if let Some(path) = args.get("path") {
                    let path = save_path(project, path)?;
                    let bytes = serde_json::to_vec(&save)?;
                    if bytes.len() > engine_runtime::savegame::MAX_SAVE_BYTES { anyhow::bail!("save exceeds 4 MB limit"); }
                    atomic_save(&path, |file| {
                        use std::io::Write;
                        file.write_all(&bytes)?;
                        Ok(())
                    })?;
                }
                save
            }
            "load_game" => {
                if self.simulation.is_some() { anyhow::bail!("already playing; call stop before load_game"); }
                let save = match (args.get("path"), args.get("save")) {
                    (Some(path), None) => {
                        use std::io::Read;
                        let mut bytes = Vec::new();
                        std::fs::File::open(save_path(project, path)?)?.take((engine_runtime::savegame::MAX_SAVE_BYTES + 1) as u64).read_to_end(&mut bytes)?;
                        if bytes.len() > engine_runtime::savegame::MAX_SAVE_BYTES { anyhow::bail!("save exceeds 4 MB limit"); }
                        serde_json::from_slice(&bytes)?
                    }
                    (None, Some(save)) => save.clone(),
                    _ => anyhow::bail!("load_game requires exactly one of path or save"),
                };
                let sim = Simulation::load_game(self.world.scene.clone(), project, save)?;
                let state = sim.state();
                self.simulation = Some(sim);
                state
            }
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

fn save_path(project: &std::path::Path, value: &Value) -> anyhow::Result<std::path::PathBuf> {
    let path = std::path::Path::new(value.as_str().ok_or_else(|| anyhow::anyhow!("save path must be a string"))?);
    if path.extension().and_then(|s| s.to_str()) != Some("json") { anyhow::bail!("save path must end in .json"); }
    engine_core::project_path(project, path)
}

fn atomic_save(path: &std::path::Path, write: impl FnOnce(&mut std::fs::File) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow::anyhow!("save path requires a parent directory"))?;
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|_| anyhow::anyhow!("save nonce generation failed"))?;
    let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let temporary = parent.join(format!(".ai-engine-save-{suffix}.tmp"));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    let result = (|| -> anyhow::Result<()> {
        write(&mut file)?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    let result = result.and_then(|()| { std::fs::rename(&temporary, path)?; Ok(()) });
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    #[cfg(unix)]
    if result.is_ok() { let _ = std::fs::File::open(parent).and_then(|directory| directory.sync_all()); }
    result
}

#[cfg(test)]
mod save_file_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn atomic_save_replaces_only_complete_writes_and_cleans_failures() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&root).unwrap();
        let dir=root.join(format!("atomic-save-test-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path=dir.join("save.json");
        std::fs::write(&path,b"original").unwrap();
        assert!(atomic_save(&path,|file| { file.write_all(b"partial")?; anyhow::bail!("injected write failure") }).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(),1);
        atomic_save(&path,|file| {file.write_all(b"complete")?; Ok(())}).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(),b"complete");
        let directory=dir.join("directory.json");std::fs::create_dir_all(&directory).unwrap();
        assert!(atomic_save(&directory,|file| {file.write_all(b"complete")?; Ok(())}).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(),2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
