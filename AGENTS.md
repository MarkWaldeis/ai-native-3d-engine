# ai-engine — AI-native 3D Game Engine (Prototyp v0.1)

Rust-Workspace mit strikter Crate-Trennung (siehe Masterplan). `shared/` ist
frozen — Contracts dort nur vom Lead-Agenten ändern.

## M6 · Himmelsgarten / HTTP-Schwarm

`cargo build`, dann `python3 tools/swarm_demo.py --out-dir target/sky-site/sky_garden`.
Vier Rollen, eigene X-Agent-Id, abhängige Tasks, Entity-Claims und Journal.
**Ersetzt die aktive Blender-Szene.** Exportziel neu/leer. Quelle: `.blend` in
`examples/sky_garden/`; Rezepte in `tools/sky_garden/`; Befunde in
`docs/SWARM_FINDINGS.md`, Ausführung in `docs/swarm_journal.json`.

- `write_script {ids,path,source,expected_revision?}`: Compile vor Dateischreiben;
  projekt-relative `.rhai`, existierender Elternordner, Locks/CAS/Play-Sperre/Journal.
- `save_scene {path,bundle_assets:true}`: GLBs nach `assets/` neben die Szene;
  Referenzen bleiben relativ zum Projekt-Root, nicht Szenenordner.
- Rhai `ctx.velocity()` / `ctx.grounded()` lesen den letzten Physikschritt;
  ohne Körper Nullvektor/false. `ai_lock_rotation:true` hält Physikkörper aufrecht.
- Player-Prop `ai_ui` (JSON-String oder Objekt): `title`, `mission`, `score_label`,
  `collectible_tag`, `action_label`, `intro_title`, `intro_text`, `win_text`,
  `badge`, `win_badge`, `hud_keys`, `sky_color`. `_`-State bleibt im HUD unsichtbar.
- `cargo test -p engine-tests --test swarm_game`: Writer, Grounding/Wandkontakt,
  portables Speichern, simulate-basierter Sprung/Sieg/Sturz-Test.

## Build (Windows, GNU-Toolchain)

Rust wurde als `stable-x86_64-pc-windows-gnu` installiert. Zwei Stolpersteine:

1. `windows-sys`/`libloading` Build-Skripte brauchen echte `dlltool`/`as` —
   Rusts gebündelte Self-contained-`dlltool.exe` kann keinen Assembler spawnen.
   Lösung: w64devkit-Binutils auf PATH:

   ```bash
   export PATH="$USERPROFILE/.cargo/bin:/c/Users/Mark Waldeis/tools/w64devkit/w64devkit/bin:$PATH"
   ```

2. Das Linken muss über Rusts Self-contained-GCC laufen (sonst fehlt
   `libgcc_eh.a`) — ist in `.cargo/config.toml` als `linker` gepinnt.

Danach: `cargo build` / `cargo build --release` / `cargo test --workspace`.

## Build (macOS / Linux)

## Terrain (CPU / native Physik)

`crates/engine-terrain`: deterministische unabhängige 256-m-Kacheln, PNG16/R16-
Import, Manifest + f32-Höhen mit Halo, lokale Meshes und f64-Floating-Origin-API.
44-km²-Preset: 26×26 Kacheln mit 129 Proben pro Kante. Randkacheln werden
aufgefüllt; Sample-API prüft die echte Kartengrenze. `shared/` bleibt unverändert.
MCP-Katalog/Dispatcher ergänzen Terrain lokal in `engine-mcp`, Pfade projektgebunden.
Tools: `terrain_generate`, `terrain_import`, `terrain_info`, `terrain_sample`.
`props.ai_terrain` referenziert das Manifest relativ zum Projektroot;
`ai_terrain_radius` (Default 512 m) begrenzt native Rapier-Collider um Player und
dynamische Körper. Nur Translation, keine Terrain-Rotation/Skalierung.
Floating-Origin ist eine explizite API, kein automatischer Umbau der f32-Szene.
Blender: `blender/terrain_heightmap.py`, nach Sculpting `--bake-active`;
PNG16 + `.import.json`, Blender XY/Z → Engine XZ/Y, R16 Little Endian.
Tests: `cargo test -p engine-tests --test terrain`; Release-Messung:
`cargo run --release -p engine-terrain --example generate`. Web-LOD siehe unten.

### Installation

