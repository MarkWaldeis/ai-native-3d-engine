//! asset-pipeline: finds Blender, runs `blender --background --factory-startup
//! --python <script>` with a timeout, and watches assets/ for changed files
//! (cheap mtime polling — no `notify` dependency in v0.1).

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

pub struct AssetPipeline {
    pub assets_dir: PathBuf,
    blender: Option<PathBuf>,
    known: HashMap<PathBuf, SystemTime>,
}

impl AssetPipeline {
    pub fn new(assets_dir: impl Into<PathBuf>) -> Self {
        let assets_dir = assets_dir.into();
        let _ = std::fs::create_dir_all(&assets_dir);
        Self {
            assets_dir,
            blender: find_blender(),
            known: HashMap::new(),
        }
    }

    pub fn blender_path(&self) -> Option<&Path> {
        self.blender.as_deref()
    }

    /// Run a python script in headless Blender. Returns captured stdout tail.
    /// Times out after `timeout` (default suggestion: 120s) and kills Blender.
    pub fn run_blender_script(&self, script: &Path, timeout: Duration) -> Result<String> {
        let blender = self
            .blender
            .clone()
            .context("blender.exe not found (checked PATH + Blender Foundation dir)")?;
        if !script.exists() {
            bail!("script not found: {}", script.display());
        }
        // NOTE: `-b` alone hangs on this box when opening .blend files —
        // always pair with --factory-startup (see agent-memory/blender-52-headless).
        let mut child = Command::new(blender)
            .args([
                "--background",
                "--factory-startup",
                "--python",
                &script.to_string_lossy(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn blender")?;

        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        bail!("blender timed out after {}s", timeout.as_secs());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e).context("wait blender"),
            }
        }
        let out = child.wait_with_output().context("blender output")?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stdout
            .lines()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        if !out.status.success() {
            bail!("blender exited {:?}: {tail}\n{stderr}", out.status.code());
        }
        Ok(if tail.is_empty() {
            "(blender ok, no stdout)".to_string()
        } else {
            tail
        })
    }

    /// List files under assets/ (relative paths, forward slashes).
    pub fn list_assets(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect(&self.assets_dir, &self.assets_dir, &mut out);
        out.sort();
        out
    }

    /// Cheap polling watcher: returns paths whose mtime changed since last call.
    pub fn poll_changes(&mut self) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        let files = self.list_assets();
        for rel in files {
            let p = self.assets_dir.join(&rel);
            if let Ok(meta) = std::fs::metadata(&p) {
                if let Ok(mtime) = meta.modified() {
                    match self.known.get(&p) {
                        Some(&old) if old == mtime => {}
                        _ => {
                            self.known.insert(p.clone(), mtime);
                            changed.push(p);
                        }
                    }
                }
            }
        }
        changed
    }
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, root, out);
            } else if let Ok(rel) = p.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// Locate blender.exe: PATH, then Blender Foundation install dirs (newest first).
fn find_blender() -> Option<PathBuf> {
    if let Ok(o) = Command::new("where").arg("blender").output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = s.lines().next() {
                let p = PathBuf::from(line.trim());
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let base = Path::new(r"C:\Program Files\Blender Foundation");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path().join("blender.exe"))
        .filter(|p| p.exists())
        .collect();
    dirs.sort();
    dirs.pop()
}
