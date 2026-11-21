use engine_geo::skinning::SkinnedAsset;
use engine_render::{inspection, Renderer};
use serde_json::{json, Value};
use shared::{animation::Animator, scene::Scene};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn character() -> Scene {
    let mut scene: Scene = serde_json::from_slice(
        &std::fs::read(root().join("examples/sky_garden/scene.json")).unwrap(),
    )
    .unwrap();
    scene.entities.retain(|e| e.animator.is_some());
    let entity = &mut scene.entities[0];
    entity.mesh = shared::scene::PrimitiveKind::Gltf {
        path: root()
            .join("examples/sky_garden/assets/milo_rig.glb")
            .to_string_lossy()
            .into_owned(),
    };
    entity.transform.position = [0.0; 3];
    scene.camera.position = [2.4, 1.8, 4.0];
    scene.camera.target = [0.0; 3];
    scene
}
fn glb(document: Value, mut binary: Vec<u8>) -> Vec<u8> {
    let mut text = serde_json::to_vec(&document).unwrap();
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let mut bytes = Vec::new();
    for value in [
        0x46546c67_u32,
        2,
        (28 + text.len() + binary.len()) as u32,
        text.len() as u32,
        0x4e4f534a,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(text);
    bytes.extend((binary.len() as u32).to_le_bytes());
    bytes.extend(0x004e4942_u32.to_le_bytes());
    bytes.extend(binary);
    bytes
}
fn rig() -> (Value, Vec<u8>) {
    let mut bin = Vec::new();
    let mut views = Vec::new();
    for data in [
        vec![0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        vec![0.5, 1.5, 0.0, 0.0].repeat(3),
        vec![0.0, 1.0],
        vec![2.0, 0.0, 0.0, 4.0, 0.0, 0.0],
    ] {
        let offset = bin.len();
        for n in data {
            bin.extend(n.to_le_bytes());
        }
        views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":bin.len()-offset}));
    }
    let offset = bin.len();
    for _ in 0..3 {
        for j in [0_u16, 1, 0, 0] {
            bin.extend(j.to_le_bytes());
        }
    }
    views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":bin.len()-offset}));
    (
        json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":bin.len()}],"bufferViews":views,
      "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
        {"bufferView":1,"componentType":5126,"count":3,"type":"VEC4"},
        {"bufferView":2,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},
        {"bufferView":3,"componentType":5126,"count":2,"type":"VEC3"},
        {"bufferView":4,"componentType":5123,"count":3,"type":"VEC4"}],
      "meshes":[{"primitives":[{"attributes":{"POSITION":0,"WEIGHTS_0":1,"JOINTS_0":4}}]}],
      "nodes":[{"mesh":0,"skin":0},{},{"translation":[2,0,0]},{"mesh":0,"translation":[0,0,3]}],
      "skins":[{"joints":[1,2]}],"scenes":[{"nodes":[0,1,2,3]}],"scene":0,
      "animations":[{"name":"walk","samplers":[{"input":2,"output":3,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":2,"path":"translation"}}]}]}),
        bin,
    )
}
fn animator() -> Animator {
    serde_json::from_value(json!({"clips":[{"name":"walk","duration":1}],"machine":{"initial":"walk","states":{"walk":{"clip":"walk"}}}})).unwrap()
}

#[test]
fn weighted_skinning_mixed_meshes_time_sampling_and_independent_instances() {
    let (document, bin) = rig();
    let asset = SkinnedAsset::from_slice(&glb(document, bin)).unwrap();
    let bind = asset.pose(None).unwrap();
    assert_eq!(bind.mesh.vertices.len(), 6);
    assert_eq!(bind.mesh.vertices[0].position, [1.5, 0.0, 0.0]);
    assert_eq!(bind.mesh.vertices[3].position, [0.0, 0.0, 3.0]);
    let mut animator = animator();
    animator.time = 0.5;
    let pose = asset.pose(Some(&animator)).unwrap();
    assert_eq!(pose.mesh.vertices[0].position, [2.25, 0.0, 0.0]);
    animator.time = 0.0;
    assert_eq!(
        asset.pose(Some(&animator)).unwrap().mesh.vertices[0].position,
        bind.mesh.vertices[0].position
    );
    assert_eq!(
        asset.pose(None).unwrap().node_matrices,
        bind.node_matrices,
        "sampling cannot mutate shared asset data"
    );
    animator.time = 1.0;
    let end = asset.pose(Some(&animator)).unwrap();
    animator.time = 5.0;
    assert_eq!(
        asset.pose(Some(&animator)).unwrap().node_matrices,
        end.node_matrices,
        "sample clips at the supplied sim time, clamped to key endpoints"
    );
}

#[test]
fn malformed_missing_external_and_unsupported_animation_data_fail_closed() {
    let (document, bin) = rig();
    let cases: Vec<Box<dyn Fn(&mut Value)>> = vec![
        Box::new(|d| {
            d["accessors"]
                .as_array_mut()
                .unwrap()
                .push(json!({"componentType":5126,"count":2,"type":"MAT4"}));
            d["skins"][0]["inverseBindMatrices"] = json!(5);
        }),
        Box::new(|d| {
            d["bufferViews"][0]["byteStride"] = json!(4);
        }),
        Box::new(|d| {
            d["buffers"][0]["uri"] = json!("https://invalid.example/buffer.bin");
        }),
        Box::new(|d| {
            d["meshes"][0]["primitives"][0]["attributes"]
                .as_object_mut()
                .unwrap()
                .remove("WEIGHTS_0");
        }),
        Box::new(|d| {
            d["accessors"][4]["count"] = json!(1);
        }),
        Box::new(|d| {
            d["skins"][0]["joints"] = json!([1]);
        }),
        Box::new(|d| {
            d["animations"][0]["channels"][0]["target"]["path"] = json!("weights");
        }),
        Box::new(|d| {
            d["nodes"][1]["children"] = json!([2]);
            d["nodes"][2]["children"] = json!([1]);
        }),
        Box::new(|d| {
            d["accessors"][3]["count"] = json!(1);
        }),
    ];
    for (i, mutate) in cases.iter().enumerate() {
        let mut d = document.clone();
        mutate(&mut d);
        assert!(
            SkinnedAsset::from_slice(&glb(d, bin.clone())).is_err(),
            "invalid asset {i}"
        );
    }
    let mut zero = bin.clone();
    zero[36..84].fill(0);
    assert!(SkinnedAsset::from_slice(&glb(document.clone(), zero)).is_err());
    assert!(SkinnedAsset::from_slice(&glb(
        json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":4}],"scenes":[{}],"scene":0}),
        vec![0; 4]
    ))
    .is_err());
    let asset = SkinnedAsset::from_slice(&glb(document, bin)).unwrap();
    let mut a = animator();
    a.current_state = "missing".into();
    assert!(asset.pose(Some(&a)).is_err());
    a = animator();
    a.machine.states.get_mut("walk").unwrap().clip = "missing".into();
    assert!(asset.pose(Some(&a)).is_err());
    a = animator();
    a.time = f64::NAN;
    assert!(asset.pose(Some(&a)).is_err());
    a = animator();
    a.blend_elapsed = -1.0;
    assert!(asset.pose(Some(&a)).is_err());
}

#[test]
fn real_blender_asset_deforms_bounds_and_native_frame_at_the_same_scene_revision() {
    let mut scene = character();
    let mut animator = scene.entities[0].animator.clone().unwrap();
    animator.current_state = "idle".into();
    animator.time = 0.25;
    scene.entities[0].animator = Some(animator.clone());
    let idle = inspection::bounds(&scene.entities[0]).unwrap();
    let mut renderer = match Renderer::new() {
        Ok(r) => r,
        Err(error) => {
            eprintln!("SKIP native skin framebuffer: {error}");
            return;
        }
    };
    let a = renderer.render_rgba(&scene, 7, 320, 320).unwrap();
    animator.current_state = "walk".into();
    scene.entities[0].animator = Some(animator.clone());
    let walk = inspection::bounds(&scene.entities[0]).unwrap();
    assert_ne!((idle.min, idle.max), (walk.min, walk.max));
    let b = renderer.render_rgba(&scene, 7, 320, 320).unwrap();
    assert_ne!(
        a, b,
        "bone pose must change native pixels without a scene-revision bump"
    );
    assert_eq!(
        b,
        renderer.render_rgba(&scene, 7, 320, 320).unwrap(),
        "rendering must not advance animation"
    );
    animator.current_state = "idle".into();
    scene.entities[0].animator = Some(animator);
    assert_eq!(
        a,
        renderer.render_rgba(&scene, 7, 320, 320).unwrap(),
        "restart/loop must return to identical pixels"
    );
    scene.entities[0].animator.as_mut().unwrap().current_state = "missing".into();
    assert!(renderer.render_rgba(&scene, 7, 320, 320).is_err());
}