Stable Rust via rustup installieren, dann `cargo build --workspace` und
`cargo test --workspace`. Bei frischer Installation ggf. `source "$HOME/.cargo/env"`.
Die Windows-GNU-Linker-Konfiguration ist target-spezifisch und wird hier nicht
benutzt. GPU-Screenshot-Tests melden `SKIP`, falls kein Renderer verfügbar ist.

## Web-Player / Pages (M5)

- Einmal installieren: `rustup target add wasm32-unknown-unknown` und
  `cargo install wasm-bindgen-cli --version 0.2.100 --locked`.
- Im Projektroot: `cargo run -- --export-web examples/coin_game/scene.json target/site/coin_game`.
  Oder MCP: `export_web {out_dir, scene?}` (`scene`: JSON oder Dateipfad).
  Ohne Szene: Authoring-Snapshot, unverändert und ohne GPU/Blender. Zielordner
  muss neu/leer sein. Zum erneuten Export einen anderen leeren Ordner wählen.
- `python3 -m http.server 8000 --directory target/site`; `/coin_game/` öffnen.
  Niemals `file://`. Alle JS-/WASM-/three.js- und GLB-Abhängigkeiten sind lokal;
  GLBs mit externen Ressourcen werden abgelehnt. Projekt-/assets-relative Rhai-
  Dateien werden gebündelt, Inline-Skripte bleiben inline. Dynamisch gespawnte
  Scripts müssen inline oder schon im Bundle enthalten sein.
- `crates/engine-web` ist ein wasm-bindgen-cdylib über **engine-runtime**.
  Keine JS-Spielregeln/Physik hinzufügen. WebGame: `load(scene_json,scripts_json)`
  und `tick(inputs_json)` → JSON (`transforms/state/logs/removed/spawned/tick`).
  In-Memory-Quellen umgehen nur das native Dateisystem; Rapier/Rhai sind identisch.
  Export speichert GLB-Lokalbounds in `props.web_mesh_bounds` für Bounds-Collider.
- WebGL2, three.js **0.160.1 vendort** mit GLTFLoader und Lizenz; kein WebGPU/CDN.
  Loader/Utils/Versionshinweis zusammen aktualisieren.
- Deutschsprachiger Touch-Joystick (move_x/z), Aktion (jump/action), WASD/Pfeile/
  Space, Player-Tag-Kamera (Fallback Szenenkamera), State-HUD, Pause/Neustart.
  Sieg-Overlay reagiert auf `state.won`/`gewonnen`; Zielprüfung im Rhai-Skript.
  iPhone-Vollbild-Fallback: „Teilen → Zum Home-Bildschirm“.
- `.github/workflows/pages.yml`: Landingpage + `/coin_game/`, Push `main` oder
  workflow_dispatch, deploy-pages. Repo Settings → Pages → GitHub Actions aktivieren.
- Checks: `cargo test --workspace -- --nocapture` (Export-Strukturtest benutzt
  minimales Fixture-WASM, braucht deshalb kein WASM-Tooling im nativen CI).
  Echter WASM-Build/Export plus Browser-Touch/Tastatur bis Score=3/„Gewonnen!“
  separat prüfen; Desktop, iPhone Hoch-/Querformat fotografieren und ansehen.
  Bei verfügbarem Simulator Mobile Safari auf iPhone **und** iPad öffnen;
  Screenshots nach `docs/web_*.png`. Headless-Chrome in GPU-losen macOS-VMs
  benötigt ggf. `--use-angle=swiftshader --enable-unsafe-swiftshader`.

## Web-Terrain (Streaming)

- Export bündelt jedes `ai_terrain`-Manifest einmal: `terrain/<n>/terrain.json`,
  `coarse.bin`, `tiles/<x>_<z>.bin`; Szenenpfade werden umgeschrieben. Proben
  inklusive Halo sind u16 Little Endian (12-Byte-Header `AITT`/`AITC`). Web-Physik
  nutzt quantisierte Höhen: Abweichung von nativen f32-Höhen ≤ Höhenbereich/65535;
  ohne Vollkachel dient das grobe Raster als Näherung.
- WASM: `set_terrain(path, manifest_json, coarse_bytes)` vor `load`, danach
  `terrain_tiles_wanted(128)` → JSON `[{path,x,z}]`,
  `add_terrain_tile(path,x,z,bytes)`, `terrain_height(x,z)`. Erste Physikticks
  warten auf benötigte Kacheln; danach Polling alle 0,5 s, maximal sechs Fetches.
  Rhai `ctx.terrain_height(x,z)` nutzt Weltkoordinaten, ohne Terrain 0.
