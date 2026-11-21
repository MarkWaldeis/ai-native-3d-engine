use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn exchange(addr: &str, method: &str, headers: &str, body: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    write!(stream, "{method} /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        head.into(),
        body.into(),
    )
}

#[test]
fn streamable_http_negotiates_sessions_notifications_resources_and_termination() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_string();
    std::thread::spawn(move || ai_engine::http::serve(server).unwrap());
    assert_eq!(exchange(&addr, "GET", "", "").0, 405);
    let accept =
        "Accept: application/json, text/event-stream\r\nContent-Type: application/json\r\n";
    for (index, version) in engine_mcp::SUPPORTED_VERSIONS.iter().enumerate() {
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}}).to_string();
        let (status, headers, body) = exchange(&addr, "POST", accept, &init);
        assert_eq!(status, 200);
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["result"]["protocolVersion"], *version);
        let session = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("Mcp-Session-Id"))
                    .map(|(_, value)| value.trim())
            })
            .unwrap();
        assert_eq!(session.len(), 48);
        let headers =
            format!("{accept}Mcp-Session-Id: {session}\r\nMCP-Protocol-Version: {version}\r\n");
        let (status, _, body) = exchange(
            &addr,
            "POST",
            &headers,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert_eq!(status, 202);
        assert!(body.is_empty());
        let call = |method: &str, params: Value| {
            exchange(
                &addr,
                "POST",
                &headers,
                &json!({"jsonrpc":"2.0","id":2,"method":method,"params":params}).to_string(),
            )
        };
        let (_, _, body) = call("resources/read", json!({"uri":"engine://scene"}));
        let value: Value = serde_json::from_str(&body).unwrap();
        let scene: Value =
            serde_json::from_str(value["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(scene["entities"].as_array().unwrap().len(), index);
        let (status, _, body) = call(
            "tools/call",
            json!({"name":"spawn_primitive","arguments":{"kind":"box","name":"client-box"}}),
        );
        assert_eq!(status, 200);
        assert_ne!(
            serde_json::from_str::<Value>(&body).unwrap()["result"]["isError"],
            true
        );
        let (_, _, body) = call("resources/read", json!({"uri":"engine://scene"}));
        assert!(body.contains("client-box"));
        assert_eq!(exchange(&addr, "POST", &headers, "not json").0, 400);
        assert_eq!(exchange(&addr, "POST", &headers, "[]").0, 400);
        assert_eq!(exchange(&addr, "DELETE", &headers, "").0, 204);
        assert_eq!(call("ping", json!({})).0, 404);
    }
}

#[test]
fn http_rejects_wrong_accept_origin_media_type_version_and_missing_session() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_string();
    std::thread::spawn(move || ai_engine::http::serve(server).unwrap());
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#;
    for (headers, status) in [
        ("Accept: text/event-stream\r\n", 406),
        ("Content-Type: text/plain\r\n", 415),
        ("Origin: https://evil.example\r\n", 403),
        ("Origin: http://localhost.evil.example\r\n", 403),
        ("MCP-Protocol-Version: 2099-01-01\r\n", 400),
        ("Mcp-Session-Id: bogus\r\n", 404),
    ] {
        assert_eq!(exchange(&addr, "POST", headers, init).0, status);
    }
    assert_eq!(
        exchange(
            &addr,
            "POST",
            "",
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#
        )
        .0,
        400
    );
    assert_eq!(
        exchange(&addr, "POST", "Origin: http://localhost:7777\r\n", init).0,
        200
    );
}

struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/client-tests");
        std::fs::create_dir_all(&root).unwrap();
        Self(root.join(format!(
            "{}-{} project",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn initialized_templates_are_portable_playable_and_configured() {
    for template in ["coin_game", "sky_garden", "empty"] {
        let project = Project::new();
        ai_engine::project::initialize(&project.0, template).unwrap();
        let guide = std::fs::read_to_string(project.0.join("AGENTS.md")).unwrap();
        assert_eq!(guide.contains("blender_scene.py"), template == "sky_garden");
        for directory in ["scripts", "assets"] {
            assert!(project.0.join(directory).is_dir());
        }
        let mut engine = ai_engine::Engine::from_project(&project.0).unwrap();
        for config in [".mcp.json", ".cursor/mcp.json"] {
            let config: Value =
                serde_json::from_slice(&std::fs::read(project.0.join(config)).unwrap()).unwrap();
            assert_eq!(config["mcpServers"]["ai-engine"]["args"][0], "--project");
            assert_eq!(
                Path::new(
                    config["mcpServers"]["ai-engine"]["args"][1]
                        .as_str()
                        .unwrap()
                ),
                project.0.canonicalize().unwrap()
            );
        }
        if template == "empty" {
            assert!(engine.scene().unwrap()["entities"]
                .as_array()
                .unwrap()
                .is_empty());
            let sim =
                engine_runtime::Simulation::new(engine.world.scene.clone(), &project.0).unwrap();
            assert_eq!(sim.tick, 0);
        } else {
            for entity in &engine.world.scene.entities {
                if let Some(shared::components::Script::Path(path)) = &entity.script {
                    assert!(path.starts_with("scripts/"));
                    assert!(project.0.join(path).is_file());
                }
                if let shared::scene::PrimitiveKind::Gltf { path } = &entity.mesh {
                    assert!(path.starts_with("assets/"));
                    assert!(project.0.join(path).is_file());
                }
            }
            let mut validation: Value =
                serde_json::from_slice(&std::fs::read(project.0.join("validation.json")).unwrap())
                    .unwrap();
            if template == "sky_garden" {
                validation = validation["win"].clone();
            }
            let result = engine.handle("simulate", &validation).unwrap();
            let report: Value =
                serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(report["passed"], true, "{template}: {report}");
            if template == "sky_garden" {
                let recipe = std::fs::read_to_string(project.0.join("blender_scene.py")).unwrap();
                assert!(!recipe.contains("examples/sky_garden/"));
            }
        }
        assert!(ai_engine::project::initialize(&project.0, template).is_err());
        assert!(project.0.join("scene.json").is_file());
    }
    let project = Project::new();
    assert!(ai_engine::project::initialize(&project.0, "missing").is_err());
    assert!(!project.0.exists());
    std::fs::write(&project.0, b"do not overwrite").unwrap();
    assert!(ai_engine::project::initialize(&project.0, "empty").is_err());
    assert_eq!(std::fs::read(&project.0).unwrap(), b"do not overwrite");
    std::fs::remove_file(&project.0).unwrap();
}
