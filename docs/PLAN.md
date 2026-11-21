# Plan v2: KI-Swarm-Engine mit Blender-First-Pipeline

> Ersetzt die Roadmap aus [masterplan.md](masterplan.md) (v0.1 = erledigt).
> Leitfrage für jede Entscheidung: **Baut ein Team aus KI-Agenten damit ein
> 3D-Spiel schneller, sicherer und überprüfbarer als mit Godot/Unity?**

## 1. Warum eine KI sich für diese Engine entscheiden soll

| Problem mit Godot/Unity für KI | Antwort dieser Engine |
|---|---|
| Szenen/Editor sind für Menschen gebaut (Klicks, Inspector) | Alles ist ein typisiertes MCP-Tool + Klartext-JSON |
| Ein Agent = ein Editor; mehrere Agenten überschreiben sich | **Ein Engine-Server, viele Agenten**: Identität, Locks, Aufgabenbrett, Änderungs-Journal |
| KI sieht nicht, was sie gebaut hat | Headless-Screenshots, Mehrfach-Ansichten, ID-Overlay, Szenen-Lint |
| KI kann Gameplay nicht prüfen | Deterministische Simulation `simulate(ticks, inputs)` + Assertions per Tool |
| 3D-Modelle bauen ist mühsam | **Blender ist das Modellier-Werkzeug**, live per Bridge verbunden |
| Kompilieren dauert, Fehler sind kryptisch | Gameplay als Skript (Rhai) mit Hot-Reload, Fehler mit Zeilennummer + Hinweis |
| Ergebnis testen ist umständlich | Ein Tool exportiert einen Web-Player (iPhone/iPad-tauglich) |

## 2. Blender-First: die Quelle der Wahrheit

```
   Agenten ──MCP──► Engine-Server ──TCP-Bridge──► Blender (Add-on, live oder headless)
                         ▲                              │
                         └──── GLB + Manifest ◄─────────┘   blender_sync
```

* **Geometrie, Materialien, Platzierung** entstehen in Blender. Die Engine
  modelliert selbst nur Blockouts (Primitive) für schnelle Prototypen.
* **Gameplay-Daten** (Collider, Rigidbody, Skript, Tags) hängen als Custom
  Properties (`ai_*`) am Blender-Objekt → überleben den Roundtrip.
* **Stabile IDs**: jedes Blender-Objekt bekommt `ai_uid`; `blender_sync`
  aktualisiert vorhandene Entities statt sie neu anzulegen (keine kaputten
  Verweise in Skripten).
* **Bridge** `blender/ai_engine_bridge.py`: Blender-Add-on mit JSON-über-TCP
  (Port 9876). Läuft im offenen Blender (Mensch sieht live zu) *oder* wird
  von der Engine headless gestartet (`blender -b --factory-startup`).
* Tools: `blender_status`, `blender_exec` (Python), `blender_scene_info`,
  `blender_sync` (Blender → Engine), `blender_push_props` (Engine → Blender).

## 3. Swarm-Betrieb (mehrere KI-Agenten gleichzeitig)

* **Transport**: stdio (1 Client) **und** HTTP (`ai-engine --serve 127.0.0.1:7777`,
  MCP-JSON-RPC per `POST /mcp`) → beliebig viele Agenten an *einer* Welt.
* **Identität**: jeder Aufruf trägt `agent` (Argument oder Header
  `X-Agent-Id`); Journal und Locks wissen, wer was getan hat.
* **Ownership/Locks**: `claim_entities` / `release_entities` mit Ablaufzeit.
  Fremde gesperrte Entities → Fehler `locked by <agent>` statt Überschreiben.
* **Optimistic Concurrency** (schon da): `expected_revision`, `rollback`, `apply_batch`.
* **Änderungs-Journal**: `get_changes_since {revision}` → kompakte Diffs
  (wer, was, welche IDs). Agenten pollen nur Deltas statt ganzer Szenen.
* **Aufgabenbrett** in der Engine: `task_create / task_claim / task_complete /
  task_list` — Lead-Agent verteilt, Worker holen sich Aufgaben, Abhängigkeiten
  werden erzwungen.
* **Rollen-Empfehlung**: Lead (Plan, Aufgaben) · Blender-Artist · Level-Designer ·
  Gameplay-Skripter · QA/Kritiker (Screenshots, Lint, Simulation).

## 4. Spiel-Laufzeit

* **Komponenten** am Entity: `tags`, `collider`, `rigidbody`, `script`, `light`.
* **Physik**: `rapier3d`, fester Zeitschritt 60 Hz, deterministisch.
* **Skripte**: Rhai (`on_start`, `on_tick(dt)`, `on_input`), Hot-Reload, sandboxed.
* **Simulation als Test**: `simulate {ticks, inputs[], asserts[]}` → Report
  (z. B. „Spieler erreicht Ziel in < 600 Ticks“). Kern des KI-QA-Loops.
* `query {tag|name|near}` statt ganze Szene lesen.

## 5. KI-Sicht & QA

* `get_screenshot` mit `views: ["front","top","iso"]` (Kontaktbogen) und
  `overlay_ids` (Entity-IDs ins Bild) → Vision-Modelle können Objekte benennen.
* `validate_scene`: Lint (schwebende Objekte, Überlappungen, fehlende Assets,
  Objekte außerhalb der Kamera, unbenannte Entities) mit Fix-Vorschlägen.
* Fehlermeldungen immer mit gültigen Alternativen („unknown kind 'cube', did you mean 'box'?“).

## 6. Auslieferung

* `export_web` → statischer Web-Player (WASM + WebGL2/WebGPU, Touch-Steuerung)
  für GitHub Pages — direkt auf iPhone/iPad testbar.
* Native Binary (`ai-engine --play scene.json`) mit Fenster (winit).

## 7. Meilensteine (Reihenfolge der Umsetzung)

| # | Meilenstein | Prüfung (Definition of Done) |
|---|---|---|
| M0 | Build/Tests laufen auch auf macOS/Linux, CI grün | `cargo test --workspace` |
| M1 | Swarm-Server: HTTP-Transport, Agent-ID, Locks, Journal, Aufgabenbrett | E2E-Test: 3 Agenten parallel per HTTP, kein Verlust, Lock-Konflikt erkannt |
| M2 | Blender-Bridge + `blender_sync` mit stabilen IDs und `ai_*`-Properties | E2E-Test mit echtem Blender: Objekt bauen → sync → Screenshot; erneut sync = Update statt Duplikat |
| M3 | Komponenten, Physik (rapier), Rhai-Skripte, `simulate` mit Asserts | Test: Kugel fällt auf Boden und ruht; Skript bewegt Spieler zum Ziel |
| M4 | KI-QA: Mehrfach-Ansichten, ID-Overlay, `validate_scene`, `query` | Tests + Sichtprüfung der Bilder |
| M5 | Web-Player + `export_web`, Demo-Spiel auf GitHub Pages | Link läuft auf iPhone/iPad (Touch) |
| M6 | Demo: KI-Team baut ein kleines 3D-Spiel komplett über die Engine | Spielbarer Link + Aufzeichnung des Agenten-Journals |

Jeder Meilenstein: bauen → Tests → Screenshots ansehen → Prüf-Agent → Befunde fixen → push auf `main`.