- `player/terrain.js`: grobe Einzelkacheln, nahe Vollkacheln mit Stride-LOD
  1/2/4/8 + Hysterese, Halo-Normalen und vertikale Skirts gegen Risse.
  Höhen-/Neigungs-Splat (Sand/Gras/Erde/Fels), prozedurale Welt-UV-Details,
  Licht/Nebel. Lokale Vertexkoordinaten + Kachelursprung erhalten kamerarelative
  Präzision auch bei 6,4 km; kein automatischer Floating-Origin-Umbau.
- `window.__engineStats`: `terrainTilesLoaded`, `terrainTilesVisible`,
  `terrainTriangles`, `terrainPendingFetches`, `terrainBytesFetched`;
  `window.__aiTerrain`: `heightAt(x,z)`, `physicsHeightAt(x,z)`, `stats()`.
  Dekodierte JS-Kacheln: LRU maximal 96; Binärcache maximal 128.
- Optionales Visuals-API: `__aiEngineVisuals.setHeightSampler(heightAt)` wird
  bei Initialisierung und bei später verfügbarem API im Poll verbunden;
  Weltkoordinaten inklusive Entity-Offset, Vollkachel vor grobem Raster.
- Demo: `cargo run -p engine-terrain --example web_demo`; danach Ursprung/Ferne
  mit `cargo run -- --export-web target/terrain-demo-source/origin.json target/site/terrain_demo_origin`
  bzw. `far.json` → `target/site/terrain_demo_far` exportieren (Ziele neu/leer).
  HTTP: `python3 -m http.server 8000 --bind 127.0.0.1 --directory target/site`;
  `node tools/bench/terrain_check.mjs terrain_demo_origin terrain_demo_far`
  prüft Höhe, Requests, FPS, Konsole und Präzision; Bilder: `target/terrain_shots/`.
  Checks: `node --test tools/test_web_*.mjs`,
  `cargo test -p engine-tests --test terrain --test web_export`.

## Große Web-Welten / Streaming

- HLOD: `ai_world.hlod` (true), `hlod_distance` (1600 m, größer als
  `draw_distance`), `hlod_cluster` (4×4 Chunks), `hlod_min_radius` (1,5 m).
  Statische Fernobjekte werden mit Welttransforms/Farben zusammengeführt,
  GLBs durch lokale Bounds ersetzt. Ein Cluster-Aufbau pro Frame, gemeinsame
  Materialien und zusammenhängende Chunk-Indexbereiche sparen Draw Calls.
  Chunkweiser Wechsel mit Hysterese; bei asynchroner Nah-LOD bleibt der Proxy
  bis zur Bereitschaft erhalten. `hlod:false` erhält den bisherigen Renderpfad.
  Nebel/Kamera reichen bis `hlod_distance`; Stats/Bench: `hlodClusters`,
  `hlodDrawCalls`, `hlodTriangles`. Optionaler Occluder über `setHlodOcclusion`.

- `ai_world.occlusion` (Default true), `occlusion_margin` (Default 2 m): Terrain-
  Strahlproben blenden verdeckte Chunks und Terrain-Kacheln konservativ aus.
  Neun obere Boxpunkte, zwei verdeckte Ergebnisse vor Ausblenden, sofort sichtbar
  bei freier Sicht; gemeinsames Round-Robin-Budget von 96 Tests pro Frame.
  Stats: `occludedChunks`, `occludedTerrainTiles`, `occlusionTests` (auch Benchmark).

- `ai_graphics`: `time_of_day`, `foliage:{density,distance}`, `water:{level,color}`,
  `fog_density`; deprecated Alias `ai_render` überschreibt gleiche Felder.
  Vegetation/Wasser folgen der bestehenden Grafikqualität; Terrain-Sampler:
  `window.__aiEngineVisuals.setHeightSampler((x,z)=>y)`. Siehe docs/WEB_GRAPHICS.md.

- Optionales Player-Prop `ai_world` (Objekt oder JSON-String): `chunk_size` (128 m),
  `stream_radius` (512 m), `draw_distance` (480 m), `lod_distances` ([100,260] m),
  `hysteresis` (16 m), `instancing` (true). Automatisch aktiv ab >1000 Entities
  oder Koordinaten >1 km; kleine Release-Demos bleiben unverändert.
