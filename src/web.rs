use anyhow::{bail, Context, Result};
use crate::asset_bundle::content_asset;
use serde_json::{json, Value};
use shared::{
    components::Script,
    scene::{PrimitiveKind, Scene},
};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

const PLAYER_FILES: &[(&str, &str)] = &[
    ("animation.js", include_str!("../crates/engine-web/player/animation.js")),
    ("favicon.svg", include_str!("../crates/engine-web/favicon.svg")),
    ("hlod.js", include_str!("../crates/engine-web/player/hlod.js")),
    ("terrain.js", include_str!("../crates/engine-web/player/terrain.js")),
    ("surfaces.js", include_str!("../crates/engine-web/player/surfaces.js")),
    (
        "nature.js",
        include_str!("../crates/engine-web/player/nature.js"),
    ),
    (
        "nature-settings.js",
        include_str!("../crates/engine-web/player/nature-settings.js"),
    ),
    (
        "streaming.js",
        include_str!("../crates/engine-web/player/streaming.js"),
    ),
    (
        "world.js",
        include_str!("../crates/engine-web/player/world.js"),
    ),
    (
        "effects.js",
        include_str!("../crates/engine-web/player/effects.js"),
    ),
    (
        "presentation.js",
        include_str!("../crates/engine-web/player/presentation.js"),
    ),
    (
        "graphics-settings.js",
        include_str!("../crates/engine-web/player/graphics-settings.js"),
    ),
    ("hdr.js", include_str!("../crates/engine-web/player/hdr.js")),
    (
        "gpu-timer.js",
        include_str!("../crates/engine-web/player/gpu-timer.js"),
    ),
    (
        "contact-occlusion.js",
        include_str!("../crates/engine-web/player/contact-occlusion.js"),
    ),
    ("sky.js", include_str!("../crates/engine-web/player/sky.js")),
    (
        "materials.js",
        include_str!("../crates/engine-web/player/materials.js"),
    ),
    (
        "local-lights.js",
        include_str!("../crates/engine-web/player/local-lights.js"),
    ),
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
    ("fps.js", include_str!("../crates/engine-web/player/fps.js")),
    ("fps_core.js", include_str!("../crates/engine-web/player/fps_core.js")),
    ("fx.js", include_str!("../crates/engine-web/player/fx.js")),
    ("hud.js", include_str!("../crates/engine-web/player/hud.js")),
    (
        "vendor/three.module.js",
        include_str!("../crates/engine-web/player/vendor/three.module.js"),
    ),
    (
        "vendor/addons/loaders/GLTFLoader.js",
        include_str!("../crates/engine-web/player/vendor/addons/loaders/GLTFLoader.js"),
    ),
    (
        "vendor/addons/loaders/RGBELoader.js",
        include_str!("../crates/engine-web/player/vendor/addons/loaders/RGBELoader.js"),
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
    let mut navigation_scene=scene.clone();
    if let Some(mesh)=&scene.navmesh {navigation_scene.navmesh=Some(engine_runtime::navigation::bake(scene,root,mesh.params.clone())?.0);}
    let scene=&navigation_scene;
    let out = engine_core::project_path(root, out)?;
    let out = out.as_path();
    engine_core::validate_scene_paths(scene, root)?;
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
    engine_core::validate_scene_paths(scene, root)?;
    empty_output(out)?;
    let root = root.canonicalize()?;
    let mut scene = scene.clone();
    scene.prefabs.clear();
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
    let mut content = BTreeMap::<u64, Vec<String>>::new();
    let mut bounds = BTreeMap::<String, Value>::new();
    let mut terrains = BTreeMap::<String, (String, engine_terrain::Terrain)>::new();
    for entity in &mut scene.entities {
        if let Some(props)=entity.props.as_object_mut(){props.remove("_prefab");}
        if let Some(original) = entity.props.get("ai_terrain").and_then(Value::as_str).map(str::to_owned) {
            if !terrains.contains_key(&original) {
                let source = engine_core::project_path(&root, Path::new(&original))?;
                let terrain = engine_terrain::Terrain::load(&source)?;
                let target = format!("terrain/{}", terrains.len());
                terrains.insert(original.clone(), (target, terrain));
            }
            entity.props["ai_terrain"] = json!(format!("{}/terrain.json", terrains[&original].0));
        }
        if let Some(script) = &entity.script {
            engine_runtime::Simulation::check_script(script, &entity.name, &root)?;
            let original = match script {
                Script::Path(path) | Script::File { path } => Some(path.clone()),
                Script::Inline { .. } | Script::Versioned { .. } => None,
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
            if !entity.props.is_object(){bail!("GLB entity props must be an object");}
            let original = path.clone();
            if !bounds.contains_key(&original) {
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
            bounds.insert(original.clone(),json!([min,max]));
            {
                let target = content_asset(bytes,&mut files,&mut content);
                assets.insert(original.clone(), target);
            }
            }
            entity.props["web_mesh_bounds"]=bounds[&original].clone();
            *path = assets[&original].clone();
        }
        if let Some(lods)=entity.props.get_mut("web_lods") {
            for path in lods.as_object_mut().context("web_lods must be an object")?.values_mut(){
                let original=path.as_str().context("LOD path must be a string")?.to_string();
                if !assets.contains_key(&original){let file=engine_core::project_path(&root,Path::new(&original))?;let bytes=std::fs::read(file)?;embedded_glb(&bytes)?;let target=content_asset(bytes,&mut files,&mut content);assets.insert(original.clone(),target);}
                *path=json!(assets[&original]);
            }
        }
        if let Some(original) = entity.props.get("ai_viewmodel").and_then(Value::as_str).filter(|p| !p.is_empty()).map(str::to_owned) {
            if !assets.contains_key(&original) {
                let file = [root.join(&original), root.join("assets").join(&original)]
                    .into_iter()
                    .find(|p| p.is_file())
                    .with_context(|| format!("missing ai_viewmodel GLB: {original}"))?
                    .canonicalize()?;
                if !file.starts_with(&root) {
                    bail!("ai_viewmodel path escapes project root: {original}");
                }
                let bytes = std::fs::read(&file)?;
                embedded_glb(&bytes).with_context(|| format!("{} must be a self-contained GLB", file.display()))?;
                let target = content_asset(bytes, &mut files, &mut content);
                assets.insert(original.clone(), target);
            }
            entity.props["ai_viewmodel"] = json!(assets[&original]);
        }
    }
    let has_environment = bundle_environment(&mut scene, &root, &mut files)?;
    let material_maps = bundle_material_maps(&mut scene, &root, &mut files)?;
    if let Some(mesh)=&scene.navmesh {files.insert("navigation.nav.json".into(),serde_json::to_vec(mesh)?);}
    files.insert("scene.json".into(), serde_json::to_vec_pretty(&scene)?);
    files.insert("scripts.json".into(), serde_json::to_vec_pretty(&scripts)?);
    files.insert(".nojekyll".into(), vec![]);
    for directory in ["scripts", "assets", "pkg"] {
        std::fs::create_dir_all(out.join(directory))?;
    }
    for (target, terrain) in terrains.values() {
        engine_terrain::web::write_web_bundle(terrain, &out.join(target), engine_terrain::web::DEFAULT_COARSE_STEP)?;
    }
    for (path, bytes) in &files {
        let path = out.join(path);
        std::fs::create_dir_all(path.parent().context("output path has no parent")?)?;
        std::fs::write(path, bytes)?;
    }
    Ok(
        json!({"out_dir":out.canonicalize()?.display().to_string(),"entry":"index.html","files":files.keys().collect::<Vec<_>>(),"scripts":script_paths.len(),"assets":content.values().map(Vec::len).sum::<usize>() + usize::from(has_environment) + material_maps,"terrain":terrains.len(),"entities":scene.entities.len(),"serve":"Serve this folder over HTTP/HTTPS, not file://"}),
    )
}

fn bundle_material_maps(
    scene: &mut Scene,
    root: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<usize> {
    const MAX_BYTES: usize = 16 * 1024 * 1024;
    const MAX_PIXELS: usize = 32 * 1024 * 1024;
    const MAX_BUNDLE_BYTES: usize = 128 * 1024 * 1024;
    let mut paths = BTreeMap::new();
    let mut pixels = 0;
    let mut total_bytes = 0;
    for entity in &mut scene.entities {
        if matches!(entity.mesh, PrimitiveKind::Gltf { .. }) {
            continue;
        }
        let Some(authored) = entity.props.get_mut("ai_material") else {
            continue;
        };
        let was_string = authored.is_string();
        let mut material = match authored.as_str() {
            Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
            None => authored.clone(),
        };
        let mut changed = false;
        for field in ["base_color_map", "normal_map", "orm_map"] {
            let Some(reference) = material.get_mut(field) else {
                continue;
            };
            let path = reference.as_str().map(Path::new)
                .with_context(|| format!("ai_material.{field} must be a project-relative .png path"))?;
            anyhow::ensure!(
                path.is_relative() && path.extension().and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png")),
                "ai_material.{field} must be a project-relative .png path"
            );
            let source = engine_core::project_path(root, path)
                .with_context(|| format!("invalid ai_material.{field} path"))?;
            let target = if let Some(target) = paths.get(&source) {
                target
            } else {
                anyhow::ensure!(source.is_file(), "material map not found: {}", path.display());
                let mut bytes = Vec::new();
                std::fs::File::open(&source)?.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
                anyhow::ensure!(bytes.len() <= MAX_BYTES, "material map exceeds 16 MiB: {}", path.display());
                total_bytes += bytes.len();
                anyhow::ensure!(total_bytes <= MAX_BUNDLE_BYTES, "material maps exceed 128 MiB per export");
                pixels += check_material_png(&bytes)
                    .with_context(|| format!("invalid material PNG: {}", path.display()))?;
                anyhow::ensure!(pixels <= MAX_PIXELS, "material maps exceed 32 Mi pixels per export");
                let target = format!("assets/material/{}.png", paths.len());
                files.insert(target.clone(), bytes);
                paths.entry(source).or_insert(target)
            };
            *reference = json!(target);
            changed = true;
        }
        if changed {
            *authored = if was_string { json!(serde_json::to_string(&material)?) } else { material };
        }
    }
    Ok(paths.len())
}

fn check_material_png(bytes: &[u8]) -> Result<usize> {
    let mut reader = png::Decoder::new(bytes).read_info()?;
    let info = reader.info();
    anyhow::ensure!(
        info.width <= 4096 && info.height <= 4096
            && info.bit_depth == png::BitDepth::Eight
            && info.animation_control.is_none(),
        "expected a static 8-bit PNG at most 4096×4096"
    );
    let pixels = info.width as usize * info.height as usize;
    reader.next_frame(&mut vec![0; reader.output_buffer_size()])?;
    reader.finish()?;
    Ok(pixels)
}

fn bundle_environment(
    scene: &mut Scene,
    root: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<bool> {
    let authored = scene
        .entities
        .iter_mut()
        .find(|entity| entity.tags.iter().any(|tag| tag == "player"))
        .and_then(|entity| entity.props.get_mut("ai_graphics"));
    let Some(authored) = authored else {
        return Ok(false);
    };
    let mut config = match authored.as_str() {
        Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        None => authored.clone(),
    };
    let Some(path) = config.get_mut("environment_map") else {
        return Ok(false);
    };
    let path = path
        .as_str()
        .map(Path::new)
        .context("ai_graphics.environment_map must be a project-relative .hdr path")?;
    anyhow::ensure!(
        path.is_relative()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("hdr")),
        "ai_graphics.environment_map must be a project-relative .hdr path"
    );
    let file = engine_core::project_path(root, path)
        .context("invalid ai_graphics.environment_map path")?;
    anyhow::ensure!(
        file.is_file(),
        "environment map not found: {}",
        path.display()
    );
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(&file)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= MAX_BYTES, "environment map exceeds 32 MiB");
    check_environment_header(&bytes)
        .with_context(|| format!("invalid Radiance environment map: {}", path.display()))?;
    let target = "assets/environment.hdr";
    files.insert(target.to_owned(), bytes);
    config["environment_map"] = json!(target);
    *authored = config;
    Ok(true)
}

fn check_environment_header(bytes: &[u8]) -> Result<()> {
    let mut lines = bytes[..bytes.len().min(4096)].split_inclusive(|&byte| byte == b'\n');
    let mut offset = 0;
    let mut read = || -> Result<&str> {
        let line = lines
            .next()
            .filter(|line| line.ends_with(b"\n") && line.len() <= 128)
            .context("missing or oversized Radiance header line")?;
        offset += line.len();
        Ok(std::str::from_utf8(line)?.trim_end_matches(['\r', '\n']))
    };
    anyhow::ensure!(
        matches!(read()?, "#?RADIANCE" | "#?RGBE"),
        "expected Radiance HDR signature"
    );
    let mut format = None;
    loop {
        let line = read()?.trim_ascii();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("FORMAT=") {
            format = Some(value);
        }
        anyhow::ensure!(
            line.split_ascii_whitespace().next() != Some("-Y"),
            "expected a blank line before dimensions"
        );
    }
    anyhow::ensure!(
        format == Some("32-bit_rle_rgbe"),
        "expected Radiance RGBE format"
    );
    let dimensions: Vec<_> = read()?.split_ascii_whitespace().collect();
    let ["-Y", height, "+X", width] = dimensions[..] else {
        bail!("expected Radiance -Y/+X orientation");
    };
    anyhow::ensure!(
        [width, height]
            .iter()
            .all(|value| value.bytes().all(|b| b.is_ascii_digit())),
        "expected decimal Radiance dimensions"
    );
    let width: usize = width.parse()?;
    let height: usize = height.parse()?;
    anyhow::ensure!(
        (64..=4096).contains(&width) && width % 2 == 0 && height == width / 2,
        "environment map must be a 2:1 panorama (64–4096 pixels wide)"
    );
    let image = &bytes[offset..];
    anyhow::ensure!(image.len() >= 4, "missing RGBE pixel data");
    if image[..2] != [2, 2] || image[2] & 0x80 != 0 {
        anyhow::ensure!(
            image.len() == width * height * 4,
            "invalid flat RGBE pixel data length"
        );
    } else {
        check_environment_scanlines(image, width, height)?;
    }
    Ok(())
}

fn check_environment_scanlines(mut image: &[u8], width: usize, height: usize) -> Result<()> {
    let [hi, lo] = (width as u16).to_be_bytes();
    for _ in 0..height {
        image = image
            .strip_prefix(&[2, 2, hi, lo])
            .context("invalid RGBE scanline header")?;
        for _ in 0..4 {
            let mut pixels = 0;
            while pixels < width {
                let (&packet, rest) = image.split_first().context("truncated RGBE scanline")?;
                let (count, bytes) = if packet > 128 {
                    (usize::from(packet - 128), 1)
                } else {
                    (usize::from(packet), usize::from(packet))
                };
                pixels += count;
                anyhow::ensure!(count > 0 && pixels <= width, "invalid RGBE scanline packet");
                image = rest
                    .get(bytes..)
                    .context("truncated RGBE scanline packet")?;
            }
        }
    }
    anyhow::ensure!(image.is_empty(), "unexpected data after RGBE scanlines");
    Ok(())
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
