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

## Laufmodi

- `ai-engine` — MCP-stdio-Server (newline-delimited JSON-RPC 2.0, echte
  `initialize`/`tools/list`/`tools/call`-Methoden). In `mcp_config.json`
  eines KI-Clients als `command` eintragbar.
- `ai-engine --demo out.png` — rendert Mock-Szene, schreibt PNG, exit.

## Tools (MCP)

`get_scene_state, spawn_primitive (box|plane|sphere|cylinder|wall_with_door),
set_transform, set_color, remove_entity, set_camera, get_screenshot (base64 PNG),
save_scene, load_scene, list_assets, run_blender_script`

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