- Chunks werden aus dem Szenengraphen entfernt, statische Props pro Chunk gebündelt;
  reduzierte Primitive und gerenderte GLB-Billboards sparen ferne Dreiecke.
  Spielregeln/Physik bleiben vollständig in Rust/Rhai. WASM-Ticks liefern nur
  geänderte Transforms, `load` weiterhin alle.
- `sim_radius` (256 m), `sim_hysteresis` (32 m): ferne Scripts pausieren,
  dynamische/kinematische Rapier-Körper werden deaktiviert und zustandserhaltend
  reaktiviert. Statische Collider bleiben; Tags `player`/`always_active` laufen
  überall. Native Simulation ohne aktive Region bleibt unverändert.
- `?stats=1`: FPS, Draw Calls, Dreiecke, geladene Chunks und Instanzen;
  dieselben Werte stehen immer in `window.__engineStats`.
- `python3 tools/gen_large_world.py target/large_world.json` erzeugt die große
  Demo (nicht einchecken); danach regulär mit `--export-web` exportieren.

## Eingabe / Gamepad

Web-Player: WASD/Pfeile, Leertaste sowie Standard-Gamepads (linker Stick/D-Pad:
Bewegen, rechter Stick: Zielen, A: Aktion, RT: Feuer, X: Nachladen, L3: Sprint,
Start: Pause). A startet auch das sichtbare Start-/Neustart-Modal.
Player-Prop `ai_input` (Objekt/JSON): `bindings`, `deadzone` (0.15),
`look_sensitivity` (1 rad/s), `invert_y` (false). Footer „Steuerung“ pausiert,
erlaubt Neubelegung und speichert pro Spieltitel lokal. Touch-Steuerung wird bei
groben Zeigern oder erster Touch-Eingabe sichtbar. Schema/Details: `docs/INPUT.md`.
Checks: `node --test tools/test_web_*.mjs`, Web-Export und Release-Parität.

## Multiplayer / Netcode

- `engine-net`: 60-Hz-Autorität, `net_player`-Slots nach ID, eigene Inputs,
  Prediction/Rollback, Snapshot-Deltas und 100-ms-Interpolation; `shared/` frozen.
- MCP `net_simulate {clients:2,ticks:600,rtt_ms:120,jitter_ms:20,loss:0.05,seed:1,
  inputs:[[{tick:0,action:"move_x",value:1}],[]]}`: isoliert von Authoring/Play;
  optional `scene` als JSON oder projektgebundener Pfad. Positionsvergleich am
  gleichen Tick; `null` bedeutet kein Vergleich, nicht Konvergenz.
- Arena: `examples/net_arena/`; `cargo run -p engine-net --example loopback --
  --scene examples/net_arena/scene.json --rtt-ms 120 --jitter-ms 20 --loss 0.05`.
- Checks: `cargo test -p engine-net`; `cargo test -p engine-tests --test netcode`;
  `cargo build -p engine-net --target wasm32-unknown-unknown`. Doku: `docs/NETCODE.md`.
  Spawn-Replikation mit Definitionen; keine Auth/Verschlüsselung, kein UDP-Fragmentieren;
  Browser: `cargo run -- --export-web examples/net_arena/scene.json target/site/net_arena_ws`,
  `cargo run -- --net-serve examples/net_arena/scene.json --addr 127.0.0.1:7790`,
  `python3 -m http.server 8000 --bind 127.0.0.1 --directory target/site`;
  zwei Tabs `http://127.0.0.1:8000/net_arena_ws/?net=ws://127.0.0.1:7790`.
  NetGame rechnet in Rust, JS transportiert/rendert; HUD/Diagnose `window.__engineNet`.
  Nur vertrauenswürdiges Netz, kein TLS; ohne `?net` bleibt Singleplayer unverändert.

## Shooter / First Person

Player-Prop `ai_fps` schaltet den Web-Player auf Ego-Perspektive (Pointer Lock,
Touch-Look rechts, FEUER-Taste); Inputs `move_x/z`, `look_yaw/pitch` (absolut, rad),
`fire/reload/sprint/jump`. Rhai `raycast/damage/player_pos/emit_fx` plus KI-Helfer;
WASM-Tick liefert zusätzlich `fx`/`events`. JS rendert nur (fps.js, fx.js, hud.js).
Doku: `docs/SHOOTER.md`; Beispiel `examples/fps_arena`; Tests
`cargo test -p engine-tests --test shooter`, `node --test tools/test_web_fps.mjs`.

