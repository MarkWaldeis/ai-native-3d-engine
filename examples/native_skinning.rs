use engine_geo::skinning::SkinnedAsset;
use engine_runtime::{Input, Simulation};
use serde_json::{json, Value};
use shared::{animation::Animator, scene::Scene};
use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut scene: Scene =
        serde_json::from_slice(&std::fs::read(root.join("examples/sky_garden/scene.json"))?)?;
    let entity = scene
        .entities
        .iter()
        .find(|e| e.animator.is_some())
        .unwrap()
        .clone();
    let asset_path = root.join("examples/sky_garden/assets/milo_rig.glb");
    let asset = SkinnedAsset::load(&asset_path)?;
    let mut cases: Vec<(String, Option<Animator>)> = vec![("bind".into(), None)];
    for (name, time) in [
        ("idle", 0.25),
        ("walk", 0.25),
        ("jump", 0.25),
        ("walk", 0.0),
        ("jump", 0.8333333134651184),
    ] {
        let mut a = entity.animator.clone().unwrap();
        a.current_state = name.into();
        a.time = time;
        cases.push((format!("{name}-{time:.3}"), Some(a)));
    }
    let mut blend = entity.animator.clone().unwrap();
    blend.current_state = "jump".into();
    blend.time = 0.2;
    blend.previous_state = Some("walk".into());
    blend.previous_time = 0.25;
    blend.blend_time = 0.4;
    blend.blend_elapsed = 0.1;
    cases.push(("crossfade".into(), Some(blend)));
    let traces: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("tools/web_traces.json"))?)?;
    let trace = traces.iter().find(|t| t["name"] == "sky_win").unwrap();
    let inputs: Vec<Input> = serde_json::from_value(trace["inputs"].clone())?;
    let mut sim = Simulation::new(scene.clone(), root)?;
    for tick in 0..240 {
        sim.step(
            1,
            &inputs
                .iter()
                .filter(|i| i.tick == tick)
                .cloned()
                .collect::<Vec<_>>(),
        )?;
        if [0, 12, 29, 59, 76, 99, 149, 207].contains(&tick) {
            cases.push((
                format!("sim-tick-{}", tick + 1),
                sim.scene()
                    .entities
                    .iter()
                    .find(|e| e.id == entity.id)
                    .unwrap()
                    .animator
                    .clone(),
            ));
        }
    }
    let out = std::env::args().nth(1).map(PathBuf::from);
    let mut renderer = if out.is_some() {
        Some(engine_render::Renderer::new()?)
    } else {
        None
    };
    if let Some(out) = &out {
        std::fs::create_dir_all(out)?;
    }
    scene.entities = vec![entity];
    scene.entities[0].transform.position = [0.0; 3];
    scene.entities[0].mesh = shared::scene::PrimitiveKind::Gltf {
        path: asset_path.to_string_lossy().into_owned(),
    };
    scene.camera.position = [2.4, 1.8, 4.0];
    scene.camera.target = [0.0; 3];
    let mut results = Vec::new();
    for (i, (name, animator)) in cases.into_iter().enumerate() {
        let pose = asset.pose(animator.as_ref())?;
        if let (Some(renderer), Some(out)) = (&mut renderer, &out) {
            scene.entities[0].animator = animator.clone();
            std::fs::write(
                out.join(format!("{name}.png")),
                renderer.screenshot_png(&scene, i as u64, 480, 480)?,
            )?;
        }
        results.push(json!({"name":name,"animator":animator,"vertices":pose.mesh.vertices.iter().map(|v| v.position).collect::<Vec<_>>(),"node_matrices":pose.node_matrices}));
    }
    println!(
        "{}",
        json!({"asset":"examples/sky_garden/assets/milo_rig.glb","cases":results})
    );
    Ok(())
}
