//! Local newline-delimited JSON bridge client and owned headless process.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct BlenderBridge {
    port: u16,
    child: Option<Child>,
}

impl Default for BlenderBridge {
    fn default() -> Self {
        Self::new(
            std::env::var("AI_ENGINE_BLENDER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(9876),
        )
    }
}

impl BlenderBridge {
    pub fn new(port: u16) -> Self {
        Self { port, child: None }
    }

    pub fn request(&self, request: Value) -> Result<Value> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
            .with_context(|| format!("Blender bridge unavailable at {addr}; start Blender with --background --factory-startup --python blender/ai_engine_bridge.py (AI_ENGINE_BLENDER_PORT={}) or enable the add-on", self.port))?;
        stream.set_read_timeout(Some(Duration::from_secs(if request["command"] == "ping" {
            2
        } else {
            120
        })))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        writeln!(stream, "{request}")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let response: Value =
            serde_json::from_str(&line).context("invalid Blender bridge response")?;
        if response["ok"] != true {
            bail!("Blender bridge: {}", response["error"]);
        }
        Ok(response["result"].clone())
    }

    pub fn start(&mut self, blender: &Path, script: &Path) -> Result<Value> {
        if let Ok(ping) = self.request(json!({"command":"ping"})) {
            return Ok(ping);
        }
        if !script.is_file() {
            bail!("bridge script not found: {}", script.display());
        }
        if self
            .child
            .as_mut()
            .is_some_and(|c| c.try_wait().ok().flatten().is_none())
        {
            bail!("owned Blender is running but bridge is unresponsive; inspect Blender stderr");
        }
        self.child = Some(
            Command::new(blender)
                .args(["--background", "--factory-startup", "--python"])
                .arg(script)
                .env("AI_ENGINE_BLENDER_PORT", self.port.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .context("start headless Blender bridge")?,
        );
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait()? {
                bail!("Blender exited before bridge became ready: {status}");
            }
            if let Ok(ping) = self.request(json!({"command":"ping"})) {
                return Ok(ping);
            }
            if start.elapsed() > Duration::from_secs(30) {
                if let Some(mut child) = self.child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                bail!("Blender bridge did not become ready within 30s");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for BlenderBridge {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
