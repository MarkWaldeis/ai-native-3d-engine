# Deklaratives Web-HUD

Die Player-Property `ai_hud` akzeptiert ein Objekt oder einen JSON-String.
JavaScript stellt ausschließlich Werte aus dem WASM-Tick-`state` dar; Regeln,
Punktevergabe und Lebenspunkte bleiben in Rust/Rhai. `ai_ui` steuert weiterhin
Titel, Mission und die eingebauten Anzeigen; `ai_hud` ergänzt frei platzierte Widgets.

```json
{
  "replace_default": true,
  "widgets": [
    {"id":"coins","type":"counter","anchor":"top-right","offset":[0,110],
     "text":"Münzen {score}/{total:coin}","icon":"🪙","width":240,
     "phone":{"anchor":"bottom-left","offset":[0,-12],"size":"sm"}},
    {"id":"health","type":"bar","anchor":"left","bind":"health","max":"max_health",
     "text":"Leben {health|0}","color":"#c3f683"},
    {"id":"collect","type":"toast","anchor":"bottom","bind":"score",
     "text":"Gesammelt! {score} Münzen","phone":{"anchor":"top","offset":[0,160]}}
  ]
}
```

## Schema

- `replace_default`: boolesch, Standard `false`. Bei `true` wird nur `#state`
  verborgen, nicht Mission, Titel, Steuerung oder das FPS-`ShooterHud`.
- `widgets`: Liste. Jedes Widget hat eine eindeutige `id` und einen `type`:
  `text`, `counter` (Text mit Bindung), `bar`, `panel`, `toast`.
- `anchor`: `top-left`, `top`, `top-right`, `left`, `center`, `right`,
  `bottom-left`, `bottom`, `bottom-right`; Standard `top-left`.
- `offset: [x,y]`: Pixel relativ zum Anker; viewport-sicher begrenzt.
  Am oberen Rand verschiebt positives y nach unten, am unteren negatives y nach oben.
- `width`: positive Pixelbreite, Standard 220, auf Viewportbreite begrenzt.
- `text`: Vorlage; ohne Text wird `{bind}` dargestellt, wenn `bind` gesetzt ist.
- `bind`: Name eines `state`-Werts. Balken lesen den numerischen Wert;
  Toasts erscheinen bei Änderungen für ca. zwei Sekunden (nicht beim ersten Snapshot).
- `max`: positive Zahl oder Name eines numerischen `state`-Werts; Standard 100.
  Balken begrenzen auf 0–100 %, erhalten bei <30 % die Klasse `low`.
- `color`: CSS-Farbname, Hex oder RGB/RGBA; keine URLs oder HTML.
- `size`: `sm`, `md` (Standard), `lg`, `xl`; `style`: `card` (Standard), `plain`.
- `visible_if`: `key` oder `!key`, nach Wahrheit des `state`-Werts.
- `icon`: Emoji/Zeichen vor dem Text. Alle Texte werden als Text, nie HTML gesetzt.
- `phone: {anchor,offset,size,hidden}`: Überschreibungen bei Breite **<600 px**.
- `panel.children`: Widgets als Spalte mit Abstand; Kind-Anker werden nicht separat
  positioniert. Panels können bis vier Ebenen verschachtelt werden.

Vorlagen unterstützen `{key}`, `{key|0}`, `{key|0.0}` (feste Dezimalstellen),
`{total:tag}` (Anzahl beim Szenenstart) und `{left:tag}` (aktuelle Entitäten,
einschließlich Spawn/Remove). Neustart setzt die Zählungen und Toasts zurück.
Unbekannte Zustandswerte erscheinen als „–“, unbekannte Tags als 0.

Widgets am gleichen Anker werden mit 8 px Abstand gestapelt; Safe-Area-Inset und
12 px Rand bleiben frei. Auf Touch-Geräten werden untere linke/rechte Widgets
oberhalb der tatsächlichen Joystick-/Aktionstasten positioniert. Überlange Inhalte
werden innerhalb der viewport-begrenzten Karten abgeschnitten. Andere Anker
können sich bewusst überlagern; Autoren sollten die eingebauten Anzeigen beachten.

Ungültige Felder erhalten sichere Standardwerte; ungültige Widgets werden
ignoriert und Warnungen im Log angezeigt. Die DOM-Struktur bleibt über Ticks
erhalten, Text und Balken werden nur bei Änderungen aktualisiert. Texte und
Toasts verwenden `aria-live="polite"`.

## Prüfen

`node --test tools/test_web_*.mjs` und
`cargo test -p engine-tests --test web_export`.
Nach echtem WASM-Export:
`PLAYWRIGHT_MODULE=…/playwright/index.mjs node tools/verify_web_hud.mjs target/hud-site`.
Der Verifier injiziert nur in der HTTP-Antwort `ai_hud`, verändert keine Demo und
prüft Desktop/Touch, echte Münzsammlung, Toast-Ablauf, Balken und Layout.
Bilder: `target/hud-shots/`. Release-Parität: `tools/verify_web_release.mjs`.
