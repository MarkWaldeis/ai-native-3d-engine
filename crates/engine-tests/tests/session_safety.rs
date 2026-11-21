use ai_engine::{
    http::{serve_with_options, HttpOptions},
    Engine,
};
use engine_mcp::ToolHandler;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project {
    base: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}
impl Project {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "session-safety-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("project");
        let outside = base.join("outside");
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::create_dir(root.join("scripts")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("scene.json"), "{}").unwrap();
        std::fs::write(outside.join("private.json"), "{}").unwrap();
        Self {
            base,
            root,
            outside,
        }
    }
    fn engine(&self) -> Engine {
        Engine::from_project(&self.root).unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}
fn call(engine: &mut Engine, tool: &str, args: Value) -> Value {
    let result = engine
        .handle(tool, &args)
        .unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn project_paths_allow_local_files_but_reject_traversal_and_scene_reference_escapes() {
    let project = Project::new();
    let mut engine = project.engine();
    call(&mut engine, "spawn_primitive", json!({}));
    call(
        &mut engine,
        "write_script",
        json!({"ids":[1],"path":"scripts/player.rhai","source":"fn on_tick(ctx) {}"}),
    );
    call(&mut engine, "save_scene", json!({"path":"saved.json"}));
    assert!(project.root.join("saved.json").is_file());
    call(&mut engine, "load_scene", json!({"path":"saved.json"}));
    let before = engine.world.scene_json();
    let revision = engine.world.revision;
    let cases = vec![
        ("save_scene", json!({"path":"../outside/new.json"})),
        (
            "load_scene",
            json!({"path":project.outside.join("private.json")}),
        ),
        (
            "write_script",
            json!({"ids":[1],"path":"../outside/new.rhai","source":""}),
        ),
        (
            "run_blender_script",
            json!({"script":project.outside.join("private.py")}),
        ),
        (
            "spawn_primitive",
            json!({"kind":"gltf","path":project.outside.join("private.glb")}),
        ),
        (
            "spawn_primitive",
            json!({"kind":"gltf","path":"../../outside/private.glb"}),
        ),
        ("export_web", json!({"out_dir":"../outside/site"})),
        (
            "export_web",
            json!({"out_dir":"site","scene":project.outside.join("private.json")}),
        ),
        (
            "export_web",
            json!({"out_dir":"site","scene":{"entities":[{"id":1,"name":"escape","kind":"gltf","path":project.outside.join("private.glb")}]}}),
        ),
        (
            "apply_batch",
            json!({"commands":[{"tool":"save_scene","arguments":{"path":"../outside/new.json"}}]}),
        ),
    ];
    for (tool, args) in cases {
        let result = engine.handle(tool, &args);
        let error = match result {
            Err(e) => e,
            Ok(value) => {
                assert_eq!(value["isError"], true, "{tool}: {value}");
                value.to_string()
            }
        };
        assert!(error.contains("escapes project"), "{tool}: {error}");
        assert_eq!(engine.world.scene_json(), before);
        assert_eq!(engine.world.revision, revision);
    }
    for component in [
        json!({"kind":"gltf","path":project.outside.join("private.glb")}),
        json!({"kind":"box","size":[1,1,1],"script":{"path":project.outside.join("private.rhai"),"source":"fn on_tick(ctx) {}"}}),
    ] {
        let mut entity = component;
        entity["id"] = json!(5);
        entity["name"] = json!("escape");
        std::fs::write(
            project.root.join("bad.json"),
            json!({"entities":[entity]}).to_string(),
        )
        .unwrap();
        assert!(engine
            .handle("load_scene", &json!({"path":"bad.json"}))
            .unwrap_err()
            .contains("escapes project"));
        assert_eq!(engine.world.scene_json(), before);
        assert_eq!(engine.world.revision, revision);
    }
    assert_eq!(
        std::fs::read_to_string(project.outside.join("private.json")).unwrap(),
        "{}"
    );
    assert!(!project.outside.join("new.json").exists());
    assert!(!project.root.join("site").exists());
    assert!(engine.handle("export_web", &json!({})).is_err());
    assert!(engine_core::freeze_script(
        &shared::components::Script::Versioned {
            path: project
                .outside
                .join("private.rhai")
                .to_string_lossy()
                .into(),
            source: String::new()
        },
        &project.root
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_at_startup_or_through_existing_and_future_paths() {
    use std::os::unix::fs::symlink;
    let project = Project::new();
    let mut engine = project.engine();
    symlink(&project.outside, project.root.join("linked")).unwrap();
    symlink(
        project.outside.join("private.json"),
        project.root.join("linked.json"),
    )
    .unwrap();
    for (tool, args) in [
        ("save_scene", json!({"path":"linked.json"})),
        ("load_scene", json!({"path":"linked.json"})),
        ("export_web", json!({"out_dir":"linked/not/yet/site"})),
        (
            "write_script",
            json!({"ids":[],"path":"linked/new.rhai","source":""}),
        ),
        ("blender_sync", json!({})),
    ] {
        if tool == "blender_sync" {
            symlink(&project.outside, project.root.join("assets/blender")).unwrap();
        }
        assert!(engine
            .handle(tool, &args)
            .unwrap_err()
            .contains("escapes project"));
    }
    assert!(!project.outside.join("not").exists());
    let startup = project.root.join("startup");
    std::fs::create_dir(&startup).unwrap();
    symlink(
        project.outside.join("private.json"),
        startup.join("scene.json"),
    )
    .unwrap();
    assert!(Engine::from_project(&startup).is_err());
    let startup = project.root.join("startup-assets");
    std::fs::create_dir(&startup).unwrap();
    std::fs::write(startup.join("scene.json"), "{}").unwrap();
    symlink(&project.outside, startup.join("assets")).unwrap();
    assert!(Engine::from_project(&startup).is_err());
    assert_eq!(
        std::fs::read_to_string(project.outside.join("private.json")).unwrap(),
        "{}"
    );
}

#[test]
fn task_leases_renew_release_expire_take_over_and_fence_late_workers() {
    let project = Project::new();
    let mut engine = project.engine();
    let revision = engine.world.revision;
    assert_eq!(call(&mut engine, "task_list", json!({})), json!([]));
    assert_eq!(
        call(&mut engine, "get_task_changes_since", json!({"sequence":0}))["entries"],
        json!([])
    );
    let a = call(&mut engine, "task_create", json!({"title":"model"}))["id"].clone();
    let b = call(
        &mut engine,
        "task_create",
        json!({"title":"review","depends_on":[a]}),
    )["id"]
        .clone();
    assert!(engine
        .handle("task_claim", &json!({"id":b,"agent":"reviewer"}))
        .unwrap_err()
        .contains("dependency"));
    let first = call(
        &mut engine,
        "task_claim",
        json!({"id":a,"agent":"worker-a","ttl_s":60}),
    );
    assert!(first["ttl_s"].as_f64().unwrap() > 0.0);
    assert!(engine
        .handle(
            "task_renew",
            &json!({"id":a,"agent":"worker-a","lease":999})
        )
        .unwrap_err()
        .contains("stale"));
    call(
        &mut engine,
        "task_renew",
        json!({"id":a,"agent":"worker-a","lease":first["lease"],"ttl_s":60}),
    );
    call(
        &mut engine,
        "task_release",
        json!({"id":a,"agent":"worker-a","lease":first["lease"]}),
    );
    let second = call(
        &mut engine,
        "task_claim",
        json!({"id":a,"agent":"worker-a","ttl_s":60}),
    );
    assert!(engine
        .handle(
            "task_complete",
            &json!({"id":a,"agent":"worker-a","lease":first["lease"]})
        )
        .unwrap_err()
        .contains("stale"));
    call(
        &mut engine,
        "task_renew",
        json!({"id":a,"agent":"worker-a","lease":second["lease"],"ttl_s":1}),
    );
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(
        call(&mut engine, "task_list", json!({"status":"expired"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for tool in ["task_complete", "task_renew"] {
        assert!(engine
            .handle(
                tool,
                &json!({"id":a,"agent":"worker-a","lease":second["lease"]})
            )
            .unwrap_err()
            .contains("expired"));
    }
    let third = call(
        &mut engine,
        "task_claim",
        json!({"id":a,"agent":"worker-b","ttl_s":60}),
    );
    assert!(third["lease"].as_u64() > second["lease"].as_u64());
    assert!(engine
        .handle(
            "task_complete",
            &json!({"id":a,"agent":"worker-a","lease":second["lease"]})
        )
        .unwrap_err()
        .contains("must be claimed"));
    assert!(engine
        .handle(
            "task_complete",
            &json!({"id":a,"agent":"worker-b","lease":second["lease"]})
        )
        .unwrap_err()
        .contains("stale"));
    assert!(engine
        .handle("task_complete", &json!({"id":a,"agent":"worker-b"}))
        .unwrap_err()
        .contains("lease required"));
    call(
        &mut engine,
        "task_complete",
        json!({"id":a,"agent":"worker-b","lease":third["lease"],"note":"recovered"}),
    );
    call(
        &mut engine,
        "task_claim",
        json!({"id":b,"agent":"reviewer"}),
    );
    call(
        &mut engine,
        "task_complete",
        json!({"id":b,"agent":"reviewer"}),
    );
    let journal = call(&mut engine, "get_task_changes_since", json!({"sequence":0}));
    let events = journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec![
            "create", "create", "claim", "renew", "release", "claim", "renew", "takeover", "complete",
            "claim", "complete"
        ]
    );
    assert_eq!(journal["entries"][7]["previous_agent"], "worker-a");
    assert_eq!(
        call(&mut engine, "get_task_changes_since", json!({"sequence":6}))["entries"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(engine.world.revision, revision);
    assert_eq!(
        call(&mut engine, "get_changes_since", json!({"revision":0}))["entries"],
        json!([])
    );
    let pending = call(&mut engine, "task_create", json!({"title":"pending"}))["id"].clone();
    for ttl in [json!(0), json!(86401), json!("1"), json!(1.5), Value::Null] {
        assert!(engine
            .handle("task_claim", &json!({"id":pending,"ttl_s":ttl}))
            .is_err());
    }
    assert!(engine
        .handle("task_release", &json!({"id":999,"lease":1}))
        .is_err());
    assert!(engine
        .handle("get_task_changes_since", &json!({"sequence":null}))
        .is_err());
}

fn exchange(addr: &str, method: &str, headers: &str, body: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(stream,"{method} /mcp HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    (
        headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        headers.into(),
        body.into(),
    )
}
fn start(project: &Project, privileged: bool) -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_string();
    let options = HttpOptions {
        allow_remote: true,
        allow_privileged_tools: privileged,
        credentials: BTreeMap::from([
            ("alice".into(), "alice-test-credential".into()),
            ("bob".into(), "bob-test-credential".into()),
        ]),
    };
    let root = project.root.clone();
    std::thread::spawn(move || serve_with_options(server, Some(root), options).unwrap());
    addr
}
fn initialize(addr: &str, token: &str) -> String {
    let (status, headers, _) = exchange(
        addr,
        "POST",
        &format!("Authorization: Bearer {token}\r\nContent-Type: application/json\r\n"),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    );
    assert_eq!(status, 200);
    headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("Mcp-Session-Id"))
                .map(|(_, v)| v.trim().into())
        })
        .unwrap()
}
fn remote(addr: &str, token: &str, session: &str, tool: &str, args: Value) -> Value {
    let (status,_,body)=exchange(addr,"POST",&format!("Authorization: Bearer {token}\r\nMcp-Session-Id: {session}\r\nX-Agent-Id: forged-header\r\nContent-Type: application/json\r\n"),&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string());
    assert_eq!(status, 200);
    serde_json::from_str::<Value>(&body).unwrap()["result"].clone()
}
fn data(result: Value) -> Value {
    assert_ne!(result["isError"], true, "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn remote_http_requires_bearer_and_binds_sessions_agents_batches_and_tasks_to_credentials() {
    let project = Project::new();
    let addr = start(&project, false);
    assert_eq!(
        exchange(
            &addr,
            "POST",
            "",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#
        )
        .0,
        401
    );
    assert_eq!(
        exchange(&addr, "POST", "Authorization: Bearer wrong\r\n", "").0,
        401
    );
    let alice = initialize(&addr, "alice-test-credential");
    let bob = initialize(&addr, "bob-test-credential");
    assert_eq!(
        exchange(
            &addr,
            "DELETE",
            &format!("Authorization: Bearer bob-test-credential\r\nMcp-Session-Id: {alice}\r\n"),
            ""
        )
        .0,
        403
    );
    data(remote(
        &addr,
        "alice-test-credential",
        &alice,
        "spawn_primitive",
        json!({"agent":"bob"}),
    ));
    data(remote(
        &addr,
        "alice-test-credential",
        &alice,
        "claim_entities",
        json!({"ids":[1],"agent":"bob"}),
    ));
    let denied = remote(
        &addr,
        "bob-test-credential",
        &bob,
        "set_color",
        json!({"id":1,"color":[1,0,0,1],"agent":"alice"}),
    );
    assert_eq!(denied["isError"], true);
    assert!(denied.to_string().contains("locked by alice"));
    let batch = remote(
        &addr,
        "alice-test-credential",
        &alice,
        "apply_batch",
        json!({"agent":"bob","commands":[{"tool":"set_color","arguments":{"id":1,"color":[1,0,0,1],"agent":"bob"}}]}),
    );
    assert_ne!(batch["isError"], true, "{batch}");
    let journal = data(remote(
        &addr,
        "alice-test-credential",
        &alice,
        "get_changes_since",
        json!({"revision":0}),
    ));
    assert!(journal["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["agent"] == "alice"));
    let task = data(remote(
        &addr,
        "alice-test-credential",
        &alice,
        "task_create",
        json!({"title":"secure task"}),
    ))["id"]
        .clone();
    let claim = data(remote(
        &addr,
        "alice-test-credential",
        &alice,
        "task_claim",
        json!({"id":task,"agent":"bob"}),
    ));
    assert_eq!(claim["agent"], "alice");
    assert_eq!(
        remote(
            &addr,
            "bob-test-credential",
            &bob,
            "task_complete",
            json!({"id":task,"agent":"alice","lease":claim["lease"]})
        )["isError"],
        true
    );
    data(remote(
        &addr,
        "alice-test-credential",
        &alice,
        "task_complete",
        json!({"id":task,"lease":claim["lease"]}),
    ));
    for (tool, args) in [
        ("blender_exec", json!({"code":"result=1"})),
        ("run_blender_script", json!({"script":"anything.py"})),
        (
            "apply_batch",
            json!({"commands":[{"tool":"blender_exec","arguments":{"code":"result=1"}}]}),
        ),
    ] {
        let denied = remote(&addr, "alice-test-credential", &alice, tool, args);
        assert_eq!(denied["isError"], true);
        assert!(denied.to_string().contains("privileged tool disabled"));
    }
    assert!(remote(
        &addr,
        "alice-test-credential",
        &alice,
        "save_scene",
        json!({"path":project.outside.join("no.json")})
    )
    .to_string()
    .contains("escapes project"));
    assert_eq!(
        exchange(
            &addr,
            "DELETE",
            &format!("Authorization: Bearer alice-test-credential\r\nMcp-Session-Id: {alice}\r\n"),
            ""
        )
        .0,
        204
    );
}

#[test]
fn remote_opt_in_requires_tokens_and_privilege_opt_in_never_relaxes_path_guards() {
    assert!(ai_engine::http::serve_project_addr("0.0.0.0:0", None)
        .unwrap_err()
        .to_string()
        .contains("non-loopback"));
    let options = HttpOptions {
        allow_remote: true,
        ..Default::default()
    };
    assert!(
        ai_engine::http::serve_project_addr_with_options("127.0.0.1:0", None, options)
            .unwrap_err()
            .to_string()
            .contains("bearer token")
    );
    let options = HttpOptions {
        allow_remote: true,
        credentials: BTreeMap::from([("a".into(), "same".into()), ("b".into(), "same".into())]),
        ..Default::default()
    };
    assert!(
        ai_engine::http::serve_project_addr_with_options("127.0.0.1:0", None, options).is_err()
    );
    let project = Project::new();
    let addr = start(&project, true);
    let session = initialize(&addr, "alice-test-credential");
    let denied = remote(
        &addr,
        "alice-test-credential",
        &session,
        "run_blender_script",
        json!({"script":"../outside/no.py"}),
    );
    assert!(denied.to_string().contains("escapes project"));
    assert!(!denied.to_string().contains("privileged tool disabled"));
}