## Swarm-Server

`ai-engine --serve 127.0.0.1:7777`: JSON-RPC per `POST /mcp`, eine gemeinsame
Engine auf dediziertem Thread. Identität: Tool-Argument `agent` vor Header
`X-Agent-Id`, sonst `anonymous`. Nur vertrauenswürdige lokale Clients; keine Auth.
Tools: `claim_entities {ids, ttl_s?}` (Default 60s), `release_entities {ids}`,
`list_locks`, `get_changes_since {revision}`, `task_create {title, description?,
depends_on?}`, `task_claim {id}`, `task_complete {id, note?}`, `task_list {status?}`.
Fremde Locks verhindern Entity-Mutationen und globale Szenenersetzung;
Batch-Unterbefehle erben die Agent-ID. Journal hält 1024 Szenen-Mutationen,
Lock-/Task-Metadaten ändern keine Szenenrevision. Status: pending/claimed/completed.
Alle Koordinationsdaten sind flüchtig. HTTP-Tests: `cargo test -p engine-tests --test swarm_http`.

## Laufmodi (stdio / Demo)

- `ai-engine` — MCP-stdio-Server (newline-delimited JSON-RPC 2.0, echte
  `initialize`/`tools/list`/`tools/call`-Methoden). In `mcp_config.json`
  eines KI-Clients als `command` eintragbar.
- `ai-engine --demo out.png` — rendert Mock-Szene, schreibt PNG, exit.

## Tools (MCP)

`get_scene_state, spawn_primitive (box|plane|sphere|cylinder|wall_with_door|gltf),
set_transform, set_color, remove_entity, set_camera, get_screenshot (base64 PNG),
save_scene, load_scene, get_revision, rollback, list_assets, run_blender_script`

- Mutations-Tools akzeptieren optional `expected_revision` (optimistic
  concurrency, CAS in `World::apply_expected`) — Konflikt → `revision conflict`.
- `rollback {revision}` restauriert Szenenstand aus dem Snapshot-Ring (64).
- `spawn_primitive kind:"gltf" path:"x.glb"` lädt GLBs (relativ → `assets/`).
  Beim Blender-Export: `--factory-startup`-Szene enthält Default-Cube/Light/Cam
  — vor dem Export `select_all` + `delete` ausführen, sonst landet der Cube
  mit im GLB (siehe assets/suzanne.glb-Gen-Skript).
- `ai-engine --demo out.png [scene.json]` rendert auch gespeicherte Szenen.

Blender: `C:\Program Files\Blender Foundation\Blender 5.2\blender.exe` —
wird mit `--factory-startup` gestartet (siehe agent-memory `blender-52-headless`:
`-b` auf .blend allein hängt auf dieser Maschine).

## Blender-Bridge (M2)

macOS: `/Applications/Blender.app/Contents/MacOS/Blender`, alternativ `blender`
auf PATH. `blender_start` startet headless mit Bridge; eigener Prozess endet mit
Engine. Manuell: `blender -b --factory-startup --python blender/ai_engine_bridge.py`.
Interaktiv: Skript als Legacy-Add-on installieren/aktivieren oder im Text Editor
öffnen und Run Script. TCP nur localhost, Standard 9876; `AI_ENGINE_BLENDER_PORT`
in beiden Prozessen setzen. Beliebiger Python-Zugriff: ausschließlich vertraute Clients.
Alle bpy-Befehle laufen auf dem Hauptthread (Timer-Queue; headless gleicher Pump).

Tools: `blender_status`, `blender_start`, `blender_exec {code}` (stdout + optionale
JSON-Variable `result`), `blender_scene_info`, `blender_sync {objects?, prune?}`.
Sync übernimmt `ai_uid` → `source_uid`, `ai_*` → `props` und erhält Entity-IDs;
Locks/CAS/Journal gelten auch hier. `.blend` speichern, um UIDs zu persistieren.
Blender-Z-up → Engine-Y-up: `(x,y,z)` → `(x,z,-y)`, Mesh lokal, Transform genau einmal.
`prune:true` entfernt nur verschwundene Blender-Meshes; Filter löschen keine
weiter existierenden Objekte. GLBs in `assets/blender/` sind versioniert für Rollback.
Noch keine Texturen/Multi-Material-Darstellung; eine Grundfarbe pro Objekt.
E2E: `cargo test -p engine-tests --test blender_bridge -- --nocapture` (SKIP ohne Blender).
Demo: `blender/sync_demo.py`; Vergleichsbilder in `docs/blender_sync_demo.png`
und `docs/blender_sync_reference.png` (macOS; bestehender Windows-QA-Pfad bleibt gültig).

