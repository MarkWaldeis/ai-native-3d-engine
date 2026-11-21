# Multiplayer netcode

`engine-net` provides transport-independent authoritative multiplayer on the
same 60 Hz Rust/Rapier/Rhai runtime as native and WebAssembly play. The server
and clients load the same scene and script sources. `shared/` is unchanged.
This is a trusted-session prototype, not a public multiplayer service.

## Model and tick alignment

- State **T** is the state before executing input frame **T**. One step produces
  state **T + 1**. Snapshot ticks and rollback checkpoints follow this convention.
- Joiners receive `Welcome {client_id, entity_id, server_tick}`. The next free
  entity tagged `net_player`, sorted by entity ID, becomes their slot. Arrival
  order determines which client gets which slot. Input messages cannot select
  another entity. Disconnect/timeout frees the slot and zeros its last inputs.
- Client input frames carry tick, sequence and a complete action map. Clients
  resend up to 32 unacknowledged frames. Duplicate frames are ignored; new past
  frames are dropped and counted once as late. Missing frames repeat the last
  sample; omitted action keys in a new sample become zero.
- `ctx.input`, `ctx.move_dir` and `ctx.aim_dir` read the hook entity's input first,
  then fall back to global inputs. Existing single-player scenes remain valid.

The application supplies a synchronized server clock and monotonic milliseconds.
`NetClient::advance_to` targets server tick plus an adaptive lead. Initial lead is
configurable; estimates from input acknowledgements and late-frame feedback add
a jitter allowance to approximately RTT/2 in ticks. Increasing lead predicts
extra ticks; decreasing it pauses prediction. `tick` is available for callers
that schedule prediction themselves. The headless runner drives a virtual clock,
so its results do not depend on machine speed.

## Prediction and reconciliation

Each client keeps a bounded 128-tick ring of full `Simulation::save_state`
checkpoints and its own input history. At snapshot T, own position/velocity/
rotation and shared script state are compared with predicted state T. The
default position threshold is 1e-3 m. A mismatch restores T, applies authoritative
poses/velocities and script state, and replays own frames through the previous
prediction tick. Unknown remote inputs hold previous values, initially zero.

Corrections count toward `NetStats`, including maximum/average positional error.
Own rendered position retains a correction offset that decays over 100 ms;
rendering never changes the physics simulation. An out-of-ring correction starts
a fresh simulation at T: discarded input history cannot be replayed exactly.

## Remote interpolation

Remote entities render from buffered full snapshots at approximately latest
server tick minus six ticks (100 ms). Positions lerp; rotations use shortest-arc
quaternion slerp with Rapier/nalgebra's Euler convention (`qz * qy * qx`, Euler
degrees on the wire). Linear/angular velocities permit at most 15 ticks (250 ms)
of extrapolation when the buffer runs dry. A monotonic render cursor prevents
reordered arrivals from rewinding playback. `render_states(fractional_ticks)`
combines these remotes with the smoothed predicted own player.

## Snapshot deltas and transports

The server sends per-client snapshots every two ticks by default. JSON messages
include processed input sequence, cumulative late-input count, script-state map
and entity position/rotation/linear/angular velocity. Acknowledged snapshots are
delta baselines: unchanged entities are omitted within a tiny configurable
epsilon; removed IDs are explicit. Both sides retain the reconstructed baseline,
not an untransmitted ideal state. Missing/expired baselines request a full state.
The decoder caps payloads at 4 MiB. Encoded byte-vector length is JSON payload
size, excluding transport/IP overhead.

- `Transport` sends/receives bytes addressed by opaque `PeerId`.
- `SimNetwork` is a seeded xorshift virtual network with per-direction latency,
  jitter, loss, duplication and extra reordering delay. Advance its millisecond
  clock before draining endpoints; no sockets or wall-clock reads are involved.
- Native-only `UdpTransport` uses nonblocking `std::net::UdpSocket`. Browser
  transport is **not implemented**; the protocol, runner, prediction and
  SimNetwork compile for `wasm32-unknown-unknown`.

