# ai-native-3d-engine

Charakteranimation (Blender-Armature/Actions, deterministische State-Machine,
Rhai-Parameter, MCP-Lint und sim-getriebene Web-Crossfades):
[Pipeline und reproduzierbarer Milo-Rig](docs/CHARACTER_ANIMATION.md).

44-km²-Web-Baseline (FPS, Speicher, Ladezeiten; Desktop/Mobile):
[Benchmark-Anleitung und Referenzwerte](docs/BENCHMARK.md).

Multiplayer-Netcode (60-Hz-Server, Prediction/Reconciliation, Interpolation,
MCP `net_simulate`, reproduzierbare Netz-Arena): [Modell und Checks](docs/NETCODE.md).

## Mit Claude Code / Devin / Cursor verbinden

```sh
cargo build
./target/debug/ai-engine init ../mein-spiel --template sky_garden
# Alternativen: --template coin_game (Standard) oder --template empty
```

Der Zielordner muss neu oder leer sein. Enthalten sind `scene.json`, `scripts/`,
`assets/`, Projekt-`AGENTS.md`, `.mcp.json` für Claude Code und `.cursor/mcp.json`
für Cursor. Spielvorlagen enthalten `validation.json`; Himmelsgarten zusätzlich
das portable Blender-Rezept `blender_scene.py`, das eine lokale `.blend` speichert.
Szenen-/Skript-/Assetpfade werden auf das neue Projekt umgeschrieben.

**Claude Code:** im Projektordner starten und den Projekt-MCP-Server aus `.mcp.json`
bestätigen. Alternativ auf einer Zeile:
`claude mcp add --scope project ai-engine -- /absolut/ai-engine --project /absolut/mein-spiel`.
**Cursor:** Projekt öffnen, unter MCP den generierten Server aktivieren.
**Devin:** in der MCP-Konfiguration denselben stdio-Eintrag verwenden: absoluter
Binary-Pfad als Command, `--project /absolut/mein-spiel` als Args; alternativ die
unten beschriebene HTTP-URL bei einem vom Client erreichbaren lokalen Server.
Konfigurationspfade sind absolut; beim Verschieben von Binary/Projekt aktualisieren.
`--project` lädt die Szene beim Start und setzt das Arbeitsverzeichnis zuverlässig.

Erste Nachricht: **„Lies engine://guide und engine://scene. Baue ein kleines Spiel
Blender-first; beweise Sieg und Sturz/Rettung mit simulate, prüfe validate_scene und
Screenshots, dann export_web.“** Die Ressourcen enthalten eine Deutsch/English-
Anleitung und den aus `tools/list` generierten Werkzeugkatalog. Prompts:
`build_game {idea}`, `qa_pass`, `swarm_lead {goal}` (via `prompts/get`).
`engine://scene` ist die aktuelle Authoring-Szene; Spielzustand: `get_sim_state`.

Für mehrere Agenten **einen gemeinsamen HTTP-Server** starten (separate stdio-
Prozesse haben separate Welten):

```sh
/absolut/ai-engine --project /absolut/mein-spiel --serve 127.0.0.1:7777
# Claude Code: claude mcp add --transport http ai-engine http://127.0.0.1:7777/mcp
```

Streamable HTTP: `POST /mcp`, `Accept: application/json, text/event-stream`,
`Content-Type: application/json`. Der Server antwortet JSON, ohne SSE;
`GET /mcp` liefert 405. `initialize` handelt `2024-11-05`, `2025-03-26` oder
`2025-06-18` aus (unbekannte Version → neueste unterstützte Version). Den dort
ausgegebenen `Mcp-Session-Id` bei Folgeaufrufen mitsenden, ebenso
`MCP-Protocol-Version`; Benachrichtigungen erhalten 202 ohne Body. `DELETE /mcp`
beendet eine Sitzung; unbekannte/beendete IDs ergeben 404. SDK-basierte MCP-Clients
erledigen den Handshake und die Header automatisch.

Standardmäßig nur Loopback (`--serve` ohne Adresse: `127.0.0.1:7777`). Lokales
Blender-Python bleibt privilegiert und setzt vertrauenswürdige Clients voraus.
Remote-Betrieb erfordert `--allow-remote` und Bearer-Authentifizierung (siehe unten).
Session-IDs sind keine Authentifizierung. Fremde Browser-Origins werden abgewiesen.
Tool-Annotations (`readOnlyHint`, `destructiveHint`) sind Hinweise, keine Rechte.
Web-Export benötigt weiterhin Engine-Quellcheckout, Rust-WASM-Target und
wasm-bindgen CLI; das exportierte Spiel benötigt nur einen statischen Host.

Optionaler Client-Smoke-Test mit dem offiziellen Inspector (Node/npm):

```sh
npx --yes @modelcontextprotocol/inspector --cli --config /absolut/mein-spiel/.mcp.json --server ai-engine --method tools/list
npx --yes @modelcontextprotocol/inspector --cli --config /absolut/mein-spiel/.mcp.json --server ai-engine --method tools/call --tool-name get_revision
npx --yes @modelcontextprotocol/inspector --cli --config /absolut/mein-spiel/.mcp.json --server ai-engine --method resources/read --uri engine://guide
# Für HTTP stattdessen: --config ... --server ... durch
# http://127.0.0.1:7777/mcp --transport http ersetzen.
```

## Web release checks (M9)

CI runs `node --test tools/test_web_*.mjs`, plus Linux Chromium and required WebKit
iPhone/iPad checks on fresh exports of **all three** demos. Reproduce from the repository root (Rust WASM
target and wasm-bindgen-cli 0.2.100 installed, as in Pages):

