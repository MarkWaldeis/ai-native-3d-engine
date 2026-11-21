# ai-engine — AI-native 3D Game Engine (Prototyp v0.1)

Rust-Workspace mit strikter Crate-Trennung (siehe Masterplan). `shared/` ist
frozen — Contracts dort nur vom Lead-Agenten ändern.

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

## Crates

| Crate | Aufgabe |
|---|---|
| `shared` | Scene-Schema (Klartext-JSON), MeshData, MCP-Protokoll-Typen, Mocks |
| `crates/engine-core` | World/ECS-lite, CommandQueue, scene.json I/O |
| `crates/engine-render` | wgpu Headless-Renderer → RGBA/PNG/base64 |
| `crates/engine-mcp` | JSON-RPC-Framing + Tool-Dispatch (ToolHandler-Trait) |
| `crates/engine-geo` | Primitive-Meshes inkl. Wand-mit-Tür (CSG-lite) |
| `crates/asset-pipeline` | Blender-Runner (Timeout 120s), Asset-Watcher (mtime-Polling) |
| `crates/engine-tests` | E2E-Integrationstests über den MCP-Pfad |

Screenshots/QA-Bilder immer nach `C:\Users\Mark Waldeis\devin-screenshots\own-3d-game-engine\`.
