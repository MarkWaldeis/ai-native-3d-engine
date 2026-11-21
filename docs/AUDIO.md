# Audio (Web)

Präsentation über Web Audio, keine Spielregeln oder Physik in JavaScript.
Der Listener folgt der **Kamera**, nicht dem Player. Rhai setzt State; Audio liest
State und Entity-Transforms aus den Simulationsergebnissen.

## Authoring

Alle folgenden Props akzeptieren ein Objekt oder einen JSON-String.

Player-Prop `ai_audio` (Defaults):

```json
{
  "master": 1, "sfx": 1, "music_volume": 1,
  "max_voices": 24, "stream_radius": 80, "hysteresis": 8,
  "memory_budget_mb": 32, "stream_threshold_s": 20,
  "music": {
    "crossfade_s": 2,
    "layers": [
      {"id":"base", "synth":"pulse", "volume":0.1, "when":"always"},
      {"id":"cave", "src":"sounds/cave.ogg", "volume":0.2,
       "when":{"key":"in_cave", "op":">=", "value":1}}
    ]
  }
}
```

Musik-Layer sind geloopt; alternativ zu `src` gibt es `synth`. `when` ist
`"always"` (auch Default) oder `{key,op,value}`. Operatoren:
`==`, `===`, `!=`, `<`, `<=`, `>`, `>=`; kein `eval` oder eigener JS-Code.
Alle Layer werden vorbereitet und bekommen eine gemeinsame Startzeit;
Gain-Ziele folgen dem neuesten State mit geglättetem Crossfade.

Entity-Prop `ai_sound`:

```json
{
  "src":"sounds/fountain.wav", "volume":1, "loop":true, "autoplay":true,
  "ref_distance":2, "max_distance":60, "rolloff":1,
  "cone":{"inner":360, "outer":360, "outer_gain":0}
}
```

Ohne Datei: `"synth":"wind"`, `water`, `hum`, `fire` oder `pulse`.
Die Synths verwenden Rauschbuffer/Oszillatoren. `stream:true` erzwingt
HTML-Media-Streaming. Panner: HRTF, inverses Distanzmodell; Kegelwinkel in Grad.
Position und Richtung folgen dem Entity-Transform, auch bei Spawn/Move/Remove.
`autoplay:false` deaktiviert den Emitter (kein separates Trigger-API).

Entity-Prop `ai_reverb_zone`:

```json
{"shape":"box", "size":[12,8,20], "preset":"cave", "wet":0.7, "fade":4}
```

Alternativ `shape:"sphere", radius:6`. Position/Skalierung kommen vom Entity;
Box-Größen sind volle Kantenlängen. Zonen sind achsenparallel (Rotation wird
nicht berücksichtigt); Kugeln nutzen den größten Skalierungsfaktor.
Presets: `room`, `hall`, `cave`, `outdoor`, `underwater`. `wet` ist 0–1
(Default 0.5), `fade` die innere Randüberblendung in Metern (Default 4).
Die Zone mit dem höchsten gewichteten Wet-Anteil gewinnt. Stereo-Impulse
werden pro Preset erst bei Bedarf erzeugt und wiederverwendet.
Alternativ enthält `ai_audio.reverb_zones` solche Objekte mit explizitem
`position:[x,y,z]` und `size`/`radius` (optional `name` für Stats).

## Export und Dateipfade

`ai_sound.src` und die Musik-Layer des Players werden beim Export nach
`audio/<n>.<ext>` kopiert und im Szenensnapshot umgeschrieben. Identischer
Dateiinhalt wird einmal kopiert, auch über verschiedene Quellpfade hinweg.
JSON-String-Props bleiben Strings. Pfade sind relativ zum **Projektroot**,
nicht zur Szenendatei. Erlaubt: ogg/mp3/wav/m4a/opus/flac/webm/aac.
HTTP(S), absolute Pfade, `..` und Symlinks außerhalb des Projekts werden
abgelehnt; fehlende Dateien melden einen Exportfehler. Dynamisch gespawnte
Datei-Emitter müssen einen bereits exportierten Pfad verwenden.
Codec-Unterstützung bleibt browserabhängig; Export transkodiert nicht.

## Streaming, Speicher und Lifecycle

- Emitter bekommen nur innerhalb des Radius Audio-Nodes; aktive Stimmen bleiben
  bis Radius + Hysterese. Bei Überbelegung gewinnen die nächsten Emitter.
  Musik-Layer liegen außerhalb des räumlichen Stimmenlimits.
- Virtuelle Stimmen behalten ihre Zeitbasis; decodierte Loops setzen mit
  passendem Offset fort. Entfernte Stimmen/Media-Elemente werden freigegeben.
- Metadaten bestimmen die Dauer **vor** dem vollständigen Decode. Dateien über
  dem Schwellwert, unbekannte/endlose Dauer oder `stream:true` verwenden
  `HTMLAudioElement` + `MediaElementSource`. Gleichzeitige Ladevorgänge werden
  zusammengefasst; Fehler werden einmal geloggt und der Emitter übersprungen.
- Decodierte Buffer-LRU: `length * channels * 4` Bytes. Nur unbenutzte Buffer
  werden verdrängt; aktive Buffer bleiben gepinnt und können das Budget
  vorübergehend überschreiten. Synth-/Impulsbuffer zählen nicht zum Datei-LRU.
- Kontext erst bei Nutzergeste. Pause/verborgener Tab suspendieren Audio und
  pausieren Streams. Mute wirkt am Master; Collect/Jump/Win bleiben erhalten.
  Ohne AudioContext bleibt das Spiel spielbar.

Grenzen: HTML-Media-Layer werden zeitlich angeglichen, sind aber nicht
samplegenau synchron (Start/Seek hängen vom Browser ab). Oszillator-/Rausch-
Synths können bei Reaktivierung eine neue Phase bekommen. Kein Occlusion-Ray-
Tracing, kein Doppler, keine nativen Audio-Backends. Hörqualität muss zusätzlich
mit echten Lautsprechern/Kopfhörern geprüft werden.

## Demo und Checks

`examples/audio_garden` enthält Brunnen, Feuer, Klangstein, WAV-Glocke,
Höhlen-/Hallenzonen und einen Rhai-gesteuerten Musik-Layer. WASD/Pfeile bewegen;
W führt vom Start geradeaus in die Höhle. Der Listener läuft mit der Kamera
hinterher, daher beginnt der Hall später als der Player-State.

```sh
python3 examples/audio_garden/generate_chime.py
cargo run -- --export-web examples/audio_garden/scene.json target/site/audio_garden
python3 -m http.server 8769 --bind 127.0.0.1 --directory target/site
node tools/verify_web_audio.mjs
```

Exportziel neu/leer. Für den Browsercheck einmal `npm ci --prefix tools/bench`;
Chrome wird auf macOS automatisch gefunden, sonst Playwright-Chromium installieren
oder `CHROME_PATH` setzen. `GAME_URL` überschreibt die komplette Demo-URL.
Screenshot: `docs/web_audio_garden.png`.

`window.__aiAudio = {stats(),engine}` und `window.__engineStats` liefern:
`audioVoices`, `audioVirtualVoices`, `audioBufferBytes`, `audioStreams`,
`audioReverbZone` (Name/Preset oder null), `musicLayers` (aktive Layer-IDs).
Tests: `node --test tools/test_web_*.mjs` und
`cargo test -p engine-tests --test web_export`.
