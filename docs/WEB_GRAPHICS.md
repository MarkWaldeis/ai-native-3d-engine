# Browser graphics

Exports are self-contained WebGL 2 sites. Existing scenes keep the legacy look.
Enable the enhanced renderer with an `ai_graphics` object (or JSON string) in
`props` of the entity tagged `player`. An empty object enables the defaults.

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
the environment lighting; zero bloom strength disables the post-process.

For authored reflections, set `environment_map` in `ai_graphics` to a
project-relative Radiance RGBE `.hdr` file. Use a 2:1 equirectangular
panorama (64–4096 pixels wide, up to 32 MiB, standard `-Y +X` orientation).
Export validates and bundles it locally. It becomes both the background and
the source of reflected light; set `environment_background: false` to keep the
procedural sky visible while using the HDR map for lighting. The map doesn't
change the authored sun or fog; align the sun direction with the image when
needed. If the asset fails to load, the player uses the procedural sky.

**Try it:** Export the material studio to compare chrome, copper, clearcoat and
neon under the same studio lighting. Its HDR map can be regenerated with
`python3 examples/material_studio/make_studio_map.py` and its glazed slate floor
with `python3 examples/material_studio/make_studio_floor.py`.

```sh
cargo run -- --export-web examples/material_studio/scene.json target/site/material_studio
python3 -m http.server 8000 --directory target/site
```

Open `http://localhost:8000/material_studio/?quality=ultra`.

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
If HDR targets aren't supported, the player uses direct rendering and
standard lighting without HDR bloom. ACES tone mapping outputs to sRGB.

For primitives, set `props.ai_material` (object or JSON string) to supply
`roughness`, `metalness`, `clearcoat`, and `clearcoat_roughness` (all 0–1).
`emissive` is an sRGB `#rrggbb` color; `emissive_intensity` is a linear
multiplier (0–32). Emissive materials above the bloom threshold can glow without
adding a light to the scene. The existing entity `color` is the linear base color.
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
shadows. The web player renders the closest 2, 4 or 8 lights for low, balanced
or ultra, so the GPU light count remains bounded in large scenes.

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