RTT and `packets_lost` in `NetStats` are estimates from input timing / snapshot
tick gaps, not a precise ping or delivery protocol. Bandwidth is cumulative
snapshot bytes divided by elapsed time. SimNetwork also reports actual simulated
packet drops separately.

## AI-native MCP: `net_simulate`

Available through `tools/list`, `tools/call` and the generated `engine://guide`.
The facade loads project-confined scene/script files; the reusable runner uses
`Simulation::from_sources` and never reads files. A call runs a fresh server plus
N clients; it changes neither authoring scene/revision nor an existing play
session. Default scene is the current authoring scene. Optional `scene` accepts
a JSON scene or project-relative JSON path; script references remain relative to
the **project root**, not the scene directory.

```json
{
  "clients": 2,
  "ticks": 600,
  "rtt_ms": 120,
  "jitter_ms": 20,
  "loss": 0.05,
  "seed": 17,
  "scene": "examples/net_arena/scene.json",
  "inputs": [
    [{"tick": 0, "action": "move_x", "value": 1},
     {"tick": 180, "action": "move_x", "value": 0}],
    [{"tick": 0, "action": "move_x", "value": -0.7},
     {"tick": 180, "action": "move_x", "value": 0}]
  ]
}
```

`inputs` is an array of per-client event lists in zero-based client order. Events
use **local simulation ticks** and persist until replaced. Missing lists are
empty; at most 4096 events / 64 action names per client. `clients` is 1..=8,
`ticks` is 0..=36000 (`MAX_TICKS`), RTT/jitter are 0..=5000 ms, loss is 0..=1;
seed is an unsigned integer. The scene must have enough player slots. Event ticks
must be less than `ticks`. Invalid requests are MCP tool errors.

The response includes server tick/stats, per-client stats and positions,
`max_position_delta`, `compared_clients`, `converged`, and actual simulated
`packets_dropped`. Per-client `server_position` and `client_position` compare the
same `comparison_tick` (the final server tick); `predicted_position` is the
current ahead-of-server pose at `client_tick`. A missing connection or checkpoint
produces null comparison fields, **not** zero error. `converged` requires every
client to be comparable and maximum error <=1e-3 m. No extra settle time is added:
stop input events early if final resting-state convergence is the target. At
100% loss or zero ticks, clients may never connect and convergence is false.

## Arena and verification

`examples/net_arena/scene.json` contains four colored dynamic player boxes,
pushable crates and a floor. `player.rhai` preserves vertical velocity, applies
per-entity movement, and awards one point per player on reaching the center
strip (`score_<entity_id>`). Unassigned slots remain stationary. From repo root:

```sh
cargo run -p engine-net --example loopback -- \
  --scene examples/net_arena/scene.json --rtt-ms 120 --jitter-ms 20 --loss 0.05 --seconds 10
cargo test -p engine-net
cargo test -p engine-tests --test netcode
rustup target add wasm32-unknown-unknown
cargo build -p engine-net --target wasm32-unknown-unknown
cargo test --workspace
cargo clippy -p engine-net -p engine-mcp
```

The loopback example defaults to the arena, uses two clients, moves for the first
60% of the run, then stops and prints the same JSON report as the MCP runner.
Tests cover bit-exact zero-latency prediction, lossy two-client convergence,
interpolation/slerp, deltas, smoothing, slots/timeouts, UDP loopback, MCP isolation,
seed reproducibility, invalid/empty requests, and arena scoring/crate pushing.
These are numerical/headless checks, not visual verification of the arena.

## Limits

### Browser multiplayer (native WebSocket authority)

From the project/workspace root, use three terminals:

```sh
cargo run -- --export-web examples/net_arena/scene.json target/site/net_arena_ws
cargo run -- --net-serve examples/net_arena/scene.json --addr 127.0.0.1:7790
python3 -m http.server 8000 --bind 127.0.0.1 --directory target/site
```

The export directory must be new/empty. The exporter builds release WASM and
requires `rustup target add wasm32-unknown-unknown` and
`cargo install wasm-bindgen-cli --version 0.2.100 --locked` once.
Open **two tabs** at:

`http://127.0.0.1:8000/net_arena_ws/?net=ws://127.0.0.1:7790`

Press **Spielen** in each tab; use WASD/arrows or the touch joystick. Each
connection receives a different `net_player` slot. The camera follows that slot;
the HUD shows Spieler N, estimated ping and connected player count. Push the
brown crates toward the centre. The gold box at the back is script-spawned and
replaced every three seconds; all clients see the same authoritative lifecycle.
Pausing/hiding sends neutral input but does not pause the server. Server-driven
WebSocket pings and browser-generated pongs keep hidden tabs connected without
depending on JavaScript timers. Hidden tabs suspend prediction and retain bounded
reconstructed snapshots. Returning to the foreground discards stale inputs,
jumps to the latest authority, and resets the clock/RTT estimate. A closed
connection offers **Neu verbinden**, which reloads the client and requests a free
slot. There are four arena slots.

`?net` (empty) defaults to the current hostname on port 7790 (`wss` on HTTPS).
Alternatively set `props.ai_net` to a WS URL or `{ "url": "ws://..." }` on the
player; `?net=off` overrides it. Without either opt-in, WebGame/single-player is
unchanged. `window.__engineNet` exposes connection/slot/RTT/correction/entity
counts and rendered positions for automation.

`engine-web::NetGame` wraps `NetClient`. `load(scene_json,scripts_json)` prepares
the local scene; `receive(msg)` queues text protocol bytes; `tick(inputs_json,
now_ms)` returns outgoing protocol messages plus WebGame-style render deltas,
state, ownership and stats. Clock estimation, prediction, reconciliation,
interpolation and physics remain in Rust. JS only moves bytes and renders.

The native adapter uses tungstenite 0.28, bounded worker queues and a 60-Hz
authority thread. Text and binary frames carry the existing JSON encoding.
It is for **trusted local sessions**, without authentication or TLS. HTTPS pages
need an explicitly allowed origin (`--allow-origin https://game.example`,
repeatable) unless their hostname is localhost, 127.0.0.1 or [::1]. Native
requests without Origin remain allowed; foreign browser origins are rejected
during the handshake by default. This is not authentication. HTTPS pages
need a separately configured TLS WebSocket endpoint. Browser network terrain
streaming/asset transfer are not implemented; use the bundled net_arena scene.

Optional real-browser smoke check (Chrome installed, a fresh native authority
and HTTP server above running): install Playwright outside production dependencies, then run
`tools/verify_web_net.mjs`. It checks distinct slots, movement visible in both
tabs, crate pushing, spawned mesh lifecycle, HUD and disconnects; screenshots
and a JSON report are written to `target/net-shots/`.

```sh
npm install --prefix target/net-check-tools playwright
PLAYWRIGHT_MODULE="$PWD/target/net-check-tools/node_modules/playwright/index.mjs" \
  node tools/verify_web_net.mjs
```

### Remaining limits

- Script-spawned entities carry full definitions in full snapshots and on first
  appearance in deltas. Script file references must be available on every peer;
  this is not an asset-transfer protocol. Renderer mesh lifetimes can be driven
  by diffing `NetClient::entity_definitions()` IDs (aligned with interpolation).
- `SimSource::ProjectRoot(root)` loads native terrain and scripts via
  `Simulation::new`; MCP `net_simulate` uses the project root. Bundled-script
  callers can keep passing a map or `SimSource::InMemory(map)`.
- No authentication, encryption, reliable event channel or congestion control.
  Do not expose the UDP prototype to untrusted networks.
- UDP has **no fragmentation**: each encoded message must fit one datagram.
  Large scenes require packetization or another transport.
- Browser transport is WebSocket only; no WebRTC/WebTransport adapters yet.
- Late join and network correction transfer poses/velocities and shared script
  state, not server Rapier contact caches or combat internals. Scene/script
  versions must match; cross-platform bit determinism is not guaranteed.
- Full rollback rings are intentionally bounded but clone physics state and can
  consume substantial memory with large scenes. MCP network simulations are
  synchronous CPU work; use short runs for large worlds.
