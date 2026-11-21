# Browser graphics

Exports are self-contained WebGL 2 sites. Existing scenes keep the legacy look.
Enable the enhanced renderer with an `ai_graphics` object (or JSON string) in
`props` of the entity tagged `player`. An empty object enables the defaults.

For distant streamed GLBs without an authored low-detail mesh, set
`"impostor_views": 4` in the player's `ai_world`. The lowest-detail
billboard uses the nearest of four baked side views, so it no longer
shows the same face from every angle. It uses a single 128×128 atlas
(four 64×64 views). The default keeps a single 128×128 view; authored
`web_lods` meshes still take precedence.

```json
{
  "ai_graphics": {
    "sky": "#447cbb",
    "horizon": "#d3c8b1",
    "ground": "#626457",
    "sun_color": "#ffe5c1",
    "sun_direction": [-6, 16, 9],
    "sun_intensity": 3,
    "environment_intensity": 0.66,
    "exposure": 1,
    "bloom_threshold": 1.1,
    "bloom_strength": 0.22
  }
}
```

The sun direction points **from the scene to the sun**, in engine X/Y/Z
coordinates (Y up). The horizon color also sets the fog color. The sky generates
a prefiltered environment map for diffuse and specular lighting on PBR
materials, including existing GLBs. Zero environment intensity disables
the environment lighting. Zero bloom strength skips all bloom passes and
allocates no bloom targets; Ultra can still run edge smoothing.

For authored reflections, set `environment_map` in `ai_graphics` to a
project-relative Radiance RGBE `.hdr` file. Use a 2:1 equirectangular
panorama (64–4096 pixels wide, up to 32 MiB, standard `-Y +X` orientation).
Export validates and bundles it locally. It becomes both the background and
the source of reflected light; set `environment_background: false` to keep the
procedural sky visible while using the HDR map for lighting. Set
`environment_rotation_deg: 90` to turn the panorama and its reflections from
+X toward +Z (rounds to one horizontal HDRI pixel). Rotation affects only the
authored map, not the sun or fog. If the asset fails to load, the player uses
the procedural sky.

**Try it:** Export the material studio to compare chrome, copper, clearcoat and
neon under the same studio lighting. Its HDR map can be regenerated with
`python3 examples/material_studio/make_studio_map.py` and its glazed slate floor
with `python3 examples/material_studio/make_studio_floor.py`.

```sh
cargo run -- --export-web examples/material_studio/scene.json target/site/material_studio
python3 -m http.server 8000 --directory target/site
```

Open `http://localhost:8000/material_studio/?quality=ultra`.

QA captures: [desktop Chromium](web_material_studio_desktop.png),
[iPhone WebKit profile](web_material_studio_iphone.png),
[iPad WebKit profile](web_material_studio_ipad.png).

Preview a preset without editing the scene using
`?quality=low`, `?quality=balanced`, `?quality=ultra`, or `?quality=legacy`.
The first three enable the enhanced renderer and fix the quality tier.
Without a preset, the enhanced renderer starts at ultra on desktop or balanced
on touch, then steps down when performance is consistently low.
Ultra uses a 2048-pixel sun shadow map and half-resolution bloom; balanced uses
1024 and quarter-resolution bloom. Low uses 512 and renders directly.
Ultra also smooths high-contrast edges in the final HDR pass without an
additional full-resolution target. Set `hdr_antialias: false` in `ai_graphics`
to disable this filter; it is not used on lower tiers or in direct rendering.
All enhanced tiers align the moving sun shadow map to its texel grid, reducing
shadow shimmer as the camera follows the player.
Set `ai_graphics.shadow_extent` (8–160 m, default 24 m) for a wider or tighter
sun shadow volume. Wider coverage trades away detail at the same quality tier.
For decorative primitives and GLBs, set `props.ai_sun_shadow` to an object or
JSON string with `cast: false` or `receive: false` to skip the corresponding
sun shadow work. Both default to true; this does not change contact occlusion
or the player's ground marker.
If HDR targets aren't supported, the player uses direct rendering and
standard lighting without HDR bloom. ACES is the default tone mapper. Set
`tone_mapping: "agx"` inside `ai_graphics` to apply a softer highlight rolloff
to bright saturated materials and lights. Both curves respect `exposure` and
work on direct and HDR paths. The HDR composite dithers the final sRGB output
to reduce visible banding in gradients.

On Ultra, set `contact_occlusion` (0–1, default 0) for soft grounding where
geometry meets. `contact_radius` controls its reach in metres (0.1–3, default
1.2). It samples the rendered depth in one half-resolution pass and filters
across nearby depths without re-rendering geometry. It does not see off-screen
surfaces or transparent objects that don't write depth. Lower tiers and GPUs
without the necessary framebuffer support skip this effect.

