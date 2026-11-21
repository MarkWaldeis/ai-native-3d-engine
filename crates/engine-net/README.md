# engine-net

Transport-independent authoritative netcode over `engine-runtime`, at 60 Hz.
All simulation construction uses in-memory script sources. Only `UdpTransport`
is native-only; the remaining crate has no socket or wall-clock dependency.

## Tick contract

State `T` is the state **before** input frame `T`; executing it produces state
`T + 1`. Load the same scene/scripts on server and clients. Tag assignable
players `net_player`; assignment is in entity-ID order. Client messages cannot
choose an entity. Action maps are complete samples: missing keys become zero.
Missing frames repeat the last sample. Redundant frames are deduplicated; newly
received past frames are dropped and counted once as late.

1. Construct `NetServer::new(scene, sources, ServerConfig::default())` and
   `NetClient::new(scene, sources, ClientConfig::default())`.
2. Route encoded `ClientMessage::Hello` to `NetServer::receive(peer, message)`;
   return its `Welcome` response to `NetClient::receive(message, now_ms)`.
   Retry Hello until welcomed; handling it is idempotent.
3. On the application's synchronized server clock, call
   `client.advance_to(server_tick, actions, now_ms)` and send its returned
   message, or call `client.tick(actions, now_ms)` once per prediction tick.
   `input_message()` also permits heartbeats without advancing simulation.
4. Feed received inputs to the server **before** `server.tick()`. Route returned
   `(peer, ServerMessage)` pairs back to clients. The server snapshots every two
   ticks by default; peers silent for 300 ticks time out. `disconnect(peer)`
   frees the slot and zeros its last action keys on the next authority tick.
5. Render with `client.render_states(fractional_ticks_since_latest_update)`;
   do not apply rendered poses back to physics.

The application owns clock synchronization, transport polling, and peer IDs.
`advance_to` incorporates adaptive lead and pauses prediction if lead decreases.
The lead estimate uses acknowledged input timing (subtracting intentional lead),
a configurable jitter allowance, and late-frame feedback; it is not a precise
clock-synchronization or ping protocol. `NetStats::packets_lost` estimates gaps
between accepted snapshot ticks, not confirmed end-to-end packet loss.

## Snapshots and reconciliation

The protocol has generic JSON `encode` / `decode` functions; encoded `Vec<u8>`
length is wire payload size. Full snapshots include all entity poses/velocities
and the complete shared script-state map. Deltas include changed entities and
removed IDs relative to the peer's last acknowledged baseline. Both ends retain
the **reconstructed** baseline, preventing epsilon omissions from accumulating
drift. A missing/expired baseline requests a full snapshot by clearing the ACK.

Prediction stores a bounded 128-tick ring of full `SimState` checkpoints and
own input history. Corrections restore `T`, apply authority poses/velocities and
script state, then replay own frames through the previous prediction tick.
Unobserved remote inputs hold the simulation's prior values (initially zero).
Out-of-ring snapshots bootstrap a fresh simulation at `T`; replay of discarded
input history cannot be exact. Visual position error decays linearly over 100 ms.
Remote rendering uses delayed snapshots, shortest-arc quaternion slerp matching
Rapier's XYZ Euler convention, and at most 250 ms of velocity extrapolation.
The monotonic render cursor prevents reordered arrivals from rewinding playback.

The wire schema contains no mesh/collider/script definitions. Entity removal is
supported; spawning an ID absent from the client's scene is rejected explicitly.
Late joiners receive authority transforms/state, not full Rapier contact caches
or combat internals. This is a trusted-session prototype: no authentication,
encryption, fragmentation, congestion control, or reliable event channel. UDP
payloads must fit one datagram; large scenes should use another `Transport` or
add packetization. JSON decoding is capped at 4 MiB.

## Transports and checks

`Transport` sends/receives opaque bytes addressed by `PeerId`.
`SimNetwork::new(seed, LinkConfig)` creates a shared network with endpoints;
advance `advance_to(now_ms)` before draining received packets. Per-direction
overrides model asymmetric links. Seeded xorshift drives jitter, loss,
duplication, and additional reordering delay. `UdpTransport` uses nonblocking
`std::net::UdpSocket`, maps addresses to peer IDs, and supports explicit peers
via `add_peer`.

```sh
cargo test -p engine-net -- --nocapture
cargo run -p engine-net --example loopback -- --rtt-ms 120 --jitter-ms 20 --loss 0.05 --seconds 10
cargo clippy -p engine-net
# When installed:
cargo build -p engine-net --target wasm32-unknown-unknown
```
