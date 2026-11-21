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

## Mobile-Politur: Ursache und Abnahme

- Native Workspace-Tests: 44 bestanden, inklusive Blender/GPU und beiden
  Himmelsgarten-Simulationen. Desktop-Browser: drei Sprünge, Sieg, Neustart und
  Sturz/Rettung geprüft. Safari auf iPhone- und iPad-Simulator gestartet und gerendert.
- **Ursache mit Tick-/Eingabetrace:** `mobile_input_trace_before.json` zeigt
  Sprung zwei bei Tick 105 / x=5.087, bereits am Ende der Bodengnade. Mit konstant
  3.5 m/s fliegt Milo über die nächste Landefläche: Tick 160 x=8.295/y=1.065,
  Tick 170 x=8.878/y=0.233, ohne erneuten Bodenkontakt. Danach hält der Joystick
  ihn bei x≈8.85 gegen die Seitenwand der letzten Insel. Gemittelte Reibung und
  pro Tick neu gesetzte Horizontalgeschwindigkeit bremsen das Fallen fast vollständig
  (Tick 400 y=-0.164; Tick 778 y=-0.482). Deshalb wird die Bodenbedingung für den
  dritten Tastendruck nie wieder wahr; im Trace fehlt kein versandter Touch.
  Die zweite Pointer-ID bleibt korrekt vom Joystick getrennt. Eingabeverlust als
  Hauptursache wurde damit widerlegt; Render-/CDP-Latenz kann den Absprung verspäten.
- **Physik-Fix:** optionale Entity-Prop `ai_friction_combine` akzeptiert
  `average` (Standard), `min`, `max`, `multiply`. Milo nutzt Reibung 0 plus `min`;
  Blender-Quelle, Artist-Rezept und portable Szene stimmen überein. Ein nativer
  Gegenversuch reproduziert die Wandfalle mit `average`, mit `min` fällt dieselbe
  Figur auch bei gedrücktem Joystick. Andere Materialien behalten ihr Verhalten.
- **Faire Steuerung:** leichte Beschleunigung, 3.2 m/s am Boden, 3.0 m/s mit
  Luftkontrolle, 10 Ticks Bodengnade und 180 ms Sprungpuffer. Schnelle Tastendrücke
  bleiben bis zum nächsten Simulationstick gespeichert; Pointer-Freigabe gilt nur
  für deren Besitzer. Die native Siegvalidierung läuft wegen der geringeren
  Geschwindigkeit 240 statt 210 Ticks; unverändert drei echte Sprünge und keine Rettung.
- **Sicht:** kamerarelative Browser-Eingaben; nähere, höhere Kamera mit mindestens
  6.1 m horizontalem Sichtfeld und richtungsabhängigem Vorlauf. Weicher Blob-Schatten
  folgt per Abwärts-Ray echten tragenden Flächen, nicht Wolken oder dem Spieler.
  Kamera, Eingabepuffer und Schatten gelten auch für Münzenpfad; dessen native
  Skriptgeschwindigkeit und Weltachsen-Simulationsschnittstelle bleiben unverändert.
- **Abgenommen:** Desktop-Tastatur, iPhone-Hoch-/Querformat und Münzenpfad-Touch
  erreichen `Gewonnen!`, einschließlich Neustart; Himmelsgarten außerdem Sturz/Rettung.
  Vorher-/Nachher-Traces liegen unter `docs/mobile_input_trace_*.json`.
  Safari-Simulatoren iPhone/iPad: Render-/Startprüfung, kein vollständiger Safari-Sieg.

Reproduzierbare Browserprüfung (Playwright-Modul und Chrome installiert):

```sh
cargo run -- --export-web examples/sky_garden/scene.json target/mobile-site/sky_garden
cargo run -- --export-web examples/coin_game/scene.json target/mobile-site/coin_game
python3 -m http.server 8769 --directory target/mobile-site
PLAYWRIGHT_MODULE=/pfad/zu/playwright/index.mjs node tools/verify_mobile.mjs
node --test tools/test_web_controls.mjs
```
