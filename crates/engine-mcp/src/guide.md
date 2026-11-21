# AI engine guide / Leitfaden für KI-Agenten

Local stdio/loopback clients are trusted. HTTP defaults to 127.0.0.1:7777;
non-loopback requires --allow-remote with a bearer token on every request.
AI_ENGINE_HTTP_TOKEN sets one "remote" principal; AI_ENGINE_HTTP_TOKENS is a JSON
agent-to-token map for independent workers. Without either, remote startup prints
a generated token once. Use a trusted network/TLS tunnel: HTTP is not encrypted.
Remote blender_exec/run_blender_script require separate --allow-privileged-tools
(unsandboxed Python). MCP/CLI scene/script/mesh/export paths remain project-root
confined in every mode: no traversal or escaping symlinks. Privileged raw Python
is not sandboxed. Nur vertrauenswürdige Projekte; keine Tokens in Projektdateien.
Session IDs and tool annotations are not authentication or permissions.

## Workflow / Arbeitsweise

1. Read `engine://scene` (authoring snapshot) and `tools/list` (argument schemas).
   Neues Projekt: `ai-engine init mein-spiel --template sky_garden`.
2. Blender-first: `blender_start`, `blender_exec`, `blender_scene_info`; model real
   shapes/materials, assign stable `ai_uid` and gameplay props, then `blender_sync`.
   Blender Z-up becomes engine Y-up; engine rotations are degrees. Keep the Blender
   source/recipe. / Modelle und Komponenten zuerst in Blender pflegen.
3. `write_script` compiles project-relative `.rhai` files and attaches them to IDs.
   `play` snapshots authoring; `step` advances fixed 60 Hz; `stop` discards play.
   Authoring edits are blocked while playing. / Spielzustand ist getrennt.
4. `simulate` runs a fresh isolated snapshot; assert win, collection and fall/respawn.
   Inputs are absolute zero-based ticks, applied before hooks and held until changed.
   Use explicit release events. Same build/platform + inputs = deterministic results.
5. `validate_scene`, then `get_screenshot` with `views` and `overlay_ids`; actually
   inspect the image. / Bilder ansehen, nicht nur erzeugen.
6. `save_scene {path:"scene.json",bundle_assets:true}`; `export_web` into a new/empty
   folder. Serve over HTTP, not file://. Web export needs the engine source checkout,
   Rust wasm32 target and wasm-bindgen CLI. Browser playback is self-contained.

## Swarm etiquette / Zusammenarbeit

Local: distinct `agent` argument or `X-Agent-Id` (argument wins). Remote: credential
fixes identity and overrides both, including batches; sessions are principal-bound.
`task_create` with `depends_on`, `task_claim`, then `claim_entities`
before editing existing IDs. Renew expiring claims; `release_entities` when finished.
Use `get_revision` + `expected_revision` for CAS; after conflicts refresh scene and
`get_changes_since`. Journal retention is 1024 scene mutations; snapshots retain 64.
Task claims have TTL 1–86400 seconds (default 60) and return a fencing `lease`.
Pass it to `task_renew {id,lease,ttl_s?}`, `task_release {id,lease}` and
`task_complete {id,lease,note?}`. Renew before expiry, choose TTLs covering long
blocking calls. `task_list` shows expired claims; `task_claim` takes them over with
a new lease. Old owners/leases cannot complete late, even on same-agent reclaim.
Legacy lease omission is allowed only for first-generation task_complete.
`get_task_changes_since {sequence:0}` is the separate 1024-event task journal:
create/claim/renew/release/takeover/complete, lease, actor, previous owner. Task
events do not advance scene revisions. Leases and journals reset on restart.
`task_complete` only after verification. `apply_batch` defaults to `mode:"partial"`:
earlier successful commands remain if a later one fails. Use `mode:"atomic"` plus
batch `expected_revision` for one scene revision/journal commit, or no changes on
rejection. Atomic commands are only `spawn_primitive`, `set_transform`, `set_color`,
`remove_entity`, `set_camera`; no child CAS, nested batches, scripts, Blender,
save/export or other side effects. Empty atomic batches do not create revisions.
Every supplied revision must be a nonnegative integer (not null/string/float).

