# M6 · Ein Schwarm baut den Himmelsgarten

Vier Threads, eigene HTTP-Identitäten, abhängige Tasks, Claims und Änderungsabfragen.
`swarm_journal.json` enthält Revisionen, Tasks und Aufrufe (Quelltexte als SHA-256).

## Reibung → Engine-Fix

- **Kein Bodenwissen:** Rhai `ctx.grounded()` verwendet aktive Rapier-Stützkontakte
  mit Aufwärtsnormale; Wandkontakte zählen nicht. `ctx.velocity()` erhält beim
  horizontalen Lenken die Fallgeschwindigkeit. Daten vom letzten Physikschritt;
  ohne Körper zunächst false / Nullvektor.
- **Kapsel kippt:** `ai_lock_rotation:true` sperrt die Körperrotation. Sechs Ticks
  Sprungkulanz und Tastendruck-Flanke bleiben normale Rhai-Spielregeln.
- **Skripter konnte keine Dateien schreiben:** `write_script {ids,path,source}`
  kompiliert vor dem Schreiben, hängt projektinterne `.rhai`-Dateien an IDs und
  beachtet Locks, CAS, Play-Sperre und Journal. Elternordner muss existieren;
  Pfadflucht/Symlinks werden abgewiesen. Dateiinhalt ist nicht Teil des Rollbacks.
- **Native QA verlor mehrfarbige Materialien:** GLB-Import übernimmt Basisfarben
  pro Primitive als Vertexfarben; Shader multipliziert Entity-Tönung. Blender-Sync
  setzt diese neutral, um Doppeltönung zu vermeiden. Primitive bleiben unverändert.
  Texturen und volle PBR-Beleuchtung bleiben beim Web-Renderer.
- **GLBs waren rechnergebunden:** `save_scene {bundle_assets:true}` kopiert nach
  `assets/` neben der Szene und speichert projekt-relative Referenzen, ohne die
  Authoring-Szene zu verändern. Native Simulation löst relative GLBs gegen den
  Projekt-Root auf; WASM nutzt weiterhin exportierte Bounds.
- **UI war auf Münzen festgelegt:** Player-Prop `ai_ui` konfiguriert Titel,
  Mission, Sammel-Tag, HUD-Schlüssel, Aktionsbeschriftung, Himmel und Siegtext.
  `_`-State bleibt im HUD unsichtbar; mobiles HUD bleibt einzeilig.

## Reproduzieren

```sh
cargo build
python3 tools/swarm_demo.py --out-dir target/sky-site/sky_garden
python3 -m http.server 8000 --directory target/sky-site
```

Ausgabe-Verzeichnis neu/leer; Blender installiert. **Der Artist ersetzt die aktive
Blender-Szene.** Quelle: `examples/sky_garden/sky_garden.blend`, Rezepte:
`tools/sky_garden/`. Modelle, Komponenten und Spielskripte entstehen ausschließlich
über HTTP-MCP. Lokale Python-Schreibvorgänge betreffen Verzeichnisse, QA-Eingaben,
Reports und vom MCP gelieferte PNGs. GLBs betten Materialien vollständig ein.

`validation.json`: drei echte Sprünge, Kristalle/Portalsieg sowie frische Simulation
mit Sturz/Checkpoint-Rettung. Pages exportiert beide Spiele; bei deaktivierten
Pages bleibt das Deployment absichtlich ein Fehler, bis Pages eingeschaltet wird.

## Prüfstand / offen

- Native Workspace-Tests: 44 bestanden, inklusive Blender/GPU und beiden
  Himmelsgarten-Simulationen. Desktop-Browser: drei Sprünge, Sieg, Neustart und
  Sturz/Rettung geprüft. Safari auf iPhone- und iPad-Simulator gestartet und gerendert.
- **Touch-Sieg noch nicht abgenommen:** echte CDP-Multitouch-Eingaben steuern den
  Joystick und lösen Sprünge aus; Sturz/Rettung funktioniert. Der automatisierte
  Lauf erreicht jedoch das Sieg-Overlay nicht zuverlässig (15-s-Timeout).
  Ein Diagnose-Lauf zeigte vor Sprung drei bereits x=8.85, y=-0.11 und nur zwei
  registrierte Sprünge. Vermutung: Eingabelatenz im SwiftShader-Test läuft am
  Inselrand vorbei; eine reine Testursache ist nicht bewiesen. Auch nach korrektem
  Erhalt des Joystick-Touches bleibt der Timeout offen. Keine iOS-Siegprüfung behauptet.