For primitives, set `props.ai_material` (object or JSON string) to supply
`roughness`, `metalness`, `clearcoat`, and `clearcoat_roughness` (all 0–1).
`emissive` is an sRGB `#rrggbb` color; `emissive_intensity` is a linear
multiplier (0–32). Emissive materials above the bloom threshold can glow without
adding a light to the scene. The existing entity `color` is the linear base color.
Optional finishes use Three.js physical shading only when enabled:

| Finish | `ai_material` properties |
| --- | --- |
| Brushed metals | `anisotropy` (0–1), `anisotropy_rotation_deg` (UV-space angle) |
| Cloth highlights | `sheen` (0–1), `sheen_color` (sRGB hex), `sheen_roughness` (0–1) |
| Thin-film colors | `iridescence` (0–1), `iridescence_ior` (1–2.5), `iridescence_thickness_nm` (100–1200) |

Set the strength to zero or omit it to keep the corresponding shader work off.
Authored GLB textures and material parameters are not overridden, and streamed
primitive instances only share a draw when their resolved materials match.
For mipmapped GLB textures, balanced and ultra sharpen shallow viewing angles
with up to 4× and 8× anisotropic filtering respectively, within GPU limits.
The filtering is fixed at the starting tier so automatic quality changes don't
re-upload textures. Nearest-filtered, low and legacy assets keep their sampling.

To light nearby surfaces (rather than just glow), give an entity a point light
with `props.ai_light` (object or JSON string) in an enhanced scene:

```json
{"ai_light": {"color": "#f68dcb", "intensity": 12, "range": 5, "offset": [0, 0.3, 0]}}
```

Intensity is measured in candela (0–1000); zero disables the light. Range is
in metres (0.25–64). The optional offset is in the entity's local coordinates.
Lights follow moving entities and illuminate PBR surfaces, but do not cast
shadows. The web player renders the closest 2, 4 or 8 point lights for low,
balanced or ultra. Streamed worlds select around the player rather than the distant
follow camera, so nearby fixtures stay lit without increasing the GPU budget.

Set `type: "spot"` for a directional cone that follows the entity's local axes
under rotation and scale.
It defaults to local −Z; set `direction` to aim along a different local vector.
`outer_angle` sets the cone half-angle in degrees (1–89, default 35).
`inner_angle` is the half-angle before the soft edge begins (defaults to 65%
of the outer angle). The separate spotlight budget is 1, 2 or 4 by tier.

```json
{"ai_light": {"type": "spot", "direction": [0, -1, 0], "intensity": 30, "range": 7, "inner_angle": 12, "outer_angle": 25}}
```

These settings affect browser rendering only. They don't change native
screenshots or the deterministic simulation.

## Vegetation, water and daylight

```json
{"ai_graphics":{"time_of_day":17,"foliage":{"density":1,"distance":64},"water":{"level":-0.4,"color":"#126e61"},"fog_density":0.004}}
```

`time_of_day` (0–24, periodic) derives sun direction/color/intensity and sky/horizon
defaults. Explicit authored values always win. Without this key the original
graphics defaults are unchanged. `ai_render` is a deprecated object/JSON alias:
it is merged over `ai_graphics`. `quality=high` and `quality=medium` both mean
balanced; ultra/low/legacy retain their existing meanings.

`foliage` accepts true or `{density,distance}` (density 0–4, distance 16–128 m,
with preset distance caps of 24/48/64 m).
Grass tufts and broadleaf trees are deterministic GPU instances on independent
16-m tiles, with wind and distance fade. Camera-near trees shrink away to avoid
blocking gameplay. Placement skips water and steep/non-finite sampled ground.
Default heights raycast static support meshes or fall back to zero; terrain may
replace this with `window.__aiEngineVisuals.setHeightSampler((x,z)=>y)`.
`water` accepts false or `{level,color}` and uses animated procedural normals,
the existing environment reflection and a camera-following plane. Neither feature
adds colliders or game rules. Both are off unless authored.

Foliage density/distance and water normal detail follow the existing low/balanced/
ultra level. The same governor steps down after six slow seconds and can recover
after twelve seconds above 58 FPS, never above its initial tier. Fixed URL presets
never adapt. `fog_density` opts into exponential fog (0 disables it); choose a low
density for saturated vegetation instead of the legacy near-distance fog.
`?stats=1` includes the nature quality tier. No additional post stack or addons
are introduced. Screenshot: `docs/web_visuals.png`.
