use crate::PeerId;
use anyhow::{bail, Result};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

pub trait Transport {
    fn send(&mut self, to: PeerId, bytes: &[u8]) -> Result<()>;
    fn recv(&mut self) -> Result<Option<(PeerId, Vec<u8>)>>;
}

#[derive(Clone, Copy, Debug)]
pub struct LinkConfig {
    pub latency_ms: f64,
    pub jitter_ms: f64,
    pub loss: f64,
    pub duplication: f64,
    /// Additional random delay; independent packets can overtake each other.
    pub reorder_ms: f64,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            latency_ms: 0.0,
            jitter_ms: 0.0,
            loss: 0.0,
            duplication: 0.0,
            reorder_ms: 0.0,
        }
    }
}

struct Packet {
    at: f64,
    order: u64,
    from: PeerId,
    to: PeerId,
    bytes: Vec<u8>,
}
struct Network {
    now: f64,
    seed: u64,
    next: u64,
    default: LinkConfig,
    links: BTreeMap<(PeerId, PeerId), LinkConfig>,
    packets: Vec<Packet>,
    inboxes: BTreeMap<PeerId, VecDeque<(PeerId, Vec<u8>)>>,
    dropped: u64,
}

impl Network {
    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

/// Shared deterministic virtual network. Advance its clock, then drain endpoints.
#[derive(Clone)]
pub struct SimNetwork(Rc<RefCell<Network>>);

impl SimNetwork {
    pub fn new(seed: u64, config: LinkConfig) -> Self {
        Self(Rc::new(RefCell::new(Network {
            now: 0.0,
            seed: seed.max(1),
            next: 0,
            default: config,
            links: BTreeMap::new(),
            packets: vec![],
            inboxes: BTreeMap::new(),
            dropped: 0,
        })))
    }
    pub fn endpoint(&self, id: PeerId) -> SimTransport {
        self.0.borrow_mut().inboxes.entry(id).or_default();
        SimTransport {
            id,
            network: self.clone(),
        }
    }
    pub fn set_link(&self, from: PeerId, to: PeerId, config: LinkConfig) {
        self.0.borrow_mut().links.insert((from, to), config);
    }
    pub fn dropped_packets(&self) -> u64 {
        self.0.borrow().dropped
    }
    pub fn now_ms(&self) -> f64 {
        self.0.borrow().now
    }
    pub fn advance_to(&self, now_ms: f64) -> Result<()> {
        let mut net = self.0.borrow_mut();
        if !now_ms.is_finite() || now_ms < net.now {
            bail!("virtual clock must be finite and monotonic");
        }
        net.now = now_ms;
        net.packets
            .sort_by(|a, b| a.at.total_cmp(&b.at).then(a.order.cmp(&b.order)));
        let count = net.packets.partition_point(|packet| packet.at <= now_ms);
        let ready: Vec<_> = net.packets.drain(..count).collect();
        for p in ready {
            net.inboxes
                .entry(p.to)
                .or_default()
                .push_back((p.from, p.bytes));
        }
        Ok(())
    }
}

pub struct SimTransport {
    id: PeerId,
    network: SimNetwork,
}
impl Transport for SimTransport {
    fn send(&mut self, to: PeerId, bytes: &[u8]) -> Result<()> {
        let mut net = self.network.0.borrow_mut();
        let config = net
            .links
            .get(&(self.id, to))
            .copied()
            .unwrap_or(net.default);
        if net.random() < config.loss {
            net.dropped += 1;
            return Ok(());
        }
        let copies = if net.random() < config.duplication {
            2
        } else {
            1
        };
        for _ in 0..copies {
            let delay = (config.latency_ms
                + (2.0 * net.random() - 1.0) * config.jitter_ms
                + net.random() * config.reorder_ms)
                .max(0.0);
            let at = net.now + delay;
            let order = net.next;
            net.next += 1;
            net.packets.push(Packet {
                at,
                order,
                from: self.id,
                to,
                bytes: bytes.to_vec(),
            });
        }
        Ok(())
    }
    fn recv(&mut self) -> Result<Option<(PeerId, Vec<u8>)>> {
        Ok(self
            .network
            .0
            .borrow_mut()
            .inboxes
            .entry(self.id)
            .or_default()
            .pop_front())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod udp {
    use super::*;
    use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

    pub struct UdpTransport {
        socket: UdpSocket,
        peers: BTreeMap<PeerId, SocketAddr>,
        next: PeerId,
    }
    impl UdpTransport {
        pub fn bind(address: impl ToSocketAddrs) -> Result<Self> {
            let socket = UdpSocket::bind(address)?;
            socket.set_nonblocking(true)?;
            Ok(Self {
                socket,
                peers: BTreeMap::new(),
                next: 1,
            })
        }
        pub fn local_addr(&self) -> Result<SocketAddr> {
            Ok(self.socket.local_addr()?)
        }
        pub fn add_peer(&mut self, id: PeerId, address: SocketAddr) {
            self.peers.insert(id, address);
            self.next = self.next.max(id.saturating_add(1));
        }
    }
    impl Transport for UdpTransport {
        fn send(&mut self, to: PeerId, bytes: &[u8]) -> Result<()> {
            let address = self
                .peers
                .get(&to)
                .ok_or_else(|| anyhow::anyhow!("unknown UDP peer {to}"))?;
            self.socket.send_to(bytes, address)?;
            Ok(())
        }
        fn recv(&mut self) -> Result<Option<(PeerId, Vec<u8>)>> {
            let mut bytes = vec![0; 65536];
            match self.socket.recv_from(&mut bytes) {
                Ok((count, address)) => {
                    bytes.truncate(count);
                    let id = self
                        .peers
                        .iter()
                        .find(|(_, a)| **a == address)
                        .map(|(id, _)| *id)
                        .unwrap_or_else(|| {
                            let id = self.next;
                            self.next += 1;
                            self.peers.insert(id, address);
                            id
                        });
                    Ok(Some((id, bytes)))
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                Err(error) => Err(error.into()),
            }
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub use udp::UdpTransport;
