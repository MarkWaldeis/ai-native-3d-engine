# SDF-Text im Web-Player

Die Entity-Property `ai_text` ist ein Textstring oder ein Objekt. Nur die
Darstellung liegt in JavaScript; Werte und Spielregeln bleiben in Rust/Rhai.

```json
{"ai_text":{"text":"Münzen {score}/{total:coin}\nÜbrig: {left:coin}",
 "size":0.5,"color":"#fff","outline_color":"#000","outline_width":0.15,
 "billboard":true,"align":"center","max_width":4,"depth_test":true,"fade_distance":30}}
```

- `text`: Text, unterstützt `{key}`, `{key|0.0}`, `{total:tag}`, `{left:tag}`
  über dieselbe `hudTemplate`-Funktion wie das HUD. Unbekannte State-Werte: „–“.
- `size`: Schriftgröße in Weltmetern (Standard 0.5); Zeilenabstand 1.2 × Größe.
- `color` / `outline_color`: CSS-Farbe (Standard Weiß/Schwarz).
- `outline_width`: SDF-Randbreite 0–0.4 (Standard 0.15).
- `offset: [x,y,z]`: lokale Position; Standard in Welt-Y über der oberen
  Entity-Bounds plus 0.4 m (auch bei Rotation und nach GLB-Ladevorgängen).
- `billboard`: Standard `true`, folgt der Kamera um die Welt-Y-Achse, bleibt
  aufrecht; `false` übernimmt die feste Entity-Orientierung.
- `align`: `center` (Standard), `left`, `right`. Mehrzeiliger Block um Y=0;
  horizontale Ausrichtung relativ zum Label-Ursprung. Umbruch an Leerzeichen
  und `\n`, `max_width` in Metern (Standard 0 = kein automatischer Umbruch).
  Einzelne zu lange Wörter bleiben ungetrennt.
- `depth_test`: Standard `true`; `false` zeichnet auch durch Geometrie.
- `fade_distance`: Standard 0 = nie ausblenden; blendet über die letzten 20 %
  der angegebenen Entfernung weich aus.

Ungültige Werte erhalten Standardwerte und Warnungen im Log. Unterstützt die
bestehenden Mesh-Entity-Kinds. Terrain-/ersetzte Wasser-Entities erhalten kein
Label. Labels folgen Entities, verschwinden bei Remove und werden bei Neustart
oder Save-Restore entsorgt/neu erzeugt. Die Glyphen-Geometrie ändert sich nur,
wenn sich der aufgelöste Text ändert. Ein Label verwendet eine BufferGeometry
und ein transparentes, nebelunabhängiges ShaderMaterial ohne Depth-Write.

Keine Schriftdateien/CDNs: System-`sans-serif`, Canvas-Rasterung mit 48 px,
8 px Rand und SDF-Radius 8. Zwei euklidische Distanztransformationen erzeugen
den 8-Bit-SDF. Ein 1024²-Rotkanal-Atlas wächst durch neue Glyphen; bei Überlauf
wird einmal gewarnt und weitere Glyphen werden ausgelassen. Umlaute/ß werden
wie andere Unicode-Codepoints unterstützt. Emoji-/Ligatur-/komplexe Schrift-
Formung hängt von System-Fallback ab und wird nicht garantiert.

Stats: `window.__engineStats.sdfLabels`, `.sdfGlyphs`.
Tests: `node --test tools/test_web_*.mjs`, Export-Strukturtest und
`tools/verify_web_sdf.mjs` gegen einen echten `coin_game`-WASM-Export.
Der Browser-Verifier injiziert nur HTTP-Antworten; Demos bleiben unverändert.
Bilder: `target/sdf-shots/`.
