//! Native, bounded WebSocket adapter for trusted local sessions (no TLS/authentication).
use crate::{
    protocol::{decode, encode},
    transport::Transport,
    NetServer, PeerId, ServerConfig, SimSource, TICK_MS,
};
use anyhow::Result;
use shared::scene::Scene;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::ErrorKind,
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{
    accept_hdr_with_config,
    handshake::server::{Callback, ErrorResponse, Request, Response},
    http::{StatusCode, Uri},
    protocol::WebSocketConfig,
    Error, Message,
};

#[derive(Debug)]
enum Event {
    Packet(PeerId, Vec<u8>),
    Activity(PeerId),
    Closed(PeerId),
}

pub struct WsServer {
    listener: TcpListener,
    peers: BTreeMap<PeerId, SyncSender<Vec<u8>>>,
    events: Receiver<Event>,
    sender: SyncSender<Event>,
    disconnected: VecDeque<PeerId>,
    next: PeerId,
    stopped: Arc<AtomicBool>,
    allow_origins: Arc<Vec<String>>,
    active: BTreeSet<PeerId>,
}

impl WsServer {
    pub fn bind(addr: impl ToSocketAddrs) -> Result<Self> {
        Self::bind_with_origins(addr, vec![])
    }
    pub fn bind_with_origins(addr: impl ToSocketAddrs, allow_origins: Vec<String>) -> Result<Self> {
        anyhow::ensure!(
            allow_origins
                .iter()
                .all(|origin| origin_host(origin).is_some()),
            "--allow-origin requires an http(s) origin without a path, credentials or query"
        );
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let (sender, events) = mpsc::sync_channel(1024);
        Ok(Self {
            listener,
            peers: BTreeMap::new(),
            events,
            sender,
            disconnected: VecDeque::new(),
            next: 1,
            stopped: Arc::new(AtomicBool::new(false)),
            allow_origins: Arc::new(allow_origins),
            active: BTreeSet::new(),
        })
    }
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }
    pub fn disconnect(&mut self, peer: PeerId) {
        if self.peers.remove(&peer).is_some() {
            self.disconnected.push_back(peer);
        }
    }
    pub fn take_disconnected(&mut self) -> Option<PeerId> {
        self.disconnected.pop_front()
    }

    fn accept(&mut self) -> Result<()> {
        for _ in 0..8 {
            match self.listener.accept() {
                Ok((socket, _)) => {
                    if self.peers.len() >= 64 {
                        continue;
                    }
                    let peer = self.next;
                    self.next += 1;
                    let (sender, receiver) = mpsc::sync_channel(64);
                    self.peers.insert(peer, sender);
                    let events = self.sender.clone();
                    let stopped = self.stopped.clone();
                    let allow_origins = self.allow_origins.clone();
                    thread::spawn(move || {
                        let _ =
                            connection(socket, peer, receiver, &events, &stopped, &allow_origins);
                        let _ = events.send(Event::Closed(peer));
                    });
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Drain bounded input work, step authority once, and enqueue snapshots.
    pub fn tick(&mut self, server: &mut NetServer) -> Result<()> {
        for _ in 0..1024 {
            let Some((peer, bytes)) = self.recv()? else {
                break;
            };
            match decode(&bytes).and_then(|message| server.receive(peer, message)) {
                Ok(Some(message)) => self.send(peer, &encode(&message)?)?,
                Ok(None) => (),
                Err(_) => self.disconnect(peer),
            }
        }
        for peer in std::mem::take(&mut self.active) {
            server.activity(peer);
        }
        while let Some(peer) = self.take_disconnected() {
            server.disconnect(peer);
        }
        let assigned: Vec<_> = self
            .peers
            .keys()
            .copied()
            .filter(|peer| server.assigned_entity(*peer).is_some())
            .collect();
        for (peer, message) in server.tick()? {
            self.send(peer, &encode(&message)?)?;
        }
        for peer in assigned {
            if server.assigned_entity(peer).is_none() {
                self.disconnect(peer);
            }
        }
        Ok(())
    }
}

impl Drop for WsServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

impl Transport for WsServer {
    fn send(&mut self, to: PeerId, bytes: &[u8]) -> Result<()> {
        if self
            .peers
            .get(&to)
            .is_some_and(|sender| sender.try_send(bytes.to_vec()).is_err())
        {
            self.disconnect(to);
        }
        Ok(())
    }
    fn recv(&mut self) -> Result<Option<(PeerId, Vec<u8>)>> {
        self.accept()?;
        loop {
            match self.events.try_recv() {
                Ok(Event::Packet(peer, bytes)) if self.peers.contains_key(&peer) => {
                    return Ok(Some((peer, bytes)))
                }
                Ok(Event::Packet(..)) => (),
                Ok(Event::Activity(peer)) => {
                    self.active.insert(peer);
                }
                Ok(Event::Closed(peer)) => self.disconnect(peer),
                Err(_) => return Ok(None),
            }
        }
    }
}

fn blocked(error: &Error) -> bool {
    matches!(error,Error::Io(e) if e.kind()==ErrorKind::WouldBlock)
}

fn origin_host(origin: &str) -> Option<String> {
    let uri: Uri = origin.parse().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.path() != "/"
        || uri.query().is_some()
    {
        return None;
    }
    let authority = uri.authority()?;
    if authority.as_str().contains('@') {
        return None;
    }
    Some(authority.host().to_ascii_lowercase())
}

fn origin_allowed(request: &Request, allow_origins: &[String]) -> bool {
    let mut origins = request.headers().get_all("origin").iter();
    let Some(origin) = origins.next() else {
        return true;
    };
    if origins.next().is_some() {
        return false;
    }
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Some(host) = origin_host(origin) else {
        return false;
    };
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]" | "::1")
        || allow_origins.iter().any(|allowed| allowed == origin)
}

struct OriginCheck(Vec<String>);

impl Callback for OriginCheck {
    fn on_request(
        self,
        request: &Request,
        response: Response,
    ) -> std::result::Result<Response, ErrorResponse> {
        if origin_allowed(request, &self.0) {
            Ok(response)
        } else {
            let mut denied =
                tungstenite::http::Response::new(Some("Origin not allowed".to_owned()));
            *denied.status_mut() = StatusCode::FORBIDDEN;
            Err(denied)
        }
    }
}

fn connection(
    socket: TcpStream,
    peer: PeerId,
    outgoing: Receiver<Vec<u8>>,
    events: &SyncSender<Event>,
    stopped: &AtomicBool,
    allow_origins: &[String],
) -> Result<()> {
    socket.set_nonblocking(false)?;
    socket.set_nodelay(true)?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024))
        .max_write_buffer_size(8 * 1024 * 1024);
    let mut ws = accept_hdr_with_config(socket, OriginCheck(allow_origins.to_vec()), Some(config))?;
    ws.get_mut().set_nonblocking(true)?;
    let mut last_ping = Instant::now();
    while !stopped.load(Ordering::Relaxed) {
        for _ in 0..64 {
            match ws.read() {
                Ok(Message::Text(text)) => {
                    events.try_send(Event::Packet(peer, text.as_bytes().to_vec()))?
                }
                Ok(Message::Binary(bytes)) => {
                    events.try_send(Event::Packet(peer, bytes.to_vec()))?
                }
                Ok(Message::Close(_)) => return Ok(()),
                Ok(Message::Pong(_)) => events.try_send(Event::Activity(peer))?,
                Ok(_) => (),
                Err(e) if blocked(&e) => break,
                Err(e) => return Err(e.into()),
            }
        }
        if last_ping.elapsed() >= Duration::from_secs(1) {
            if let Err(error) = ws.write(Message::Ping(Vec::new().into())) {
                if !blocked(&error) {
                    return Err(error.into());
                }
            }
            last_ping = Instant::now();
        }
        for _ in 0..64 {
            match outgoing.try_recv() {
                Ok(bytes) => {
                    if let Err(e) = ws.write(Message::Binary(bytes.into())) {
                        if !blocked(&e) {
                            return Err(e.into());
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if let Err(e) = ws.flush() {
            if !blocked(&e) {
                return Err(e.into());
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

pub fn serve(scene: Scene, source: SimSource, addr: &str) -> Result<()> {
    serve_with_origins(scene, source, addr, vec![])
}

pub fn serve_with_origins(
    scene: Scene,
    source: SimSource,
    addr: &str,
    allow_origins: Vec<String>,
) -> Result<()> {
    let mut transport = WsServer::bind_with_origins(addr, allow_origins)?;
    let mut server = NetServer::new(scene, source, ServerConfig::default())?;
    eprintln!(
        "[net] WebSocket authority listening on ws://{}",
        transport.local_addr()?
    );
    let dt = Duration::from_secs_f64(TICK_MS / 1000.0);
    let mut next = Instant::now();
    loop {
        transport.tick(&mut server)?;
        next += dt;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else if now.duration_since(next) > Duration::from_millis(250) {
            next = now;
        }
    }
}
