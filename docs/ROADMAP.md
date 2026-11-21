# AI-native 3D Engine: Roadmap ab M7

Stand: **21. November 2026**, [`3ae0be5`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/3ae0be59ac8082d8b79bdd9d909af05815d3e658) · CI **7/7 grün**, einschließlich Windows, nativer/WASM-Parität und Pages.

## Status: M7–M10 abgeschlossen

| Meilenstein | Gelieferter Stand | Commit |
|---|---|---|
| M7 · Verlässliche Änderungen und Blender-Roundtrip | Unveränderliche Skriptbindungen, monotone IDs, Gameplay-Props-Roundtrip, striktes CAS und atomare Batches. | [`abdc608`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/abdc608) |
| M8 · Sichere Schwarm-Sitzungen | Loopback als Standard, expliziter authentifizierter Remote-Betrieb, Projektpfad-Grenzen sowie Task-Leases mit Übernahme und Fencing. | [`6ca2d2a`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/6ca2d2a) |
| M9 · Web-Release-Prüfung | Echte exportierte WASM-Player, JS-/Browser-CI, Sieg/Rettung/Neustart und native/WASM-Eingabetraces. | [`3701f39`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/3701f39) |
| M10 · Workflow-Nachweis | Laternen-Labyrinth aus frischem Projekt: Blender → MCP → Rhai → QA → Web, mit gemessenem Worker-Ausfall, Übernahme und Rollback. | [`47ed41e`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/47ed41e) |

**Einordnung:** Abgeschlossen sind die gelieferten Implementierungen und dokumentierten Workflow-Nachweise, nicht sämtliche langfristigen Qualitätsziele. Physische iPhone-/iPad-Abnahme aus M9 bleibt offen; WebKit-/Simulator-Prüfungen ersetzen sie nicht. M10 nutzte ein einzelnes KI-Modell mit interleavten Rollen und parallelen Skript-Workern, kein unabhängiges Multi-Modell-Team. Messwerte und Reproduktion: [M10-Bericht](M10_REPORT.md). Der integrierte Touch-Verifier berücksichtigt seit [`8b9983b`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/8b9983b) sparse WASM-Snapshots.

### Heutige Ergänzungen

| Bereich | Geliefert | Commit / Dokumentation |
|---|---|---|
| Persistenter Schwarmzustand | Atomare Projekt-Checkpoints für Autorenszene, Tasks/Leases, Journale und Locks mit Offline-Ablauf; projektgebundene Blender-Ports und Identitätsprüfung. | [`2fc9501`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/2fc9501), [M10-Nachtrag](M10_REPORT.md) |
| Large-World-Authoring | Hierarchische Prefabs mit Overrides/Propagation, deterministische Bulk-Platzierung, Blender-Collection-Instanzen, LODs, GLB-Deduplizierung und paginierte Queries samt 44-km²-Messung. | [`82b2d90`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/82b2d90), [Details](LARGE_WORLD_AUTHORING.md) |
| WebKit-CI und Review-Fixes | Verpflichtende iPhone-/iPad-WebKit-Release-Prüfungen, Coin-Touch-Toleranz/Kamera, robuste erste Aktivierung und Favicon; integriert und nach Rebase verifiziert. | [`e3b23b3`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/e3b23b3) |
| Charakteranimation | Echter Blender-Rig mit mehreren Actions, deterministische Animator-State-Machine/Rhai-Parameter, MCP-Lint sowie simulationsgetriebene Web-Skinning-Crossfades. | [`8028ee6`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/8028ee6), [Details](CHARACTER_ANIMATION.md) |
| NPC-Navigation | Konservatives 2.5D-Quad-Navmesh mit Region-Tiles und inkrementellem Bake, deterministisches A*/String-Pulling, Nav-Agent/Rhai, MCP-Lint/QA-Overlay und Laternen-Patrouille mit Paritätstrace. | [`417a43d`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/417a43d), [Details](NAVIGATION.md) |

Der Windows-Flake im Mock-Blender-Socket des Projekt-Recovery-Tests ist mit [`3ae0be5`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/3ae0be5) behoben. Das ist eine Test-Fixture-Korrektur, keine Änderung der Navigations- oder Renderinglogik.

## Vorschlag: nächste Meilensteine M11+

Priorisierungsvorschlag aus den dokumentierten Grenzen, **keine bereits zugesagten oder abgeschlossenen Features**. Renderingqualität bleibt ein eigener Arbeitsstrang.

