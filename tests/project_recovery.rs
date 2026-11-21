use ai_engine::Engine;
use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn check_net_serve_project_order(project_first: bool) {
    let project=Project::new();
    let port=port();
    let addr=format!("127.0.0.1:{port}");
    let mut args=vec!["--net-serve","scene.json","--addr",&addr,
        "--allow-origin","https://game.example","--allow-origin","http://second.example:8000"];
    if project_first {args.splice(0..0,["--project",project.0.to_str().unwrap()]);}
    else {args.extend(["--project",project.0.to_str().unwrap()]);}
    let child=Command::new(env!("CARGO_BIN_EXE_ai-engine")).args(args)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
    let mut server=Server{child,port};
    let start=Instant::now();
    loop {
        assert!(server.child.try_wait().unwrap().is_none(),"network server exited: project_first={project_first}");
        if TcpStream::connect(("127.0.0.1",port)).is_ok() {break;}
        assert!(start.elapsed()<Duration::from_secs(30),"network server did not listen");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn net_serve_accepts_project_before_mode_and_repeatable_origins() {
    check_net_serve_project_order(true);
}

#[test]
fn net_serve_accepts_project_after_mode_and_repeatable_origins() {
    check_net_serve_project_order(false);
}

struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "engine-project-recovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        ai_engine::project::initialize(&path, "empty").unwrap();
        assert!(std::fs::read_to_string(path.join(".gitignore"))
            .unwrap()
            .contains(".ai-engine/"));
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn call(engine: &mut Engine, name: &str, args: Value) -> Value {
    let response = engine
        .handle(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap()
}
fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
struct Server {
    child: Child,
    port: u16,
}
impl Server {
    fn start(root: &Path) -> Self {
        let port = port();
        let child = Command::new(env!("CARGO_BIN_EXE_ai-engine"))
            .args([
                "--project",
                root.to_str().unwrap(),
                "--serve",
                &format!("127.0.0.1:{port}"),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut server = Self { child, port };
        let start = Instant::now();
        loop {
            assert!(
                server.child.try_wait().unwrap().is_none(),
                "HTTP engine exited during startup"
            );
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(10));
        }
        server
    }
    fn request(&self, session: Option<&str>, body: Value) -> (String, Value) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let body = body.to_string();
        let session = session
            .map(|s| format!("Mcp-Session-Id: {s}\r\nMCP-Protocol-Version: 2025-06-18\r\n"))
            .unwrap_or_default();
        write!(stream,"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nConnection: close\r\n{session}Content-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 200"), "{response}");
        let session = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("mcp-session-id"))
                    .map(|(_, v)| v.trim().to_string())
            })
            .unwrap_or_default();
        (session, serde_json::from_str(body).unwrap())
    }
    fn initialize(&self) -> String {
        self.request(None,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"restart-test","version":"1"}}})).0
    }
    fn tool(&self, session: &str, name: &str, args: Value) -> Result<Value, String> {
        let response=self.request(Some(session),json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}})).1;
        if let Some(error) = response.get("error") {
            return Err(error.to_string());
        }
        let result = &response["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        if result["isError"] == true {
            Err(text.into())
        } else {
            Ok(serde_json::from_str(text).unwrap())
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn killed_http_server_resumes_scene_journals_live_claims_and_expired_takeover() {
    let project = Project::new();
    let server = Server::start(&project.0);
    let session = server.initialize();
    let tool = |name, args| server.tool(&session, name, args).unwrap();
    let a = tool("spawn_primitive", json!({"kind":"box","name":"persistent"}))["id"]
        .as_u64()
        .unwrap();
    let removed = tool("spawn_primitive", json!({"kind":"box"}))["id"]
        .as_u64()
        .unwrap();
    tool("remove_entity", json!({"id":removed}));
    let c = tool("spawn_primitive", json!({"kind":"box"}))["id"]
        .as_u64()
        .unwrap();
    tool(
        "write_script",
        json!({"ids":[a],"path":"scripts/test.rhai","source":"fn on_start(ctx){ctx.state_set(\"marker\",7);}"}),
    );
    let stable = tool("get_revision", json!({})).as_u64().unwrap();
    tool("set_color", json!({"id":a,"color":[1,0,0,1]}));
    tool(
        "claim_entities",
        json!({"ids":[a],"agent":"original","ttl_s":60}),
    );
    tool(
        "claim_entities",
        json!({"ids":[c],"agent":"original","ttl_s":1}),
    );
    let live = tool("task_create", json!({"title":"live"}))["id"]
        .as_u64()
        .unwrap();
    let short = tool("task_create", json!({"title":"takeover"}))["id"]
        .as_u64()
        .unwrap();
    tool(
        "task_claim",
        json!({"id":live,"agent":"original","ttl_s":60}),
    );
    tool(
        "task_claim",
        json!({"id":short,"agent":"original","ttl_s":1}),
    );
    let revision = tool("get_revision", json!({}));
    let journal = tool("get_changes_since", json!({"revision":0}));
    let tasks = tool("get_task_changes_since", json!({"sequence":0}));
    drop(server);
    std::fs::write(
        project.0.join("scripts/test.rhai"),
        "fn on_start(ctx){ctx.state_set(\"marker\",99);}",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let server = Server::start(&project.0);
    let session = server.initialize();
    let tool = |name, args| server.tool(&session, name, args).unwrap();
    assert_eq!(tool("get_revision", json!({})), revision);
    assert_eq!(tool("get_changes_since", json!({"revision":0})), journal);
    assert_eq!(tool("get_task_changes_since", json!({"sequence":0})), tasks);
    let locks = tool("list_locks", json!({}));
    assert_eq!(locks.as_array().unwrap().len(), 1);
    assert_eq!(locks[0]["id"], a);
    assert!(server
        .tool(&session, "set_color", json!({"id":a,"agent":"new"}))
        .unwrap_err()
        .contains("locked"));
    assert!(server
        .tool(&session, "task_claim", json!({"id":live,"agent":"new"}))
        .is_err());
    assert!(server
        .tool(
            &session,
            "task_complete",
            json!({"id":short,"agent":"original","lease":1})
        )
        .unwrap_err()
        .contains("expired"));
    let takeover = tool("task_claim", json!({"id":short,"agent":"new","ttl_s":60}));
    assert_eq!(takeover["lease"], 2);
    assert!(server
        .tool(
            &session,
            "task_complete",
            json!({"id":short,"agent":"original","lease":1})
        )
        .is_err());
    tool("task_complete", json!({"id":short,"agent":"new","lease":2}));
    tool(
        "task_renew",
        json!({"id":live,"agent":"original","lease":1,"ttl_s":60}),
    );
    tool(
        "task_release",
        json!({"id":live,"agent":"original","lease":1}),
    );
    assert_eq!(
        tool("task_claim", json!({"id":live,"agent":"new"}))["lease"],
        2
    );
    tool("claim_entities", json!({"ids":[c],"agent":"new"}));
    tool("release_entities", json!({"ids":[c],"agent":"new"}));
    assert_eq!(
        tool(
            "simulate",
            json!({"ticks":1,"asserts":[{"state":"marker","equals":7}]})
        )["passed"],
        true
    );
    tool("rollback", json!({"revision":stable,"agent":"original"}));
    assert!(tool("get_revision", json!({})).as_u64().unwrap() > revision.as_u64().unwrap());
    assert!(
        tool("spawn_primitive", json!({"kind":"box"}))["id"]
            .as_u64()
            .unwrap()
            > c.max(removed)
    );
    println!("PASS process restart: scene/history, both journals, live leases/locks, offline expiry, fencing, immutable scripts and monotonic IDs");
}

#[test]
fn checkpoint_is_atomic_fails_closed_and_has_one_writer() {
    let project = Project::new();
    let mut engine = Engine::from_project(&project.0).unwrap();
    assert!(Engine::from_project(&project.0)
        .err()
        .unwrap()
        .to_string()
        .contains("coordination writer"));
    let state = project.0.join(".ai-engine/state.json");
    let before = std::fs::read(&state).unwrap();
    assert!(engine.handle("apply_batch",&json!({"mode":"atomic","commands":[{"tool":"spawn_primitive","arguments":{"kind":"box"}},{"tool":"remove_entity","arguments":{"id":999}}]})).is_err());
    assert_eq!(std::fs::read(&state).unwrap(), before);
    std::fs::rename(&state, state.with_extension("backup")).unwrap();
    std::fs::create_dir(&state).unwrap();
    assert!(engine
        .handle("task_create", &json!({"title":"not committed"}))
        .unwrap_err()
        .contains("checkpoint failed"));
    assert_eq!(call(&mut engine, "task_list", json!({})), json!([]));
    std::fs::remove_dir(&state).unwrap();
    std::fs::rename(state.with_extension("backup"), &state).unwrap();
    assert_eq!(engine.handle("apply_batch",&json!({"commands":[{"tool":"spawn_primitive","arguments":{"kind":"box"}},{"tool":"remove_entity","arguments":{"id":999}}]})).unwrap()["isError"],true);
    assert_eq!(engine.world.scene.entities.len(), 1);
    drop(engine);
    std::fs::write(
        project.0.join(".ai-engine/state.interrupted.tmp"),
        "partial",
    )
    .unwrap();
    let mut engine = Engine::from_project(&project.0).unwrap();
    assert_eq!(engine.world.scene.entities.len(), 1);
    call(&mut engine, "task_create", json!({"title":"committed"}));
    drop(engine);
    let valid = std::fs::read(&state).unwrap();
    for invalid in [b"{".to_vec(), br#"{"version":999}"#.to_vec()] {
        std::fs::write(&state, &invalid).unwrap();
        assert!(Engine::from_project(&project.0).is_err());
        assert_eq!(std::fs::read(&state).unwrap(), invalid);
    }
    std::fs::write(&state, valid).unwrap();
    let mut engine = Engine::from_project(&project.0).unwrap();
    assert_eq!(
        call(&mut engine, "task_list", json!({}))[0]["title"],
        "committed"
    );
    println!("PASS checkpoint safety: one writer, rejected atomic batch unchanged, failed write unacknowledged, partial batch retained, torn temp ignored, corrupt state never reset");
}

#[test]
fn project_config_is_stable_validated_and_overridden_without_global_test_env() {
    let project = Project::new();
    let config = project.0.join(".ai-engine/config.json");
    let original: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    drop(Engine::from_project(&project.0).unwrap());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&config).unwrap()).unwrap(),
        original
    );
    let configured = port();
    let override_port = port();
    let mut value = original.clone();
    value["blender_port"] = json!(configured);
    std::fs::write(&config, value.to_string()).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", override_port)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_ai-engine"))
        .args(["--project", project.0.to_str().unwrap()])
        .env("AI_ENGINE_BLENDER_PORT", override_port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let bridge = std::thread::spawn(move || {
        let start = Instant::now();
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(peer) => break peer,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(start.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = String::new();
        use std::io::BufRead;
        std::io::BufReader::new(&mut stream)
            .read_line(&mut request)
            .unwrap();
        let project = serde_json::from_str::<Value>(&request).unwrap()["project_id"].clone();
        writeln!(
            stream,
            "{}",
            json!({"ok":true,"result":{"bridge_protocol":1,"project_id":project,"background":true}})
        )
        .unwrap();
    });
    let mut child = child;
    writeln!(child.stdin.as_mut().unwrap(),"{}",json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"blender_status","arguments":{}}})).unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    bridge.join().unwrap();
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    let info: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(info["connected"], true);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&config).unwrap()).unwrap()["blender_port"],
        configured
    );
    value["blender_port"] = json!(0);
    std::fs::write(&config, value.to_string()).unwrap();
    assert!(Engine::from_project(&project.0).is_err());
}

