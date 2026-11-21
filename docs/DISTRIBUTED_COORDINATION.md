# M14 · Mehrserver-Vertrag für Authoring-Koordination

**Status: vorgeschlagener Vertrag und Abnahmekatalog, keine implementierte
Mehrserver-Unterstützung.** Der sichere Teilumfang dieses Blocks ist ausschließlich
Dokumentation. Backend, Wire-Format, Migration und API-Versionierung sind noch
offen. Es gibt weder einen neuen Servermodus noch neue MCP-Felder oder Garantien.

## Heutiger, geprüfter Umfang

`src/project_state.rs::Storage::open` hält einen Projekt-Dateilock bis zum Ende der
Engine-Instanz. `Storage::save/restore` schreibt/restauriert Autorenszene, Historie,
Task-Board, beide Journale, Leases und Locks atomar über einen lokalen Checkpoint.
TTL läuft während eines Neustarts weiter. `configuration` bindet die Identität an
den kanonischen Projektpfad; kopierte/verschobene Metadaten werden abgelehnt.
Die bestehenden Regressionen stehen in `tests/project_recovery.rs`.

Das ist **projektlokaler Single-Writer**, keine replizierte Authority. Für NFS,
getrennte Dateisysteme oder Partitionen sind daraus keine Lock-/Failover-Garantien
ableitbar. Ein kopierter Checkpoint ist eine Sicherung, kein zweiter zulässiger
Writer. Ein Gameplay-Netzwerkserver aus #40 ist ebenfalls keine Authority für
Swarm-Tasks oder Autorenänderungen.

## Anforderungen an eine spätere verteilte Authority

Diese Anforderungen betreffen nur Autorenmetadaten und Koordination, nicht
Simulation, Savegames, Netzwerk-Gameplay oder Renderzustand.

### Identität und dauerhafte Ordnung

- Eine weltweit eindeutige, dauerhafte Projektkennung muss unabhängig vom
  Mount-Pfad sein. Provisionierung und Übernahme bestehender Pfad-Identitäten
  brauchen eine explizite Migration; keine stille Umdeutung alter Checkpoints.
- Dauerhafte Worker-/Serverkennungen und eine neue Boot-Kennung pro Prozessstart
  unterscheiden Wiederanlauf von derselben laufenden Instanz. Authentifizierung
  ist separat erforderlich; frei behauptete Agentennamen sind keine Autorisierung.
- Die Authority speichert einen monotonen Leader-Epoch und monoton geordnete
  Lease-Fences. Ein Token umfasst mindestens Projekt, Epoch, Ressource, Fence und
  Besitzer. Token dürfen nach Neustart, Backup-Restore oder Failover nicht erneut
  gültig werden. Überlauf muss fehlschlagen, nie umbrechen.
- Autorenrevisionen, Task-Sequenzen, Journale und idempotente Request-Receipts
  müssen mit den zugehörigen Metadaten dauerhaft geordnet sein. Ein Restore darf
  alte Ergebnisse nicht als neue Operationen quittieren.

### Lease, Mutation und Quittierung

1. Claim/Verlängerung/Freigabe laufen über eine **linearisierbare** Authority.
   Eine lokale Checkpoint-Kopie oder nur eventual-consistent Replikation genügt
   nicht. Das Backend muss seine Commit-, Zeit- und Failover-Annahmen offenlegen.
2. Vor jeder Mutation prüft derselbe atomare Commit-Punkt Projektidentität,
   aktuellen Epoch/Fence/Besitzer, Lease-Gültigkeit, Ressourcenbereich und
   `expected_revision`. Nur zuvor am Frontend zu prüfen wäre ein TOCTOU-Fehler.
3. Ablauf wird durch die Authority entschieden, nicht durch Worker-Uhren oder
   lokal rekonstruierte Rest-TTLs. Failover darf einen alten Writer erst ersetzen,
   wenn dessen Rechte an allen betroffenen Commit-Punkten sicher gefencet sind;
   sonst bleibt Schreiben gesperrt. Unbelegte Clock-Annahmen sind ein Blocker.
4. Szene, Koordinationsänderung, Journal und Request-Receipt werden gemeinsam
   dauerhaft quittiert. Bei Timeout ist der Ausgang **unbekannt**, nicht automatisch
   fehlgeschlagen. Wiederholung derselben Request-ID liefert das gespeicherte
   Ergebnis ohne zweite Mutation oder Lease-Verlängerung. Dieselbe ID mit anderem
   Payload ist ein Konflikt.
