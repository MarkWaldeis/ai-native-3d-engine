# Large-world browser baseline

Requires Python 3, Node 20+, Rust with `wasm32-unknown-unknown`, and
`wasm-bindgen-cli` 0.2.100. Dependencies and lockfile are isolated here.

```sh
npm ci --prefix tools/bench
npx --prefix tools/bench playwright install chromium
bash tools/bench/run.sh --swiftshader --duration 20 --warmup 3
```

Use `--channel chrome` for installed Google Chrome instead of bundled Chromium.
`run.sh` regenerates the default seed/density scene including 44 km² terrain,
replacing `target/bench/terrain` and `target/bench/site/large_world` only.
All 10,900 props and 4096 static colliders remain. `BENCH_TERRAIN=0` selects the
old box-ground scene; use `BENCH_OUT` to preserve separate comparison results.
It forwards harness options. Terrain counters are in JSON and Markdown reports.

To measure an existing export without rebuilding:

```sh
node tools/bench/run_browser_bench.mjs --site target/bench/site \
  --scene-path large_world --profiles desktop,mobile --duration 20 --warmup 3 \
  --mobile-cpu-throttle 4 --timeout 180 --swiftshader \
  --budgets tools/bench/budgets.json --out target/bench/results
```

`--timeout` is in seconds for navigation/readiness and browser operations.
An additional context watchdog bounds unresponsive renderer operations.
Profiles use fresh contexts and disabled HTTP caching. Mobile is Chromium with
iPhone UA/touch/390×844/DPR3 and configurable CPU throttling, **not Mobile Safari**.
Software rendering is opt-in and records the three requested GPU flags.

Outputs: `results.json` (schema version 1), `summary.md`, `desktop.png`, and
`mobile.png`. `--baseline path/to/results.json` adds absolute deltas for ready
milliseconds, FPS, and ending JS heap MiB; keep browser, flags, machine and
profile settings comparable. `--budgets path/to/budgets.json` applies per-profile
maximum `loadAssetsReadyMs`, minimum `minAvgFps`, maximum `maxJsHeapMB` (MiB).
Budget violations, readiness failures, page errors, and console errors exit 1.

Marks are navigation-relative milliseconds; `load` subtracts `bootStart`.
Time from invoking click/tap to the first rendered running frame is reported
separately. Frame intervals are rAF timestamps after warmup, sampled for at
least the requested duration (a slow final frame can overrun it). FPS is frame
interval count / elapsed time, percentiles are nearest-rank, and long-frame
percentages use >33.3 ms and >50 ms. Tick timing wraps the WASM bridge and includes
simulation and JSON serialization, but excludes JS parsing/transform application.
Memory captures CDP JS heap at ready/end, `performance.memory` when available,
and the WASM exported memory buffer size. JS heap is **not** total process memory;
resource bytes are Resource Timing `transferSize`, including response overhead.
Renderer geometry counts are uploaded resources, not total entity meshes.

Tests: `npm --prefix tools/bench test` and
`python3 -m unittest discover -s tools/bench -p 'test_*.py'`.
