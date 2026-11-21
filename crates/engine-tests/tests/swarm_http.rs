use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;

fn rpc(addr: &str, agent: &str, tool: &str, args: Value) -> Value {
    let initialize = json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"swarm-test","version":"1"}}}).to_string();
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(stream,"POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{initialize}", initialize.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let session = response.lines().find_map(|line| line.split_once(':').filter(|(name,_)| name.eq_ignore_ascii_case("Mcp-Session-Id")).map(|(_,value)|value.trim().to_owned())).unwrap();
    let body = json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":tool, "arguments":args}}).to_string();
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    write!(stream, "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMcp-Session-Id: {session}\r\nMCP-Protocol-Version: 2025-06-18\r\nX-Agent-Id: {agent}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    serde_json::from_str::<Value>(body).unwrap()["result"].clone()
}

fn data(result: Value) -> Value {
    assert_ne!(result["isError"], true, "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn error(result: Value, message: &str) {
    assert_eq!(result["isError"], true, "{result}");
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(message),
        "{result}"
    );
}

#[test]
fn concurrent_agents_locks_journal_and_tasks_over_http() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_string();
    std::thread::spawn(move || ai_engine::http::serve(server).unwrap());
    assert_eq!(data(rpc(&addr, "lead", "list_locks", json!({}))), json!([]));
    assert_eq!(data(rpc(&addr, "lead", "task_list", json!({}))), json!([]));
    assert_eq!(
        data(rpc(
            &addr,
            "lead",
            "get_changes_since",
            json!({"revision":0})
        ))["entries"],
        json!([])
    );
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let threads: Vec<_> = (0..3)
        .map(|i| {
            let addr = addr.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                (0..5)
                    .map(|n| {
                        data(rpc(
                            &addr,
                            &format!("agent-{i}"),
                            "spawn_primitive",
                            json!({"kind":"box", "name":format!("{i}-{n}")}),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut revisions = vec![];
    for thread in threads {
        let entries = thread.join().unwrap();
        assert!(entries
            .windows(2)
            .all(|w| w[0]["revision"].as_u64() < w[1]["revision"].as_u64()));
        revisions.extend(entries.iter().map(|e| e["revision"].as_u64().unwrap()));
    }
    revisions.sort_unstable();
    assert_eq!(revisions, (1..=15).collect::<Vec<_>>());
    assert_eq!(
        data(rpc(&addr, "lead", "get_scene_state", json!({})))["entities"]
            .as_array()
            .unwrap()
            .len(),
        15
    );
    let journal = data(rpc(
        &addr,
        "lead",
        "get_changes_since",
        json!({"revision":0}),
    ));
    let entries = journal["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 15);
    for i in 0..3 {
        assert_eq!(
            entries
                .iter()
                .filter(|e| e["agent"] == format!("agent-{i}"))
                .count(),
            5
        );
    }
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(entry["revision"], i + 1);
        assert_eq!(entry["tool"], "spawn_primitive");
        assert_eq!(entry["ids"].as_array().unwrap().len(), 1);
    }
    data(rpc(
        &addr,
        "lead",
        "claim_entities",
        json!({"ids":[1], "ttl_s":1}),
    ));
    error(
        rpc(&addr, "other", "set_color", json!({"id":1})),
        "locked by lead",
    );
    error(
        rpc(&addr, "other", "release_entities", json!({"ids":[1]})),
        "locked by lead",
    );
    error(
        rpc(&addr, "other", "claim_entities", json!({"ids":[2,1]})),
        "locked by lead",
    );
    assert_eq!(
        data(rpc(&addr, "other", "list_locks", json!({})))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    error(
        rpc(&addr, "other", "rollback", json!({"revision":1})),
        "locked by lead",
    );
    error(
        rpc(
            &addr,
            "other",
            "apply_batch",
            json!({"commands":[{"tool":"set_color", "arguments":{"id":1, "agent":"lead"}}]}),
        ),
        "locked by lead",
    );
    data(rpc(
        &addr,
        "header",
        "set_color",
        json!({"id":1, "agent":"lead"}),
    ));
    let changes = data(rpc(
        &addr,
        "lead",
        "get_changes_since",
        json!({"revision":15}),
    ));
    assert_eq!(changes["entries"][0]["agent"], "lead");
    assert_eq!(changes["entries"][0]["ids"], json!([1]));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        data(rpc(&addr, "other", "list_locks", json!({}))),
        json!([])
    );
    data(rpc(&addr, "other", "set_color", json!({"id":1})));
    error(
        rpc(&addr, "lead", "claim_entities", json!({"ids":[999]})),
        "no entity",
    );
    error(
        rpc(
            &addr,
            "lead",
            "claim_entities",
            json!({"ids":[1], "ttl_s":0}),
        ),
        "ttl_s",
    );
    data(rpc(&addr, "lead", "claim_entities", json!({"ids":[1]})));
    data(rpc(&addr, "lead", "release_entities", json!({"ids":[1]})));
    assert_eq!(data(rpc(&addr, "lead", "list_locks", json!({}))), json!([]));
    let a = data(rpc(&addr, "lead", "task_create", json!({"title":"model"})))["id"].clone();
    let b = data(rpc(
        &addr,
        "lead",
        "task_create",
        json!({"title":"review", "depends_on":[a]}),
    ))["id"]
        .clone();
    error(
        rpc(&addr, "worker", "task_claim", json!({"id":b})),
        "dependency",
    );
    error(
        rpc(&addr, "worker", "task_complete", json!({"id":a})),
        "must be claimed",
    );
    data(rpc(&addr, "worker", "task_claim", json!({"id":a})));
    error(
        rpc(&addr, "other", "task_claim", json!({"id":a})),
        "only pending",
    );
    error(
        rpc(&addr, "other", "task_complete", json!({"id":a})),
        "must be claimed",
    );
    data(rpc(
        &addr,
        "worker",
        "task_complete",
        json!({"id":a, "note":"done"}),
    ));
    data(rpc(&addr, "other", "task_claim", json!({"id":b})));
    assert_eq!(
        data(rpc(
            &addr,
            "lead",
            "task_list",
            json!({"status":"completed"})
        ))
        .as_array()
        .unwrap()
        .len(),
        1
    );
    error(
        rpc(
            &addr,
            "lead",
            "task_create",
            json!({"title":"bad", "depends_on":[999]}),
        ),
        "no task",
    );
    error(
        rpc(&addr, "lead", "task_create", json!({"title":""})),
        "title",
    );
    assert_eq!(data(rpc(&addr,"lead","task_list",json!({"status":"expired"}))),json!([]));
    error(
        rpc(&addr, "lead", "task_list", json!({"status":"invalid"})),
        "pending, claimed, completed, or expired",
    );
    error(
        rpc(&addr, "lead", "get_changes_since", json!({})),
        "revision",
    );
}

#[test]
fn stdio_protocol_keeps_anonymous_and_explicit_identity() {
    let mut engine = ai_engine::Engine::new();
    let mut call = |tool: &str, arguments: Value| {
        engine_mcp::handle_line(&json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":tool, "arguments":arguments}}).to_string(), &mut engine).unwrap()["result"].clone()
    };
    error(
        call("spawn_primitive", json!({"kind":"cube"})),
        "valid kinds: box, plane, sphere, cylinder, wall_with_door, gltf",
    );
    data(call("claim_entities", json!({"ids":[]})));
    data(call("release_entities", json!({"ids":[]})));
    assert_eq!(data(call("apply_batch", json!({"commands":[]}))), json!([]));
    data(call("spawn_primitive", json!({})));
    data(call(
        "apply_batch",
        json!({"agent":"batch-agent", "commands":[{"tool":"set_color", "arguments":{"id":1}}]}),
    ));
    let journal = data(call("get_changes_since", json!({"revision":0})));
    assert_eq!(journal["entries"][0]["agent"], "anonymous");
    assert_eq!(journal["entries"][1]["agent"], "batch-agent");
    let invalid = engine_mcp::handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"task_list","arguments":[]}}"#, &mut engine).unwrap();
    assert_eq!(invalid["error"]["code"], -32602);
    let tools = engine_mcp::handle_line(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        &mut engine,
    )
    .unwrap();
    for name in [
        "claim_entities",
        "release_entities",
        "list_locks",
        "get_changes_since",
        "task_create",
        "task_claim",
        "task_renew",
        "task_release",
        "get_task_changes_since",
        "task_complete",
        "task_list",
    ] {
        assert!(tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == name
                && t["inputSchema"]["properties"]["agent"]["type"] == "string"));
    }
}
