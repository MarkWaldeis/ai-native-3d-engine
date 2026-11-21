# Large-world browser benchmark

The default benchmark includes streamed heightfield terrain. Smoke budgets are
not a claim that the current player handles large worlds interactively.

## Scene

`tools/bench/gen_large_world.py` uses only Python's standard library. Default seed
1 produces `large_world_bench`: a 6633 × 6633 m square (43.996689 km²), centered at
the origin, with 14,998 entities when terrain is enabled:

- 64 × 64 static foundation boxes (4096 colliders), 103.640625 m wide. With
  `BENCH_TERRAIN=0` these instead form the original seeded 5–59 m box hills.
- 4000 trees (trunk + crown: 8000 entities), 2400 rocks, 500 village buildings.
  Props have no colliders and sit on native terrain heights.
- One player-tagged sphere with an inline deterministic Rhai script, no gravity
  body, and benchmark HUD. It flies 8 m above `ctx.terrain_height` at ~80 m/s along a
  ~14.7 km diagonal out-and-back route. The standard follow camera is unchanged.
- One 44 km² terrain entity translated to the centered benchmark coordinates.
  `run.sh` generates its native f32 manifest with the `engine-terrain` `bench`
  example under `target/bench/terrain`, using the same seed. The 4096 original
  box entities/colliders remain as foundations 10 m below minimum terrain height;
  they are not removed or disabled. Prop counts, flight route and density remain
  unchanged. Rendering streams quantized u16 tiles with LOD/skirt/splat; see
  **Web-Terrain (Streaming)** in `AGENTS.md` for APIs and quantization limits.

`--seed`, `--density` (scales props only), `--terrain-manifest <native terrain.json>`
and `--out path/to/scene.json` are
generator options. Keep seed/density identical when comparing implementations.
The simulation's load and scripted-spawn limit is 32,768 entities.

## Run locally

Prerequisites: Python 3, Node 20+ (CI uses 22), stable Rust, WASM target and matching
bindings generator. From the repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.100 --locked
npm ci --prefix tools/bench
npx --prefix tools/bench playwright install chromium
bash tools/bench/run.sh --channel chromium --swiftshader --duration 20
```

Use `--channel chrome` for system Chrome instead. `run.sh` generates
`target/bench/large_world/scene.json`, regenerates **only** `target/bench/terrain`,
removes the old `target/bench/site/large_world` export, builds/exports, and runs both profiles.
Export directories must be new/empty and project-relative (not `/tmp/...`).
`--swiftshader` adds `--use-angle=swiftshader --enable-unsafe-swiftshader
--ignore-gpu-blocklist`; omit it for native GPU comparisons and record that change.

The script accepts `BENCH_SEED`, `BENCH_DENSITY`, `BENCH_DURATION`,
`BENCH_PROFILES`, `BENCH_CHANNEL`, `BENCH_OUT`, and `BENCH_SKIP_NPM_CI=1`.
Use `BENCH_TERRAIN=0 BENCH_OUT=target/bench/no-terrain bash tools/bench/run.sh ...`
for the original 14,997-entity box-ground baseline (same prop density and route).
This comparison changes ground representation, not just the terrain draw cost.
CLI harness options override the corresponding environment defaults. The default
output is `target/bench/results`; pipeline logs are `target/bench/pipeline.log`.

To measure an already exported site:

```sh
node tools/bench/run_browser_bench.mjs --site target/bench/site \
  --scene-path large_world --profiles desktop,mobile --channel chromium \
  --swiftshader --duration 20 --warmup 3 --mobile-cpu-throttle 4 --timeout 180 \
  --budgets tools/bench/budgets.json --out target/bench/results
