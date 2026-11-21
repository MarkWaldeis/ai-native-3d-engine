# AI-native 3D Engine: Roadmap ab M7

Stand: [`8cff236`](https://github.com/MarkWaldeis/ai-native-3d-engine/commit/8cff23639981b74dbcafa0dfecbc30fc03e875fa) · [Review](https://app.devin.ai/sessions/2a09d353466842a2b5ab7b6ff04490d2)

**Empfehlung:** Erst verhindern, dass Agenten unbemerkt Gameplay oder Szenenzustand verlieren. Danach die veröffentlichte Blender-zu-Web-Pipeline reproduzierbar machen und erst dann in zusätzliche Engine-Funktionen investieren.

## Verifizierter Ausgangspunkt

- **Build:** macOS ARM64: Workspace-Build mit Lockfile, 53 Rust- und 4 JS-Tests bestanden. Echter Blender-Bridge-Test mit Blender 4.5.16 LTS; die GPU-Tests wurden ausgeführt. [Linux-/Windows-CI](https://github.com/MarkWaldeis/ai-native-3d-engine/actions/runs/44889733444) und [WASM-Export/Pages](https://github.com/MarkWaldeis/ai-native-3d-engine/actions/runs/44889733453) für denselben Commit grün.
- **Live:** [Himmelsgarten](https://markwaldeis.github.io/ai-native-3d-engine/sky_garden/) verbindet Sprünge, Kristalle und Portal zu einem klaren Ziel. [Münzenpfad](https://markwaldeis.github.io/ai-native-3d-engine/coin_game/) ist eine verständliche, kurze Sammeldemo, aber die Kugel kann ohne Rücksetzung über den sichtbaren Rand hinausrollen. Beide auf Desktop-Chrome bis zum Sieg gespielt; keine physischen Mobilgeräte geprüft.
- **Einordnung:** Die drei hohen Befunde wurden im verlinkten Review reproduziert und sind im aktuellen Code weiterhin erkennbar. Der fehlende Origin-Check aus dem Review ist behoben; konfigurierbares Netzwerk-Binding ohne Authentifizierung bleibt offen.

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
