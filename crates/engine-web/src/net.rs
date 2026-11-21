use engine_net::{
    protocol::{decode, encode},
    Actions, ClientConfig, ClientMessage, NetClient, ServerMessage, TICK_MS,
};
use serde_json::{json, Value};
use shared::scene::{Entity, Scene};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Default)]
pub struct NetGame {
    client: Option<NetClient>,
    pending: VecDeque<ServerMessage>,
    known: BTreeMap<u64, Entity>,
    clock_tick: f64,
    clock_ms: Option<f64>,
    last_now: Option<f64>,
    last_hello: Option<f64>,
    closed: bool,
}

#[wasm_bindgen]
impl NetGame {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn load(&mut self, scene_json: &str, scripts_json: &str) -> Result<String, JsValue> {
        self.load_inner(scene_json, scripts_json)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn receive(&mut self, message: &str) -> Result<(), JsValue> {
        self.receive_inner(message)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn tick(&mut self, inputs_json: &str, now_ms: f64) -> Result<String, JsValue> {
        self.tick_inner(inputs_json, now_ms)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn disconnect(&mut self) {
        self.closed = true;
        self.pending.clear();
    }
}

impl NetGame {
    fn load_inner(&mut self, scene_json: &str, scripts_json: &str) -> Result<String, String> {
        let scene: Scene = serde_json::from_str(scene_json).map_err(|e| e.to_string())?;
        let sources: BTreeMap<String, String> =
            serde_json::from_str(scripts_json).map_err(|e| e.to_string())?;
        let client =
            NetClient::new(scene, sources, ClientConfig::default()).map_err(|e| e.to_string())?;
        *self = Self {
            client: Some(client),
            ..Self::default()
        };
        self.snapshot(vec![])
    }
    fn receive_inner(&mut self, message: &str) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        if self.pending.len() >= 256 {
            return Err("network receive queue overflow".into());
        }
        self.pending
            .push_back(decode(message.as_bytes()).map_err(|e| e.to_string())?);
        Ok(())
    }
    fn tick_inner(&mut self, inputs_json: &str, now: f64) -> Result<String, String> {
        if !now.is_finite() || self.last_now.is_some_and(|previous| now < previous) {
            return Err("network time must be finite and monotonic".into());
        }
        let actions: Actions = serde_json::from_str(inputs_json).map_err(|e| e.to_string())?;
        let client = self.client.as_mut().ok_or("load a scene before tick")?;
        self.last_now = Some(now);
        let mut outgoing = vec![];
        while let Some(message) = self.pending.pop_front() {
            let tick = match &message {
                ServerMessage::Welcome { server_tick, .. } => *server_tick,
                ServerMessage::Snapshot(snapshot) => snapshot.server_tick,
            };
            self.clock_tick = tick as f64;
            self.clock_ms = Some(now);
            client.receive(message, now).map_err(|e| e.to_string())?;
        }
        if !self.closed {
            let message = if client.entity_id().is_some() {
                let estimated =
                    self.clock_tick + self.clock_ms.map_or(0.0, |at| (now - at) / TICK_MS);
                // Long background suspensions wait for a fresh authority snapshot,
                // rather than replaying an unbounded wall-clock gap.
                if estimated as u64 > client.simulation().tick.saturating_add(60) {
                    self.clock_tick = client
                        .latest_snapshot()
                        .map_or(client.simulation().tick, |s| s.server_tick)
                        as f64;
                    self.clock_ms = Some(now);
                }
                let estimated =
                    self.clock_tick + self.clock_ms.map_or(0.0, |at| (now - at) / TICK_MS);
                Some(
                    client
                        .advance_to(estimated.floor() as u64, actions, now)
                        .map_err(|e| e.to_string())?,
                )
            } else if self.last_hello.is_none_or(|at| now - at >= 250.0) {
                self.last_hello = Some(now);
                Some(ClientMessage::Hello {
                    name: "browser".into(),
                })
            } else {
                None
            };
            if let Some(message) = message {
                outgoing.push(
                    String::from_utf8(encode(&message).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?,
                );
            }
        }
        self.snapshot(outgoing)
    }
    fn snapshot(&mut self, outgoing: Vec<String>) -> Result<String, String> {
        let client = self.client.as_ref().ok_or("load a scene before snapshot")?;
        let fraction = self
            .last_now
            .zip(self.clock_ms)
            .map_or(0.0, |(now, at)| (now - at) / TICK_MS);
        let definitions = if client.latest_snapshot().is_some() {
            client.entity_definitions_at(fraction)
        } else {
            client.simulation().scene().entities
        };
        let current: BTreeSet<_> = definitions.iter().map(|e| e.id).collect();
        let mut removed: Vec<_> = self
            .known
            .keys()
            .filter(|id| !current.contains(id))
            .copied()
            .collect();
        let mut spawned = vec![];
        for entity in &definitions {
            let changed = self.known.get(&entity.id).is_none_or(|old| {
                let mut old = old.clone();
                old.transform.position = entity.transform.position;
                old.transform.rotation_deg = entity.transform.rotation_deg;
                old != *entity
            });
            if changed {
                if self.known.contains_key(&entity.id) {
                    removed.push(entity.id);
                }
                spawned.push(entity.clone());
            }
        }
        self.known = definitions.iter().cloned().map(|e| (e.id, e)).collect();
        let transforms: Vec<_> = definitions
            .iter()
            .map(|e| json!({"id":e.id,"name":e.name,"tags":e.tags,"transform":e.transform}))
            .collect();
        let state = client
            .latest_snapshot()
            .map_or(Value::Object(Default::default()), |s| {
                s.script_state.clone()
            });
        let players = client.latest_snapshot().map_or(0, |s| s.players.len());
        Ok(json!({"tick":client.simulation().tick,"outgoing":outgoing,"transforms":transforms,"state":state,
            "logs":[],"spawned":spawned,"removed":removed,"fx":[],"events":[],"animators":[],"nav_agents":[],
            "my_entity":client.entity_id(),"player_number":client.client_id(),"connected":!self.closed && client.entity_id().is_some(),
            "players":players,"stats":client.stats()}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_net::{NetServer, ServerConfig};

    fn scene() -> Scene {
        serde_json::from_value(json!({"entities":[
        {"id":1,"name":"player","kind":"box","size":[1,1,1],"tags":["net_player"],
        "script":{"source":"fn on_tick(ctx,dt) { let p=ctx.position();ctx.set_position([p[0]+ctx.input(\"move_x\")*dt,p[1],p[2]]); }"}}
    ]})).unwrap()
    }

    #[test]
    fn clock_reanchors_when_authority_runs_slower_than_wall_time() {
        let scene = scene();
        let mut game = NetGame::new();
        game.load_inner(&serde_json::to_string(&scene).unwrap(), "{}")
            .unwrap();
        let mut server = NetServer::new(
            scene,
            BTreeMap::new(),
            ServerConfig {
                snapshot_interval: 1,
                ..ServerConfig::default()
            },
        )
        .unwrap();
        let welcome = server
            .receive(
                1,
                ClientMessage::Hello {
                    name: "browser".into(),
                },
            )
            .unwrap()
            .unwrap();
        game.receive_inner(&serde_json::to_string(&welcome).unwrap())
            .unwrap();
        for tick in 1..20 {
            for (_, snapshot) in server.tick().unwrap() {
                game.receive_inner(&serde_json::to_string(&snapshot).unwrap())
                    .unwrap();
            }
            let result: Value =
                serde_json::from_str(&game.tick_inner("{}", tick as f64 * 100.0).unwrap()).unwrap();
            assert!(
                result["tick"].as_u64().unwrap()
                    <= tick + ClientConfig::default().max_lead_ticks as u64
            );
            for message in result["outgoing"].as_array().unwrap() {
                server
                    .receive(1, decode(message.as_str().unwrap().as_bytes()).unwrap())
                    .unwrap();
            }
        }
    }

    #[test]
    fn network_bridge_drives_rust_prediction_and_lifetimes() {
        let mut game = NetGame::new();
        let scene = scene();
        let initial: Value = serde_json::from_str(
            &game
                .load_inner(&serde_json::to_string(&scene).unwrap(), "{}")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(initial["spawned"].as_array().unwrap().len(), 1);
        let mut server = NetServer::new(scene, BTreeMap::new(), ServerConfig::default()).unwrap();
        for tick in 0..180 {
            let result: Value = serde_json::from_str(
                &game
                    .tick_inner("{\"move_x\":1}", tick as f64 * TICK_MS)
                    .unwrap(),
            )
            .unwrap();
            for message in result["outgoing"].as_array().unwrap() {
                if let Some(reply) = server
                    .receive(1, decode(message.as_str().unwrap().as_bytes()).unwrap())
                    .unwrap()
                {
                    game.receive_inner(&serde_json::to_string(&reply).unwrap())
                        .unwrap();
                }
            }
            for (_, reply) in server.tick().unwrap() {
                game.receive_inner(&serde_json::to_string(&reply).unwrap())
                    .unwrap();
            }
        }
        let result: Value =
            serde_json::from_str(&game.tick_inner("{}", 180.0 * TICK_MS).unwrap()).unwrap();
        assert_eq!(result["my_entity"], 1);
        assert_eq!(result["connected"], true);
        assert_eq!(result["players"], 1);
        assert!(
            result["transforms"][0]["transform"]["position"][0]
                .as_f64()
                .unwrap()
                > 2.0
        );
        assert!(result["spawned"].as_array().unwrap().is_empty());
        game.disconnect();
        let result: Value =
            serde_json::from_str(&game.tick_inner("{}", 181.0 * TICK_MS).unwrap()).unwrap();
        assert_eq!(result["connected"], false);
        assert!(result["outgoing"].as_array().unwrap().is_empty());
    }
    #[test]
    fn network_bridge_rejects_bad_inputs_and_empty_lifecycle_is_explicit() {
        let mut game = NetGame::new();
        assert!(game.tick_inner("{}", 0.0).is_err());
        assert!(game.receive_inner("bad JSON").is_err());
        assert!(game.load_inner("bad JSON", "{}").is_err());
        game.load_inner(&serde_json::to_string(&scene()).unwrap(), "{}")
            .unwrap();
        assert!(game.tick_inner("[]", 0.0).is_err());
        assert!(game.tick_inner("{}", f64::NAN).is_err());
        game.tick_inner("{}", 1.0).unwrap();
        assert!(game.tick_inner("{}", 0.0).is_err());
    }
}
