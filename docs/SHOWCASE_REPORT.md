# Waldwächter — gemessener Blender-/Swarm-Showcase

## Spiel und Umfang

Fünf goldene Relikte am Waldpfad bergen und drei Wächtern ausweichen.
Erwischt wird man innerhalb von 1,8 m; anschließend geht es zum Wald-Anker
zurück, **ohne** gesammelte Relikte zu verlieren. Ab dem dritten Relikt wandert
der Anker zum zuletzt geborgenen Relikt. 90 Ticks Schutz verhindern sofortige
Wiederholungsfänge. WASD/Pfeile oder Bildschirm-Joystick bewegen die
Forstläuferin; Aktion/Leertaste springt. Texte, Aufgaben und Sieg sind deutsch.

Das ist ein begrenzter, bewusst zugänglicher Low-Poly-Showcase, kein AAA-Nachweis.
Die sichere goldene Route ist für Touch geeignet; die Patrouillen bewachen den
nördlichen Waldstreifen. Man muss keinen zufälligen, frameabhängigen Kampf lösen.

## Gemessener Aufbau

Lokaler macOS-ARM64-Entwicklungsrechner, Blender 5.3; ein eigener HTTP-MCP-Server
mit Projektidentität und isoliertem Blender-Port. Das Rezept startet vier
**geskriptete Rollen**, keine vier unabhängig schlussfolgernden KI-Modelle.
Die Rollen arbeiten über echte separate MCP-Sessions, Task-Abhängigkeiten und
Fencing-Leases; Skriptzuweisung verwendet Entity-Locks und CAS, Übergaben lesen
den aktuellen Szenenstand. Der Task-/Szenenverlauf wird als lokales Messartefakt
gesichert. Wartezeit vor Abhängigkeiten ist Bestandteil der Laufzeit.

| Messung im erfolgreichen Rezeptlauf | Wert |
|---|---:|
| Laufzeit von Projekt-Init bis portabler Szene einschließlich drei nativen QA-Traces | **13,004 s** |
| Rollen / abgeschlossene Tasks / protokollierte MCP-Aufrufe | **4 / 4 / 73** |
| Bodenfläche | **512 × 512 m = 0,262144 km²** |
| Autoren-Entities einschließlich unsichtbarer Instanzwurzeln | **1.434** |
| Echte Blender-Collection-Instanzen | **700: 500 Tannen + 200 Felsen** |
| Prefab-Definitionen / LOD-Stufen je Landschaftsmodell | **2 / 3** |
| Echte gewichtete Armatures / Actions je Rig | **4 / idle, walk, jump** |
| Nav-Tiles / begehbare Quad-Polygone / Zellgröße | **16 / 15.876 / 4 m** |
| Gemeinsame Regionsgröße für Weltkonfiguration und Nav-Tiles | **128 m** |
| Portable Szene / editierbare Blender-Datei | **5.833.615 / 221.266 Bytes** |
| Eindeutige GLBs / zusätzliche Nav-Assetdatei | **14 / 1** |
| Größe dieser 15 Assetdateien zusammen | **1.105.725 Bytes** |

Seed `71023` erzeugt Tannen, `71024` Felsen; 100 Tannen rahmen den Spielpfad
dichter ein. Blender modelliert und exportiert die Geometrie tatsächlich.
Die sechs LOD-Meshes heißen `Tanne_LOD0/1/2` und `Fels_LOD0/1/2`. Die
Forstläuferin verwendet das vorhandene gewichtete Rig-Rezept; Wächter haben
eigene Armatures und rostrote Mäntel. Keine kopierte Standbild-Animation.

## Ablauf und Inhalts-Workarounds

1. **Artist:** echte Blender-Geometrie, Armatures/Actions, Collection-Instanzen,
   Custom Properties und `.blend` speichern; `blender_sync` importiert sie.
2. **Gameplay:** bestehende `nav_agent`-Komponenten als Szenendaten ergänzen,
   Namen eindeutig machen, Rhai über `write_script` versionieren. CAS und
   Locks bleiben aktiv; keine neue Engine-Komponente.
3. **Navigation:** der öffentliche HTTP-Pfad hat `nav_bake` in diesem Lauf mit
   `unknown tool: nav_bake` abgelehnt. Statt geschützte Engine-Dateien zu ändern,
   ruft das **Inhaltsrezept** `waldwaechter_bake` die vorhandene native Bake-API
   auf. Es prüft Wege zu allen fünf Relikten und lädt die gebackenen Szenendaten
   wieder per MCP/CAS. Ein separater `assets/forest.nav.json` bleibt erhalten.
4. **QA:** native Sieg-, Fang- und fortgeschrittene Checkpoint-Traces ausführen;
   danach portabel speichern. Das Inhaltsrezept kanonisiert GLBs einschließlich
   LOD-Verweisen nach SHA256, statt den bestehenden per-Entity-Bundler zu ändern.
   Alte lokale Zwischenstände bleiben für die private Checkpoint-Historie
   archiviert; das veröffentlichte Projekt enthält nur portable Assets und Quellen.

Keine Änderungen an Runtime, Spieler, Eingabe, Physik, Audio, Savegames,
Netcode, Streaming-Implementierung oder Renderingqualität. Gemeinsame Web-Dateien
erhalten nur die zusätzliche Export-/Link-Registrierung; `src/web.rs` braucht
keine Spiel-Sonderbehandlung. Die dedizierte Release-Prüfung ergänzt CI additiv.

## Reproduktion

Aus dem Repository mit installiertem Blender:

```sh
cargo build
cargo build --example waldwaechter_bake
python3 tools/waldwaechter/reproduce.py /NEUES/PROJEKT --work /NEUES/MESSVERZEICHNIS
cargo test --test waldwaechter -- --nocapture
cargo run -- --project examples/waldwaechter --export-web scene.json web
cargo run --example waldwaechter_parity > target/forest-native.json
```

`reproduce.py` verweigert existierende Projekt-/Messziele. Die vollständige
lokale Arbeitskopie darf zusätzliche per-Entity-Bundle-Dateien enthalten, weil
lokale Checkpoints sie referenzieren; das portable Projekt verwendet die
kanonischen Hash-Dateien. Die Blender-Quelle wird niemals durch GC ersetzt.
Bei Gameplay-Änderungen Rezept, `scripts/*.rhai` und eingefrorene Szenenquelle
gemeinsam aktualisieren. Der bestehende Web-Export dedupliziert erneut.

## Abnahme und Messgrenzen

Die drei reproduzierbaren Traces in `tools/waldwaechter/traces.json` prüfen:

| Trace | Nativer Swarm-QA-Endzustand |
|---|---|
| `forest_win` | 5 Relikte, Sieg, 0 Fänge; 2 erreichte Patrouillenwechsel |
| `forest_caught` | 0 Relikte, kein Sieg, genau 1 Fang, Start-Anker |
| `forest_checkpoint` | 3 Relikte, kein Sieg, genau 1 Fang, Anker bei x=24 m |

Die eigenständige native Referenz zeichnet alle 30 Ticks sowie am Ende
Spielzustand, Spieler-/Wächterpositionen, Animator-Zeiten/Crossfades und vollständige
Nav-Routen auf. Der Browser vergleicht diese gegen die **wirklich exportierte**
WASM-Datei (numerische Toleranz `2e-5`, keine ausgelassenen Schlüssel). Separate
UI-Fälle prüfen Start, Sieg, Pause/Neustart, fünf wiederhergestellte Relikte,
fehlenden horizontalen Überlauf sowie JavaScript-/HTTP-Fehler.

Desktop verwendet Tastatur. Chromium iPhone-Profil verwendet echte CDP-Touch-
Ereignisse am Bildschirm-Joystick. WebKit iPhone-/iPad-Profile verwenden echten
Touch zum Starten und einen vertrauenswürdigen **Maus-Pointer-Drag** am selben
Joystick für den Sieg — **keine Behauptung eines echten WebKit-Touch-Drags oder
physischen iOS-Geräts**. Die Render-Schleife wird für reproduzierbare Ticks mit
virtueller Uhr gefahren; das ist **kein FPS-, Akkulaufzeit- oder Gerätebudget**.

Weitere Grenzen:

Lokale Abschlussprüfung (lean Dev-/Test-Build, Debugsymbole und Incremental aus):
**231 Cargo-Tests bestanden, 0 fehlgeschlagen, 1 vorhandener Shooter-Langtest
ignoriert; 196 Node-Tests bestanden, 0 fehlgeschlagen.** Frische Exporte aller
bisherigen drei Spiele plus Material Studio bestanden die vollständigen
Chromium-/WebKit-Release-Checks. Waldwächter bestand zusätzlich alle drei
Paritätstraces mit insgesamt **47 Samples je Profil**, UI-Sieg und Neustarts auf
Chromium Desktop/iPhone sowie WebKit iPhone/iPad.

Nativer Sieg in Tick **584**. Im lokalen, parallel zu weiteren Checks laufenden
Dev-Build lag die gemessene Tick-p95 bei **0,998–1,199 ms**, das Maximum bei
**22,556 ms**, einschließlich kalter Pfadplanung; keine Hardware-/60-FPS-Zusage.
Die erste Screenshot-Aufnahme erfasste noch Streaming-Platzhalter. Die Prüfung
wartet nun ausdrücklich auf die vorhandene Asset-Queue, ohne Streaming-Code
zu ändern. Geladene Start- und Siegansichten wurden tatsächlich angesehen:
`/Users/devin/shots/showcase/{chromium-desktop-start,webkit-iPhone-13-start,
webkit-iPhone-13-win,webkit-iPad-Pro-11-win}.png`.

- Instanzen bleiben materialisiert. Prefab-native Runtime ist wie entschieden
  bis #34/#46/#48/#51 und Vertragsfreigabe verschoben; #51 ist inzwischen auf main.
- Navigation ist das bestehende konservative 2.5D-Quad-System, keine neue
  dreiecksgenaue Waldnavigation. Tannen/Felsen sind kollisionsfreie Dekoration;
  die drei Wächter patrouillieren auf dem gebackenen Boden im freien Streifen.
  Kein Crowd-Avoidance, kein Hindernis-Carving, kein Nav-Tile-Streaming.
- Lint im erfolgreichen Autorenlauf: **0 Fehler**, 676 `outside_camera` für
  den großen Wald und 36 `heavy_overlap` für bewusst überlappende Dekoration.
  Keine doppelten Namen nach dem Inhalts-Namensschritt.
- Native CPU-Skinning-Funktionalität stammt aus M16. Dieser Showcase erweitert
  weder Shader noch GPU-Skinning, Beleuchtung, Schatten oder Postprocessing.
- Die Messdauer ist ein lokaler warmer Rezeptlauf, keine allgemeine Agenten-
  Produktivitätsmessung. Modelle, Inferenzkosten und menschliche Arbeitszeit
  werden nicht als automatisiert gemessene Werte ausgegeben.