## Persistente Spielstände

`save_game {path?}` speichert die aktive Simulation; `load_game {path?|save?}`
startet sie nach `stop` aus der Autorenszene. Pfade: projektgebundene `.json`.
Version 1 speichert Entity-Deltas, Tick, Zustand, Flags, Inventar und Geschwindigkeiten.
Rhai: `flag_set/flag`, `inventory_add/remove/count`; fehlende Flags liefern `()`.
Beim Laden nur `on_load(ctx)`, nicht `on_start`; `base_changed` meldet Basisänderungen.
Web: F5/F9 oder `window.__aiSaveGame/__aiLoadGame`, lokal pro Spielpfad.

## Gameplay / M3

- `engine-runtime`: separate Rapier-0.22/Rhai-1.20.1-Simulation, 60 Hz,
  `enhanced-determinism`; identische Inputs → bitgleiche Transform-Werte auf
  gleichem Build/Plattform. Physik/Script-Reihenfolge nach Entity-ID sortiert.
- Entity-Felder mit serde-Defaults: `tags`, `collider`, `rigidbody`, `script`.
  Kollidertypen: box/sphere/capsule/mesh (Bounds, kein Dreiecksmesh).
  Blender `ai_tags`, `ai_collider`, `ai_rigidbody`, `ai_script` werden gemappt;
  verschachtelte Properties am einfachsten als JSON-String setzen.
- MCP `play`, `step`, `stop`, `get_sim_state`, `simulate`, `query`.
  Autorenszene/Revision bleiben unverändert. Während play keine Autoren-Edits;
  `query`/Screenshot lesen Simulation, `get_scene_state` bleibt Autorenszene.
  `simulate` startet immer frisch, verändert eine laufende play-Session nicht.
- Inputs `{tick,action,value}`: absolute Ticks, vor Hooks angewendet, persistent
  bis zum nächsten Wert. `at_tick:N` prüft nach N Schritten (0 nach on_start).
  Assertions und Kontext-API sind vollständig in README dokumentiert.
- Script-Hooks `on_start(ctx)`, `on_tick(ctx,dt)`; Pfade relativ Projekt/assets
  oder inline `{source:"..."}`. Keine Top-Level-Ausführung/Imports/Zeit/eval.
  50000 Operationen pro Hook, begrenzte Daten/Logs/Entities; Fehler mit Datei/Zeile.
  Script-Spawn nur Primitive. Zustand über `ctx.state_get/state_set`, nicht
  über Top-Level-Variablen. Nach Laufzeitfehler stop/play zum Neustart.
- Tests: `cargo test -p engine-tests --test gameplay -- --nocapture`.
  Beispiel: `examples/coin_game/{scene.json,validation.json,scripts/}`;
  `cargo run -- --demo docs/coin_game.png examples/coin_game/scene.json`.
  Beispiel-PNG auf macOS: `docs/coin_game.png` (Windows-QA-Pfad bleibt gültig).

## AI-QA / M4

- `get_screenshot {views:["current","front","back","left","right","top","iso"],
  overlay_ids:true}` → ein beschriftetes Kontaktblatt (480×320 pro Kachel,
  width/height optional). Ohne Optionen identisches bisheriges Rendering.
  Auto-Framing aller sichtbaren Mesh-Bounds; current behält Blickrichtung.
  Eingebetteter ASCII-Bitmapfont; IDs/Kurznamen an projizierten Mesh-Zentren,
  auch bei Verdeckung. Kein Font-/Display-Server nötig.
- `validate_scene {}` → Findings severity/entity/code/message/suggested_fix.
  Nur lesend, aktive Simulation oder Autorenszene. Bounds-basierte physische
  Heuristiken, Frustum 16:9; Fixes vor Anwendung prüfen. Compile-Checks führen
  keine Rhai-Hooks aus. Keine Collider-Dreiecks-/Occlusion-Analyse.
