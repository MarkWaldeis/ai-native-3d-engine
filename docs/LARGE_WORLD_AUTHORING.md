# Large-world authoring

This builds on f7dad85's existing chunk grid, distance/hysteresis LOD selection,
instanced rendering and simulation regions, and 8f5e396's tiled terrain pipeline.
It does not add a second streaming/terrain implementation or change shading,
lighting, shadows or post-processing.

## Prefabs

`prefab_create {name,nodes}` stores an immutable-at-revision template in the scene.
Each node has a unique `key`, optional `parent` key, and `entity` components with a
**local** transform. Exactly one root, 1–256 nodes, no cycles. Template entity IDs
are placeholders, not scene IDs. Missing entity geometry defaults to a unit box.
Example:

```json
{"name":"house","nodes":[
  {"key":"body","entity":{"kind":"box","size":[4,3,4],"tags":["building"],"collider":{"shape":"box"}}},
  {"key":"roof","parent":"body","entity":{"kind":"box","size":[5,0.5,5],"transform":{"position":[0,2,0]}}}
]}
```

`prefab_instantiate` accepts `name`, optional layout `transform`, a node-keyed
`overrides` map, and one of:

- `layout:{mode:"grid",counts:[x,z],spacing:[x,z]}` (default one instance).
- `layout:{mode:"scatter",count,min:[x,y,z],max:[x,y,z],seed}`: SplitMix64 position
  samples and Y yaw, reproducible independently of scene IDs. Seed is required;
  grid/path have no random choices.
- `layout:{mode:"along_path",count,points:[[x,y,z],...]}`: equal arc-length
  spacing including endpoints, fixed orientation. Zero-length segments allowed;
  zero-length paths rejected.
- Explicit `instances:[{transform,overrides},...]`; these override the common
  patches per instance. Do not combine with layout.

Up to 10,000 instances / 100,000 expanded entities per call. Empty layouts are
no-ops. One bulk call commits one revision, not one per child. Returns stable
root `instances` IDs and all affected `ids` for coordination.

`prefab_update {name,nodes}` replaces a definition and propagates to every
instance. Node keys are stable identities: existing IDs survive; new keys get new
monotonic IDs; removed keys disappear. The root key cannot change. Clear overrides
for a removed key before deleting it. Definitions and overrides survive save/load,
snapshots, rollback and project restart. Scripts are captured as immutable source.

`prefab_set_overrides {instance_id,overrides}` replaces an instance's patch map;
`{}` resets it. Patches use JSON merge semantics: objects merge, arrays replace,
null removes an optional component. Patches are resolved before hierarchy
composition. `set_transform` takes world coordinates and records a local override;
children follow authoring parent edits. `set_color` and `write_script` also record
overrides, so later updates do not silently wipe them. An edit may touch the root
binding and descendants: claim every member returned by `query_entities` first.
`prefab_remove_instance` removes a complete instance; individual removal is
rejected to avoid orphaned children. Shared script locks still apply.

Every operation respects CAS and foreign locks on all affected members. `prefab_*`
tools are reversible atomic-batch members. Rejected batches change neither
templates, scene, IDs, journal, checkpoint nor assets. No Blender/filesystem writes
are allowed inside atomic batches; capturing existing script source is read-only.

Hierarchy is an **authoring** hierarchy, expanded into ordinary flat runtime
entities for the existing physics/web systems. It is not a runtime skeletal or
animated parent graph: independently scripted/dynamic children remain independent
bodies. TRS composition uses XYZ Euler rotations. Nonuniform scale + rotations
that produce shear are rejected rather than approximated; bake such transforms
into Blender geometry or use uniform parent scales. Zero scales are rejected.

### Minimal shared contract extension

