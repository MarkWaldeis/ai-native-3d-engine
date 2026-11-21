# Rapier physics

The engine runs the same Rapier simulation natively and in WASM at 60 Hz.
Physics belongs to the live simulation: stepping or damaging an entity does
not edit the authoring scene. With the same build, platform, scene and inputs,
simulation order and results are deterministic.

## Authoring a body

Use `rigidbody.type` of `static` (fixed), `kinematic` (velocity-controlled), or
`dynamic`. Existing `mass`, `friction`, `restitution`, `collider`, and
`props.ai_lock_rotation` work as before. Add optional settings via
`props.ai_physics` (an object in JSON, or a JSON string custom property in Blender):

```json
{
  "linear_damping": 0.1,
  "angular_damping": 0.2,
  "ccd": true,
  "groups": {"memberships": 4, "filter": 3},
  "collider": {
    "shape": "compound",
    "parts": [
      {"shape": "box", "half_extents": [0.5, 0.3, 0.4]},
      {"shape": "sphere", "radius": 0.3, "position": [0, 0.5, 0]}
    ]
  }
}
```

- `collider` here overrides the legacy collider. Geometry is in entity-local
  units. Choose `box {half_extents}`, `sphere {radius}`,
  `capsule {radius, half_height}` (Y-up), `convex_hull {vertices}`,
  `trimesh {vertices, triangles}` (zero-based triples, static/kinematic only),
  or `compound {parts}` (1–64 convex parts with optional local `position` and
  `rotation_deg`). Mesh vertices are explicit, so no collider asset fetch is
  needed on the web. Legacy `collider.shape: "mesh"` is still an AABB, not a trimesh.
- `groups.memberships` and `groups.filter` are unsigned 32-bit masks, each
  defaulting to all bits. Two colliders interact only if **each** collider's
  memberships intersect the other's filter. `sensor: true` detects overlaps
  without creating contact forces.
- `joints` is a list of up to 16 constraints from this body to `target`
  (entity ID). Types: `fixed`, `spherical`, `revolute` (requires local
  `axis`, optional `limits_deg: [min,max]`). `anchor` and `target_anchor`
  are physics-local offsets from the two body origins, in world-sized units;
  they default to zero. `contacts_enabled` defaults to false between linked
  bodies. An endpoint without a body leaves the joint unlinked; despawning a
  body removes its links.

Rhai: `ctx.apply_force([x,y,z])` acts for one tick, `ctx.apply_impulse(...)`
changes momentum once, and `ctx.set_velocity(...)` sets linear velocity.
`ctx.collision_events()` reports the caller's previous physics step.
The WASM tick's `events` array contains physics transitions alongside script
events. Physics records `{"type":"sensor"|"contact","phase":"started"|"stopped","a":id,"b":id}`
at tick boundaries; multiple collider pairs between the same owners are
coalesced. Output is capped at 256 events per tick.

## Queries and hitboxes

MCP tools `raycast`, `shape_cast`, and `overlap` use live physics during play
or authored colliders without running scripts otherwise. Rhai accepts the same
maps (and excludes the caller by default). Cast directions are normalized and
`distance` is in world units, up to 10,000. For example:

```json
{
  "origin": [0, 2, -4],
  "direction": [0, 0, 1],
  "distance": 8,
  "shape": {"shape": "sphere", "radius": 0.25},
  "filter": {"exclude": 12, "include_sensors": false}
}
```

Use the full example for `shape_cast`, omit `shape` for `raycast`, or omit
`direction` and `distance` for `overlap`. Shape queries require convex geometry
and accept an optional `rotation_deg`. `filter.groups` takes the same masks
as colliders. Sensors are skipped by default. Casts return `hit`, and on a hit
`id`, `point`, `normal`, `distance`, `hitbox` (nullable), and
`damage_multiplier`. `overlap` returns up to 256 sorted unique `ids` and a
`truncated` flag. Rhai's `ctx.raycast(origin, direction, distance)` remains valid.

Add up to 32 `hitboxes` under `ai_physics`. Each region has a unique `name`,
a convex `shape` object, optional local `position` and `rotation_deg`, and
optional `damage_multiplier` (default 1). Named regions replace that entity's
normal collider **for rays only**; they do not add mass, block movement, or
trigger collision events.

```rhai
let hit = ctx.raycast([0.0, 2.0, -4.0], [0.0, 0.0, 1.0], 8.0);
if hit.hit { ctx.damage(hit, 60.0); }
```

`ctx.damage(hit, amount)` resolves the multiplier from the target's actual
hitbox configuration, not from the mutable result map. Legacy
`ctx.damage(id, amount)` remains unscaled. For targets with a ragdoll, pass an
optional third argument, a world-space impulse, to push the struck part; if
omitted, an impulse is derived from the hit's normal and damage.

## Humanoid ragdoll

Set `ai_physics.ragdoll: {"preset":"humanoid"}` on a root entity at pelvis
height. A feet-origin model can set `offset` to the local position of the
pelvis (default `[0,0,0]`). Options: `scale` (uniform preset scale), `start_active` (default
false), and `activate_on_death` (default true). The preset uses 11 Rapier
capsule bodies with limited joints. Activation swaps out the root collider
and transfers linear velocity to the limbs. The pelvis drives the root
transform and ray hits use the root entity ID and part name. Deactivation
removes the limbs and restores the original root collider relative to the
pelvis, accounting for the offset, with its pre-ragdoll orientation and zero velocity.
Loading a save rebuilds ragdolls that activate on death in a lying pose at
their saved pelvis position, rather than restoring individual limb poses.
Deactivating one of these rebuilt ragdolls restores the authored root transform
if it exists.

`ctx.apply_impulse(vector)` pushes the script's own body;
`ctx.apply_impulse(id, vector)` pushes another body.

In Rhai, `ctx.set_ragdoll(true)` or `ctx.set_ragdoll(id, true)` queues
activation; pass `false` to deactivate. `ctx.ragdoll_active(id)` reads the
applied state. Damage can activate the preset on death before applying its
impulse. The web tick includes part poses and the player shows capsule
proxies, not a retargeted GLTF skeleton.

## Try it

```sh
cargo run -- --export-web examples/physics_lab/scene.json target/site/physics_lab
python3 -m http.server 8000 --directory target/site
```

Open `http://localhost:8000/physics_lab/`, start the demo, then press Space or
the action button to hit the target and toggle ragdoll reset. The scene also
contains a hinged pendulum, a compound barrier, and a ball dropping through a
sensor gate. For headless verification, run
`cargo test -p engine-tests --test physics --test gameplay`.