- Absichtlich schwebend: Tag `floating`/`trigger` oder Props
  `ai_floating:true`/`ai_trigger:true`. Scripted ohne Collider unterdrückt
  ebenfalls floating/overlap (coin_game). Dynamische Bodies dürfen fallen.
  Fehlende Assets/Scripts, Namen, ungültige Skalierung werden trotzdem geprüft.
- Edit-Distance-Hinweise bei unbekannten Tools, Primitiven und Entity-Namen
  (query/Assertions). Fehlgeschlagene Assertions bleiben Reports, keine Exceptions.
- Test: `cargo test -p engine-tests --test ai_qa -- --nocapture`.
  Sichtgeprüftes Beispiel-Kontaktblatt: `docs/contact_sheet.png`.

## Large-World-Benchmark

- Anleitung/Referenz: `docs/BENCHMARK.md`; Generator: `tools/bench/gen_large_world.py`.
  Standard: 6633×6633 m, 14998 Entities inklusive 44-km²-Heightfield,
  unverändert 10900 Props und 4096 statische Fundament-Collider unter dem Terrain.
  `MAX_ENTITIES` ist 32768, auch für Skript-Spawns; Dichte nicht heimlich reduzieren.
- Setup: `rustup target add wasm32-unknown-unknown`,
  `cargo install wasm-bindgen-cli --version 0.2.100 --locked`,
  `npm ci --prefix tools/bench`, `npx --prefix tools/bench playwright install chromium`.
- `bash tools/bench/run.sh --channel chromium --swiftshader --duration 20`:
  generiert Terrain unter `target/bench/terrain` neu, leert den Exportordner
  `target/bench/site/large_world`, exportiert und misst beide
  Profile. Exportziel muss neu/leer und projekt-relativ sein, nicht `/tmp/...`.
  System-Chrome: `--channel chrome`; Mobile ist Chromium-Emulation mit 4× CPU,
  kein Safari-Test. SwiftShader setzt `--use-angle=swiftshader`,
  `--enable-unsafe-swiftshader`, `--ignore-gpu-blocklist`.
- `BENCH_TERRAIN=0` misst die alte Boxboden-Baseline bei gleicher Prop-Dichte;
  `BENCH_OUT=target/bench/no-terrain` trennt die Ergebnisse. Standard nutzt
  native Höhen für Props und Rhai `ctx.terrain_height` für den Kameraflug.
  Terrain-Kacheln/Dreiecke/Fetch-Bytes stehen in JSON und Markdown.
- `?bench` aktiviert `window.__aiEngineBench` (`ready`, `marks`, `stats()`).
  Marks: `bootStart`, `sceneFetched`, `wasmInitialized`, `webGameLoaded`,
  `assetsReady`, `firstFrameAfterStart` (`ai-engine:`-Präfix in Performance API).
- Ergebnisse: `target/bench/results/{results.json,summary.md,desktop.png,mobile.png}`;
  Log: `target/bench/pipeline.log`. Baseline: `--baseline <results.json>`;
  Budgets: `tools/bench/budgets.json` sind Smoke-Untergrenzen, später verschärfen.
  `.github/workflows/bench.yml` lädt Artefakte auch bei Fehlern hoch.
- Checks: `npm --prefix tools/bench test`, `node --test tools/test_web_*.mjs`,
  `python3 -m unittest discover -s tools/bench -p 'test_*.py'` sowie Runtime-/Web-Export-Tests.

## Crates (Übersicht)

| Crate | Aufgabe |
|---|---|
| `shared` | Scene-Schema (Klartext-JSON), MeshData, MCP-Protokoll-Typen, Mocks |
| `crates/engine-core` | World/ECS-lite, CommandQueue, scene.json I/O |
| `crates/engine-render` | wgpu Headless-Renderer → RGBA/PNG/base64 |
| `crates/engine-mcp` | JSON-RPC-Framing + Tool-Dispatch (ToolHandler-Trait) |
| `crates/engine-geo` | Primitive-Meshes inkl. Wand-mit-Tür (CSG-lite) |
| `crates/engine-runtime` | Isolierte Rapier-Physik, Rhai-Hooks, Assertions/Query |
| `crates/asset-pipeline` | Blender-Runner (Timeout 120s), Asset-Watcher (mtime-Polling) |
| `crates/engine-tests` | E2E-Integrationstests über den MCP-Pfad |

Screenshots/QA-Bilder immer nach `C:\Users\Mark Waldeis\devin-screenshots\own-3d-game-engine\`.
