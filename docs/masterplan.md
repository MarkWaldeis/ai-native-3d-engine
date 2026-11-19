# Masterplan: AI-Native 3D Game Engine (Multi-Agent Architektur)

> **Status:** v0.1 umgesetzt und gepusht — Workspace, `/shared`-Contracts,
> alle 6 Crates und die E2E-Tests existieren wie hier beschrieben.
> Die tatsächlich ausgelieferte Tool-Oberfläche steht im
> [README](../README.md); einzelne Namen weichen minimal vom Plan ab
> (z. B. `get_screenshot` statt `capture_viewport_base64`, MCP-`tools/call`
> statt freistehender `McpCommand`-Methoden).

Dieser Plan ist speziell darauf ausgelegt, von einem **Team aus 5–6 parallelen KI-Agenten (Sub-Agents / Sessions)** ohne gegenseitige Blockaden oder Merge-Konflikte umgesetzt zu werden. Das Ziel ist ein lauffähiger Prototyp (v0.1 Vertical Slice), der für KI-Workflows effizienter, schneller und direkter steuerbar ist als Unity.

---

## 1. Kernarchitektur & Technologie-Stack

* **Programmiersprache:** Rust (höchste Performance, Speichersicherheit, ideal für parallele Workspaces via Cargo).
* **Grafik-API:** `wgpu` (plattformübergreifend: Vulkan, Metal, DX12, WebGPU).
* **Datenformat:** Klartext-ECS (`scene.json` / `.yaml`) statt binärer Szenen-Dateien.
* **KI-Schnittstelle:** Eingebauter **MCP-Server (Model Context Protocol)** via JSON-RPC / `stdio` oder `localhost`.
* **Asset-Pipeline:** Hybrid aus nativem CSG (Constructive Solid Geometry) für Sofort-Blockouts und Headless-Blender (`blender --background`) für komplexe `.glb`-Assets.

---

## 2. Verzeichnisstruktur (Rust Cargo Workspace)

Damit 6 Agenten gleichzeitig arbeiten können, wird das Projekt in isolierte Sub-Projekte (*Crates*) aufgeteilt. Jeder Agent besitzt genau einen Ordner:

```text
ai-engine/
├── Cargo.toml                  # Workspace-Konfiguration (Lead-Agent)
├── shared/                     # Unveränderliche Schnittstellen & Datenmodelle (Phase 0)
│   └── src/
│       ├── lib.rs              # Zentrale Exporte
│       ├── scene.rs            # JSON-Schema für Entities, Transforms & Komponenten
│       ├── mcp_protocol.rs     # Definition aller KI-Befehle & Responses
│       └── mocks.rs            # Dummy-Szenen für isoliertes Testen
├── crates/
│   ├── engine-core/            # [Agent 1] ECS, Game-Loop, State-Management
│   ├── engine-render/          # [Agent 2] wgpu-Renderer, Kamera, Framebuffer-Capture
│   ├── engine-mcp/             # [Agent 3] MCP-Server & Event-Dispatcher
│   ├── engine-geo/             # [Agent 4] Prozedurale Meshes & CSG-Generator
│   ├── asset-pipeline/         # [Agent 5] File-Watcher & Headless Blender Runner
│   └── engine-tests/           # [Agent 6] Headless-Runner & End-to-End Integrationstests
└── src/
    └── main.rs                 # Zusammenführung aller Crates (Phase 3)
```

---

## 3. Aufgabenverteilung für das Multi-Agent-Team

### Phase 0: Fundament (Lead-Agent – ca. 30 Min.)
* Erstellt das Git-Repository, die `Cargo.toml` Workspace-Datei und den Ordner `/shared`.
* Definiert die festen Datenstrukturen (`Entity`, `Transform`, `MeshData`, `McpCommand`, `McpResponse`).
* **Regel:** Nach Phase 0 darf `/shared` nicht mehr eigenmächtig von Sub-Agents verändert werden.

### Phase 1: Parallele Entwicklung (6 Sub-Agents gleichzeitig)

1. **Agent 1: Core & ECS (`crates/engine-core`)**
   * **Aufgabe:** Baut das Entity-Component-System, den deterministischen Fixed-Timestep-Loop und das Laden/Speichern von `scene.json`.
   * **Schnittstelle:** Stellt den Thread-sicheren `WorldState` bereit.
2. **Agent 2: Renderer (`crates/engine-render`)**
   * **Aufgabe:** Initialisiert `wgpu`, rendert Meshes mit einfachem PBR-Lighting und baut eine Funktion `capture_viewport_base64() -> String`, die den aktuellen Frame als PNG/Base64 zurückgibt.
   * **Schnittstelle:** Liest ausschließlich Daten aus `WorldState` und `MeshData`.
3. **Agent 3: MCP & API (`crates/engine-mcp`)**
   * **Aufgabe:** Implementiert den MCP-Server (JSON-RPC). Nimmt Befehle wie `spawn_entity`, `set_transform`, `run_blender_script` und `get_screenshot` entgegen.
   * **Schnittstelle:** Schreibt Befehle in eine `CommandQueue` für `engine-core` und ruft Bilder aus `engine-render` ab.
