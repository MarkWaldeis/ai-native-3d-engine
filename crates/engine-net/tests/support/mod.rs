use anyhow::Result;
use engine_net::{
    protocol::{decode, encode},
    transport::{LinkConfig, SimNetwork, SimTransport, Transport},
    Actions, ClientConfig, ClientMessage, NetClient, NetServer, ServerConfig, ServerMessage,
    TICK_MS,
};
use serde_json::json;
use shared::scene::Scene;
use std::collections::BTreeMap;

pub fn scene() -> Scene {
    let script = r#"fn on_tick(ctx, dt) {
        let v = ctx.velocity();
        ctx.set_velocity([ctx.input("move_x") * 2.0, v[1], ctx.input("move_z") * 2.0]);
        let score=ctx.state_get("score" + ctx.id().to_string());
        if score==() { score=0; }
        ctx.state_set("score" + ctx.id().to_string(),score+1);
    }"#;
    serde_json::from_value(json!({"entities":[
        {"id":1,"name":"floor","kind":"box","size":[100,1,100],"transform":{"position":[0,-0.5,0]},
         "collider":{"shape":"box"},"rigidbody":{"type":"static"}},
        {"id":2,"name":"A","kind":"box","size":[1,1,1],"tags":["net_player"],
         "transform":{"position":[-3,0.6,0]},"collider":{"shape":"box"},"rigidbody":{"type":"dynamic"},
         "props":{"ai_lock_rotation":true},"script":{"source":script}},
        {"id":3,"name":"B","kind":"box","size":[1,1,1],"tags":["net_player"],
         "transform":{"position":[3,0.6,0]},"collider":{"shape":"box"},"rigidbody":{"type":"dynamic"},
         "props":{"ai_lock_rotation":true},"script":{"source":script}},
        {"id":4,"name":"crate","kind":"box","size":[0.7,0.7,0.7],"transform":{"position":[0,0.4,0]},
         "collider":{"shape":"box"},"rigidbody":{"type":"dynamic","mass":0.2}}
    ]})).unwrap()
}

pub fn actions(player: usize, tick: u64, moving_ticks: u64) -> Actions {
    let value = if tick < moving_ticks {
        if player == 0 {
            1.0
        } else {
            -0.7
        }
    } else {
        0.0
    };
    BTreeMap::from([
        ("move_x".into(), value),
        ("move_z".into(), 0.0),
        ("score".into(), player as f64 + 1.0),
    ])
}

pub struct Harness {
    pub server: NetServer,
    pub clients: Vec<NetClient>,
    pub network: SimNetwork,
    authority: SimTransport,
    endpoints: Vec<SimTransport>,
    pub tick: u64,
    pub snapshots_received: u64,
}

impl Harness {
    pub fn new(link: LinkConfig, lead: u64) -> Result<Self> {
        let network = SimNetwork::new(0x123456, link);
        let authority = network.endpoint(0);
        let endpoints = vec![network.endpoint(1), network.endpoint(2)];
        let config = ClientConfig {
            initial_lead_ticks: lead,
            ..ClientConfig::default()
        };
        Ok(Self {
            server: NetServer::new(scene(), BTreeMap::new(), ServerConfig::default())?,
            clients: (0..2)
                .map(|_| NetClient::new(scene(), BTreeMap::new(), config.clone()))
                .collect::<Result<_>>()?,
            network,
            authority,
            endpoints,
            tick: 0,
            snapshots_received: 0,
        })
    }

    fn pump(&mut self, now: f64) -> Result<()> {
        // Multiple zero-time passes deliver a zero-latency handshake in one tick.
        for _ in 0..3 {
            self.network.advance_to(now)?;
            while let Some((peer, bytes)) = self.authority.recv()? {
                if let Some(response) = self.server.receive(peer, decode(&bytes)?)? {
                    self.authority.send(peer, &encode(&response)?)?;
                }
            }
            self.network.advance_to(now)?;
            for (i, endpoint) in self.endpoints.iter_mut().enumerate() {
                while let Some((_, bytes)) = endpoint.recv()? {
                    let message: ServerMessage = decode(&bytes)?;
                    if matches!(message, ServerMessage::Snapshot(_)) {
                        self.snapshots_received += 1;
                    }
                    self.clients[i].receive(message, now)?;
                }
            }
        }
        Ok(())
    }

    pub fn step(&mut self, moving_ticks: u64) -> Result<()> {
        let now = self.tick as f64 * TICK_MS;
        for (i, endpoint) in self.endpoints.iter_mut().enumerate() {
            if self.clients[i].entity_id().is_none() && self.tick.is_multiple_of(15) {
                endpoint.send(
                    0,
                    &encode(&ClientMessage::Hello {
                        name: format!("player-{i}"),
                    })?,
                )?;
            }
        }
        self.pump(now)?;
        for (i, client) in self.clients.iter_mut().enumerate() {
            if client.entity_id().is_some() {
                let message =
                    client.advance_to(self.tick, actions(i, self.tick, moving_ticks), now)?;
                self.endpoints[i].send(0, &encode(&message)?)?;
            }
        }
        self.pump(now)?;
        for (peer, message) in self.server.tick()? {
            self.authority.send(peer, &encode(&message)?)?;
        }
        self.pump(now)?;
        self.tick += 1;
        Ok(())
    }
}