```sh
npm install --prefix target/web-ci --no-save --package-lock=false playwright@1.65.1
target/web-ci/node_modules/.bin/playwright install --with-deps --only-shell chromium
target/web-ci/node_modules/.bin/playwright install --with-deps webkit
rm -rf target/m9-site
cargo run -- --export-web examples/coin_game/scene.json target/m9-site/coin_game
cargo run -- --export-web examples/sky_garden/scene.json target/m9-site/sky_garden
cargo run -- --export-web examples/material_studio/scene.json target/m9-site/material_studio
cargo run -- --project examples/lantern_maze --export-web scene.json web
mv examples/lantern_maze/web target/m9-site/lantern_maze
cargo run --example web_parity > target/web-native.json
PLAYWRIGHT_MODULE="$PWD/target/web-ci/node_modules/playwright/index.mjs" \
  node tools/verify_web_release.mjs target/m9-site target/web-native.json
PLAYWRIGHT_MODULE="$PWD/target/web-ci/node_modules/playwright/index.mjs" \
  BROWSER_ENGINE=webkit SHOTS_DIR=target/web-release-shots/webkit \
  node tools/verify_web_release.mjs target/m9-site target/web-native.json
```

WebKit uses iPhone 13 and iPad Pro 11 portrait profiles, native touchscreen first
activation, keyboard-driven victories/restarts and the same native/WASM parity
traces. These are Playwright WebKit profiles, not physical iOS Safari acceptance.
Missing browser binaries/dependencies fail clearly; no silent skip or Chromium
fallback. Chromium additionally exercises real multi-touch lantern gameplay;
`tools/verify_mobile.mjs` covers the Coin straight-right joystick route with zero
respawns. Coin's follow-camera yaw now aligns with its horizontal path and pickup
radius is 0.9 m; a 0.67 m off-centre recorded trace guards that tolerance.
Start/resume/restart accepts pointerup, touchend or keyboard click with a shared
in-flight/visible-menu fence, preventing compatibility events from starting twice.
Every exported player and the Pages landing page include the local SVG favicon.

`tools/web_traces.json` contains fixed input traces for win and rescue in each
demo, including Sky Garden rescue **after** advancing a checkpoint and Coin Game
rescue preserving a collected score. The native helper uses the MCP `simulate`
implementation, validates expected outcomes, and records first win tick plus
score/checkpoint/rescue samples every 30 ticks and at the end. The browser feeds
the identical events to the actual exported WASM runtime and compares exactly.
Missing artifacts, parity differences, browser errors and asset HTTP errors fail
the job; there are no renderer skips or automatic success-on-retry paths.

Player checks deliver keyboard events through the real UI controls, prove an
off-platform rescue, frozen ticks while paused, pause-menu **Neustart**, victory
and **Nochmal spielen**. A virtual animation-frame clock removes wall-time/FPS
races without replacing gameplay, physics, input mapping or rendering. Screenshots
and the native reference are uploaded as `web-release-evidence`; local screenshots
default to `target/web-release-shots` (`SHOTS_DIR` overrides it).

This Chromium check does **not** certify Mobile Safari or physical iOS devices.
Those still require a separate full touch victory; opening the page or seeing the
start screen is not an acceptance pass.

## M6 · Himmelsgarten: Blender-first HTTP swarm

`examples/sky_garden/` is a floating-island platformer: steer Milo with the touch
joystick or WASD/arrows, jump with **Springen / Space**, collect three crystals
and reach the portal. Falling returns you to the last crystal checkpoint.
The `.blend` file is the modeling source; GLBs embed modeled scenery/materials.

```sh
cargo build
python3 tools/swarm_demo.py --out-dir target/sky-site/sky_garden
python3 -m http.server 8000 --directory target/sky-site
# Open http://localhost:8000/sky_garden/
```

Python stdlib only. Four concurrent roles use distinct `X-Agent-Id` headers,
dependent task claims, entity locks and revision queries. All models/components/
scripts are authored through MCP HTTP. **Replaces the active Blender scene.**
Requires Blender and existing web-export prerequisites; output must be new/empty.
Recipes: `tools/sky_garden/`; execution: `docs/swarm_journal.json`;
German friction notes: [SWARM_FINDINGS.md](docs/SWARM_FINDINGS.md).

- `write_script {ids,path,source,expected_revision?}` compiles before writing a
  project-relative `.rhai` file and binding immutable `{path,source}` versions to
  explicitly selected entities. Snapshots retain executable source, not mutable
  file references. Shared paths require claims on every referencing entity;
  all references appear in the journal, but only selected IDs are rebound.
  Pass all IDs for a common update. Existing parent required; CAS and play-mode restrictions apply.
- `save_scene {path,bundle_assets:true}` copies GLBs alongside the scene into
  `assets/`, using project-relative references without changing the authoring world.
- Rhai `ctx.velocity()` / `ctx.grounded()` read the previous physics step
  (zero/false initially or without a body). Grounding requires upward support,
  not a wall. Blender `ai_lock_rotation:true` keeps bodies upright.
- Player `props.ai_ui` customizes title, mission, collectible tag, action, HUD keys,
  intro/win text and sky color. `_`-prefixed state keys stay private in the HUD.

`cargo test -p engine-tests --test swarm_game` covers script writing, grounding,
portable assets and simulate-based win/jump/fall-respawn validation. Pages exports
both `/sky_garden/` and `/coin_game/`.

