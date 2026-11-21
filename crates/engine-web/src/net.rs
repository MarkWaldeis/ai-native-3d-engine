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
    background: bool,
    resync_pending: bool,
    input_ticks: u64,
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
    pub fn tick_with_impulses(&mut self, inputs_json: &str, first_inputs_json: &str, now_ms: f64) -> Result<String, JsValue> {
        self.tick_with_impulses_inner(inputs_json, first_inputs_json, now_ms)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn disconnect(&mut self) {
        self.closed = true;
        self.pending.clear();
    }
    pub fn set_background(&mut self, background: bool) -> Result<String, JsValue> {
        self.background_inner(background)
            .map_err(|error| JsValue::from_str(&error))
    }
}

impl NetGame {
    fn background_inner(&mut self, background: bool) -> Result<String, String> {
        if self.background != background {
            self.background = background;
            self.resync_pending = true;
            if let Some(client) = &mut self.client {
                client.set_paused(true);
                if background && !self.closed && client.entity_id().is_some() {
                    return serde_json::to_string(&ClientMessage::Pause)
                        .map_err(|error| error.to_string());
                }
            }
        }
        Ok(String::new())
    }
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
        let message = decode(message.as_bytes()).map_err(|e| e.to_string())?;
        if self.pending.len() >= 128 {
            self.resync_pending = true;
            if let Some(client) = &mut self.client {
                client.set_paused(true);
            }
        }
        self.pending.push_back(message);
        if self.background || self.resync_pending {
            self.drain_pending(self.last_now.unwrap_or(0.0))?;
        }
        Ok(())
    }
    fn drain_pending(&mut self, now: f64) -> Result<(), String> {
        let client = self
            .client
            .as_mut()
            .ok_or("load a scene before receiving")?;
        while let Some(message) = self.pending.pop_front() {
            self.clock_tick = match &message {
                ServerMessage::Welcome { server_tick, .. } => *server_tick,
                ServerMessage::Snapshot(snapshot) => snapshot.server_tick,
            } as f64;
            self.clock_ms = Some(now);
            client.receive(message, now).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn tick_inner(&mut self, inputs_json: &str, now: f64) -> Result<String, String> {
        self.tick_with_impulses_inner(inputs_json, inputs_json, now)
    }
    fn tick_with_impulses_inner(&mut self, inputs_json: &str, first_inputs_json: &str, now: f64) -> Result<String, String> {
        self.input_ticks = 0;
        if !now.is_finite() {
            return Err("network time must be finite".into());
        }
        let now = self.last_now.map_or(now, |previous| now.max(previous));
        let actions: Actions = serde_json::from_str(inputs_json).map_err(|e| e.to_string())?;
        let first_actions: Actions = serde_json::from_str(first_inputs_json).map_err(|e| e.to_string())?;
        if self
            .last_now
            .is_some_and(|previous| now - previous > 1000.0)
        {
            self.resync_pending = true;
            if let Some(client) = &mut self.client {
                client.set_paused(true);
            }
        }
        self.last_now = Some(now);
        let mut outgoing = vec![];
        self.drain_pending(now)?;
        if self.background {
            return self.snapshot(outgoing);
        }
        let client = self.client.as_mut().ok_or("load a scene before tick")?;
        if self.resync_pending {
            client.resync().map_err(|e| e.to_string())?;
            self.clock_tick = client
                .latest_snapshot()
                .map_or(self.clock_tick, |snapshot| snapshot.server_tick as f64);
            self.clock_ms = Some(now);
            self.resync_pending = false;
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
                let before = client.simulation().tick;
                let message = client
                    .advance_to_with_first_actions(estimated.floor() as u64, actions, first_actions, now)
                    .map_err(|e| e.to_string())?;
                self.input_ticks = client.simulation().tick - before;
                Some(message)
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
        Ok(json!({"tick":client.simulation().tick,"input_ticks":self.input_ticks,"outgoing":outgoing,"transforms":transforms,"state":state,
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
    fn impulses_wait_for_a_prediction_tick_and_run_once_during_catchup() {
        let scene: Scene = serde_json::from_value(json!({"entities":[
            {"id":1,"name":"player","kind":"box","size":[1,1,1],"tags":["net_player"],
             "script":{"source":"fn on_tick(ctx,dt) { if ctx.input(\"fire\") > 0.5 { let n=ctx.state_get(\"pulses\"); if n==() {n=0;} ctx.state_set(\"pulses\",n+1); } }"}}
        ]})).unwrap();
        let mut game = NetGame::new();
        game.load_inner(&serde_json::to_string(&scene).unwrap(), "{}").unwrap();
        let mut server = NetServer::new(scene, BTreeMap::new(), ServerConfig::default()).unwrap();
        let welcome = server.receive(1, ClientMessage::Hello { name: "impulses".into() }).unwrap().unwrap();
        game.receive_inner(&serde_json::to_string(&welcome).unwrap()).unwrap();
        game.tick_inner("{}", 0.0).unwrap();
        for now in [1.0, 2.0] {
            let result: Value = serde_json::from_str(&game.tick_with_impulses_inner("{}", "{\"fire\":1}", now).unwrap()).unwrap();
            assert_eq!(result["input_ticks"], 0);
            assert!(game.client.as_ref().unwrap().simulation().net_script_state().get("pulses").is_none());
        }
        let result: Value = serde_json::from_str(&game.tick_with_impulses_inner("{}", "{\"fire\":1}", 100.0).unwrap()).unwrap();
        assert!(result["input_ticks"].as_u64().unwrap() > 1);
        assert_eq!(game.client.as_ref().unwrap().simulation().net_script_state()["pulses"], 1);
        game.tick_with_impulses_inner("{}", "{}", 200.0).unwrap();
        assert_eq!(game.client.as_ref().unwrap().simulation().net_script_state()["pulses"], 1);
        let first_tick = game.client.as_ref().unwrap().simulation().tick;
        let result: Value = serde_json::from_str(&game.tick_with_impulses_inner("{}", "{\"fire\":1}", 900.0).unwrap()).unwrap();
        assert!(result["input_ticks"].as_u64().unwrap() > ClientConfig::default().redundant_frames as u64);
        let message = decode(result["outgoing"][0].as_str().unwrap().as_bytes()).unwrap();
        if let ClientMessage::Input { frames, .. } = message {
            assert_eq!(frames.iter().find(|f| f.tick == first_tick).unwrap().actions["fire"], 1.0);
            assert_eq!(frames.iter().find(|f| f.tick == first_tick + 1).unwrap().actions.get("fire").copied().unwrap_or(0.0), 0.0);
            assert_eq!(frames.iter().filter(|f| f.actions.get("fire").copied().unwrap_or(0.0) > 0.5).count(), 1);
        } else { panic!("expected client input frames"); }
        assert_eq!(game.client.as_ref().unwrap().simulation().net_script_state()["pulses"], 2);
        assert!(game.tick_with_impulses_inner("{}", "[]", 901.0).is_err());
        game.disconnect();
        let result: Value = serde_json::from_str(&game.tick_with_impulses_inner("{}", "{\"fire\":1}", 902.0).unwrap()).unwrap();
        assert_eq!(result["input_ticks"], 0);
    }

    #[test]
    fn stalled_animation_frames_do_not_overflow_the_snapshot_queue() {
        for background in [false, true] {
            let scene = scene();
            let mut game = NetGame::new();
            game.load_inner(&serde_json::to_string(&scene).unwrap(), "{}")
                .unwrap();
            let mut server = NetServer::new(
                scene,
                BTreeMap::new(),
                ServerConfig {
                    snapshot_interval: 1,
                    timeout_ticks: 10,
                    ..ServerConfig::default()
                },
            )
            .unwrap();
            let welcome = server
                .receive(
                    1,
                    ClientMessage::Hello {
                        name: "background".into(),
                    },
                )
                .unwrap()
                .unwrap();
            game.receive_inner(&serde_json::to_string(&welcome).unwrap())
                .unwrap();
            game.tick_inner("{\"move_x\":1}", 0.0).unwrap();
            let pause = game.background_inner(background).unwrap();
            if !pause.is_empty() {
                server
                    .receive(1, decode(pause.as_bytes()).unwrap())
                    .unwrap();
            }
            for _ in 0..1000 {
                server.activity(1);
                for (_, snapshot) in server.tick().unwrap() {
                    game.receive_inner(&serde_json::to_string(&snapshot).unwrap())
                        .unwrap();
                }
            }
            assert_eq!(server.assigned_entity(1), Some(1));
            if background {
                assert_eq!(
                    game.client.as_ref().unwrap().simulation().tick,
                    ClientConfig::default().initial_lead_ticks
                );
            }
            game.background_inner(false).unwrap();
            let result: Value =
                serde_json::from_str(&game.tick_inner("{}", 60_000.0).unwrap()).unwrap();
            assert!(
                result["tick"].as_u64().unwrap() <= 1000 + ClientConfig::default().max_lead_ticks
            );
            assert!(game.client.as_ref().unwrap().stats().corrections <= 1);
            assert!(game.pending.is_empty());
            assert_eq!(result["connected"], true);
        }
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
    fn stale_frame_timestamps_do_not_rewind_network_time_or_disconnect() {
        let mut game = NetGame::new();
        game.load_inner(&serde_json::to_string(&scene()).unwrap(), "{}")
            .unwrap();
        game.tick_inner("{}", 1000.0).unwrap();
        let mut server = NetServer::new(scene(), BTreeMap::new(), ServerConfig::default()).unwrap();
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
        for now in [995.0, 1000.0, 0.0, 1001.0] {
            let result: Value = serde_json::from_str(&game.tick_inner("{}", now).unwrap()).unwrap();
            assert_eq!(game.last_now, Some(now.max(1000.0)));
            assert_eq!(result["connected"], true);
            assert!(!game.closed);
        }
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
        for now in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                game.tick_inner("{}", now).unwrap_err(),
                "network time must be finite"
            );
        }
        game.tick_inner("{}", 1.0).unwrap();
    }
}
