# M10: Laternen-Labyrinth — Workflow-Nachweis

## Ergebnis und ehrliche Versuchsanordnung

Aus `ai-engine init examples/lantern_maze --template empty` entstand ein kleines
3D-Spiel: Lumi folgt Wegsteinen durch ein Labyrinth, entzündet vier Laternen
durch **Annähern + Aktion**, öffnet dadurch das physisch versperrte Tor und
gewinnt beim Berühren des grünen Ausgangs. Deutsche Bedienung: Joystick und
**Anzünden**, alternativ WASD/Pfeile und Leertaste.

**Keine unabhängigen Multi-Modell-Agenten:** Ein reasoning KI-Modell entwarf und
steuerte die Rollen Lead, Blender-Artist, Gameplay und QA live und verschachtelt.
Jede Rolle verwendete eine eigene MCP-HTTP-Session/Agent-ID. Daneben liefen ein
separater, skriptgesteuerter QA-Beobachter und ein tatsächlich abstürzender
Workerprozess. Das entspricht der ausdrücklich freigegebenen Versuchsanordnung,
nicht einem Benchmark dreier unabhängig denkender Modelle. Der kleine
stdlib-Client spricht MCP JSON-RPC über Streamable HTTP; die Claude-Code-/Cursor-
Anwendungen selbst wurden hier nicht getestet.

Blender 5.3 modellierte Boden, Figur, sieben Mauern, vier Laternenfassungen mit
separaten dunklen Glaskörpern, Torrahmen, Gitter, Ziel und Wegsteine. 21 Entitäten
und ihre GLBs wurden über `blender_sync` importiert. Kein bestehendes Spieltemplate
und keine fertigen Demo-Meshes wurden verwendet. Quelle: `lantern_maze.blend` und
`tools/lantern_maze/artist.py`. Rhai wurde über `write_script` gebunden, später mit
`blender_push_props` zurückgespiegelt. Finale Szene und Assets sind projekt-relativ.

## Messung des live gesteuerten Durchlaufs

Start: **2026-11-21 11:44:19.609596 UTC**, unmittelbar vor dem frischen `init`.
Die Messung umfasst Rezept-/Skripterstellung durch die KI, Modellierung, QA,
Fehlerkorrekturen, Infrastrukturpflege und Export. Installation der bereits
vorhandenen Toolchain und die vorherige Abstimmung über die Modelltopologie
sind nicht enthalten. Prozessübergreifend wurden UTC/Wall-Clock-Zeitstempel
verwendet; lokale Request-Dauern sind separat erfasst.

| Messgröße | Ergebnis |
|---|---:|
| Erstes korrektes, anschließend per Browser bestätigtes Exportartefakt | **907,85 s / 15 min 08 s** |
| Bis Desktop- und iPhone-Touch-Abnahme abgeschlossen | **954,95 s / 15 min 55 s** |
| Manuelle Nutzereingriffe während dieses Messfensters | **0** |
| Vorab nötige Nutzerabstimmung | **1**: Ausführungsmodell freigegeben |
| Absichtlich provozierte Lock-/CAS-Konflikte | **2**: je einer |
| Absichtlich verworfene atomare Batches | **2**, ohne Szenen-/Revisionsänderung |
| Abgewiesener verspäteter Abschluss des alten Workers | **1** |
| Unerwarteter MCP-Fehler | **1**: alte Blender-Bridge ohne `push_props` |
| Worker-Ausfall bis fertiggestellte Übernahme | **47,68 s**, bei 2 s Lease-TTL |
| Fehlskript einspielen, per Assert erkennen, Rollback und Gegenprobe | **0,021 s** |
| Paralleler QA-Beobachter | **1.252 Beobachtungen**, Entitätsstände 0 → 2 → 21 |

Die 47,68 s enthalten die reale KI-Rollenwechsel-/Screenshot-Verzögerung. Sie
sind **nicht** die technische Mindestdauer der Lease. Die kurze Rollback-Zeit
ist dagegen eine lokale, automatisierte MCP-Sequenz, keine menschliche
Fehlerdiagnosezeit. Die vollständigen strukturierten Messdaten, Szenen- und
Taskjournale stehen in [`M10_EVIDENCE.json`](M10_EVIDENCE.json).

KI-seitige Korrekturen sind nicht als „null Arbeit“ versteckt: Der erste
Skriptentwurf verwendete beim Spawn eine flache Position, der Blender-Dateipfad
vertraute zunächst auf den Prozess-CWD, eine alte Bridge musste nach dem Sichern
neu gestartet werden, und bereits gefüllte Exportziele mussten vor einem neuen
Export entfernt werden. Ein früher Prototyp-Export wird wegen der falschen
Lampenpositionen ausdrücklich **nicht** als korrektes Ergebnis gezählt.

## Zusammenarbeit, Ausfall und Rücknahme