[![CI](https://github.com/MarkWaldeis/ai-native-3d-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/MarkWaldeis/ai-native-3d-engine/actions/workflows/ci.yml)

An AI-native 3D game engine prototype written in Rust. The engine runs as an
**MCP stdio or HTTP server**: AI agents (Claude Code, Devin, Poke, …) connect over
newline-delimited JSON-RPC and drive the engine through typed tools — spawn
geometry, move the camera, render headless screenshots, and run Blender
scripts — instead of a human clicking in a viewport.

v0.1 is a working vertical slice: full MCP round-trip → world state → wgpu
headless render → PNG screenshot back to the agent.

![headless render: plane + Blender-exported GLB + sphere](docs/gltf_demo.png)

*`cargo run -- --demo docs/gltf_demo.png docs/demo_scene.json` — real wgpu headless
output: procedural floor, a Blender-exported `suzanne.glb` loaded through the
`gltf` spawn path, and a sphere.*

> 📐 Original design doc (German, by Gemini): [docs/masterplan.md](docs/masterplan.md) —
> the multi-agent architecture this workspace was built from.

## Workspace layout

```
Cargo.toml               root binary `ai-engine` (MCP stdio server + --demo)
src/
  main.rs                stdin/stdout JSON-RPC loop
  lib.rs                 `Engine` facade: World + Renderer + AssetPipeline + tool dispatch
shared/                  serializable contracts shared by all crates
  scene.rs               Scene / Entity / Transform / Camera / PrimitiveKind
  mcp_protocol.rs        JSON-RPC 2.0 types + the MCP tool surface (tools/list schemas)
  mesh.rs                CPU vertex/index buffers produced from primitives
  mocks.rs               deterministic mock scene for tests & --demo
crates/
  engine-core/           World state, command queue, save/load (flat Vec ECS, revision counter)
  engine-render/         wgpu headless renderer → PNG readback → base64
  engine-geo/            procedural mesh generation (box, plane, sphere, cylinder, wall_with_door)
  engine-runtime/        deterministic Rapier physics, sandboxed Rhai hooks, assertions/query
  engine-web/            wasm-bindgen adapter + offline three.js WebGL2/touch player
  engine-mcp/            JSON-RPC framing + MCP lifecycle (initialize, ping, tools/list, tools/call)
  asset-pipeline/        finds Blender, runs `blender --background --factory-startup --python`,
                         watches assets/ for changed files
  engine-tests/          end-to-end tests driving the engine like an MCP client
```

## MCP tool surface

| Tool | Purpose |
|---|---|
| `get_scene_state` | entire scene as JSON (entities, transforms, camera) |
| `spawn_primitive` | box / plane / sphere / cylinder / wall_with_door **or `kind:"gltf"` + `path`** (GLB from `assets/`) with transform + RGBA |
| `set_transform` | position / rotation (deg) / scale by entity id |
| `set_color` | RGBA by entity id |
| `remove_entity` | delete by id |
| `set_camera` | position / target / fov |
| `get_screenshot` | headless render → base64 PNG (`content[].type = "image"`) |
| `export_web` | authoring scene (or `scene` JSON/path) → self-contained WebGL2/WASM static folder; requires new/empty `out_dir` |
| `save_scene` / `load_scene` | persist scene to/from `.json` |
| `get_revision` | current world revision (concurrency token) |
| `rollback` | restore scene to an earlier revision |
| `apply_batch` | ordered `partial` (default), or single-revision `atomic` scene batch with batch CAS |
| `list_assets` | list files in `assets/` |
| `run_blender_script` | headless Blender → generated GLB assets land in `assets/` |
| `blender_status` / `blender_start` | inspect local bridge/version or launch an engine-owned headless bridge |
| `blender_exec` / `blender_scene_info` | run trusted Python on Blender's main thread; inspect objects and `ai_*` props |
| `blender_sync` | export meshes to `assets/blender/`, upsert by stable `source_uid`; optional `objects` names/UIDs and explicit `prune:true` |
| `blender_push_props` | push engine gameplay fields/custom `ai_*` props by `ai_uid`; reject concurrent Blender changes; sync to acknowledge |
| `claim_entities` / `release_entities` / `list_locks` | agent-owned entity locks with expiry (`ttl_s`, default 60, range 1–86400) |
| `get_changes_since` | scene mutation journal after `revision`: agent, tool, affected entity `ids` |
| `task_create` / `task_claim` / `task_renew` / `task_release` / `task_complete` / `task_list` | task board with dependencies, expiring claims and fencing leases |
| `get_task_changes_since` | coordination journal with its own sequence cursor, independent of scene revisions |

Every mutating tool accepts an optional `expected_revision` integer.

## Quickstart

### Web-Player: iPhone, iPad und Desktop (M5)

**[Münzenpfad spielen](https://markwaldeis.github.io/ai-native-3d-engine/coin_game/)**
— verfügbar, sobald der Pages-Workflow nach dem Push erfolgreich deployed wurde.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.100 --locked
cargo run -- --export-web examples/coin_game/scene.json target/site/coin_game
python3 -m http.server 8000 --directory target/site
# http://localhost:8000/coin_game/ — auch Mobile Safari im lokalen Simulator
```

Der Export baut `engine-web` aus diesem Workspace (Rust-Quellen und Toolchain
müssen beim Export installiert sein). Alternativ per MCP:
`export_web {"out_dir":"target/site/coin_game","scene":"examples/coin_game/scene.json"}`.
Ohne `scene` wird der **Authoring-Stand**, nicht die laufende Simulation, exportiert.
Vorhandene nicht-leere Zielordner werden aus Sicherheitsgründen abgelehnt.
Skripte werden wie nativ relativ zum Projektverzeichnis bzw. `assets/` aufgelöst;
das CLI daher im Projektverzeichnis starten. Der Export braucht weder GPU noch Blender.

Das Ergebnis enthält `index.html`, `player.js`, `style.css`, `pkg/*.wasm` und
WASM-JS-Glue, `scene.json`, `scripts.json`, `scripts/`, `assets/` und vendortes
**three.js 0.160.1** einschließlich GLTFLoader, RGBELoader und MIT-Lizenz. Kein CDN, kein
Engine-Prozess und keine externen Netzwerkanfragen zur Laufzeit. Das komplette
Verzeichnis auf einen beliebigen statischen HTTP(S)-Host kopieren; `file://`
funktioniert nicht. Relative URLs funktionieren auch unter Repository-Unterpfaden.
GLBs müssen Texturen/Buffer einbetten (Blender: GLB); externe `.gltf`-Ressourcen
werden mit einer verständlichen Fehlermeldung abgelehnt. Exportierte lokale
Mesh-Bounds ermöglichen dieselben Rapier-Bounds-Collider ohne Browser-Dateisystem.
Referenzierte Datei-Skripte werden gebündelt; zur Laufzeit erzeugte Entities sollten
Inline-Skripte oder bereits gebündelte Skriptpfade verwenden.

- **Touch:** linker Joystick → `move_x` / `move_z`; rechte Aktionstaste →
  `jump` und `action`. Pointer-Capture unterstützt gleichzeitiges Bewegen/Aktion.
- **Desktop:** WASD / Pfeile; Leertaste → Aktion. Der Münzenpfad benötigt keinen Sprung.
- Kamera folgt dem Tag `player`, sonst bleibt die Szenenkamera aktiv.
- HUD zeigt globale `state`-Werte (Score als „Münzen“); Protokoll zeigt Skriptlogs.
  `state.won=true` (oder `gewonnen=true`) öffnet „Gewonnen!“ und Neustart.
  **Die Ziel-/Sieglogik liegt im Rhai-Skript, nicht im JavaScript.**
- „Ⅱ“ pausiert, „Vollbild“ nutzt die Browser-API; auf iPhone Safari ohne
  Fullscreen-API hilft „Teilen → Zum Home-Bildschirm“. Safe-Areas, Hoch-/Querformat,
  `touch-action:none` und keine Scroll-/Zoom-Gesten sind berücksichtigt.
- Exakt dieselbe `Simulation` mit Rapier + Rhai läuft als WASM bei 60 Hz.
  JS rendert und liefert Inputs, implementiert aber keine Spielphysik/Regeln.
  Der Wrapper `WebGame.load(scene_json, scripts_json)` akzeptiert zwei JSON-Strings
  (`scripts_json`: Pfad → Quelltext). `tick(inputs_json)` führt einen Tick aus;
  Eingaben sind z.B. `{"move_x":1,"move_z":0,"jump":0,"action":0}`.
  Beide liefern JSON mit `tick`, `transforms`, `state`, `logs`, `removed` (IDs),
  `spawned` (komplette Entities). `load` setzt die Simulation neu auf.
  Determinismus gilt weiterhin innerhalb desselben Builds/Targets; keine Zusage
  bitgleicher Ergebnisse zwischen nativen und WASM-Builds.

**GitHub Pages:** Unter *Settings → Pages → Build and deployment → Source*
„GitHub Actions“ wählen. `.github/workflows/pages.yml` baut und exportiert bei
Push auf `main` (oder manuell), legt eine deutsche Landingpage im Root ab und
deployed mit `actions/deploy-pages`. Die Demo liegt in `coin_game/`.
Der Benutzer muss nur den Link auf iPhone/iPad Safari öffnen.

![Web-Player auf dem Desktop](docs/web_desktop.png)

### Native Engine

```bash
cargo build --workspace
cargo test --workspace                              # 17 tests incl. real GPU + GLB round-trips
cargo run -- --demo out.png                         # render the mock scene → PNG
cargo run -- --demo out.png docs/demo_scene.json    # render a saved scene → PNG
cargo run                                           # MCP stdio server (logs → stderr, protocol → stdout)
cargo run -- --serve 127.0.0.1:7777                  # shared-world HTTP server: POST /mcp
```

On macOS/Linux install a stable Rust toolchain with rustup; no linker overrides
are needed. The Windows-GNU linker setting is target-scoped and remains in
`.cargo/config.toml`. GPU tests print `SKIP` if no renderer is available.

HTTP accepts one JSON-RPC request per `POST /mcp` and returns one JSON response
(202 with an empty body for notifications). Multiple clients share one engine, with calls
serialized on a dedicated engine thread (including GPU work). Supply an optional
`agent` string in tool arguments or an `X-Agent-Id` HTTP header in trusted local
mode; the argument wins, otherwise identity defaults to `anonymous`. In remote
mode the bearer credential fixes the identity: headers, arguments and nested
batch commands cannot impersonate another agent. Sessions are principal-bound.

### Remote HTTP / Sicherheitsgrenze (M8)

Non-loopback binds fail unless `--allow-remote` is supplied. This flag requires
bearer authentication on **every request**, even if binding to loopback (use this
mode behind a tunnel/reverse proxy too). It prints a startup warning. Plain HTTP
does not encrypt credentials: use a trusted network or a TLS/SSH tunnel.

```sh
# Reads AI_ENGINE_HTTP_TOKEN (principal "remote"), or generates and prints a
# cryptographically random token once to stderr if neither token variable is set.
ai-engine --project /absolute/game --serve 0.0.0.0:7777 --allow-remote
# Every request, including initialize, GET and DELETE:
# Authorization: Bearer <token>
```

For multiple workers, set `AI_ENGINE_HTTP_TOKENS` to a JSON object mapping agent
names to distinct secret bearer tokens (provision via your secret manager, not
checked-in project files). Do not set both token variables. One shared token is
one identity; it cannot provide independent worker authorization. Session IDs
cannot be used by a different principal. Wrong/missing tokens return HTTP 401;
attempting another principal's session returns 403.

Remote `blender_exec` and `run_blender_script` are disabled, including inside
partial batches. Only the separate `--allow-privileged-tools` flag enables them.
This deliberately grants **unsandboxed Python/machine access** to token holders.
Tool annotations are not permissions. Do not enable this for untrusted agents.

All MCP/CLI scene, script, mesh and export **path arguments** stay inside the
active project (`--project`, otherwise cwd), in local and remote modes, even with
privileged tools enabled. Relative paths resolve there (GLB spawn paths resolve
under `assets/`); scene/asset/output absolute paths inside it are accepted.
`write_script` continues to require a project-relative `.rhai` path. Parent traversal and
symlinks escaping it are rejected, including future output directories and file
references inside scene JSON. Raw Python is not an OS sandbox and can bypass
filesystem policies when deliberately enabled. Use trusted project contents.

```bash
# First initialize and copy Mcp-Session-Id from the response headers:
curl -i http://127.0.0.1:7777/mcp -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
curl http://127.0.0.1:7777/mcp -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' -H 'Mcp-Session-Id: COPY_SESSION_ID' \
  -H 'MCP-Protocol-Version: 2025-06-18' -H 'X-Agent-Id: level-designer' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"spawn_primitive","arguments":{"kind":"box"}}}'
```

### Atomic changes and Blender ownership (M7)

`apply_batch {mode:"atomic",expected_revision,commands:[...]}` permits
`spawn_primitive`, `set_transform`, `set_color`, `remove_entity`, `set_camera`
and the reversible `prefab_*` authoring tools.
It commits one revision/snapshot and one journal entry, or changes neither scene,
revision, journal nor assets on rejection. No per-command CAS, nested batches,
`write_script`, Blender tools, load/rollback, save or export are permitted inside.
Empty atomic batches are no-ops. Default `mode:"partial"` retains earlier
successes when a later command fails. Invalid CAS types (including strings/null)
are rejected rather than treated as an omitted revision.
Atomic results contain `{mode,applied,revision,ids}`; partial success retains the
legacy result array, and partial errors include `mode:"partial"` and `applied`.

Blender owns geometry/transform. Gameplay fields use three-way merging: unchanged
Blender fields never overwrite newer engine values; divergent simultaneous edits
report conflicts. Missing fields preserve engine values. Explicit removal uses a
JSON `"null"` custom-property string, or
`obj['ai_deleted_props'] = json.dumps(['ai_script'])`. This applies to tags,
collider, rigidbody, script and custom `ai_*` properties. `blender_push_props
{ids,expected_revision?}` pushes gameplay values by stable `ai_uid` after checking
locks and concurrent Blender changes, without moving/remodeling objects. Sync
after a push to acknowledge its baseline. Script versions include source in the
scene/snapshots, so load/rollback never depends on later file edits.
`load_scene` resolves script files against the active project root, including
when the scene JSON is in a nested folder.

Locks reject edits/removal by other agents, including batch commands. Replacing
the world (`load_scene`/`rollback`) requires no foreign entity locks. Claims and
releases are all-or-nothing; expired locks disappear automatically on the next
call. A batch inherits its outer agent (subcommands cannot override it).
`get_changes_since` returns `{entries, current_revision, oldest_revision}`;
the last 1024 scene mutations are retained. Each successful **partial** batch
subcommand is journaled separately; an **atomic** batch creates one revision and
one journal entry containing all affected IDs. Lock/task metadata does not advance the scene revision.
If your revision predates the journal window, re-read the scene. Explicit project
engines persist coordination and authoring checkpoints as described below.

Tasks start `pending`, become `claimed`, then `completed` by the live owner.
`task_create` takes `title`, optional `description` and existing `depends_on` IDs.
`task_claim {id,ttl_s?}` waits for dependencies; TTL defaults to 60 seconds,
range 1–86400. It returns `lease` and remaining `ttl_s`. Always pass that fencing
lease to `task_renew {id,lease,ttl_s?}`, `task_release {id,lease}`, and
`task_complete {id,lease,note?}`. Renew before expiry; select a TTL covering long
blocking tool calls. Release returns a task to pending. Expired tasks appear as
`expired` in `task_list` and can be claimed by another worker without cooperation
from the dead worker. Every new claim gets a higher lease, including same-agent
reclaims: old completions/renewals are rejected. For compatibility, only the
first-generation `task_complete` may omit its lease; no omission after takeover.

`get_task_changes_since {sequence:0}` returns `{entries,current_sequence,
oldest_sequence}`. It journals create/claim/renew/release/takeover/complete with
task ID, actor, lease, TTL and previous owner (1024-event retention). This cursor
is independent of `get_changes_since` scene revisions: task coordination does
not bump the scene or create snapshots. `task_list` filters
pending/claimed/expired/completed.

### Large-world authoring

Reusable scene-embedded hierarchical prefabs: `prefab_create`, `prefab_update`,
`prefab_instantiate` (grid/seeded scatter/along-path/explicit transforms),
`prefab_set_overrides`, `prefab_remove_instance`. Updates propagate while retaining
per-node instance overrides and IDs; all affected locks, CAS, atomic batches,
rollback and project restart persistence apply. Child transforms are composed in
authoring and flattened for the existing runtime, not a runtime animation graph.

Blender collection instances import as prefabs. `*_LOD0/1/2` assets feed the
existing player LOD levels; canonical SHA-256 GLBs and content-aware packaging
avoid duplicate stored assets. No lighting/shadow/post-processing changes.
After upgrading, restart existing Blender processes to load the updated bridge
exporter before syncing collection instances or LOD assets.

Use `query_entities` instead of full scene dumps: AND tag/component/prefab,
origin bbox/radius/region filters, bounded ID-ordered pages (default 100, max 500),
revision-bound cursors. `get_scene_summary` returns bounded aggregate buckets.
Legacy `query` keeps its live-simulation response. Full contracts, shared-schema
extension and the 44 km² HTTP benchmark command:
[Large-world authoring](docs/LARGE_WORLD_AUTHORING.md).

### Restart-safe project coordination

`ai-engine --project X --serve 127.0.0.1:8787` resumes from
`X/.ai-engine/state.json`. Project-mode stdio uses the same storage. Successful
coordination/authoring mutations are acknowledged only after one checkpoint is
written to a unique temporary file, synced and atomically renamed. The checkpoint
contains tasks, fencing counters, task journal/sequence, scene journal/revision,
entity locks, the matching authoring scene, monotonic entity allocator and the
64-snapshot rollback window. Journals retain 1024 entries each. Partial batches
retain successful subcommands; an atomic batch is persisted only after its single
commit, never during staging. Reads and simulation ticks do not rewrite state.

Leases and locks store UTC expiry deadlines: time while the server is down counts
toward TTL, not a fresh lease on restart. Keep the system clock synchronized;
clock rollback is clamped to the last saved time and cannot grant more than the
last saved remaining TTL. Expired tasks retain their fencing counter for takeover.
Clients must initialize new HTTP sessions after restart and reread journals/claims.
Play-mode physics/state and renderer state are not restored; authoring resumes
stopped. Embedded immutable script versions remain executable after file changes.

A kernel file lock permits **one writer per project** and is released on process
death. Independent projects may run concurrently. Corrupt/unsupported checkpoints
fail startup rather than silently resetting claims. Interrupted `.tmp` files are
ignored. A failed checkpoint write returns an error and restores the in-memory
authoring/coordination state; external Blender/filesystem side effects are not an
OS transaction and may remain. The file is synced before rename; directory sync
is attempted on Unix. This is local filesystem persistence, not distributed
consensus or a guarantee against arbitrary filesystem/hardware corruption.

`init` ignores `.ai-engine/`; the metadata directory also ignores its own contents
for existing projects. Stop the engine before backups or intentionally discarding
the directory. A checkpoint takes precedence over `scene.json`: to start over,
first explicitly `save_scene` if needed, stop the engine, then move/delete
`.ai-engine/`. Copying/moving a project requires fresh `.ai-engine` metadata; its
identity is bound to the canonical project path. Bare `Engine::new()` without
project mode remains an ephemeral authoring engine.

Example session (one JSON object per line):

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"spawn_primitive","arguments":{"kind":"box","name":"crate","position":[0,0.5,0],"color":[1,0.4,0.1,1]}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_screenshot","arguments":{"width":960,"height":540}}}
```

## Design notes for the evaluator

### Blender-first bridge

Blender is found on PATH, in `/Applications/Blender.app`, or in the existing
Windows Blender Foundation directories. Start the bridge through `blender_start`
or manually:

```bash
blender --background --factory-startup --python blender/ai_engine_bridge.py
```

For **interactive Blender**, install `blender/ai_engine_bridge.py` as a legacy
add-on through Preferences → Add-ons → Install from Disk, then enable **AI Engine
Bridge**. Alternatively open the script in Blender's Text Editor and choose
**Run Script**. Rerunning replaces the previous listener; disabling the add-on
stops it. Interactive commands run through `bpy.app.timers` on Blender's main
thread; headless mode pumps the same queue on the main thread and stays alive.

The protocol is one newline-terminated JSON request/response per TCP connection
on loopback. Each project stores a deterministic path-derived `blender_port`
(20000–59999) in `.ai-engine/config.json`; edit it while stopped if a port is busy.
`AI_ENGINE_BLENDER_PORT` overrides it for this process without rewriting config
(valid range 1–65535). Two project engines use separate bridges. Ping reports
`project_id` (canonical project path), and the engine refuses mismatched identities
without stopping or editing the existing Blender. Every project command is also
identity-checked by Python, including after a reconnect. Identity is a routing
guard, **not authentication** for arbitrary local Python clients.

For manual/add-on startup, set `AI_ENGINE_BLENDER_PORT` and
`AI_ENGINE_PROJECT_ID` to that project's effective port and canonical path in
Blender's environment. `blender_start` supplies both automatically. A legacy
unidentified bridge is not accepted by project engines; save and restart it.
Commands are `ping`, `exec {code}`,
`scene_info`, and `export {dir, objects?, whole_scene?}`. `exec` returns captured
stdout and a JSON-serializable `result` variable; each call has a fresh Python
namespace (`bpy` scene state persists). This is **arbitrary local Python execution**,
not a sandbox or authentication boundary: trusted local clients only.

`blender_start` connects to an existing bridge without owning it, or starts a
headless process that is terminated when its owning Engine is dropped. Startup
uses the repository's bridge script; retain the `blender/` directory with the
engine. `blender_status` gives startup instructions if disconnected.

`blender_sync` assigns missing `ai_uid` custom properties and stores them as
`Entity.source_uid`, with all `ai_*` custom properties in `Entity.props`.
Save the `.blend` file to preserve these UIDs across Blender restarts. Re-sync
updates mesh, name, transform, base color and props while preserving engine ids.
Duplicated Blender objects with copied UIDs receive distinct UIDs on inspection/
export. Existing JSON scenes remain compatible through serde defaults.

The default export is one evaluated mesh per GLB, with world placement represented
once by the Entity transform. Blender `(x,y,z)` becomes engine `(x,z,-y)`;
orientation is converted with the same basis and residual parent shear is baked
into geometry. Parent transforms/modifiers are included. Whole-scene export
instead bakes world placement into one GLB and reports `baked_world_transform`.
The current renderer uses one base color per object; textures and multi-material
shading are not yet reproduced.

Each sync is one revision/journal entry and respects `agent`, `expected_revision`
and foreign entity locks. Without `prune:true`, vanished objects remain. With
pruning, only previously synced objects absent from the complete Blender mesh
list are removed; filtering `objects` never deletes other still-existing objects.
Export filenames are versioned so locked entities and rollback snapshots keep
their original geometry. Generated GLBs are ignored by git; retain them beside
saved scenes (old exports are not automatically garbage-collected).

`cargo test -p engine-tests --test blender_bridge -- --nocapture` runs the real
headless Blender E2E when installed, otherwise prints `SKIP`. A reproducible
scene is in `blender/sync_demo.py`; comparison images:
[`docs/blender_sync_demo.png`](docs/blender_sync_demo.png) (engine) and
[`docs/blender_sync_reference.png`](docs/blender_sync_reference.png) (Blender).

- **AI-native control loop**: the whole engine is behind typed MCP tools. An
  agent can `get_scene_state` → mutate → `get_screenshot` → iterate — the same
  critic/reviewer loop a human artist would run, fully automated.
- **Plain-JSON scene**: `shared::scene::Scene` is the single source of truth,
  serializable, diffable, and loadable — a world state agents can read and
  reason about directly.
- **Deterministic conflict resolution**: mutations are serialized by the
  single-threaded stdio loop (line order = canonical order). `apply_batch`
  runs in array order. Its default `mode:"partial"` keeps earlier successes on
  failure; `mode:"atomic"` commits all scene edits together. `expected_revision` adds optimistic
  concurrency — when two agents read the same revision and both mutate,
  the first wins and the second gets a `revision conflict` error and must
  re-read. Every mutation records a per-revision `Scene` snapshot (ring of
  64), so `rollback` restores any recent state while the revision counter
  stays monotonic.
- **Deterministic + fast**: spawn→state round-trip < 100 ms (asserted in tests);
  world `revision` counter acts as a render dirty-flag.
- **Headless rendering**: wgpu renders off-screen with no window — works on a
  headless agent box; screenshot path is exercised by a real GPU test.
- **glTF/GLB loading**: `kind:"gltf"` spawns entities from `.glb` files —
  `engine-geo::load_gltf` fuses all node meshes with baked transforms.
  `assets/suzanne.glb` (exported headless from Blender 5.2) is committed as a
  fixture; `docs/demo_scene.json` shows the format.
- **Asset pipeline hook**: agents can ship a Blender Python script through MCP
  (`run_blender_script`) to turn generated art into GLB assets in `assets/`.
- **Graceful degradation**: MCP world-editing tools work with no GPU; only
  `get_screenshot` requires one (renderer is lazily initialized).

## Gameplay runtime (M3)

Author gameplay in scene JSON (old scenes still load) or Blender custom properties:

```json
{"id":1,"name":"ball","kind":"sphere","radius":0.5,"segments":24,
 "tags":["player"],"transform":{"position":[0,3,0]},
 "collider":{"shape":"sphere","radius":0.5},
 "rigidbody":{"type":"dynamic","mass":1,"friction":0.5,"restitution":0},
 "script":{"source":"fn on_tick(ctx, dt) { if ctx.input(\"jump\") > 0.0 { ctx.apply_impulse([0.0, 0.1, 0.0]); } }"}}
```

| Tool | Purpose |
|---|---|
| `play {}` | Snapshot authoring into a separate simulation; run `on_start` |
| `step {ticks, inputs?}` | Advance the active simulation at fixed 60 Hz |
| `get_sim_state {}` | Tick, transforms/tags, global state, logs and failed flag |
| `stop {}` | Discard simulation; unchanged authoring scene/revision reappears |
| `simulate {ticks, inputs?, asserts?, screenshot?}` | Fresh isolated run, per-assert `passed`/`actual`, final state; optional final PNG content block |
| `query {tag?, name?, near?:{point,radius}}` | Compact matching entities; filters AND together; exact name |

There is one shared live play session per Engine. Authoring edits are rejected
while playing (stop before editing); `get_scene_state` always reads authoring,
while `query`/`get_screenshot` use the active simulation if present. Simulation
does not mutate authoring snapshots, locks, journal or revisions. `simulate`
always starts from authoring, without replacing a live play session. An error
stops the current play session's advancement; inspect it, then stop/play to reset.

Inputs are zero-based **absolute simulation tick** events, applied before that
tick's hooks and held until changed. `step` accepts events only within its tick
range. For example, `{"ticks":120,"inputs":[{"tick":0,"action":"move_x","value":1},
{"tick":60,"action":"move_x","value":0}]}` moves for one second, then stops.
Each call allows 0–36000 ticks and at most 4096 input events.

Rapier 0.22 with `enhanced-determinism` uses fixed 1/60 s integration and sorted
entity/script order. Identical inputs produce bitwise-identical transforms on
the same build/platform (tested each tick); cross-platform bit equality is not
promised. Bodies are `static` (default), `dynamic`, or velocity-based `kinematic`.
Colliders can be `{"shape":"box","half_extents":[0.5,0.5,0.5]}`,
`{"shape":"sphere","radius":0.5}`, `{"shape":"capsule","radius":0.3,
"half_height":0.5}`, or `{"shape":"mesh"}` (also `mesh-from-entity-bounds`).
Omitted dimensions derive from scaled entity bounds. Mesh means an AABB-based
collider, **not** a triangle mesh; planes get a minimum 0.05 thickness. Capsule
axis is local Y; explicit dimensions are local units scaled by the entity.
A collider without a rigidbody is static. A rigidbody may have no collider.

`script` accepts a relative `.rhai` string, `{"path":"file.rhai"}`, or
`{"source":"..."}`. Paths resolve under project root, then `assets/`; absolute
paths and symlink escapes are rejected. Hooks are optional; top-level script
statements are not executed. Keep persistent values in the global state map:

```rhai
fn on_start(ctx) { ctx.state_set("score", 0); ctx.log("ready"); }
fn on_tick(ctx, dt) {
    let p = ctx.position();
    p[0] += ctx.input("move_x") * 2.0 * dt;
    ctx.set_position(p);
}
```

Context methods (vectors are three-number arrays; rotations are Euler degrees):
`id()`, `position()`, `rotation()`, `set_position(v)`, `set_rotation(v)`,
`apply_impulse(v)`, `set_velocity(v)`, `find_by_name(name)` (ID or -1),
`find_by_tag(tag)` (IDs), `entity_position(id)`, `input(action)` (number, default 0),
`spawn_entity(entity_map)` (new ID, primitive only; `spawn` is reserved by Rhai), `remove(id)`, `log(text)`,
`state_get(key)` (unit if absent), `state_set(key,value)` (JSON-compatible).
Spawn maps use the entity JSON format but ID/name default if omitted. Newly
spawned scripts start on the next tick. No modules/filesystem/time/eval APIs are
exposed. Rhai limits each hook to 50000 operations plus stack/string/collection
limits; simulation caps entities at 1024 and logs at 1024 entries. Errors report
script path (or inline label), line and message.

Blender: `ai_tags` accepts a string array/JSON array or comma-separated string;
`ai_collider` accepts e.g. `"box"` or JSON `{"shape":"sphere","radius":0.5}`;
`ai_rigidbody` accepts `"dynamic"` or JSON with `type`, `mass`, `friction`,
`restitution`; `ai_script` accepts a relative path or JSON `{"source":"..."}`.
Use JSON strings for nested properties in Blender. Sync maps these into typed
components while retaining the original `ai_*` properties.

### Shooter runtime

First-person mode (`ai_fps`), raycast/damage/FX Rhai APIs, viewmodel and HUD:
see [docs/SHOOTER.md](docs/SHOOTER.md) and `examples/fps_arena/`.

### Coin collector example

Load `examples/coin_game/scene.json`, then call `simulate` with the arguments in
`examples/coin_game/validation.json`: 180 ticks with `move_x=1` collect three
coins, set `score=3`, and move the player to the green goal. Assertions support:

```json
{"entity":"player","position_near":[3,0.45,0],"tolerance":0.05,"at_tick":180}
{"entity":"player","y_greater_than":0}
{"state":"score","equals":3}
{"entities_with_tag":"coin","count":0}
```

`at_tick` checks the state **after N steps** (0 = after `on_start`); omitted means
final tick. Position tolerance defaults to 0.5. Missing entities fail with
`actual:null`; an empty assertion list passes. Set `screenshot:true` to append
a final 960×540 PNG image. GPU initialization errors are returned normally.
Re-render the example's initial scene with:

```bash
cargo run -- --demo docs/coin_game.png examples/coin_game/scene.json
```

![Coin collector](docs/coin_game.png)

## AI-QA (M4)

`get_screenshot` keeps its original camera and PNG output when no QA options
are supplied. For an inspection contact sheet:

```json
{"views":["current","front","back","left","right","top","iso"],"overlay_ids":true}
```

This returns one PNG in a near-square grid. Each tile has an embedded bitmap
view label; no system fonts are used. All requested cameras auto-fit visible
world-space mesh bounds. `current` preserves the current viewing direction but
reframes the scene; the other directions are +Z, -Z, -X, +X, +Y and isometric.
An empty scene uses unit bounds. Tile dimensions default to 480×320, configurable
with `width`/`height` (128–2048 × 96–2048; sheet limited to 20 megapixels).
`overlay_ids:true` alone preserves the authoring/live camera. Labels show entity
ID and up to 12 name characters at projected mesh centers, nudged to reduce
collisions. These are inspection labels, including occluded entities whose
centers project into the image, not depth-tested visibility measurements.
The bitmap font supports ASCII letters/digits, space, dash and underscore;
other characters render as `?`.

`validate_scene {}` returns read-only findings:

```json
{"severity":"warning","entity":2,"code":"floating",
 "message":"Bottom is 1.500 units above the closest support...",
 "suggested_fix":{"tool":"set_transform","arguments":{"id":2,"position":[0,0.5,0]}}}
```

Checks cover floating objects, heavy overlaps, missing GLB files, script
compilation (without executing hooks), camera-frustum exclusion, duplicate/empty
names, dynamic bodies without colliders, and zero/negative scale. Findings have
`error`, `warning` or `info` severity and `suggested_fix:null` where no suitable
tool exists (edit components/names/scripts in scene JSON or Blender).

Geometry checks use conservative world AABBs, not exact collision geometry;
review rotated/concave-model overlap warnings before applying fixes. Heavy
overlap means >30% of the smaller bounds volume, with >0.05 penetration on all
axes. Floating requires a horizontally overlapping support below the object;
the closest support is used, with a 0.1-unit/5%-height gap tolerance. Dynamic
bodies are allowed to fall. Intentional floating/trigger decorations can use
tags `floating` or `trigger`, or props `ai_floating:true` / `ai_trigger:true`.
Scripted entities **without colliders** also suppress floating/overlap warnings:
the coin-game coins deliberately use this policy. Physical checks ignore hidden
entities; asset/script/name/scale checks still apply. Frustum checks assume
16:9 and only flag bounds wholly beyond a clip plane. QA reads the active
simulation during play, otherwise authoring; it never changes scene/revision.

Unknown tool/primitive names and unknown entity names in `query`/simulation
assertions include a deterministic edit-distance “did you mean” hint when a
close candidate exists. An unknown query name is an error; an unknown assertion
entity remains a failed assertion with `actual:null` and an explanatory message.

![Coin-game inspection contact sheet](docs/contact_sheet.png)

Tests: `cargo test -p engine-tests --test ai_qa -- --nocapture`.

## Roadmap

### Gekacheltes Terrain (CPU)

`engine-terrain` erzeugt deterministische Ridged-fBm-Höhenfelder unabhängig pro
Kachel. `TerrainConfig::preset_44km2()` umfasst 44 km², standardmäßig 256 m pro
Kachel und 129 Randproben (26×26 Kacheln; letzte Kachel ragt über die Kartengrenze).
`terrain_generate {out_dir,size_m?:[x,z],tile_m?,samples?,height_scale?,seed?}`
schreibt `terrain.json` und Little-Endian-f32-Kacheln mit Normalen-Halo.
`terrain_import {heightmap,out_dir,...}` liest Graustufen-PNG16 oder Little-Endian
R16 (`width,height` erforderlich); Zeilen laufen von Minimum nach Maximum Z,
0–65535 entspricht 0–`height_scale` Metern. `terrain_info {path}` und
`terrain_sample {path,x,z}` lesen das Ergebnis. Alle Pfade bleiben im Projekt;
Ausgabeordner müssen neu/leer sein. Import resampelt bilinear auf das Kachelraster.

`blender/terrain_heightmap.py` erstellt ein Displace-Grid und bäckt PNG16 plus
MCP-Importargumente. Nach Sculpting erneut mit `--bake-active` ausführen;
Blender XY/Z entspricht Engine XZ/Y. Werte außerhalb des Höhenbereichs werden
geklammert. Beispiel: `blender -b --factory-startup --python
blender/terrain_heightmap.py -- --out assets/terrain.png`.

Szenen-Entity: `props.ai_terrain:"assets/map/terrain.json"`, optional
`props.ai_terrain_radius:512`. Native Rapier-Höhenfeld-Collider werden in stabiler
Reihenfolge um dynamische Körper/Player geladen und entfernt. Terrain unterstützt
Translation, keine Rotation/Skalierung. Meshes sind kachellokal, einschließlich
grenzübergreifender zentraler Normalendifferenzen. `WorldPos`/`FloatingOrigin`
stellen f64-Weltpositionen und gerasterte f32-Konvertierung bereit; bestehende
Szenen-/Simulationskoordinaten bleiben f32 (kein automatisches Runtime-Rebasing).
Web-Streaming/LOD ist nicht Teil dieser CPU-Pipeline.

Tests: `cargo test -p engine-tests --test terrain`; Messung bei Standardauflösung:
`cargo run --release -p engine-terrain --example generate`.

- [x] deterministic conflict resolution (`expected_revision` CAS + rollback)
- [x] glTF/GLB mesh import (`spawn_primitive kind:"gltf"` → `assets/*.glb` → GPU)
- [x] [deterministic tiled NPC navigation](docs/NAVIGATION.md): collider bake, incremental regions, A*, Rhai agents, MCP lint and QA overlays
- [ ] windowed mode + interactive camera (winit)
- [ ] richer primitive/CSG set (real boolean ops in `engine-geo`)
- [ ] multi-agent session support (named worlds / command queue per agent)
- [ ] textures + materials, HDR lighting
- [x] deterministic physics + sandboxed Rhai gameplay scripting + simulation assertions
