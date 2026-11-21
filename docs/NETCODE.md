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

- MCP `net_simulate` explicitly rejects scenes with `props.ai_terrain`; terrain
  sources/project-root loading are not yet wired into the network runner.

- No spawned-entity replication: snapshots contain no new mesh, collider or
  script definitions; unknown IDs are rejected. Removal of known IDs works.
- No authentication, encryption, reliable event channel or congestion control.
  Do not expose the UDP prototype to untrusted networks.
- UDP has **no fragmentation**: each encoded message must fit one datagram.
  Large scenes require packetization or another transport.
- No browser transport yet (WebSocket/WebRTC/WebTransport adapters are future
  work); WASM compilation alone does not make browser multiplayer available.
- Late join and network correction transfer poses/velocities and shared script
  state, not server Rapier contact caches or combat internals. Scene/script
  versions must match; cross-platform bit determinism is not guaranteed.
- Full rollback rings are intentionally bounded but clone physics state and can
  consume substantial memory with large scenes. MCP network simulations are
  synchronous CPU work; use short runs for large worlds.
