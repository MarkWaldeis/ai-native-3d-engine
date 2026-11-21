# Browser graphics

## Hintergrund-Assets

Nur mit Welt-Streaming lädt die pfadbasierte GLB-Warteschlange nach nächster
Chunk-Distanz, pro Frame neu sortiert (`asset_concurrency:4`). Chunks bauen ohne
Netzwerkwarten: Bounds-Boxen dienen als Platzhalter, fehlende Bounds bleiben leer.
Autoren-LODs laden zuerst in der gröbsten Stufe, danach in der gewünschten Stufe.
Ohne Autoren-Fern-LOD werden die bisherigen Billboards erst nach Texturfreigabe
erstellt und mit dem Quellasset wieder verworfen.
`texture_uploads_per_frame:2` begrenzt explizite GPU-Texturinitialisierungen;
erst danach wird das Modell sichtbar. Ein einzelner Upload/GLB-Decode kann dennoch
teuer sein; dies ist kein Worker-Decoder oder Byte-Budget.

`asset_cache_mb:256` ist ein geschätztes LRU-Ziel aus Geometriepuffern und
Bildgrößen. Nicht mehr geladene Chunks geben ihre Pins frei; Überbudget-Assets
werden inklusive Geometrien, Materialien und Texturen entsorgt und bei Bedarf
erneut geladen. Aktive Assets dürfen das Ziel überschreiten. Fehler behalten den
Platzhalter bis zum erneuten Anfordern nach Freigabe. Kleine Demos ohne Streaming
bleiben unverändert. Stats, Overlay und Benchmark melden `assetsQueued`,
`assetsLoading`, `assetsLoaded`, `assetMB`, `textureUploadsPending`, `assetEvictions`.

## Hierarchische Fern-LOD

`ai_world.hlod` ist bei Welt-Streaming standardmäßig aktiv. `hlod_cluster:4`
fasst 4×4 Chunks zusammen; `hlod_distance:1600` erweitert Nebel und Kamerareichweite.
`hlod_min_radius:1.5` filtert kleine Details. Statische Primitive werden stark
vereinfacht, GLBs verwenden `web_mesh_bounds`; dynamische/animierte Objekte,
Skripte und unsichtbare Entities sind keine Proxys. Welttransforms und Farben
werden in eine gemeinsame Vertexfarben-Geometrie gebacken, ohne Schattenwurf.

Nur vollständig außerhalb `draw_distance` liegende Chunks wechseln nach einer
Hysterese zur Fern-LOD. Zusammenhängende Indexbereiche ergeben meist einen Draw
Call pro Cluster. Ein Aufbau pro Frame begrenzt Lastspitzen; entfernte Geometrie
wird freigegeben, Änderungen invalidieren den Cluster. Beim Rückwechsel bleibt
der Proxy bis zur fertigen Nah-Geometrie stehen. Beim ersten Laden können ferne
Cluster daher zeitversetzt erscheinen. `hlod:false` lässt den alten Pfad unverändert.
`?stats=1` und Benchmarks zeigen `hlodClusters`, `hlodDrawCalls`, `hlodTriangles`.
Terrain-Occlusion kann unabhängig über `StreamingWorld.setHlodOcclusion(occluder)`
mit `occluded(key,eye,box)` angebunden werden.

Terrain-Verdeckung: `ai_world.occlusion` aktiviert standardmäßig gemeinsame
Höhenfeldproben für Chunk-Gruppen und Terrain-Kacheln (auch ohne Chunk-Streaming).
`occlusion_margin` (2 m) toleriert Höhenfehler; nahe Boxen und Kameras unter dem
Terrain bleiben sichtbar. Neun obere Boxpunkte müssen verdeckt sein, bevor nach
zwei Tests ausgeblendet wird. Das Round-Robin-Budget beträgt 96 Tests/Frame.
Dies ist eine abgetastete Näherung, kein GPU-Depth-Test; bei stark überhängenden
Objekten oder ungenauen Höhenfeldern lässt sie sich mit `occlusion:false` abschalten.
`?stats=1` und Browser-Benchmarks zeigen verdeckte Chunks/Kacheln und Tests/Frame.

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
Set `bloom_scatter` (0–1, default 0) to spread some of the Ultra bloom into
a softer, wider halo. This uses one extra pass at one-eighth resolution per
axis; it has no cost when bloom is off or on lower presets.

For scenes with both bright and dark areas, set `"auto_exposure": true` in
`ai_graphics`. Balanced and Ultra adjust exposure from the rendered HDR image
before tone mapping, like a camera going indoors. Fixed `exposure` remains an
artistic multiplier and is the only setting used on Low or without HDR support.
The first frame meters immediately; transitions then adapt gradually.

An object can override `min_ev` and `max_ev` (defaults −2 and +2 stops, bounded
to −4–0 and 0–4). `brighten_speed` and `darken_speed` control adaptation
toward darker and brighter views, respectively (defaults 1.25/s and 3/s,
bounded to 0.1–8/s). The meter adds two 1×1 targets and no CPU readback.

