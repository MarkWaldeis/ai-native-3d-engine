# Shooter runtime (first person, combat, FX, HUD)

Game rules stay in Rust/Rhai. The web player only sends inputs and renders
`fx`/`events`/`state` from each WASM tick. Example: `examples/fps_arena/`.

## Enabling first person

Player entity (tag `player`) props:

| Prop | Meaning |
|---|---|
| `ai_fps` | `{eye_height, move_speed, sprint_speed, jump_speed, mouse_sens}` (object or JSON string). Presence switches the web player to first person. Defaults `0.75, 5, 8.5, 5.5, 0.0022`. Initial yaw = player `rotation_deg[1]`. |
| `ai_viewmodel` | GLB path rendered as the first-person weapon (auto-scaled to ~0.6 m, muzzle at its −Z end). Without it a procedural rifle is used. |
| `ai_ui` | Existing UI keys, plus optional `minimap_range` (m, default 60), `fog_near`/`fog_far` (small scenes only), `death_text`. |

Without `ai_fps` the player is unchanged (coin_game, lantern_maze, large world).

## Controls and inputs

Desktop: click to lock the pointer, mouse look, WASD/arrows, Shift sprint, Space
jump, left mouse fire, R reload, Esc pauses. Touch: left joystick moves, drag on
the right half looks, FEUER fires (dragging from it also looks), R reloads, the
action button jumps.

Each tick sends exactly:

| Input | Value |
|---|---|
| `move_x`, `move_z` | local stick, x = strafe right, z = −1 forward (length ≤ 1) |
| `look_yaw`, `look_pitch` | absolute radians; yaw 0/pitch 0 looks along −Z, pitch clamped to ±1.45 |
| `fire`, `reload`, `sprint`, `jump` | 0/1 |

## Rhai API

| Call | Result |
|---|---|
| `ctx.raycast(origin, dir, max_dist)` | `#{hit, id, point, normal, distance}`; excludes the caller and sensors, ignores paused bodies. Terrain/unowned colliders give `id` 0. At most 4096 rays per tick. |
| `ctx.damage(id, amount)` | applies damage, returns remaining health (`-1` if the target is missing, dead or not damageable). Damageable: `ai_health` prop or tag `enemy`/`player`/`damageable`. Emits `damage`/`kill` events; player health mirrors to state `health`. |
| `ctx.health()`, `ctx.health_of(id)`, `ctx.set_health(hp)` / `ctx.set_health(id, hp)` | health (default `ai_health` or 100) |
| `ctx.hits()` | hits received since the last tick: `[#{source, amount, health, killed}]` |
| `ctx.player_pos()` | player position (`[0,0,0]` without player) |
| `ctx.emit_fx(kind, pos, dir)` / `ctx.emit_fx(kind, pos)` | kinds `muzzle`, `tracer`, `impact`, `blood`, `explosion`. `tracer`: `dir` is start→end (a unit vector draws 30 m). Max 512 per tick. |
| `ctx.emit_event(map)` | custom event to the player (e.g. `#{type:"kill", text:"…"}` shows in the kill feed) |
| `ctx.aim_dir()` / `ctx.move_dir()` | view direction from `look_yaw/look_pitch`; world move direction from `move_x/move_z` + yaw |
| `ctx.line_of_sight(from, to[, target_id])`, `ctx.can_see(id, max_dist)` | unobstructed check via raycast (a hit on `target_id` counts as visible) |
| `ctx.distance_to(pos)`, `ctx.steer(pos, speed)` | distance; seek velocity `[vx, vy, vz]` (keeps current vy) that probes ±0.6/1.2/1.9 rad to sidestep obstacles |
| `ctx.prop(name)` | entity prop (JSON strings parsed) |
| `ctx.memo(key)` / `ctx.set_memo(key, value)` | per-entity scratch values (32 keys) for AI cooldowns etc. |

Distant scripted enemies are cheap through the existing active simulation
region (`ai_world.sim_radius`): their scripts and bodies pause outside it.

## HUD state keys

`health`, `ammo`, `ammo_reserve`, `kills`, `objective`; `_reload` (seconds left)
drives the reload animation. Events drive the hit marker, damage vignette and
kill feed; enemies (tag `enemy`) and objectives appear on the minimap.

## FX pooling

`fx.js` uses fixed buffers (2048 sparks / 256 smoke puffs, 768/96 on touch
devices, 64 tracers, 4 flash lights) and recycles the oldest slot, so firing
never allocates GPU resources.

## Tests

`cargo test -p engine-tests --test shooter`, `node --test tools/test_web_fps.mjs`.
