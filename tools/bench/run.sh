#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
mkdir -p target/bench
exec > >(tee target/bench/pipeline.log) 2>&1
TERRAIN_ARGS=()
if [[ "${BENCH_TERRAIN:-1}" != 0 ]]; then
  rm -rf "$ROOT/target/bench/terrain"
  cargo run --release -p engine-terrain --example bench -- target/bench/terrain "${BENCH_SEED:-1}"
  TERRAIN_ARGS=(--terrain-manifest target/bench/terrain/terrain.json)
fi
python3 tools/bench/gen_large_world.py --seed "${BENCH_SEED:-1}" --density "${BENCH_DENSITY:-1.0}" \
  --out target/bench/large_world/scene.json "${TERRAIN_ARGS[@]}"
rm -rf "$ROOT/target/bench/site/large_world"
cargo run --release -- --export-web target/bench/large_world/scene.json target/bench/site/large_world
if [[ "${BENCH_SKIP_NPM_CI:-0}" != 1 ]]; then
  npm ci --prefix tools/bench
fi
node tools/bench/run_browser_bench.mjs --site target/bench/site --scene-path large_world \
  --out "${BENCH_OUT:-target/bench/results}" --budgets tools/bench/budgets.json \
  --duration "${BENCH_DURATION:-20}" --profiles "${BENCH_PROFILES:-desktop,mobile}" \
  --channel "${BENCH_CHANNEL:-chromium}" "$@"