5. Alle schreibenden Senken müssen Fences prüfen. Ein alter Leader darf weder
   über einen noch lebenden Blender-Prozess noch über Dateischreibzugriffe die
   Authority umgehen. Nicht fencebare Nebeneffekte bleiben im verteilten Modus
   gesperrt. Immutable Artefakte benötigen ein gesondert abgestimmtes, crashfestes
   Publikationsprotokoll; diese Spezifikation ändert keine Asset-/Blender-APIs.

### Partitionen und Konflikte

- Ohne nachweisbare Authority darf ein Server keine Mutation, Lease-Verlängerung
  oder Task-Übernahme erfolgreich quittieren. Lesekopien dürfen nur explizit als
  veraltet gekennzeichnet dienen. Weiterrechnen eines Workers ist kein Commit.
- Nach Wiederverbindung werden Resultate erneut gegen Revision und Fences
  geprüft. Kein stilles Last-Writer-Wins, kein Zusammenkopieren von Journalen.
  Getrennte Offline-Branches bräuchten einen eigenen Merge-Vertrag und gehören
  nicht zu diesem Umfang.
- Ein wiederkehrender alter Leader muss seine Rechte neu erlangen; alte Tokens
  dürfen auch bei noch nicht abgelaufener lokaler TTL keine Änderungen erlauben.
- Gleichzeitige Änderungen derselben Ressource ergeben einen sichtbaren
  CAS-/Lease-Konflikt. Atomare Batches dürfen nicht teilweise sichtbar werden.

## Abnahmekatalog — noch nicht ausgeführt

Ein späterer Implementierungsblock muss **zwei echte Serverprozesse mit demselben
Authority-Backend** prüfen, nicht zwei isolierte Projekte. Backendwahl und
Fehlerinjektion sind Voraussetzung; die heutige CI erfüllt diesen Katalog nicht.

| Fall | Erforderlicher Nachweis |
|---|---|
| Neues, leeres Projekt; gleichzeitiger Bootstrap | Genau eine Projektidentität/Authority; keine unabhängigen lokalen Fallbacks. |
| Zwei Claims derselben Task/Ressource | Genau ein gültiger Besitzer; eindeutige Fences; konsistente Journale. |
| Commit gegen parallel veraltete Revision | Ein Erfolg, ein sichtbarer Konflikt; kein Teil-Batch. |
| ACK geht nach durablem Commit verloren | Retry mit derselben Request-ID liefert dasselbe Ergebnis, ohne Doppelwirkung. |
| Prozessabbruch vor durablem Commit | Keine Erfolgsquittung, keine halbe Szene/Koordination; Receipt klärt den Ausgang. |
| Leader verliert Authority-Verbindung | Keine weiteren erfolgreichen Writes/Verlängerungen; verbleibende TTL ist kein Ersatz. |
| Failover während ein alter Worker arbeitet | Neuer Epoch/Fence wird dauerhaft; alter Commit wird an jeder Senke abgelehnt. |
| Wiederverbindung des alten Leaders | Kein Überschreiben durch alten Checkpoint; neuer Claim erforderlich. |
| Uhrsprung, lange Pause, Ablauf während Ausfall | Kein vorzeitiger Doppelbesitz; unsichere Zeitannahmen führen zu Schreibstopp. |
| Backup-Kopie startet als weiterer Writer | Keine automatische Promotion oder Wiederverwendung gültiger Tokens. |
| Ungültige Identität/Token/Request-ID-Payload | Explizite Ablehnung ohne Mutation oder TTL-Verlängerung. |

Ausführbare Zwei-Server-/Partitionstests und eine produktive verteilte Authority
bleiben **offen**. Lokale Recovery-Tests oder grüne allgemeine CI sind kein Beleg
für diese noch nicht implementierten Eigenschaften.

## Abgrenzung zu M13 und parallelen Arbeiten

Vor diesem Block wurden #32–#40 erneut geprüft; die betreffenden Dateien und
Domänen bleiben unangetastet. M14 liefert nur den unabhängigen Swarm-Vertrag,
keine Änderungen an `engine-net`, Runtime, MCP, Shared-Schemas oder Persistenz.

M13 wird in diesem Durchlauf **nicht implementiert**: #33 besitzt persistente
Savegame-APIs (`engine-runtime/src/savegame.rs`, `src/runtime.rs`), #40 den
vollständigen In-Memory-Rollbackzustand (`engine-runtime/src/netcode.rs` und
Physik). Der verbleibende exakte Prozess-Kill/Resume-Pfad würde diese Zuständigkeiten
und `src/lib.rs` berühren. `Engine::from_project` restauriert derzeit nur World und
Swarm und startet mit `simulation: None`; das ist keine automatische
Play-State-Recovery. Vorhandene Savegames/Rollback werden weder dupliziert noch
als vollständige M13-Abnahme ausgegeben.