`shared/src/scene.rs` adds only `Prefab {nodes}`, `PrefabNode {key,parent,entity}`
and an optional/default-empty `Scene.prefabs` map. Existing scene files continue
to deserialize unchanged. No Entity, runtime input, physics or renderer contract
changed. Instance authoring bindings use reserved `Entity.props._prefab` metadata;
do not edit it manually. Web exports strip definitions/bindings after expansion.

## Blender collections, LOD and content reuse

Collection-instance empties become prefab roots, with nested object/collection
hierarchies converted to local transforms. Collection offsets are preserved.
Collection source objects are definitions, not additional visible scene copies.
Collection names may change: stable collection/object `ai_uid` values identify
definitions (`blender:<collection_uid>`) and members. Sync preserves per-instance
engine overrides and propagates changed prototype geometry to every instance;
foreign locks reject the complete authoring commit. `objects` may select instance
names/UIDs; `prune:true` removes deleted Blender instances and their children.
Member overrides live in engine bindings; pushing synthetic child UIDs into the
shared Blender prototype is not supported. Edit prototypes in Blender or use
prefab overrides, rather than pretending each shared child is a Blender object.

Name meshes `Tree_LOD0`, `Tree_LOD1`, `Tree_LOD2`. LOD1/2 are optional and must use
LOD0's transform/origin (apply/bake transforms first). They are not separate scene
entities. Export records `props.web_lods:{"1":"...glb","2":"...glb"}`. The existing
player's same distance levels, hysteresis and chunk lifetime choose these assets;
missing levels retain the existing full-mesh/impostor fallback. Batch/cache keys
include authored LOD paths. This changes geometry selection, not shading.

Shared evaluated meshes/material references are serialized once per sync.
Canonical GLBs remove nonsemantic exported object/mesh/material labels and use
SHA-256 filenames. Identical content reuses one immutable file across objects and
syncs. Distinct material datablocks may still require serialization before exact
canonical-content deduplication. Assets are not deleted on update because old
snapshots may refer to them. Web packaging caches GLB reads/bounds, bundles all
referenced LOD levels and deduplicates identical bytes even at different paths.

## Bounded AI queries

`query_entities` reads the authoring scene without cloning it. AND filters:
`tag`, `component` (mesh/collider/rigidbody/script/prefab), `prefab`,
`bbox:{min,max}`, `near:{point,radius}`, `region:{x,z,chunk_size:128}`.
Spatial filters use entity origins, not mesh-volume intersection. Region indices
use floor(X/size), floor(Z/size), including negative coordinates, matching the
existing ChunkGrid convention; pass the world's configured chunk size explicitly
when it differs from 128 m. It does not select simulation activation regions.

Pages are ID-sorted with `limit:1..500` (default 100), `total`, `revision`, and
`next_cursor`. Send the identical filters with the cursor. Authoring edits make
old cursors fail explicitly; refresh/restart pagination. Compact rows omit props,
scripts and geometry; names/tag labels are bounded and at most eight tags are
returned per row. Use selected IDs with normal authoring tools; legacy `query`
keeps its active-simulation behavior and backwards-compatible array response.

`get_scene_summary` returns counts, origin bounds, unique base assets, template
count and logical instance counts; top 64 buckets per tag/component/prefab/128 m
region with full distinct counts. It does not dump 15k entities or thousands of
region keys. Current implementation scans O(N) and sorts matching IDs; it is not
a spatial index. See the measured HTTP latency report for its current scale.

Reproduce with the existing 44 km², 14,997-entity generator:

```sh
cargo build --bin ai-engine
python3 tools/bench/bench_authoring.py --binary target/debug/ai-engine --samples 200
```

The benchmark starts its own fresh project/HTTP server, warms each tool ten times,
measures complete MCP calls (including a fresh loopback connection/JSON response),
and writes `target/bench/authoring-latency.json`. No rendering benchmark is replaced.

### Measured local HTTP latency