```

The harness starts a localhost static server on a free port. Desktop is
1920×1080/DPR1; mobile is 390×844/DPR3 with iPhone UA, touch/mobile emulation and
4× CDP CPU throttling (configurable). Each profile gets a fresh context. This is
Chromium emulation, **not** Mobile Safari or a physical iPhone. Context isolation
does not isolate browser-process WASM compilation caches; profile order matters.

## Outputs and interpretation

- `results.json`: schema version 1, timestamp, Git SHA, browser/configuration,
  scene counts, metrics, baseline deltas and violations. Uncommitted changes are
  not represented by the SHA; archive the tested diff when measuring dirty work.
- `summary.md`: compact table suitable for CI job summaries.
  Both JSON (`profiles.<name>.terrain`) and Markdown include end-of-run decoded
  tiles, visible tiles, terrain triangles, cumulative fetched payload bytes and
  pending fetches. Absent terrain is `null`/`—`, not a fabricated zero.
- `desktop.png`, `mobile.png`: end-of-run evidence.
- `pipeline.log`: generator/export/harness output. CI uploads all `target/bench`
  evidence as `large-world-bench`, including on failure.

The player always records one-shot `performance.mark('ai-engine:…')` entries:
`bootStart`, `sceneFetched`, `wasmInitialized`, `webGameLoaded`, `assetsReady`,
`firstFrameAfterStart`. Only `?bench` exposes `window.__aiEngineBench` with
`ready`, `marks`, and `stats()`. No game rules live in instrumentation.

Load metrics subtract `bootStart`; raw marks are navigation-relative. Navigation
TTFB and DOMContentLoaded, Resource Timing transferred bytes for scene/WASM/total,
and click/tap-to-first-running-frame are separate measurements. The exported
pretty-printed scene is larger than the generator's compact JSON.

After warmup, rAF sampling runs for at least `--duration` seconds (a slow final
frame can overrun). FPS = frame interval count / elapsed time. Frame p50/p95/p99
use nearest-rank percentiles; long-frame percentages are >33.3 ms and >50 ms.
Wrapped `WebGame.tick` time includes simulation and snapshot JSON serialization,
but excludes caller JSON parsing and transform application. It is not a pure
Rapier measurement. Simulation catch-up can run multiple ticks per frame.

Memory includes CDP JSHeapUsedSize/JSHeapTotalSize at ready/end,
`performance.memory` when available, and exported WASM memory buffer size. JS
heap is not total process/GPU memory. Summary heap units are MiB. Renderer stats
are draw calls, triangles, uploaded geometries/textures, entity count and sim
ticks; uploaded geometry counts are not the full scene entity count.

Errors/readiness failures exit nonzero. `--budgets` checks ready time, ending CDP
JS heap and average FPS; page/console errors fail even without a budgets file.
Current CI floors are ready ≤120 s, heap ≤2048 MiB, desktop ≥0.5 FPS and mobile
≥0.1 FPS. These are **smoke floors**, not quality targets: tighten them as
streaming lands. The mobile floor accounts for the observed ~0.33 FPS baseline
under 4× CPU throttling and SwiftShader.

## Compare terrain/streaming work

1. With `BENCH_TERRAIN=0`, generate/export the box-ground scene and collect a baseline using the
   same browser, host, flags, profile order, duration, seed and density.
2. Preserve `results.json` outside the next output directory, e.g.
   `target/bench/main-baseline.json`.
3. Enable terrain (the default), regenerate/export at the same density and compare:

```sh
bash tools/bench/run.sh --channel chromium --swiftshader \
  --baseline target/bench/main-baseline.json --out target/bench/candidate
```

`--baseline` adds absolute deltas for ready milliseconds, FPS and ending heap MiB
to JSON and Markdown. Negative load/heap deltas and positive FPS deltas are
improvements. Keep both raw reports/screenshots; do not lower density, disable
physics, or change the flight path just to improve results. Check load, tick,
memory and frame-tail behavior together, not FPS alone. Fog currently hides much
of the world, so few draw calls do not imply a cheap full-scene simulation.

## Initial reference (step-2 run)

macOS ARM VM, **Chrome 154.0.8037.93**, SwiftShader; 20-second sample after
3-second warmup. These are reference observations, not portable hardware targets.

| Profile | CPU | Ready ms | Avg FPS | p50 / p95 / p99 frame ms | Tick avg ms | JS heap ready / end MiB |
|---|---:|---:|---:|---:|---:|---:|
| Desktop | 1× | 4490.50 | 1.49 | 666.70 / 699.90 / 700.00 | 99.76 | 89.75 / 111.64 |
| Mobile | 4× | 2246.60 | 0.33 | 2983.30 / 3066.50 / 3066.50 | 443.47 | 96.34 / 103.87 |

Both had zero page/console errors. WASM memory was 128,450,560 bytes per profile.

## Checks and CI

`.github/workflows/bench.yml` runs on PRs, pushes to main, and manual dispatch
(duration/profiles inputs). Ubuntu installs the WASM toolchain and bundled
Chromium/system libraries; the same pipeline runs with SwiftShader and budgets.
Terrain generation uses the existing Rust toolchain and Python standard library;
no downloaded heightmaps, Blender or extra CI packages are required.
Artifacts and the job summary are published even after benchmark failures.

```sh
cargo test -p engine-runtime --lib
cargo test -p engine-tests --test web_export
node --test tools/test_web_*.mjs
python3 -m unittest discover -s tools/bench -p 'test_*.py'
npm --prefix tools/bench test
```
