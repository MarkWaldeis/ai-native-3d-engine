//! Synchronous HTTP workers route requests to one engine-owning thread.
use crate::Engine;
use serde_json::{json, Value};
use std::sync::{mpsc, Arc, Mutex};
use tiny_http::{Header, Method, Response, Server};

struct Call {
    body: String,
    agent: Option<String>,
    reply: mpsc::Sender<Value>,
}

/// Serve POST /mcp until the listener is unblocked. Engine/GPU state never
/// crosses threads; the channel mutex only coordinates producer access.
pub fn serve(server: Server) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<Call>();
    let engine_thread = std::thread::spawn(move || {
        let mut engine = Engine::new();
        for call in rx {
            let mut body = call.body;
            if let Some(agent) = call.agent {
                if let Ok(mut request) = serde_json::from_str::<Value>(&body) {
                    if request["method"] == "tools/call"
                        && request.get("params").is_some_and(Value::is_object)
                    {
                        if request["params"]["arguments"].is_null() {
                            request["params"]["arguments"] = json!({});
                        }
                        if request["params"]["arguments"].is_object()
                            && request["params"]["arguments"].get("agent").is_none()
                        {
                            request["params"]["arguments"]["agent"] = json!(agent);
                        }
                    }
                    body = request.to_string();
                }
            }
            let response = engine_mcp::handle_line(&body, &mut engine).unwrap_or(Value::Null);
            let _ = call.reply.send(response);
        }
    });
    let tx = Arc::new(Mutex::new(tx));
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let tx = tx.clone();
            let server = &server;
            scope.spawn(move || {
                for mut request in server.incoming_requests() {
                    if request.url() != "/mcp" || request.method() != &Method::Post {
                        let _ = request
                            .respond(Response::from_string("Use POST /mcp").with_status_code(404));
                        continue;
                    }
                    let agent = request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("X-Agent-Id"))
                        .map(|h| h.value.as_str().to_owned());
                    let mut body = String::new();
                    if request.as_reader().read_to_string(&mut body).is_err() {
                        let _ = request.respond(
                            Response::from_string("Invalid request body").with_status_code(400),
                        );
                        continue;
                    }
                    let (reply, response) = mpsc::channel();
                    if tx
                        .lock()
                        .unwrap()
                        .send(Call { body, agent, reply })
                        .is_err()
                    {
                        break;
                    }
                    if let Ok(value) = response.recv() {
                        let _ =
                            request.respond(Response::from_string(value.to_string()).with_header(
                                Header::from_bytes("Content-Type", "application/json").unwrap(),
                            ));
                    }
                }
            });
        }
    });
    drop(tx);
    engine_thread
        .join()
        .map_err(|_| anyhow::anyhow!("engine thread panicked"))?;
    Ok(())
}

pub fn serve_addr(addr: &str) -> anyhow::Result<()> {
    let server = Server::http(addr).map_err(|e| anyhow::anyhow!("HTTP bind {addr}: {e}"))?;
    eprintln!(
        "[ai-engine] HTTP MCP server ready at http://{}/mcp",
        server.server_addr()
    );
    serve(server)
}
