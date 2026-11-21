# Laternen-Labyrinth

Lumi geht durch drei versetzte Mauergänge, entzündet vier Laternen durch
Annähern + Aktion und erreicht das erst danach geöffnete Tor. Joystick +
**Anzünden**, alternativ WASD/Pfeile + Leertaste. Wegsteine zeigen den begehbaren
Verlauf; der Zielpfeil zeigt nur Luftlinie, keine Pfadplanung.

## Authoring

Projektwurzel ist dieses Verzeichnis. `scene.json`, `scripts/`, `assets/` bleiben
projekt-relativ. .blend-Datei und Rezept erhalten. Das Repository-Rezept
`tools/lantern_maze/artist.py` per `blender_exec` mit explizitem `PROJECT_ROOT`
und `STAGE='scaffold'` oder `'maze'` ausführen. Nicht Blenders Prozess-CWD als
Projektpfad voraussetzen. `blender_sync` importiert Geometrie, `write_script`
bindet versioniertes Rhai. `blender_push_props` + Sync spiegeln Gameplay zurück.

Physik: Kugelradius 0,32, dynamisch, reibungsfrei/min; Mauern und Tor statisch.
Nur dunkle Glaskörper tragen `lantern`, die Fassungen bleiben beim Anzünden.
`Tor-Riegel` wird erst bei vier Lichtern entfernt. Skript-Spawns benötigen
`transform: #{position: [...]}`; das flache `position`-Argument gehört dagegen
zum MCP-Tool `spawn_primitive`.

## Koordination und Prüfung

Eigene MCP-Sessions/Agent-IDs; Task-Lease übernehmen und erneuern, Entity-Locks
beanspruchen, CAS verwenden, Änderungen über Journal lesen. Atomare Batches nur
für reversible Szenenoperationen; kein Blender/Save/Export im Atomic-Batch.

Alle Fälle in `validation.json` mit `simulate` ausführen, `validate_scene`,
Screenshots wirklich ansehen, dann `save_scene` mit `bundle_assets:true`.
Skriptdatei und eingefrorene Quelle in `scene.json` nach Gameplay-Änderungen
gemeinsam aktualisieren; nicht nur die Datei überschreiben.

## Export und Reproduktion

Aus dem Repository: `cargo run -- --project examples/lantern_maze --export-web
scene.json web` (Ziel neu/leer). `web/` per HTTP servieren.

Neubau: `python3 tools/lantern_maze/reproduce.py <neues-verzeichnis>`. Das Rezept
startet eigene HTTP-/Blender-Ports, einen parallelen QA-Beobachter und einen
tatsächlich sterbenden Worker. Lease-Übernahme, Fencing und Skript-Rollback sind
enthalten. **Skript-Replay, keine unabhängigen KI-Modelle.** Messgrenzen stehen in
`docs/M10_REPORT.md`.

Lokale MCP-Client-Konfigurationen aus `init` sind maschinengebunden und ignoriert;
beim Verschieben neu konfigurieren. Niemals Tokens oder Sessions committen.