4. **Agent 4: Procedural & CSG (`crates/engine-geo`)**
   * **Aufgabe:** Schreibt Generatoren für Grundformen (Box, Sphere, Plane) und einfache Boolean-Operationen (z. B. Türöffnung in Wand schneiden), damit die KI sofort Räume bauen kann.
   * **Schnittstelle:** Gibt reine Vertex- und Index-Buffer (`MeshData`) zurück.
5. **Agent 5: Headless Blender (`crates/asset-pipeline`)**
   * **Aufgabe:** Überwacht `/assets` via File-Watcher (`notify`-Crate). Führt Python-Skripte über `blender --background --python` aus und triggert bei neuen `.glb`-Dateien ein Hot-Reload-Event.
   * **Schnittstelle:** Sendet `AssetReloaded`-Events an `engine-core`.
6. **Agent 6: QA & Test-Harness (`crates/engine-tests`)**
   * **Aufgabe:** Schreibt automatisierte Tests, die ohne echtes Fenster (Headless) laufen, KI-Befehle simulieren, die Roundtrip-Zeit (<100 ms) messen und Memory-Leaks abfangen.
   * **Schnittstelle:** Importiert alle Crates und prüft die Kompilierbarkeit des gesamten Workspaces.

---

## 4. Synchronisations-Regeln (System-Prompt für jeden Agenten)

* **Crate-Isolation:** Bearbeite *ausschließlich* Dateien in deinem zugewiesenen `crates/<name>`-Verzeichnis.
* **Mock-First:** Nutze die Dummy-Daten aus `shared/src/mocks.rs`, um dein Modul unabhängig vom Fortschritt der anderen Agenten zu entwickeln und zu testen.
* **Definition of Done:** Deine Session ist erst beendet, wenn `cargo test -p <dein-crate>` fehlerfrei durchläuft.

---

## 5. Vor- und Nachteile dieser Architektur

### Vorteile (Pros)
* **Maximale KI-Geschwindigkeit:** Keine binären Szenen-Dateien oder GUI-Klickwege. Die KI liest und schreibt den gesamten Spielzustand direkt als JSON.
* **Echte Parallelität:** Durch die strikte Trennung in Rust-Crates und vorab definierte `/shared`-Contracts können 6 KI-Sessions gleichzeitig coden, ohne sich gegenseitig Dateien zu überschreiben.
* **Visueller Feedback-Loop:** Durch den eingebauten Base64-Framebuffer-Export kann ein multimodales LLM (Vision) das Ergebnis seiner Befehle sofort selbst sehen, bewerten und korrigieren.
* **Zero-Lag Blender-Workflow:** Blender läuft nur im Hintergrund als Compiler für Python-Skripte. Die Engine bleibt schlank und lädt nur fertige `.glb`-Dateien in Millisekunden nach.

### Nachteile & Risiken (Cons)
* **Kein visueller Editor für Menschen am Anfang:** In v0.1 gibt es keine Drag-and-Drop-Oberfläche, keine Gizmos und keinen Inspector für menschliche Level-Designer. Alles läuft über Code, JSON und KI-Prompts.
* **Hoher Initialaufwand bei Basis-Features:** Dinge, die Unity out-of-the-box mitbringt (Physik, Animation-Trees, Partikelsysteme, Audio-Mixer), müssen selbst programmiert oder über Rust-Crates (`rapier3d`, `kira`) integriert werden.
* **Contract-Starrheit:** Wenn während der Entwicklung auffällt, dass eine Datenstruktur in `/shared` fehlt, müssen alle 6 Agenten kurz pausieren, während der Lead-Agent den Contract anpasst.

---

## 6. Integration in deinen KI-Workflow (So nutzt du es am Ende)

Sobald der Prototyp kompiliert, verbindest du die Engine wie folgt mit deinem KI-Setup (z. B. Claude Code, Devin, Cursor oder eigenen Agents):

1. **MCP-Registrierung:** Du trägst die kompilierte Engine-Binary (`./target/release/ai-engine`) in die `mcp_config.json` deines KI-Clients ein.
2. **Der Arbeits-Loop der KI:**
   * **Schritt 1 (Lesen):** Die KI ruft `get_scene_state()` auf und erhält das komplette `scene.json`.
   * **Schritt 2 (Bauen):** Die KI ruft `generate_csg_room(...)` für die Architektur und `run_blender_script(...)` für ein komplexes 3D-Prop auf.
   * **Schritt 3 (Prüfen):** Die KI ruft `get_screenshot()` auf, analysiert das gerenderte Bild auf Platzierungs- oder Lichtfehler und passt die Koordinaten über `set_transform()` an.
3. **Export / Runtime:** Da das Spiel auf einer sauberen `scene.json` und kompiliertem Rust-Code basiert, lässt sich der fertige Prototyp ohne Engine-Overhead als eigenständige, extrem performante `.exe` / Binary ausliefern.
