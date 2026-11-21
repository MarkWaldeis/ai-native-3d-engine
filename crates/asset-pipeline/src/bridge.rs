//! Local newline-delimited JSON bridge client and owned headless process.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn compatible_ping(ping: Value) -> Result<Value> {
    if ping["bridge_protocol"].as_u64() != Some(1) {
        bail!("Blender bridge protocol mismatch; save your .blend and restart Blender with the current ai_engine_bridge.py. The existing process was not stopped.");
    }
    Ok(ping)
}

pub struct BlenderBridge {
    port: u16,
    child: Option<Child>,
    project_id: Option<String>,
    default_project: bool,
}

impl Default for BlenderBridge {
    fn default() -> Self {
        let mut bridge = Self::new(
            std::env::var("AI_ENGINE_BLENDER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(9876),
        );
        bridge.default_project = true;
        bridge
    }
}

impl BlenderBridge {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            child: None,
            project_id: None,
            default_project: false,
        }
    }

    pub fn for_project(port: u16, project_id: String) -> Self {
        Self {
            port,
            child: None,
            project_id: Some(project_id),
            default_project: false,
        }
    }

    pub fn needs_project_configuration(&self) -> bool {
        self.default_project
    }

    fn check_peer(&self, ping: Value) -> Result<Value> {
        let ping = compatible_ping(ping)?;
        if let Some(project) = &self.project_id {
            if ping["project_id"].as_str() != Some(project.as_str()) {
                bail!("Blender project identity mismatch at port {}; expected {project:?}, got {}. Existing Blender was not changed; configure a different project blender_port or AI_ENGINE_BLENDER_PORT.",self.port,ping["project_id"]);
            }
        }
        Ok(ping)
    }

    pub fn request(&self, request: Value) -> Result<Value> {
        if self.project_id.is_some() {
            let ping = self.check_peer(self.exchange(json!({"command":"ping"}))?)?;
            if request["command"] == "ping" {
                return Ok(ping);
            }
        }
        self.exchange(request)
    }

    fn exchange(&self, mut request: Value) -> Result<Value> {
        if let Some(project) = &self.project_id {
            request["project_id"] = json!(project);
        }
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
        if let Ok(ping) = self.exchange(json!({"command":"ping"})) {
            return self.check_peer(ping);
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
        let mut command = Command::new(blender);
        command
            .args(["--background", "--factory-startup", "--python"])
            .arg(script)
            .env("AI_ENGINE_BLENDER_PORT", self.port.to_string())
            .env(
                "AI_ENGINE_PROJECT_ID",
                self.project_id.as_deref().unwrap_or(""),
            );
        if let Some(project) = &self.project_id {
            command.current_dir(project);
        }
        self.child = Some(
            command
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
            if let Ok(ping) = self.exchange(json!({"command":"ping"})) {
                return self.check_peer(ping);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_checks_a_reused_peer_before_attempting_to_spawn_blender() {
        for (ping, accepted) in [
            (json!({"bridge_protocol":1}), true),
            (json!({"version":"5.3.0"}), false),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let peer = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(&mut stream).read_line(&mut request).unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(&request).unwrap()["command"],
                    "ping"
                );
                writeln!(stream, "{}", json!({"ok":true,"result":ping})).unwrap();
            });
            let result = BlenderBridge::new(port)
                .start(Path::new("not-a-blender"), Path::new("missing-bridge.py"));
            if accepted {
                assert!(result.is_ok());
            } else {
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("protocol mismatch"));
            }
            peer.join().unwrap();
        }
    }

    #[test]
    fn bridge_handshake_accepts_current_and_rejects_stale_or_malformed_peers() {
        let ping = json!({"version":"5.3.0","bridge_protocol":1});
        assert_eq!(compatible_ping(ping.clone()).unwrap(), ping);
        for ping in [
            json!({}),
            Value::Null,
            json!({"version":"5.3.0"}),
            json!({"bridge_protocol":0}),
            json!({"bridge_protocol":"1"}),
        ] {
            assert!(compatible_ping(ping)
                .unwrap_err()
                .to_string()
                .contains("save your .blend"));
        }
    }
}
