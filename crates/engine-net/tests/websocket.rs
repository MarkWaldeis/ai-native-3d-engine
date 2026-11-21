#![cfg(not(target_arch = "wasm32"))]
use anyhow::Result;
use engine_net::{
    protocol::{decode, encode},
    websocket::WsServer,
    Actions, ClientConfig, ClientMessage, NetClient, NetServer, ServerConfig, ServerMessage,
};
use shared::scene::Scene;
use std::{
    collections::BTreeMap,
    net::{SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket};

fn scene() -> Scene {
    serde_json::from_value(serde_json::json!({"entities":[
        {"id":1,"name":"a","kind":"box","size":[1,1,1],"tags":["net_player"],
        "script":{"source":"fn on_tick(ctx,dt) {let p=ctx.position();ctx.set_position([p[0]+ctx.input(\"move_x\")*dt,p[1],p[2]]);}"}},
        {"id":2,"name":"b","kind":"box","size":[1,1,1],"tags":["net_player"],"transform":{"position":[4,0,0]},
        "script":{"source":"fn on_tick(ctx,dt) {let p=ctx.position();ctx.set_position([p[0]+ctx.input(\"move_x\")*dt,p[1],p[2]]);}"}}
    ]})).unwrap()
}
struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<Result<()>>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.take().unwrap().join().unwrap().unwrap();
    }
}
fn connect(addr: SocketAddr) -> Result<WebSocket<TcpStream>> {
    let stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(tungstenite::client(format!("ws://{addr}"), stream)?.0)
}
fn receive(socket: &mut WebSocket<TcpStream>) -> Result<ServerMessage> {
    loop {
        match socket.read()? {
            Message::Text(text) => return decode(text.as_bytes()),
            Message::Binary(bytes) => return decode(&bytes),
            _ => (),
        }
    }
}

#[test]
fn handshake_rejects_foreign_origin_but_allows_loopback_and_native() -> Result<()> {
    origin_handshakes(
        vec![],
        &[
            (None, true),
            (Some("http://localhost:8000"), true),
            (Some("http://127.0.0.1:8000"), true),
            (Some("http://[::1]:8000"), true),
            (Some("https://evil.example"), false),
            (Some("null"), false),
            (Some("http://localhost.evil.example"), false),
            (Some("http://localhost@evil.example"), false),
        ],
    )
}

#[test]
fn handshake_accepts_only_explicitly_allowed_foreign_origins() -> Result<()> {
    assert!(WsServer::bind_with_origins("127.0.0.1:0", vec!["null".into()]).is_err());
    origin_handshakes(
        vec![
            "https://game.example".into(),
            "http://another.example:8000".into(),
        ],
        &[
            (Some("https://game.example"), true),
            (Some("http://another.example:8000"), true),
            (Some("https://other.example"), false),
            (Some("http://game.example"), false),
            (None, true),
        ],
    )
}

fn origin_handshakes(allowed_origins: Vec<String>, cases: &[(Option<&str>, bool)]) -> Result<()> {
    use tungstenite::client::IntoClientRequest;
    let mut transport = WsServer::bind_with_origins("127.0.0.1:0", allowed_origins)?;
    let addr = transport.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let handle = thread::spawn(move || -> Result<()> {
        let mut server = NetServer::new(scene(), BTreeMap::new(), ServerConfig::default())?;
        while !signal.load(Ordering::Relaxed) {
            transport.tick(&mut server)?;
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    });
    let _worker = Worker {
        stop,
        handle: Some(handle),
    };
    for &(origin, allowed) in cases {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut request = format!("ws://{addr}").into_client_request()?;
        if let Some(origin) = origin {
            request.headers_mut().insert("Origin", origin.parse()?);
        }
        let result = tungstenite::client(request, stream);
        assert_eq!(result.is_ok(), allowed, "origin {origin:?}");
        if !allowed {
            assert!(
                matches!(result,Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response))) if response.status()==403)
            );
        }
    }
    Ok(())
}

