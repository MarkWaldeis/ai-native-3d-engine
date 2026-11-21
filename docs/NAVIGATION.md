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