macOS ARM64, unoptimized dev binary (debug symbols/incremental caches disabled),
14,997 entities / 44 km², 200 samples per tool after 10 warmups. This is an
ordinary development host measurement, not an isolated laboratory or release-mode
performance budget. Test startup overlapped the first benchmark case.

| Tool/filter | p50 ms | p95 ms | Response bytes | Matches |
|---|---:|---:|---:|---:|
| Query page, limit 100 | 4.595 | 5.497 | 19,060 | 14,997 |
| Tag ground | 4.060 | 4.944 | 19,060 | 4,096 |
| Component collider | 5.126 | 6.265 | 19,060 | 4,096 |
| Bounding box | 5.915 | 6.635 | 19,397 | 370 |
| Radius 500 m | 6.075 | 6.753 | 18,742 | 286 |
| Region (0,0), 128 m | 3.823 | 4.292 | 662 | 3 |
| Scene summary | 13.935 | 15.250 | 2,869 | 14,997 |

## M15 safe subset: indexed authoring queries

The World owns immutable ID-sorted postings for tags, component presence, prefab
names and the existing 128 m position regions. Combined filters intersect the
smallest posting first. Pagination preserves the existing revision/filter-bound
cursor, total count, ID order and response schema. Bounding boxes, radii and
custom region sizes remain exact position filters on the selected candidates;
this is not a geometry BVH or a runtime spatial index.

Caches are lazy, synchronized and disposable. World clones share immutable
postings, not cache ownership; edits, atomic commits and rollback invalidate the
local cache. Restart rebuilds it: no checkpoint or shared schema changed. Because
embedding code can edit the public `World.scene` directly, every reuse also checks
the exact relevant entity projection (IDs, position bits, tags, component flags,
prefab names). No hash-collision or revision-only freshness assumption is made.
That check is still **O(N)**; display names are read live and scene summaries still
scan. Acquire a fresh `World.authoring_index()` before using its row indices after
an edit; an externally retained Arc is a snapshot, not a live view.

Sequential HTTP MCP comparison on the same 14,997-entity / 44 km² generator,
macOS ARM64, unoptimized dev builds without debug symbols/incremental compilation,
200 samples after 10 warmups per case. The generator's box-ground variant is used,
not a terrain streaming/rendering benchmark. All seven match counts and response
byte sizes stayed identical. These are measurements, not cross-machine budgets.

| Tool/filter | Before p50 / p95 ms | Indexed p50 / p95 ms |
|---|---:|---:|
| Page, limit 100 | 4.879 / 6.862 | 2.443 / 2.506 |
| Tag ground | 4.315 / 5.706 | 2.500 / 2.749 |
| Component collider | 5.349 / 6.279 | 2.481 / 2.550 |
| Bounding box | 5.737 / 8.236 | 3.325 / 3.456 |
| Radius 500 m | 6.144 / 8.285 | 3.536 / 3.654 |
| Region (0,0), 128 m | 4.261 / 5.846 | 1.344 / 1.385 |
| Summary (not indexed) | 13.715 / 15.681 | 13.271 / 15.916 |

Cold index construction: **9.165 ms**; warm exact validation: **0.733 ms**.
Owned vector/string capacity payload: **1,716,762 bytes** for a 3,608,993-byte
scene JSON. That excludes BTreeMap nodes, Arc/allocator overhead and is **not RSS**.
The cache adds memory; it does not deduplicate runtime entity components.

```sh
cargo test --test authoring_index -- --nocapture
python3 tools/bench/bench_authoring.py --samples 200
cargo run --example authoring_index -- /path/to/generated/scene.json
```

**M15 remains partial:** prefab instances still expand to flat runtime entities.
Shared-template runtime contracts, incremental authoring checkpoints and safe
asset garbage collection are deferred: they need coordination with the frozen
shared contract, savegame/persistence and active asset-streaming ownership. No
physics, savegame, netcode, player streaming or rendering-quality files changed.
Neither these authoring features nor the existing renderer establish AAA
production readiness.
