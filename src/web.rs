use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use shared::{
    components::Script,
    scene::{PrimitiveKind, Scene},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

const PLAYER_FILES: &[(&str, &str)] = &[
    (
        "index.html",
        include_str!("../crates/engine-web/player/index.html"),
    ),
    (
        "style.css",
        include_str!("../crates/engine-web/player/style.css"),
    ),
    (
        "player.js",
        include_str!("../crates/engine-web/player/player.js"),
    ),
    (
        "controls.js",
        include_str!("../crates/engine-web/player/controls.js"),
    ),
    (
        "vendor/three.module.js",
        include_str!("../crates/engine-web/player/vendor/three.module.js"),
    ),
    (
        "vendor/addons/loaders/GLTFLoader.js",
        include_str!("../crates/engine-web/player/vendor/addons/loaders/GLTFLoader.js"),
    ),
    (
        "vendor/addons/utils/BufferGeometryUtils.js",
        include_str!("../crates/engine-web/player/vendor/addons/utils/BufferGeometryUtils.js"),
    ),
    (
        "vendor/LICENSE.three",
        include_str!("../crates/engine-web/player/vendor/LICENSE.three"),
    ),
];

fn empty_output(out: &Path) -> Result<()> {
    if out.exists() && (!out.is_dir() || std::fs::read_dir(out)?.next().is_some()) {
        bail!(
            "out_dir must be a new or empty directory: {}",
            out.display()
        );
    }
    Ok(())
}

fn executable(name: &str) -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(home) = home {
        let path = PathBuf::from(home)
            .join(".cargo/bin")
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from(name)
}

fn run(command: &mut Command, help: &str) -> Result<()> {
    let output = command.output().with_context(|| help.to_owned())?;
    if !output.status.success() {
        bail!(
            "{help}\n{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }
    Ok(())
}

pub fn export_web(scene: &Scene, root: &Path, out: &Path) -> Result<Value> {
    empty_output(out)?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    run(
        Command::new(executable("cargo"))
            .current_dir(workspace)
            .args([
                "build",
                "-p",
                "engine-web",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
                "--target-dir",
                "target",
            ]),
        "WASM build failed; install the target with rustup target add wasm32-unknown-unknown",
    )?;
    let pkg = workspace.join("target/web-pkg");
    run(
        Command::new(executable("wasm-bindgen"))
            .current_dir(workspace)
            .arg(workspace.join("target/wasm32-unknown-unknown/release/engine_web.wasm"))
            .args(["--target", "web", "--no-typescript", "--out-dir"])
            .arg(&pkg),
        "WASM bindings failed; install cargo install wasm-bindgen-cli --version 0.2.100 --locked",
    )?;
    export_bundle(scene, root, out, &pkg)
}

pub fn export_bundle(scene: &Scene, root: &Path, out: &Path, pkg: &Path) -> Result<Value> {
    empty_output(out)?;
    let root = root.canonicalize()?;
    let mut scene = scene.clone();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (path, source) in PLAYER_FILES {
        files.insert(path.to_string(), source.as_bytes().to_vec());
    }
    for name in ["engine_web.js", "engine_web_bg.wasm"] {
        files.insert(
            format!("pkg/{name}"),
            std::fs::read(pkg.join(name)).with_context(|| format!("missing WASM bundle {name}"))?,
        );
    }
    if !files["pkg/engine_web_bg.wasm"].starts_with(b"\0asm") {
        bail!("invalid WASM package");
    }
    let mut scripts = BTreeMap::<String, String>::new();
    let mut script_paths = BTreeMap::<String, String>::new();
    let mut assets = BTreeMap::<String, String>::new();
    for entity in &mut scene.entities {
        if let Some(script) = &entity.script {
            engine_runtime::Simulation::check_script(script, &entity.name, &root)?;
            let original = match script {
                Script::Path(path) | Script::File { path } => Some(path.clone()),
                Script::Inline { .. } => None,
            };
            if let Some(original) = original {
                if !script_paths.contains_key(&original) {
                    let candidate = [root.join(&original), root.join("assets").join(&original)]
                        .into_iter()
                        .find(|p| p.is_file())
                        .context("script file not found")?
                        .canonicalize()?;
                    if !candidate.starts_with(&root) {
                        bail!("script path escapes project root: {original}");
                    }
                    let source = std::fs::read_to_string(candidate)?;
                    let path = format!("scripts/{}.rhai", script_paths.len());
                    files.insert(path.clone(), source.as_bytes().to_vec());
                    scripts.insert(path.clone(), source.clone());
                    scripts.insert(original.clone(), source);
                    script_paths.insert(original.clone(), path);
                }
                entity.script = Some(Script::Path(script_paths[&original].clone()));
            }
        }
        if let PrimitiveKind::Gltf { path } = &mut entity.mesh {
            let original = path.clone();
            let file = Path::new(&original);
            let file = if file.is_absolute() {
                file.to_path_buf()
            } else {
                root.join(file)
            };
            let bytes = std::fs::read(&file)
                .with_context(|| format!("missing GLB asset: {}", file.display()))?;
            embedded_glb(&bytes).with_context(|| {
                format!(
                    "{} must be a self-contained GLB (embed textures/buffers in Blender)",
                    file.display()
                )
            })?;
            let mesh = engine_geo::load_gltf(&file)?;
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for vertex in mesh.vertices {
                for i in 0..3 {
                    min[i] = min[i].min(vertex.position[i]);
                    max[i] = max[i].max(vertex.position[i]);
                }
            }
            if !entity.props.is_object() {
                bail!(
                    "entity {} props must be an object to store web mesh bounds",
                    entity.name
                );
            }
            entity.props["web_mesh_bounds"] = json!([min, max]);
            if !assets.contains_key(&original) {
                let target = format!("assets/{}.glb", assets.len());
                files.insert(target.clone(), bytes);
                assets.insert(original.clone(), target);
            }
            *path = assets[&original].clone();
        }
    }
    files.insert("scene.json".into(), serde_json::to_vec_pretty(&scene)?);
    files.insert("scripts.json".into(), serde_json::to_vec_pretty(&scripts)?);
    files.insert(".nojekyll".into(), vec![]);
    for directory in ["scripts", "assets", "pkg"] {
        std::fs::create_dir_all(out.join(directory))?;
    }
    for (path, bytes) in &files {
        let path = out.join(path);
        std::fs::create_dir_all(path.parent().context("output path has no parent")?)?;
        std::fs::write(path, bytes)?;
    }
    Ok(
        json!({"out_dir":out.canonicalize()?.display().to_string(),"entry":"index.html","files":files.keys().collect::<Vec<_>>(),"scripts":script_paths.len(),"assets":assets.len(),"entities":scene.entities.len(),"serve":"Serve this folder over HTTP/HTTPS, not file://"}),
    )
}

fn embedded_glb(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" || &bytes[16..20] != b"JSON" {
        bail!("expected a binary GLB with a JSON chunk");
    }
    let len = u32::from_le_bytes(bytes[12..16].try_into()?) as usize;
    let document: Value =
        serde_json::from_slice(bytes.get(20..20 + len).context("invalid GLB JSON length")?)?;
    for field in ["buffers", "images"] {
        if let Some(items) = document[field].as_array() {
            for item in items {
                if item["uri"]
                    .as_str()
                    .is_some_and(|uri| !uri.starts_with("data:"))
                {
                    bail!("external {field} URI is not portable");
                }
            }
        }
    }
    Ok(())
}
