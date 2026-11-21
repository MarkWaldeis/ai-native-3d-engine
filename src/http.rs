//! Synchronous HTTP workers route requests to one engine-owning thread.
use crate::Engine;
use serde_json::{json, Value};
use std::collections::{HashMap, BTreeMap, HashSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use tiny_http::{Header, Method, Response, Server};

struct Call {
    body: String,
    agent: Option<String>,
    authenticated: bool,
    reply: mpsc::Sender<Value>,
}

#[derive(Clone, Default)]
pub struct HttpOptions {
    pub allow_remote: bool,
    pub allow_privileged_tools: bool,
    pub credentials: BTreeMap<String, String>,
}

impl HttpOptions {
    fn validate(&self, addr: std::net::SocketAddr) -> anyhow::Result<()> {
        anyhow::ensure!(addr.ip().is_loopback() || self.allow_remote, "non-loopback HTTP requires --allow-remote and a bearer token");
        anyhow::ensure!(!self.allow_remote || !self.credentials.is_empty(), "remote HTTP requires a bearer token");
        let mut tokens = HashSet::new();
        for (agent, token) in &self.credentials {
            anyhow::ensure!(!agent.trim().is_empty() && !token.is_empty() && token.bytes().all(|b| b.is_ascii_graphic()) && tokens.insert(token), "credentials require nonempty principals and unique nonempty HTTP-safe tokens");
        }
        Ok(())
    }

    pub fn remote_from_env(allow_privileged_tools: bool) -> anyhow::Result<Self> {
        let mut credentials = BTreeMap::new();
        match (std::env::var("AI_ENGINE_HTTP_TOKENS"), std::env::var("AI_ENGINE_HTTP_TOKEN")) {
            (Ok(_), Ok(_)) => anyhow::bail!("set only AI_ENGINE_HTTP_TOKEN or AI_ENGINE_HTTP_TOKENS"),
            (Ok(tokens), _) => credentials = serde_json::from_str(&tokens).map_err(|_| anyhow::anyhow!("AI_ENGINE_HTTP_TOKENS must be a JSON object mapping agent names to bearer tokens"))?,
            (_, Ok(token)) => { credentials.insert("remote".into(), token); },
            _ => {
                let mut bytes = [0u8;32];
                getrandom::getrandom(&mut bytes).map_err(|_|anyhow::anyhow!("token generation failed"))?;
                let token: String = bytes.iter().map(|b|format!("{b:02x}")).collect();
                eprintln!("[ai-engine] Generated bearer token (principal remote; store securely): {token}");
                credentials.insert("remote".into(),token);
            }
        }
        Ok(Self {allow_remote:true,allow_privileged_tools,credentials})
    }

    fn principal(&self, request: &tiny_http::Request) -> Option<String> {
        let value = header(request,"Authorization")?;
        let (scheme, token) = value.split_once(' ')?;
        if !scheme.eq_ignore_ascii_case("Bearer") { return None; }
        self.credentials.iter().find_map(|(agent,secret)| {
            let equal = token.len()==secret.len() && token.bytes().zip(secret.bytes()).fold(0u8,|difference,(a,b)|difference | (a^b))==0;
            equal.then(||agent.clone())
        })
    }
}

#[derive(Clone)]
struct Session { version: String, principal: Option<String> }

/// Serve POST /mcp until the listener is unblocked. Engine/GPU state never
/// crosses threads; the channel mutex only coordinates producer access.
pub fn serve(server: Server) -> anyhow::Result<()> {
    serve_with_options(server, None, HttpOptions::default())
}

pub fn serve_with_options(server: Server, project: Option<PathBuf>, options: HttpOptions) -> anyhow::Result<()> {
    options.validate(server.server_addr().to_string().parse()?)?;
    if options.allow_remote {
        eprintln!("WARNING: remote HTTP enabled; bearer authentication required. Use a trusted network or TLS tunnel. Privileged Python tools {}.",if options.allow_privileged_tools {"ENABLED (unsandboxed machine access)"} else {"disabled"});
    }
    let (tx, rx) = mpsc::channel::<Call>();
    let (startup, started) = mpsc::channel();
    let privileged_tools = !options.allow_remote || options.allow_privileged_tools;
    let engine_thread = std::thread::spawn(move || {
        let engine = project
            .map(|path| Engine::from_project(&path))
            .unwrap_or_else(|| Ok(Engine::new()));
        let mut engine = match engine {
            Ok(engine) => {
                let _ = startup.send(Ok(()));
                engine
            }
            Err(error) => {
                let _ = startup.send(Err(error.to_string()));
                return;
            }
        };
        engine.privileged_tools = privileged_tools;
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
                            && (call.authenticated || request["params"]["arguments"].get("agent").is_none())
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
    started.recv()?.map_err(anyhow::Error::msg)?;
    let tx = Arc::new(Mutex::new(tx));
    let sessions = Arc::new(Mutex::new(HashMap::<String, Session>::new()));
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let tx = tx.clone();
            let sessions = sessions.clone();
            let server = &server;
            let options = &options;
            scope.spawn(move || {
                for mut request in server.incoming_requests() {
                    let principal = options.principal(&request);
                    if (options.allow_remote || !options.credentials.is_empty()) && principal.is_none() {
                        let _ = request.respond(Response::from_string("Bearer authentication required").with_status_code(401).with_header(Header::from_bytes("WWW-Authenticate","Bearer realm=\"ai-engine\"").unwrap()));
                        continue;
                    }
                    if request.url().split('?').next() != Some("/mcp") {
                        let _ = request
                            .respond(Response::from_string("Not found").with_status_code(404));
                        continue;
                    }
                    if let Some(origin) = header(&request, "Origin") {
                        if !local_origin(&origin) {
                            let _ = request.respond(
                                Response::from_string("Origin not allowed").with_status_code(403),
                            );
                            continue;
                        }
                    }
                    if !matches!(request.method(), Method::Post | Method::Delete) {
                        let _ = request.respond(
                            Response::empty(405)
                                .with_header(Header::from_bytes("Allow", "POST, DELETE").unwrap()),
                        );
                        continue;
                    }
                    let session = header(&request, "Mcp-Session-Id");
                    let session_info = session
                        .as_ref()
                        .and_then(|id| sessions.lock().unwrap().get(id).cloned());
                    if session.is_some() && session_info.is_none() {
                        let _ = request.respond(
                            Response::from_string("Unknown or terminated MCP session")
                                .with_status_code(404),
                        );
                        continue;
                    }
                    if session_info.as_ref().is_some_and(|session|session.principal != principal) {
                        let _ = request.respond(Response::from_string("Session belongs to another authenticated principal").with_status_code(403));
                        continue;
                    }
                    let version = session_info.map(|s|s.version);
                    if request.method() == &Method::Delete {
                        if let Some(session) = session {
                            sessions.lock().unwrap().remove(&session);
                            let _ = request.respond(Response::empty(204));
                        } else {
                            let _ = request.respond(
                                Response::from_string("Missing Mcp-Session-Id")
                                    .with_status_code(400),
                            );
                        }
                        continue;
                    }
                    if let Some(protocol) = header(&request, "MCP-Protocol-Version") {
                        if !engine_mcp::SUPPORTED_VERSIONS.contains(&protocol.as_str())
                            || version.as_ref().is_some_and(|version| version != &protocol)
                        {
                            let _ = request.respond(
                                Response::from_string("Invalid MCP-Protocol-Version")
                                    .with_status_code(400),
                            );
                            continue;
                        }
                    }
                    if header(&request, "Accept").is_some_and(|accept| {
                        !accept.split(',').any(|part| {
                            matches!(
                                part.trim().split(';').next(),
                                Some("application/json" | "*/*")
                            )
                        })
                    }) {
                        let _ = request.respond(
                            Response::from_string("Accept must include application/json")
                                .with_status_code(406),
                        );
                        continue;
                    }
                    if header(&request, "Content-Type").is_some_and(|content| {
                        content.split(';').next().map(str::trim) != Some("application/json")
                    }) {
                        let _ = request.respond(
                            Response::from_string("Expected application/json")
                                .with_status_code(415),
                        );
                        continue;
                    }
                    let agent = principal.clone().or_else(|| request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("X-Agent-Id"))
                        .map(|h| h.value.as_str().to_owned()));
                    let mut body = String::new();
                    const MAX_BODY: usize = 4 * 1024 * 1024;
                    if request
                        .as_reader()
                        .take(MAX_BODY as u64 + 1)
                        .read_to_string(&mut body)
                        .is_err()
                    {
                        let _ = request.respond(
                            Response::from_string("Invalid request body").with_status_code(400),
                        );
                        continue;
                    }
                    if body.len() > MAX_BODY {
                        let _ = request.respond(Response::empty(413));
                        continue;
                    }
                    let parsed = serde_json::from_str::<Value>(&body);
                    let initialize = parsed
                        .as_ref()
                        .is_ok_and(|value| value["method"] == "initialize");
                    if !initialize && parsed.is_ok() && session.is_none() {
                        let _ = request.respond(
                            Response::from_string("Initialize first; missing Mcp-Session-Id")
                                .with_status_code(400),
                        );
                        continue;
                    }
                    let (reply, response) = mpsc::channel();
                    if tx
                        .lock()
                        .unwrap()
                        .send(Call { body, agent, authenticated:principal.is_some(), reply })
                        .is_err()
                    {
                        break;
                    }
                    if let Ok(value) = response.recv() {
                        if value.is_null() {
                            let _ = request.respond(Response::empty(202));
                            continue;
                        }
                        let status = if value["error"]["code"] == -32700
                            || value["error"]["code"] == -32600
                        {
                            400
                        } else {
                            200
                        };
                        let mut response = Response::from_string(value.to_string())
                            .with_status_code(status)
                            .with_header(
                                Header::from_bytes("Content-Type", "application/json").unwrap(),
                            );
                        if initialize && value.get("result").is_some() {
                            let mut bytes = [0u8; 24];
                            if getrandom::getrandom(&mut bytes).is_err() {
                                let _ = request.respond(Response::empty(500));
                                continue;
                            }
                            let id: String =
                                bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                            sessions.lock().unwrap().insert(
                                id.clone(),
                                Session { version:value["result"]["protocolVersion"]
                                    .as_str()
                                    .unwrap()
                                    .to_owned(), principal:principal.clone() },
                            );
                            response.add_header(Header::from_bytes("Mcp-Session-Id", id).unwrap());
                        }
                        let _ = request.respond(response);
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
    serve_project_addr(addr, None)
}

pub fn serve_project_addr(addr: &str, project: Option<PathBuf>) -> anyhow::Result<()> {
    serve_project_addr_with_options(addr,project,HttpOptions::default())
}

pub fn serve_project_addr_with_options(addr: &str, project: Option<PathBuf>, options: HttpOptions) -> anyhow::Result<()> {
    use std::net::ToSocketAddrs;
    let addr = addr.to_socket_addrs()?.next().ok_or_else(||anyhow::anyhow!("HTTP address did not resolve"))?;
    options.validate(addr)?;
    let server = Server::http(addr).map_err(|e| anyhow::anyhow!("HTTP bind {addr}: {e}"))?;
    eprintln!(
        "[ai-engine] HTTP MCP server ready at http://{}/mcp",
        server.server_addr()
    );
    serve_with_options(server, project, options)
}

fn header(request: &tiny_http::Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str().to_owned())
}

fn local_origin(origin: &str) -> bool {
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    ["localhost", "127.0.0.1", "[::1]"].iter().any(|host| {
        authority == *host
            || authority
                .strip_prefix(host)
                .and_then(|rest| rest.strip_prefix(':'))
                .is_some_and(|port| port.parse::<u16>().is_ok())
    })
}