| Vorschlag | Ausgangsgrenze und nächster Nachweis |
|---|---|
| **M11 · Physische Geräte als Release-Gate** | [Opt-in Geräte-QA-Begleiter und Checkliste](DEVICE_QA.md) als Werkzeuge vorhanden: teilbarer gleich-originiger Link, begrenzter Eingabetrace, lokaler JSON-Bericht, explizite Geräte-Selbstauskunft und WASM-Replay. **Physische Abnahme bleibt offen:** Auf echten iPhones/iPads Kaltstart, erste Aktivierung, Touch-Sieg, Pause/Neustart und Orientierung prüfen. |
| **M12 · Präzisere und dynamische Navigation** | Von konservativen Collider-Quads zu dreiecksgenauer Geometrie und direkter Heightfield-Anbindung; Crowd-Avoidance, dynamische Hindernisse und bedarfsgeladenes Tile-Streaming. Enge Durchgänge, Rampen und mehrere Agenten mit festen Budgets und nativer/WASM-Parität nachweisen ([Grenzen](NAVIGATION.md)). |
| **M13 · Play-State-Recovery** | Neben Autorenszene und Koordination auch laufenden Simulationszustand wiederherstellen; Vertrag für Tick, Skript-/Physikzustand, Animator und Nav-Routen definieren. Prozess-Kill/Resume mit anschließend gleichem Trace prüfen ([M10](M10_REPORT.md)). |
| **M14 · Verteilte Koordination** | Den heutigen projektlokalen Single-Writer um einen expliziten Mehrserver-Vertrag erweitern: dauerhafte Identitäten, konsistente Leases/Fencing und Konfliktbehandlung bei Partitionen. Zwei Server und einen Verbindungsabbruch testen; kein verteiltes Locking auf Basis bloß kopierter Checkpoints behaupten ([M10](M10_REPORT.md)). |
| **M15 · Prefab-native Laufzeit und skalierbare Persistenz** | Materialisierte flache Szenen um einen expliziten Runtime-Vertrag für geteilte Templates/Instanz-Overrides ergänzen; Queries indizieren, inkrementelle Checkpoints und sichere Asset-Garbage-Collection planen. Speicher, Tool-Latenz und Wiederanlauf auf dem 44-km²-Benchmark messen ([Large-World-Grenzen](LARGE_WORLD_AUTHORING.md)). |
| **M16 · Natives Skinning und Animationsausbau** | [Native CPU-Skinning-Wiedergabe und QA-Bounds](CHARACTER_ANIMATION.md#m16--native-posen-und-skinning) nutzen dieselben deterministischen Clip-/Blendzeiten; Posen/Vertices werden gegen three.js geprüft. Keine Änderungen an Material-/Licht-/Post-Qualität. **Weiter offen:** natives GPU-Skinning/Crowd-Budgets, Root Motion, IK/Retargeting und skinned LODs. |

## Historischer Ausgangspunkt und ursprünglicher Plan

Die folgenden Abschnitte halten den Review-Stand [`8cff236`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/8cff23639981b74dbcafa0dfecbc30fc03e875fa) und die damaligen Abnahmekriterien fest ([Review](https://app.devin.ai/sessions/2a09d353466842a2b5ab7b6ff04490d2)). Sie sind **keine aktuelle Liste offener Implementierungsarbeiten**; heutiger Status und verbleibende Grenzen stehen oben.

### Damals verifizierter Ausgangspunkt

- **Build:** macOS ARM64: Workspace-Build mit Lockfile, 53 Rust- und 4 JS-Tests bestanden. Echter Blender-Bridge-Test mit Blender 4.5.16 LTS; die GPU-Tests wurden ausgeführt. [Linux-/Windows-CI](https://github.com/MarkWaldeis/ai-native-3d-engine/actions/runs/44889733444) und [WASM-Export/Pages](https://github.com/MarkWaldeis/ai-native-3d-engine/actions/runs/44889733453) für denselben Commit grün.
- **Live:** [Himmelsgarten](https://markwaldeis.github.io/ai-native-3d-engine/sky_garden/) verbindet Sprünge, Kristalle und Portal zu einem klaren Ziel. [Münzenpfad](https://markwaldeis.github.io/ai-native-3d-engine/coin_game/) ist eine verständliche, kurze Sammeldemo, aber die Kugel kann ohne Rücksetzung über den sichtbaren Rand hinausrollen. Beide auf Desktop-Chrome bis zum Sieg gespielt; keine physischen Mobilgeräte geprüft.
- **Einordnung damals:** Die drei hohen Befunde wurden im verlinkten Review reproduziert und waren in diesem Ausgangsstand weiterhin erkennbar. Der fehlende Origin-Check war bereits behoben; konfigurierbares Netzwerk-Binding ohne Authentifizierung war noch offen.

## M7 · P0: Verlässliche Änderungen und Blender-Roundtrip

- **Skripte und Verlauf:** Szenenrevisionen referenzieren unveränderliche Skriptversionen. Eine Änderung bindet nur explizit bearbeitete Entities neu; eine bewusst gemeinsame Änderung muss alle betroffenen Entities sperren und im Journal nennen. Rollback restauriert auch das ausführbare Verhalten, nicht nur Dateipfade.
- **IDs:** Allokator beim Restore/Load konsistent halten; keine wiederverwendeten IDs für andere Objekte in derselben Historie. Doppelte IDs beim Laden ablehnen, bevor die aktive Szene geändert wird.
- **Blender:** Geometrie und Transform gehören Blender. Gameplay-Felder bekommen klare Schreibzuständigkeiten pro Feld; `blender_push_props` schreibt sie über stabile `ai_uid` zurück. Beim Sync bedeutet fehlend »nicht ändern«, explizit gelöscht dagegen »entfernen«. Konflikte melden statt still überschreiben.
- **CAS und Batch:** Ungültige Revisionswerte ablehnen. Atomare Szenenänderungen mit einer Batch-Revision und gemeinsamem Commit anbieten; irreversible Nebenwirkungen (z. B. beliebiger Blender-Code oder Dateiexport) nicht darin zulassen. Bestehenden Teilmodus klar kennzeichnen bzw. versioniert ablösen; README, Tool-Schema und Guide abgleichen.

**Abnahme:** Regressionstests für konkurrierende Skriptänderungen samt Lock/Journal/Rollback, Laden → Rollback → Spawn und Blender-Verschiebung nach Skriptbindung. Ein abgelehnter atomarer Batch ändert weder Szene noch Revision, Journal oder referenzierte Assets.

## M8 · P0/P1: Sichere und ausfallsichere Schwarm-Sitzungen

- **Sofort parallel zu M7:** HTTP standardmäßig auf Loopback begrenzen. Der neue Origin-Check ist bereits da, ersetzt aber weder Authentifizierung noch Autorisierung. Kein Netzwerkbetrieb mit frei aufrufbarem Blender-Python oder freiem Szenendateizugriff. Falls Remote-Betrieb nötig ist: explizite Freigabe, projektgebundene Dateizugriffe und Berechtigungen, authentifizierte Identität statt Vertrauen in `X-Agent-Id`.
- **Worker-Ausfälle:** Befristete Task-Claims, Verlängerung, Freigabe und Neuzuweisung. Abgelaufene Claims können keinen verspäteten Abschluss mehr melden; Übernahme und Abschluss sind im Journal nachvollziehbar.

**Abnahme:** Entfernte oder nicht berechtigte Clients können ohne Freigabe keine privilegierten Tools aufrufen. Ausfall eines Workers blockiert die abhängige Aufgabe nicht dauerhaft; nach Übernahme kann nur der neue Inhaber den Task abschließen.

## M9 · P1: Beide Web-Demos als Release-Prüfung

- **Münzenpfad:** Rücksetzen beim Verlassen der Spielfläche und Neustart aus dem Pause-Menü; grüne Zielmarke im Hochformat sichtbar machen oder die Richtung anzeigen.
- **Himmelsgarten:** Shader-Diagnose nullsicher machen; Kaltstarts und vollständigen Touch-Sieg auf echten iPhones/iPads prüfen. Hilfe und Joystick im Querformat trennen, Portal im Hochformat sichtbar halten.
- **CI/Pages:** Bestehende JS-Tests in CI aufnehmen und den echten exportierten WASM-Player im Browser prüfen: Laden, Eingaben, Sieg, Rettung/Neustart. Für denselben Eingabetrace die Sieg- und Checkpoint-Ergebnisse von nativer Simulation und WASM vergleichen.

**Abnahme:** Beide Demos erreichen auf Desktop und Mobile Safari Sieg und Neustart, ohne Reload aus einem Softlock. Echter Mobiltest ergänzt den automatisierten Pages-Check; ein HTTP-200 allein reicht nicht.

## M10 · P2: Nutzen gegenüber dem bisherigen Workflow belegen

Ein frisches Projekt aus einer Spielidee mit Standard-MCP-Clients bauen: Blender-Sync, parallele Bearbeitung samt Worker-Wechsel, KI-QA, reproduzierbare Simulation und Web-Release. Am gleichen Brief messen: Zeit bis zum spielbaren Export, manuelle Eingriffe, Konflikte und Dauer der Wiederherstellung. Native Fenster, volle PBR-Materialien oder mehr CSG erst priorisieren, wenn sie dabei einen konkreten Engpass beseitigen.

**Abnahme:** Ein unabhängiges Team kann den Export aus einem frischen Projekt reproduzieren und einen dokumentierten Ausfall ohne Datenverlust beheben.
