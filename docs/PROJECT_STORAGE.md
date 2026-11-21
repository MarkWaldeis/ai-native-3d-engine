# Inkrementelle Projekt-Checkpoints und Asset-GC

Projektlokale Authoring-/Swarm-Zustände bleiben Single-Writer-Daten in
`.ai-engine/`. Dies ist **kein** Spielstand-/Netcode-Format und keine verteilte
Koordination. Änderungen an Runtime, Physik oder Player sind dafür nicht nötig.

## Checkpoints

`state.json` ist jetzt ein versionierter Basis-Checkpoint (Format 2). Die nächste
Änderung eines alten Format-1-Projekts migriert es atomar; alte Engine-Versionen
verweigern Format 2 statt still einen veralteten Zustand zu laden. Vor einem
Downgrade den **gesamten** Metadatenordner sichern, nicht einzelne Dateien kopieren.

Zwischen Basis-Checkpoints stehen strukturelle JSON-Deltas in
`journal/<20-stellige Sequenz>.json`. Eine atomare `head.json` bestätigt den
zuletzt übernommenen Journal-Eintrag. Dateiinhalt wird vor Rename synchronisiert;
Verzeichnis-Sync wird auf Unix wie bisher best-effort versucht. Ein Crash vor
dem Head-Commit hinterlässt höchstens einen unbestätigten Eintrag, der ignoriert
und beim nächsten Schreiben ersetzt wird. Nach spätestens 16 Änderungen ersetzt
ein atomarer Basis-Checkpoint das Journal; erst danach werden alte Einträge
entfernt. Ein Crash während dieser Bereinigung ist unschädlich.

Sequenzlücken, fehlende bestätigte Einträge/Heads, ungültige Patches und
abweichende Hash-Ketten/Payload-Prüfsummen verweigern den Start. Geprüft werden
die exakten JSON-Payload-Bytes, nicht erneut geparste Fließkommazahlen. Die FNV-Prüfsumme
erkennt versehentliche Beschädigungen, ist **keine kryptografische Signatur**.
TTL bleibt absolute Wandzeit: Offline-Zeit verbraucht Leases und Locks weiterhin.
Rollback-Szenen werden beim Replay aus der aktuellen Szene kopiert, statt bei
jeder kleinen Mutation erneut im Journal gespeichert zu werden.

**Grenze:** Optimiert werden Schreibmenge und Wiederanlauf durch begrenzte
Journal-Länge. `World::checkpoint()` und der Vergleich arbeiten weiterhin mit
vollständigem JSON; der Cache hält den letzten JSON-Zustand im Speicher. CPU und
Heap für sehr große Snapshot-Ringe sind damit noch nicht inkrementell.
Power-Loss-/Filesystem-Garantien bleiben vom Betriebssystem abhängig.

## Asset-GC (offline, konservativ)

```sh
python3 tools/asset_gc.py /pfad/zum/projekt              # nur Bericht
python3 tools/asset_gc.py /pfad/zum/projekt --apply      # explizit entfernen
python3 -m unittest discover -s tools -p test_asset_gc.py
```

Nur GLBs unter `assets/` sind Löschkandidaten. Alle anderen Dateitypen,
insbesondere `.blend`, Blender-Backups und Python-Quellen, bleiben erhalten.
Die Referenzwurzeln sind **alle** Projekt-JSONs (Szenen, Prefab-Templates,
Savegame-Dateien, gespeicherte Rollback-Szenen, Basis und Journal), GLB-JSON,
Textquellen und Blender-Abhängigkeiten. Damit können historische Referenzen die
Freigabe eines alten Assets bis zur Kompaktierung verhindern. Es wird lieber
zu viel behalten: gleiche Dateinamen, Pfadfragmente und dynamische Rhai-Spawn-
Aufrufe können zusätzliche GLBs schützen.

Bei Blender-Quellen/Backups ermittelt ein separater Blender-Prozess native
externe Referenzen und Custom-Property-Strings, mit `--disable-autoexec` und
ohne Änderungen an der Datei oder einer laufenden Bridge. Ohne erfolgreiche
Inventur wird **nichts** gelöscht (`--blender /pfad/zur/binary` oder `BLENDER_BIN`).
Externe verlinkte Blender-Bibliotheken und Symlinks verweigern die Inventur.
Ungültiges JSON/GLB, ein aktiver Engine-Writer oder während der Inventur geänderte
Dateien verweigern ebenfalls die Bereinigung.

Die Löschfreigabe bezieht sich auf den deklarierten Projekt-Referenzgraphen;
beliebige zukünftige Dateinamenberechnungen in fremden Programmen sind nicht
statisch beweisbar. Vor `--apply` externe Autoren/Blender stoppen und Änderungen
sichern. Der lokale Writer-Lock schützt Engine-/GC-Zugriffe, nicht Editoren,
die diesen Lock ignorieren. Außerhalb des Projekts wird nie gelöscht.

## Noch blockiert

Prefab-native Laufzeit bleibt ausdrücklich verschoben, bis **#34, #46, #48 und
#51** integriert und die Scene-/Runtime-Verträge koordiniert sind. Prefabs bleiben
in diesem Block materialisiert; keine Format- oder Laufzeitbehauptung darüber.