Skripte / scripts: `write_script` binds immutable `{path,source}` versions only to
explicit IDs. Rollback restores executable behavior even if files later change.
Claim every entity referencing a shared path before editing; all are journaled.
Blender owns geometry/transform, not unconditional gameplay replacement. Missing
`ai_*` fields mean preserve; JSON `"null"` or `ai_deleted_props` (JSON string array
of property names) explicitly removes a field. Three-way conflicts reject sync.
Use `blender_push_props {ids,expected_revision?}` to publish engine gameplay fields
by `ai_uid`; concurrent Blender property changes reject the push. Sync afterwards
to acknowledge the baseline. Push never edits geometry/transform.

## Components / Komponenten und Blender-Props

Entity: `id`, `name`, mesh `kind`, `transform` (`position`, `rotation_deg`, `scale`),
`color` RGBA, `visible`, `tags`, optional `collider`, `rigidbody`, `script`, `props`.
`ai_tags` is a JSON array (or comma-separated tags); `ai_collider`, `ai_rigidbody`
are JSON objects; `ai_script` is a project-relative path or script object.

- Collider: `{"shape":"box","half_extents":[1,0.2,1]}`, `sphere` (`radius`),
  `capsule` (`radius`,`half_height`), `mesh` / `mesh-from-entity-bounds` (AABB).
  Omitted dimensions derive from mesh bounds, scaled with the entity.
- Rigidbody: `{"type":"dynamic","mass":1,"friction":0,"restitution":0}`;
  type `static|dynamic|kinematic`. Physics gravity is -Y; vectors have three numbers.
- Script: `"scripts/player.rhai"`, `{"path":"scripts/player.rhai"}` or
  `{"source":"fn on_tick(ctx,dt) { ... }"}`. Paths resolve under project/assets.
- Props: `ai_lock_rotation:true`; `ai_friction_combine:average|min|max|multiply`
  (default average; zero friction + min prevents character wall sticking).
  `floating`/`trigger` tags or `ai_floating`/`ai_trigger:true` suppress physical QA
  warnings; scripted non-collider decorations are also exempt.
- Player tag enables browser follow camera, camera-relative controls and blob shadow.
  `props.ai_ui` (object or Blender JSON string): title, score_label, hud_keys,
  collectible_tag, action_label, sky_color, intro_title, intro_text, mission,
  win_text, win_badge, badge. Private `_` state keys stay out of HUD.

## Rhai API reference / Skript-API

Hooks: `fn on_start(ctx)`, `fn on_tick(ctx, dt)`; dt=1/60 seconds.
Sandbox: 50k operations/hook, bounded recursion/collections; no filesystem, network,
eval or clock APIs. Errors include script file/entity and line.

- `ctx.id()`, `ctx.position()`, `ctx.set_position([x,y,z])`;
  `ctx.rotation()` / `ctx.set_rotation([x,y,z])` use degrees.
- `ctx.velocity()`, `ctx.set_velocity([vx,vy,vz])`, `ctx.apply_impulse([x,y,z])`,
  `ctx.grounded()`; velocity/impulse require a rigidbody or collider.
- `ctx.find_by_name("Milo")` (ID), `ctx.find_by_tag("crystal")` (IDs),
  `ctx.entity_position(id)`; missing entity access errors.
- `ctx.input("move_x")`, `ctx.input("move_z")`, `ctx.input("action")` or `jump`;
  unspecified inputs start at zero. Native inputs are world axes; browser controls
  convert camera-relative joystick/keyboard axes before sending them to simulation.
- `ctx.state_get("score")`, `ctx.state_set("score", 3)` share global JSON-compatible
  state; missing keys return unit. `ctx.log("message")` writes bounded logs.
- `ctx.spawn_entity(#{name:"ball",kind:"sphere",radius:0.5,segments:16})` returns ID;
  only primitives, no GLB spawning. `ctx.remove(id)` removes an entity.

## Assertions / Prüfformen

`simulate {ticks:240,inputs:[{tick:0,action:"move_x",value:1}],asserts:[...]}`
accepts these JSON assert objects (all may add `at_tick:N`, default final tick):

```json
{"entity":"Milo","position_near":[0,0.75,0],"tolerance":0.2,"at_tick":0}
{"entity":"Milo","y_greater_than":0}
{"state":"score","equals":3}
{"entities_with_tag":"crystal","count":0}
```

Tick 0 means after on_start. Reports include actual values and passed flags;
`screenshot:true` adds a final PNG. Unknown names suggest close matches.
