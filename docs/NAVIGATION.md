# Deterministic NPC navigation

Navigation v1 is a bounded, conservative **2.5D quad navmesh**, not Recast or a
crowd simulator. No materials, lighting, shading or post-processing are changed.

## Bake and query

```json
{"tool":"nav_bake","arguments":{"params":{"radius":0.2,"height":1.0,"step":0.25,"max_slope_deg":45,"cell_size":0.25,"tile_size":128,"bounds":[-8,-8,8,8]},"expected_revision":12}}
{"tool":"nav_query_path","arguments":{"start":[-4.5,0,2.5],"goal":[-4.9,0,-3.3]}}
{"tool":"nav_lint","arguments":{"start":[-4.5,0,2.5]}}
{"tool":"get_screenshot","arguments":{"views":["top","iso"],"overlay_navigation":true,"overlay_ids":true}}
```

`nav_bake` writes an immutable `assets/navigation-<fingerprint>.nav.json` and
embeds the same typed data in `scene.navmesh`. Save the scene with its assets.
Global authoring locks and CAS apply; play-mode edits and atomic batches reject
the bake (filesystem output is not a reversible scene operation). Scene rollback
and persisted checkpoints restore the mesh; immutable, now-unreferenced assets
are not deleted automatically.

Static colliders are authoritative. A static visual entity can opt in with
`props.nav_geometry:true`; `props.nav_ignore:true` opts out. Dynamic/kinematic
bodies and navigation agents are excluded. This uses **the engine's collision
geometry**, including its existing mesh-collider bounding-box approximation,
not triangle-exact rendered GLBs. Rays sample surfaces and slope, supported
footprint samples erode boundaries by radius, and volume queries reject walls
and insufficient headroom. Step height limits connections across surfaces.
Conservative square footprints can reject narrow passages that a circle fits.

Tiles use the streaming grid's floor(x/tile_size), floor(z/tile_size), including
negative coordinates; initial MCP bake defaults to authored `ai_world.chunk_size`
or 128 m. Size must be a cell-size multiple. Tile fingerprints include overlapping
source geometry/content and a clearance halo: after edits, only changed/dependent
tiles are rasterized. Unchanged tiles are reused. Native simulation startup,
MCP path queries/lint and web export refresh the derived mesh incrementally; call
`nav_bake` again to persist refreshed authoring data/assets. There is **no live
obstacle carving/rebake inside fixed ticks**. Re-bake/restart after changing
static level geometry during play. Streaming heightfield terrain currently needs
explicit collider proxies and `nav_ignore:true` on the terrain source; its direct
bake is rejected rather than approximated.

Budgets: 65,536 sampled cells/polygons, 1,024 tiles, 1,024 path corners and 128
agents. Supply bounded bake regions or coarser cells for large worlds. Baked tiles
are stored separately in the mesh, but loaded together; streaming pauses agents
outside active simulation regions, not individual nav-tile IO. Separate region
bakes replace the active mesh rather than merging disjoint bake jobs.

## Agent and Rhai

```json
"nav_agent":{"speed":1.5,"offset_y":0.22,"target":[-4.9,0,-3.3]}
```

Targets/path points are **foot positions**. `offset_y` converts feet to entity
origin; the example sphere has its center 0.22 m above the floor. Use no rigidbody
or a kinematic one. Dynamic rigidbodies are rejected. The single scene bake
profile defines clearance; author agents that fit that radius/height.

```rhai
let reachable = ctx.nav_set_target([-4.9, 0.0, -3.3]);
let remaining_corners = ctx.nav_path();
```

Setter returns false and stores `status:"unreachable"` for an impossible route;
nonfinite targets or missing components/mesh produce script errors. Reassigning
the same planned target is cached. Clear `target` in authoring data to idle.
`nav_path()` returns remaining foot-position corners (empty on arrival/failure).
Serialized agent state includes path, cursor, planned target and status; loading
a simulation resets derived route state. Agent stepping follows scripts/physics
at 60 Hz, honors speed and inactive-region pausing, consumes multiple short
segments without overshoot, and updates kinematic query poses. No avoidance,
dynamic obstacle response, jumping, off-mesh links or moving platforms yet.

A* uses integer costs, stable polygon indices and ordered tie-breaking; corridor
line-of-sight string-pulling removes unnecessary corners without cutting blocked
diagonal cells. Height-changing sections retain corners. Native and WASM execute
the same Rust code; snapshots include `nav_agents` and release parity compares
their positions, targets, status and full routes every 30 ticks.