For outdoor scenes, add `sky_atmosphere: {"strength":0.8,"haze":0.45}` to
`ai_graphics`. The procedural sky adds blue Rayleigh scattering and a warm
forward halo around a low sun; more haze strengthens the halo. Both values
range from 0 to 1. Use `true` for strength 1 and haze 0.35. The sky and its
prefiltered reflections are baked once at load, with no per-frame pass.
The authored horizon fog and direct lighting are unchanged. An authored
HDRI still takes precedence for reflections and, if enabled, the background.

Set `ai_graphics.height_fog: true` for ground mist that thins with altitude.
An object can set `density` (0–0.2 m⁻¹, default 0.018), `base_height`
(world Y in metres, default 0), `falloff` (1–1000 m, default 18), and
`color` (defaults to the horizon color). Density is constant below the base
and decays exponentially above it.
Balanced and Ultra use height fog instead of distance fog, in the existing HDR
resolve pass. Sky pixels are unchanged; transparent surfaces that do not write
depth inherit the fog at the background depth. Auto exposure meters the fogged
scene when both options are on. Low and unsupported GPUs keep the existing
distance fog. Use `density: 0` to turn off both kinds of fog on supported
Balanced and Ultra renders without allocating a depth texture for fog.

Use `ai_graphics.color_grade: {"saturation":0.8,"contrast":1.25}` for a muted,
higher-contrast look. Both controls range from 0 to 2; 1 is neutral and
saturation 0 is monochrome. Grading runs after tone mapping in Balanced and
Ultra, without an extra pass when HDR is already active. It does not change
HDR bloom, exposure metering, the separate first-person 3D overlay, the HTML
HUD, or the Low and legacy paths.

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

Set `"sky_clouds": true` for static procedural clouds in the generated sky
and its reflections. An object can set `coverage` (0–1, default 0.45),
`scale` (0.25–4, default 1; higher values make smaller clouds), `opacity`
(0–1, default 0.85), and integer `seed` (0–65535). The sky is generated once
from the sun direction and color, with no per-frame cloud pass. Clouds don't
cast shadows on geometry. A visible HDRI still replaces the procedural sky;
a reflection-only HDRI leaves the clouds visible but uses the HDRI for lighting.

**Try it:** Export the material studio to compare chrome, copper, clearcoat and
neon under the same studio lighting. Its HDR map can be regenerated with
`python3 examples/material_studio/make_studio_map.py` and its glazed slate floor
with `python3 examples/material_studio/make_studio_floor.py`.

```sh
cargo run -- --export-web examples/material_studio/scene.json target/site/material_studio
python3 -m http.server 8000 --directory target/site
```

Open `http://localhost:8000/material_studio/?quality=ultra`.

For kilometer-scale scenes with nearly overlapping surfaces, opt into
`"logarithmic_depth": true` in `ai_graphics`. It improves depth precision
at long distances without moving the near plane. It works in the direct
and HDR renderer, including Ultra contact occlusion. It may cost fragment
performance on some GPUs, so leave it off unless the scene needs it.
`?quality=legacy` always uses standard depth.

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
Set `ai_graphics.render_scale` (0.5–1, default 1) to trade image sharpness
for fewer pixels without reducing shadow quality or changing the HUD.
For example, 0.75 renders each dimension at 75% of its normal size
after the quality tier's resolution limits (about 56% of the pixels).
It applies to both HDR and direct rendering; legacy mode ignores it.
Ultra also smooths high-contrast edges in the final HDR pass without an
additional full-resolution target. Set `hdr_antialias: false` in `ai_graphics`
to disable this filter; it is not used on lower tiers or in direct rendering.
All enhanced tiers align the moving sun shadow map to its texel grid, reducing
shadow shimmer as the camera follows the player.
Set `ai_graphics.shadow_extent` (8–160 m, default 24 m) for a wider or tighter
sun shadow volume. Wider coverage trades away detail at the same quality tier.
For decorative primitives and GLBs, set `props.ai_sun_shadow` to an object or
JSON string with `cast: false` or `receive: false` to skip the corresponding
shadow work from the sun or a spotlight. Both default to true; this does not
change contact occlusion or the player's ground marker.
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
| Refractive glass | `transmission` (0–1), `ior` (1–2.333), `thickness` (0–50 m), `attenuation_color` (sRGB hex), `attenuation_distance` (0.01–1000 m) |

Set the strength to zero or omit it to keep the corresponding shader work off.
Authored GLB textures and material parameters are not overridden, and streamed
primitive instances only share a draw when their resolved materials match.
For mipmapped GLB textures, balanced and ultra sharpen shallow viewing angles
with up to 4× and 8× anisotropic filtering respectively, within GPU limits.
The filtering is fixed at the starting tier so automatic quality changes don't
re-upload textures. Nearest-filtered, low and legacy assets keep their sampling.

For clear glass, use a white entity color, low roughness and metalness 0.
Glass defaults to IOR 1.5 and thickness 0.25 m; omit attenuation distance
for no absorption. Transmission samples the opaque scene, adds an offscreen
render pass when visible, and cannot refract other glass surfaces.
Streamed glass primitives use individual draws so entity scaling affects
optical thickness. To avoid an ordinary opaque shadow, set
`ai_sun_shadow.cast` to `false` on the glass.