#[test]
fn two_real_project_blenders_are_isolated_and_foreign_commands_are_refused() {
    if asset_pipeline::find_blender().is_none() {
        eprintln!("SKIP: real project isolation needs Blender");
        return;
    }
    let a = Project::new();
    let b = Project::new();
    let c = Project::new();
    let config = |root: &Path| {
        serde_json::from_slice::<Value>(
            &std::fs::read(root.join(".ai-engine/config.json")).unwrap(),
        )
        .unwrap()
    };
    let ca = config(&a.0);
    let cb = config(&b.0);
    assert_ne!(ca["blender_port"], cb["blender_port"]);
    let mut cc = config(&c.0);
    cc["blender_port"] = ca["blender_port"].clone();
    std::fs::write(c.0.join(".ai-engine/config.json"), cc.to_string()).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = |root: PathBuf, label: &'static str, ready: mpsc::Sender<Result<(), String>>| {
        let (finish_tx, finish_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut engine = Engine::from_project(&root).unwrap();
            let result = engine.handle("blender_start", &json!({})).and_then(|_| {
                engine.handle(
                    "blender_exec",
                    &json!({"code":format!("bpy.context.scene['project_marker']={label:?}")}),
                )
            });
            ready.send(result.map(|_| ())).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(60)).unwrap();
            let result = call(
                &mut engine,
                "blender_exec",
                json!({"code":"result=bpy.context.scene['project_marker']"}),
            );
            assert_eq!(result["result"], label);
        });
        (finish_tx, thread)
    };
    let (fa, ta) = worker(a.0.clone(), "A", ready_tx.clone());
    let (fb, tb) = worker(b.0.clone(), "B", ready_tx);
    let first = ready_rx.recv_timeout(Duration::from_secs(60)).unwrap();
    let second = ready_rx.recv_timeout(Duration::from_secs(60)).unwrap();
    let mut raw =
        TcpStream::connect(("127.0.0.1", ca["blender_port"].as_u64().unwrap() as u16)).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    writeln!(raw,"{}",json!({"command":"exec","project_id":"foreign","code":"bpy.context.scene['project_marker']='wrong'"})).unwrap();
    let mut rejected = String::new();
    use std::io::BufRead;
    std::io::BufReader::new(raw)
        .read_line(&mut rejected)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&rejected).unwrap()["ok"],
        false
    );
    assert!(rejected.contains("project identity mismatch"));
    let mut foreign = Engine::from_project(&c.0).unwrap();
    let start = foreign.handle("blender_start", &json!({}));
    let exec = foreign.handle(
        "blender_exec",
        &json!({"code":"bpy.context.scene['project_marker']='wrong'"}),
    );
    fa.send(()).unwrap();
    fb.send(()).unwrap();
    ta.join().unwrap();
    tb.join().unwrap();
    first.unwrap();
    second.unwrap();
    assert!(start.unwrap_err().contains("project identity mismatch"));
    assert!(exec.unwrap_err().contains("project identity mismatch"));
    println!("PASS real Blender concurrent projects: distinct stored ports, independent scenes, foreign start/exec refused without altering either process");
}