## M12 QA subset: bounded multi-agent parity workloads

`examples/nav_scaling.rs` supplies deterministic **pre-baked quad** workloads,
without modifying the physics backend, navigation implementation, player or
streaming code owned by the parallel sessions. Two L-shaped corridors (flat and
gently sloped) span three 16 m tiles, including negative coordinates. Each has
352 polygons and runs 0, 1, 16, 64 and 128 noncolliding agents for 600 ticks.
The geometry baker is deliberately bypassed via `Simulation::from_sources`;
the ramp is hand-authored nav data, **not direct heightfield support**.

```sh
# Unoptimized native smoke budget: 1000 ms per simulation step, including cold plans.
cargo run --example nav_scaling -- 1000 > target/nav-scaling.json
# Use a current export's exact WASM package; override PLAYWRIGHT_MODULE if needed.
node tools/verify_nav_scaling.mjs target/nav-scaling.json target/m15-site/coin_game
# Or choose a different positive maximum tick + snapshot budget in milliseconds:
node tools/verify_nav_scaling.mjs target/nav-scaling.json target/m15-site/coin_game 100
```

The verifier requires Chromium, WebKit iPhone 13 and WebKit iPad Pro 11; missing
browsers fail, never skip. It checks full routes, planned targets, statuses and
positions against native at five checkpoints per workload. IDs, route cursors
and ticks are exact; floating-point values allow 1e-5. Empty, malformed and
runtime-error inputs, budget failure/cleanup and deterministic arrival have
regressions in `tests/nav_scaling.rs` and `tools/test_web_nav_scaling.mjs`.

Browser verification defaults to a **100 ms maximum tick + JSON snapshot smoke
budget**, including the cold all-agent path-planning spike, and exits nonzero on
excess. Inputs are bounded to 16 cases, 128 agents, 3600 ticks and 64 samples per
case. This does **not** establish a 16.67 ms frame budget or crowd avoidance.
Actors intentionally overlap and have no colliders; no rendering is measured.

Local macOS ARM64 measurements for 128 agents (600 ticks; release WASM, dev native
without debug symbols/incremental compilation):

| Execution | Flat max / p95 ms | Ramp max / p95 ms |
|---|---:|---:|
| Native simulation step only | 204.407 / 0.126 | 174.404 / 0.141 |
| Chromium desktop tick + snapshot | 16.0 / 0.9 | 14.6 / 2.1 |
| WebKit iPhone 13 profile tick + snapshot | 17 / 1 | 16 / 2 |
| WebKit iPad Pro 11 profile tick + snapshot | 18 / 1 | 14 / 2 |

Native excludes snapshot serialization and uses a different optimization profile;
the rows are not a native/browser speed comparison. WebKit's clock rounds to
milliseconds; a reported zero is below resolution, not free work. Device profiles
are desktop emulation, **not physical-device performance evidence**.

**M12 is still partial:** triangle-exact collider/heightfield baking, dynamic
obstacles and avoidance require the #37-owned runtime/physics files; tile IO
overlaps #32/#35 streaming ownership. This QA harness neither implements nor
claims those features. Existing three-game release checks remain unchanged.

Lint checks `goal/collectible/crystal/coin/lantern` approach locations against the
player (or explicit start). It projects floating objective positions downward
onto nearby walkable surfaces; it does not prove interaction/jump/script logic.
`validate_scene` also includes these warnings. QA overlays draw green polygons
and magenta planned routes over screenshots (x-ray, first 8,192 polygons).

## Lantern example and checks

`cargo run --example bake_navigation` adds/replaces the noncolliding Laternenwart,
bakes through MCP and saves the scene. `scripts/patrol.rhai` cycles four lantern
locations without affecting player physics or gate/win logic. `lantern_patrol`
is a 720-tick native/WASM route trace, in addition to unchanged victory traces.

```sh
cargo test --test navigation -- --nocapture
cargo test --workspace -- --nocapture
node --test tools/test_web_*.mjs
cargo run --example web_parity -- target/web-native.json
```

Contract additions are optional `Scene.navmesh`, `Entity.nav_agent` and typed
`shared::navigation` data; older scenes continue to load unchanged. Blender sync
preserves an engine-authored navigation agent rather than overwriting it.