1. Taskboard mit Abhängigkeiten: Grundlage → Modellierung/Gameplay → QA;
   separater Beobachtertask. Claims mit TTL, explizite Lease bei Abschluss,
   Erneuerung von QA- und Beobachterlease. Alle sechs Tasks am Ende abgeschlossen,
   keine verbliebenen Entity-Locks.
2. Gameplay sperrte die Figur und schrieb Rhai. Währenddessen importierte der
   Artist ausschließlich neue Maze-Objekte; der Filter schloss die Figur aus.
   Ein absichtlicher Artist-Schreibzugriff auf die gesperrte Figur wurde abgewiesen.
   Ein alter CAS-Stand wurde ebenfalls abgewiesen.
3. Kamera und Torabschluss verwendeten atomare Batches. Gegenproben mit gültiger
   erster und ungültiger zweiter Operation wurden verworfen; Szene und Revision
   blieben identisch. Keine Blender-/Datei-Seiteneffekte wurden darin ausgeführt.
4. `Ausfall-Worker` claimte Task und Tor für zwei Sekunden, verschob das Tor als
   begonnene Teilarbeit und beendete sich wirklich mit **Exitcode 23**. Nach Ablauf
   übernahm `QA-Uebernahme`, erhielt Lease **2 statt 1**, sperrte das Tor erneut,
   richtete es atomar fertig aus und schloss ab. Der alte Worker konnte mit Lease 1
   nicht nachträglich abschließen. Die begonnene Szene blieb im Server erhalten.
5. Ein syntaktisch gültiges Fehlskript setzte `score=99` und `won=true`. Die
   gezielte QA-Gegenprobe schlug fehl. `rollback` auf Revision 7 stellte die
   eingefrorene ursprüngliche Skriptbindung wieder her; derselbe Assert bestand
   anschließend, ohne die ursprüngliche Datei neu einlesen zu müssen.

Der Beobachterprozess war während der live gesteuerten Arbeit aktiv und las
Journal, Szene, Validierung und kurze Simulationen. Die Engine selbst serialisiert
Tools; dies beweist **keine** parallelen Szenenmutationen. Es wurde auch kein
zeitgleiches Requestfenster mit einem Artist-`blender_exec` gemessen. Der Poller
meldete keine strukturellen Findings; das kurzlebige Fehlskript wurde durch die
gezielte QA-Sequenz erkannt, nicht nachweislich durch den Sekunden-Poller.

## Gefundene Reibung und kleine Engine-Fixes

- **Veraltete Blender-Bridge wurde still wiederverwendet.** Der laufende Prozess
  stammte aus einem älteren Durchlauf. `blender_start` akzeptierte seinen Ping;
  `push_props` scheiterte später mit `unknown command 'push_props'`. Der aktuelle
  Python-Quelltext unterstützte die Operation bereits. Nach Sichern und Neustart
  funktionierte derselbe Push. Fix: explizites Bridge-Protokoll im Ping und Prüfung
  beider Start-/Reuse-Pfade; alte Peers liefern jetzt eine sichere Speicher- und
  Neustart-Anweisung statt still weiterzuarbeiten. Kein fremder Prozess wird
  automatisch beendet. Unit- und TCP-Peer-Regressionstests decken gültige,
  fehlende, veraltete und falsch typisierte Antworten ab.
- **Browser kannte nur Münzen/Kristalle als Ziele und Burst-Auslöser.** Beide
  verwenden nun das vorhandene `ai_ui.collectible_tag`; Route-Preview ebenfalls.
  `objective_label` erlaubt „Nächste Laterne“. Der HUD-Zähler war bereits
  generisch und musste nicht auf vier umgeschrieben werden. JS-Regressionen
  prüfen benutzerdefinierte, leere und unbekannte Ziele sowie fremde Tags.
- **`init --template empty` empfahl eine nicht vorhandene Sky-Garden-Rezeptdatei.**
  Der Absatz erscheint jetzt nur beim entsprechenden Template; Regression im
  vorhandenen Initialisierungstest für alle drei Templates.
- **Skript-Spawn hat ein anderes Positionsschema als das Authoring-Tool.** Das
  Spiel und der Guide wurden korrigiert (`transform.position`). Positionsasserts
  für angezündete Lampen und Browser-Gegenproben verhindern einen reinen
  Zähler-„Erfolg“ mit Lichtkugeln am Weltursprung.
- **Blenders CWD ist nicht zwingend die neue Projektwurzel.** Das Rezept erhält
  jetzt einen expliziten Projektpfad. Der Reproduktionslauf verwendet zusätzlich
  einen eigenen Bridge-Port und schützt damit andere Blender-Szenen.

