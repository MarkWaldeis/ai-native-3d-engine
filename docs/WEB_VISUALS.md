# Web-Visuals

Opt-in über Player-Prop `ai_render`, z.B.:

```json
{"preset":"ultra","time_of_day":15,"foliage":{"density":1},"water":{"level":-1},"auto_quality":true}
```

`ai_graphics` ist ein Alias (Objekt oder JSON-String); `{}` allein aktiviert
die Defaultqualität high. Bei beiden Props überschreibt `ai_render` gleiche
Felder aus `ai_graphics`. Der Poststack hat nur einen Composer in render.js.

Ultra: vier Schattenkaskaden, GTAO, Bloom, SMAA; high: drei Kaskaden,
Bloom/SMAA; medium: ein kamerageführter Schatten und FXAA; low: direktes Rendern.
DOF ist optional. Gras/Bäume sind deterministische Instanzen auf 16-m-Kacheln,
mit Wind und Distanz-Ausblendung. Wasser ist prozedural, ohne externe Texturen.
Die Vegetation überspringt Wasser und steile Hänge. Der Height-Sampler kann
durch Terrain ersetzt werden; ohne Treffer ist die Grundhöhe null.

Die Automatik reduziert Pixelratio, Postprocessing, Schatten und Vegetation,
bevor sie das Preset absenkt. Erholung überschreitet nie die Startqualität.
Feste URL-Overrides deaktivieren die Automatik; `quality=auto` aktiviert sie.
Bestehende Szenen ohne Prop und ohne quality-URL bleiben unverändert.
