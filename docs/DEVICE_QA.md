# M11 · Geräte-QA-Werkzeuge (keine physische Geräteabnahme)

Der opt-in QA-Begleiter ergänzt einen bestehenden Web-Export, ohne Player-,
Eingabe-, Audio-, Physik- oder Renderingdateien zu ändern. Er läuft im Browser
des Besitzers. Automatisierte Chromium-/WebKit-Tests sind **kein Nachweis eines
echten iPhones/iPads**. Hardware-Abnahme und Hör-/Darstellungsqualität bleiben offen.

## Paket und teilbarer Link

```sh
node tools/device_qa/package.mjs target/m9-site
python3 -m http.server 8080 --directory target/m9-site
```

Auf dem Gerät öffnen: `https://EIGENER-HOST/device-qa/?game=../coin_game/`.
Für die anderen Spiele `../sky_garden/` bzw. `../lantern_maze/` verwenden.
Der bestehende Pages-Workflow paketiert den Begleiter ebenfalls. Nach erfolgreichem
Deploy: [Coin-Geräte-QA](https://markwaldeis.github.io/ai-native-3d-engine/device-qa/?game=../coin_game/),
[Sky-Geräte-QA](https://markwaldeis.github.io/ai-native-3d-engine/device-qa/?game=../sky_garden/) und
[Laternen-Geräte-QA](https://markwaldeis.github.io/ai-native-3d-engine/device-qa/?game=../lantern_maze/).
Das Paket enthält nur vier statische Begleiter-Dateien unter `device-qa/`;
Spielverzeichnisse bleiben unverändert. Für reale Geräte einen vom Besitzer
kontrollierten HTTPS-Host bzw. ein vertrauenswürdiges LAN verwenden. Dieses
Werkzeug deployt nichts und öffnet keinen Engine-/MCP-Server im Netzwerk.
Gleiche Origin ist zwingend; externe URLs und Query-Tokens werden abgelehnt.
Ein bestehender CSP muss das gleich-originige Begleitermodul erlauben.

1. Seite neu laden; **„Recorder bereit“ abwarten**, oben **Spiel anzeigen** wählen,
   erst dann im Spiel starten. Der kleine **QA anzeigen**-Schalter öffnet die
   Aufnahme-Steuerung wieder; das Spiel behält die volle Viewportgröße.
2. Auf echtem Gerät die Selbstauskunft markieren und Modell/OS manuell notieren.
3. Checkliste unten durchführen; jedes Feld explizit mit bestanden/fehlgeschlagen
   bewerten. Standard ist **nicht geprüft**, nicht bestanden.
4. „Aufnahme beenden“ drücken. Das beendet nur die Aufzeichnung, nicht das Spiel.
5. JSON herunterladen oder über einen **weiteren bewussten Tap** teilen.
   Unterstützt der Browser keine Datei-Freigabe, bleibt der Download verfügbar.
   Vor dem Senden Inhalt prüfen; es gibt keinen automatischen Upload.

## Checkliste je Spiel / Gerät / OS

- Safari vollständig schließen, Seite kalt öffnen; Laden ohne Grafikfehler.
- Erster Tap auf „Spielen“ startet genau einmal, ohne zweiten Tap.
- Vollständiger Touch-Sieg: Sky Garden drei Kristalle/Portal, Coin drei Münzen/Ziel,
  Laternen-Labyrinth vier Lampen per Annäherung/Aktion, dann Tor/Ziel.
- Absturz/Rettung bei Sky/Coin, Pause ohne fortschreitende Simulation,
  Pause-Neustart und Sieg-Neustart; danach erneut Eingabe prüfen.
- Hoch-/Querformat während einer Sitzung: Bedienelemente erreichbar, kein
  Überlauf. Bei Bedarf getrennte Aufnahmen mit Notizen erstellen.
- Screenshot/Video von Fehler und Sieg separat mitsenden; JSON enthält keine Bilder.

## Bericht, Datenschutz und Grenzen

Version 1 speichert logische Tick-Eingaben, Load-/Restart- und aktive-Region-
Aufrufe, Gameplay-Proben alle 30 Ticks/bei Sieg und die letzte Probe. Animator-
und Nav-Zustände werden als diagnostische Prüfsummen gespeichert. Kein Abgreifen
roher Tastatureingaben, Formulardaten, Cookies oder lokaler Speicher. Browser-
Kennung, Viewport/DPR, Sprache, Spielpfad und manuelle Notizen sind enthalten;
Fehlertexte sind gekürzt, URLs darin ohne Host/Query/Fragment. Keine Zusage,
dass beliebige eigene Fehlermeldungen frei von persönlichen Daten sind.

Maximal 80.000 Simulationsaufrufe und 256 Lifecycle-/Fehlerereignisse. Bei Limit,
Recorder-/Runtimefehler oder Start nach Tick 1 ist der Trace explizit unvollständig.
Ein leerer Bericht beweist nichts. Die JSON-Fingerprints sind **keine Signaturen**
und kein Manipulationsschutz. Die Selbstauskunft „echtes Gerät“ ist nicht geprüft.
Das iframe behält die volle Viewportgröße; ein kleiner QA-Schalter liegt darüber.
Ein iframe ist trotzdem kein Top-Level-/Vollbildstart (z. B. bei Berechtigungen);
abschließend zusätzlich den direkten Spiel-Link ohne Begleiter testen.

Offline-Gegenprobe gegen **genau denselben archivierten Export**:

```sh
node tools/device_qa/replay.mjs device-qa.json target/m9-site/coin_game
```

Die Gegenprobe nutzt das exportierte WASM, nicht den nativen Simulator. Native/
WASM-Release-Parität bleibt eine separate bestehende Prüfung. Terrain-Streaming,
Netcode und andere nicht aufgezeichnete Runtime-APIs werden nicht unterstützt;
die drei genannten Demos sind der zugesicherte Scope. JSON und Export gemeinsam
archivieren: Scene-/Script-Fingerprints allein identifizieren nicht jede GLB-/WASM-
Byteänderung. Ein Trace ersetzt weder visuelle Kontrolle noch echte Geräte-QA.
