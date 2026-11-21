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
`python3 examples/material_studio/make_studio_map.py`.

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
If HDR targets aren't supported, the player uses direct rendering and
standard lighting without HDR bloom. ACES tone mapping outputs to sRGB.

For primitives, set `props.ai_material` (object or JSON string) to supply
`roughness`, `metalness`, `clearcoat`, and `clearcoat_roughness` (all 0–1).
`emissive` is an sRGB `#rrggbb` color; `emissive_intensity` is a linear
multiplier (0–32). Emissive materials above the bloom threshold can glow without
adding a light to the scene. The existing entity `color` is the linear base color.
Authored GLB textures and material parameters are not overridden, and streamed
primitive instances only share a draw when their resolved materials match.

These settings affect browser rendering only. They don't change native
screenshots or the deterministic simulation.