Größere offene Punkte: Der Richtungspfeil kennt Luftlinie, keine begehbaren
Maze-Wege (Wegsteine erklären den Verlauf). Unbekannte Entity-JSON-Felder können
weiterhin still ignoriert werden; die Spawn-/Tool-Schemata sollten langfristig
angeglichen oder strikter diagnostiziert werden. Task-/Lease-Journale sind
speicherresident: Worker-Recovery ist hier bewiesen, Server-Crash-Recovery nicht.
Die globale Standard-Bridge benötigt langfristig Projekt-/Sitzungsisolation;
außerdem garantiert ein externes SIGTERM des Servers keinen Child-Drop. Das
POSIX-Replay beendet deshalb seine eigene Prozessgruppe. Kein OS-Sandbox- oder
ungeprüfter Windows-Prozessbaum-Nachweis wird behauptet.

## Frischer, unabhängiger Rezept-Replay

Voraussetzungen: gebautes CLI aus diesem Checkout, Blender, Rust-WASM-Target und
passendes `wasm-bindgen` wie beim bisherigen Web-Export. Vom Repository aus:

```sh
cargo build
python3 tools/lantern_maze/reproduce.py /absoluter/pfad/neues-laternenprojekt
python3 -m http.server 8080 --directory /absoluter/pfad/neues-laternenprojekt/web
```

Das Ziel muss neu/leer sein. Das Rezept startet eigenes HTTP und Blender auf
eigenen Ports, initialisiert ein leeres Projekt, reproduziert Locks/CAS/Atomic,
Exit 23, Übernahme/Fencing und Rollback, prüft fünf native Fälle und exportiert
projektlokal. `--work <neues-verzeichnis>` legt den Evidenzpfad fest;
`--skip-native-shot` ist für Maschinen ohne Headless-GPU-Adapter verfügbar.
Die eigentliche Spiel-Logik/Modelle werden über MCP geschrieben/importiert.

Separat tatsächlich ausgeführt: neuer Ordner außerhalb des Repositorys,
**5,84 s** bis Ende des vorgefertigten Rezept-Replays, **2,04 s** Worker-Recovery.
**Das ist Warm-Cache-Reproduktion mit fertigen Rezepten, keine KI-Designzeit und
kein Ersatz für die 15-Minuten-Live-Messung.**

Für den vorhandenen Checkout: `cargo run -- --project examples/lantern_maze
--export-web scene.json web`. Das Exportziel muss neu/leer und innerhalb der
Projektwurzel sein. Pages und CI exportieren so und verschieben erst anschließend
das fertige Artefakt in die gemeinsame Site. Die Landingpage verlinkt das Spiel.

## Abnahme

- Fünf native Szenarien: Sieg, Annäherung ohne Aktion, gehaltene Aktion,
  Leerlauf, Aktion außerhalb der Reichweite. Vor der vierten Laterne bleibt das
  Gitter vorhanden; danach verschwindet es. Lampen bleiben an ihren Fassungen.
- Exportierte WASM-Laufzeit gegen native `simulate`: **Sieg bei Tick 685**;
  zusätzlich Negativtrace ohne Aktion. Je 25 Samples mit Score, Sieg,
  Torzustand und Rettungszähler stimmen exakt überein.
- Chromium Desktop: echte Tastatursteuerung; iPhone-Portrait-Emulation: echte
  CDP-Touches für Joystick und Anzünden. Vier Lampen, Tor, Ausgangssieg,
  Pause/Neustart und Win-Neustart bestanden, keine Browserfehler/Assetfehler.
- Die bisherigen Sky-Garden-/Coin-Win-, Rescue-, Pause- und Paritätsprüfungen
  blieben grün. JS: **10 bestanden, 0 fehlgeschlagen**.
- Vollständig `cargo test --workspace -- --nocapture`: **71 bestanden,
  0 fehlgeschlagen, 0 ignoriert, 0 SKIP-Hinweise**, 34 Suite-Zusammenfassungen.
  Lokal nur Debug-Symbole/Incremental-Cache zur Plattenentlastung abgeschaltet:
  `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`.
  Keine Tests/Assertions deaktiviert. Blender- und GPU-Tests liefen mit.

Screenshots wurden geöffnet und visuell geprüft: `~/shots/m10/` mit Desktop und
iPhone (`*-start.png`, `*-lit.png`, `*-open-gate.png`, `*-win.png`) sowie
`landing.png`. Bilder sind bewusst nicht versioniert. Rohlogs liegen beim
gemessenen Lauf in `~/m10-work/`, Replay-Evidenz in
`~/m10-reproduction-final-evidence/`. Vorhandene Cargo-Future-Compatibility-Warnung
zu `block v0.1.6` bleibt; sie ist kein fehlendes Projekt-Environment.

Geprüft auf macOS/Blender 5.3 und Chromium/SwiftShader. Keine physischen iPhones,
kein Safari-Test dieses neuen Spiels, keine Pages-Deployment-Abnahme und kein
Windows-/Linux-Replay. Ohne identischen Godot-/Unity-Vergleich wird **keine
vergleichende Beschleunigung** behauptet. Belegt sind der nachvollziehbare
Blender→MCP→Rhai→QA→WASM-Pfad und die dokumentierten Recovery-Garantien.