### Textures on primitives

Use local material maps without converting a primitive into a GLB:

```json
{"ai_material":{"base_color_map":"assets/stone-color.png","normal_map":"assets/stone-normal.png","orm_map":"assets/stone-orm.png","uv_repeat":[4,2]}}
```

Base color is sRGB and multiplied by the entity's linear `color`. The other
maps are linear data: `normal_map` uses tangent-space +Y (OpenGL), with optional
`normal_scale` (0–2, default 1). Pack ambient occlusion, roughness, and metalness
into the red, green, and blue channels of `orm_map`. An ORM map sets the
default roughness and metalness multipliers to 1; explicit scalars still apply.
Maps use the primitive's UVs; `uv_repeat` gives the U/V tile counts (1/16–64,
default [1,1]). Base color alpha does not enable transparency.

The offline exporter validates and bundles project-relative, static 8-bit
PNGs (up to 4096×4096 and 16 MiB each). Unique maps can total up to 128 MiB
and 32 Mi pixels. Instances with different maps or repeats use distinct batches
but share image pixels. If a bundled image is unavailable at runtime, the
material uses its remaining maps and scalar values. GLB materials are unchanged.
The [material studio](web_material_maps.png) uses a tiled slate wall generated
by its map generator.

To light nearby surfaces (rather than just glow), give an entity a point light
with `props.ai_light` (object or JSON string) in an enhanced scene:

```json
{"ai_light": {"color": "#f68dcb", "intensity": 12, "range": 5, "offset": [0, 0.3, 0]}}
```

Intensity is measured in candela (0–1000); zero disables the light. Range is
in metres (0.25–64). The optional offset is in the entity's local coordinates.
Lights follow moving entities and illuminate PBR surfaces. Point lights do
not cast shadows. The web player renders the closest 2, 4 or 8 point lights for low,
balanced or ultra. Streamed worlds select around the player rather than the distant
follow camera, so nearby fixtures stay lit without increasing the GPU budget.

Set `type: "spot"` for a directional cone that follows the entity's local axes
under rotation and scale.
It defaults to local −Z; set `direction` to aim along a different local vector.
`outer_angle` sets the cone half-angle in degrees (1–89, default 35).
`inner_angle` is the half-angle before the soft edge begins (defaults to 65%
of the outer angle). The separate spotlight budget is 1, 2 or 4 by tier.
Set `cast_shadow: true` to allow a spotlight to cast shadows on Ultra.
Only the closest eligible spotlight already in the budget gets a 1024-pixel
shadow map. This keeps the extra scene render bounded even with many fixtures.

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

`foliage` accepts true or `{density,distance,trees}` (density 0–4, trees 0–1 with 0 disabling procedural trees, distance 16–128 m,
with preset distance caps of 24/36/112 m).
Short tapered grass, leafy clumps, occasional flowers and broadleaf trees are deterministic GPU instances on independent
16-m tiles, with wind and distance fade. Camera-near trees shrink away to avoid
blocking gameplay. Placement skips water and steep/non-finite sampled ground.
The player supplies authored road footprints to exclude ground cover; ultra uses
sparse, wider four-triangle tufts beyond 40 m. Balanced/low skip terrain detail
texture samples and use smaller water masks.
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

## Measure the render budget

Use `?stats=1` for CPU submission time and GPU elapsed time in milliseconds,
also available as `window.__engineStats.renderCpuMs` and `renderGpuMs`.
Both cover scene rendering, shadow maps, and post-processing, not simulation,
world updates, or UI. GPU time is the latest completed timer query, not
necessarily the same frame as the CPU reading. An unsupported extension,
invalid GPU timing, or a query that has not yet returned shows a dash.
The player does not request the timer extension without the overlay.

## Terrain surface detail

Set `props.ai_terrain_detail` to a strength from 0 to 1 on a terrain entity
(try `0.65`). The default is off. World-space triplanar bump shading adds
procedural detail to the layered terrain on slopes and flats without tile seams,
extra geometry, or requests. The detail fades from 25 to 90 m so distant
terrain keeps its existing shading. Terrain heights and colliders are unchanged.

### Valley haze and clouds

`haze: true` or `{density,height,falloff}` enables height-dependent aerial
perspective, with warmer scattering toward the sun. `height` is the low-valley
reference elevation in world metres; the default density/height/falloff are
0.002/18/0.045. Low uses a linear height approximation and omits sun scattering.
`clouds: true` or `{coverage:0.48}` enables slowly drifting layered procedural
clouds on the sky dome (four noise octaves on ultra, two on balanced, off on low).
Clouds are not baked into the environment; the existing PMREM stays unchanged.
Both features default off and add no postprocessing stack.

`shadow_extent` also accepts `{low,balanced,ultra}` widths in metres, and
`shadow_map_size` accepts per-tier power-of-two sizes from 256 to 4096 (clamped to
the GPU limit). Rio Verde authors widths 24/64/144 m and maps 512/1024/4096;
ultra therefore covers approximately 72 m either side of the target. The existing
sun-space texel grid stabilizes each tier. This is a single map, not cascades.