#[test]
fn two_websocket_clients_move_converge_and_disconnect_frees_slot() -> Result<()> {
    let transport = WsServer::bind("127.0.0.1:0")?;
    let addr = transport.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let handle = thread::spawn(move || -> Result<()> {
        let mut transport = transport;
        let mut server = NetServer::new(
            scene(),
            BTreeMap::new(),
            ServerConfig {
                snapshot_interval: 1,
                ..ServerConfig::default()
            },
        )?;
        while !signal.load(Ordering::Relaxed) {
            transport.tick(&mut server)?;
            thread::sleep(Duration::from_secs_f64(engine_net::TICK_MS / 1000.0));
        }
        Ok(())
    });
    let _worker = Worker {
        stop,
        handle: Some(handle),
    };
    let mut sockets = [connect(addr)?, connect(addr)?];
    let mut clients = [
        NetClient::new(scene(), BTreeMap::new(), ClientConfig::default())?,
        NetClient::new(scene(), BTreeMap::new(), ClientConfig::default())?,
    ];
    let hello = encode(&ClientMessage::Hello {
        name: "websocket test".into(),
    })?;
    sockets[0].send(Message::Text(String::from_utf8(hello.clone())?.into()))?;
    sockets[1].send(Message::Binary(hello.into()))?;
    for (client, socket) in clients.iter_mut().zip(&mut sockets) {
        client.receive(receive(socket)?, 0.0)?;
    }
    assert_ne!(clients[0].entity_id(), clients[1].entity_id());
    let start = Instant::now();
    for step in 0..100 {
        for (index, (client, socket)) in clients.iter_mut().zip(&mut sockets).enumerate() {
            let reply = receive(socket)?;
            let tick = match &reply {
                ServerMessage::Snapshot(s) => s.server_tick,
                _ => panic!("snapshot expected"),
            };
            let now = start.elapsed().as_secs_f64() * 1000.0;
            client.receive(reply, now)?;
            let actions: Actions = BTreeMap::from([(
                "move_x".into(),
                if step < 60 {
                    if index == 0 {
                        1.0
                    } else {
                        -1.0
                    }
                } else {
                    0.0
                },
            )]);
            let message = client.advance_to(tick, actions, now)?;
            socket.send(Message::Binary(encode(&message)?.into()))?;
        }
    }
    for client in &clients {
        let snapshot = client.latest_snapshot().unwrap();
        assert_eq!(snapshot.players.len(), 2);
        let predicted = client.predicted_states_at(snapshot.server_tick).unwrap();
        for (a, b) in predicted.iter().zip(&snapshot.entities) {
            assert!((a.position[0] - b.position[0]).abs() < 1e-3);
        }
        for (index, owner) in clients.iter().enumerate() {
            let id = owner.entity_id().unwrap();
            let initial = if id == 1 { 0.0 } else { 4.0 };
            let direction = if index == 0 { 1.0 } else { -1.0 };
            let x = snapshot
                .entities
                .iter()
                .find(|entity| entity.id == id)
                .unwrap()
                .position[0];
            assert!(
                (x - initial) * direction > 0.3,
                "entity {id}, x={x}, initial={initial}, direction={direction}, stats={:?}",
                client.stats()
            );
        }
    }
    let freed = clients[0].entity_id();
    sockets[0].close(None)?;
    loop {
        if let ServerMessage::Snapshot(snapshot) = receive(&mut sockets[1])? {
            if snapshot.players.len() == 1 {
                break;
            }
        }
    }
    let mut replacement = connect(addr)?;
    replacement.send(Message::Binary(
        encode(&ClientMessage::Hello {
            name: "replacement".into(),
        })?
        .into(),
    ))?;
    let ServerMessage::Welcome { entity_id, .. } = receive(&mut replacement)? else {
        panic!("welcome expected")
    };
    assert_eq!(Some(entity_id), freed);
    replacement.send(Message::Text("malformed protocol".into()))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        assert!(Instant::now() < deadline, "malformed peer remained open");
        match replacement.read() {
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => (),
        }
    }
    Ok(())
}
