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
