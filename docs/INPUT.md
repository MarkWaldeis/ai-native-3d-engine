# Eingabe / Gamepad

## Menüs und Overlays

D-Pad oder linker Stick (Schwelle 0,55; Wiederholung nach 400 ms, dann alle
120 ms) und Pfeile/Tab bewegen den Fokus im obersten offenen Overlay.
A, Enter oder Leertaste wählen; B/Escape schließen Dialoge bzw. setzen eine
Pause fort. Start pausiert/kehrt aus der Pause zurück; während einer
Neubelegung werden Gamepad-Tasten und Achsen ausschließlich aufgenommen.
Intro, Sieg und Niederlage werden über ihre Haupttaste gestartet, B verwirft sie nicht.
`state.lost: true` bzw. der bestehende FPS-Niederlagenstatus zeigen dasselbe
bedienbare Neustart-Menü wie ein Sieg. Touch-Spielsteuerung bleibt hinter Menüs verborgen.
Der Hauptknopf erhält anfangs Fokus; verschachtelte Dialoge geben ihn an das
darunterliegende Menü zurück, beim Spielen erhält das Spielfeld Fokus.
„Steuerung“, „Speichern“ und „Laden“ sind direkt im Menü erreichbar;
fehlende oder ungültige Spielstände werden dort gemeldet. Multiplayer deaktiviert
lokales Speichern/Laden. Navigation und gehaltene Auswahltasten gelangen nicht
als Spielaktion an WASM; Gamepad-Eingaben bleiben bis zum Loslassen gesperrt.
Sichtbare Fokusringe und „Ⓐ Auswählen · Ⓑ Zurück“ unterstützen die Navigation.
Tests: `test_web_ui_nav.mjs`, echter WASM-Browsercheck `verify_web_ui_nav.mjs`.

Die Web-Eingabe erzeugt ausschließlich die bestehenden WASM-Inputs. Spielregeln
und Physik bleiben in Rust/Rhai. Ein W3C-Standard-Gamepad benötigt keine Einrichtung.

| Aktion | Tastatur | Standard-Gamepad |
| --- | --- | --- |
| Bewegen | WASD / Pfeile | Linker Stick / Steuerkreuz |
| Aktion / Springen | Leertaste | A (0) |
| Feuern | Mausklick (Ego-Perspektive) | RT (7) |
| Nachladen | R | X (2) |
| Sprinten | Umschalt links/rechts | L3 (10) |
| Pause | Escape | Start (9) |
| Zielen | Maus / rechts ziehen | Rechter Stick (Achsen 2/3) |

A aktiviert auch den sichtbaren Start-/Weiterspielen-/Nochmal-spielen-Knopf,
einmal pro Tastendruck. Start schaltet während des Spiels die Pause um.
Mehrere Gamepads werden gemeinsam ausgewertet (stärkster Wert je Aktion).
Die radiale Stick-Totzone wird auf den verbleibenden Wertebereich 0–1 skaliert;
Tastatur, Gamepad und Touch-Joystick werden zu einem Vektor mit Länge ≤ 1 gemischt.
Touch-Steuerung erscheint bei groben Zeigern oder nach der ersten Touch-Eingabe;
Joystick und Aktion funktionieren gleichzeitig mit verschiedenen Fingern.

## Autoreneinstellungen

`props.ai_input` der Player-Entity akzeptiert ein Objekt oder dessen JSON-String:

```json
{
  "deadzone": 0.15,
  "look_sensitivity": 1,
  "invert_y": false,
  "bindings": {
    "action": [{"type":"key","code":"KeyE"}, {"type":"button","index":0}],
    "move_forward": [{"type":"axis","index":1,"dir":-1}]
  }
}
```

Aktionen: `move_left`, `move_right`, `move_forward`, `move_back`, `action`, `fire`,
`reload`, `sprint`, `pause`, `look_x`, `look_y`. Bindungen sind `key` mit DOM-
`KeyboardEvent.code`, `button` mit nichtnegativem Index oder `axis` mit Index
und `dir: 1|-1`. Look-Achsen bleiben vorzeichenbehaftet; `dir` legt ihre Richtung
fest. Achsen werden paarweise radial gefiltert (0/1, 2/3, …).

Angegebene Binding-Listen ersetzen die jeweilige Standardliste; `[]` entfernt
die Belegung. Fehlende Felder behalten Standardwerte. Totzone: `0 <= deadzone < 1`;
Look-Empfindlichkeit: positive Zahl in rad/s bei vollem Stick-Ausschlag. Das
Spiel akkumuliert diese relative Eingabe mit Frame-dt zu absolutem Yaw/Pitch.
`invert_y` invertiert die Gamepad-Look-Y-Achse. Ungültige Werte/Bindungslisten
behalten Standards, unbekannte Aktionen werden ignoriert; Hinweise stehen im Protokoll.

## Neubelegung und Speicherung

„Steuerung“ im Footer öffnet einen per Tastatur bedienbaren Dialog und pausiert
das Spiel. „Neu belegen“ ersetzt alle Bindungen dieser Aktion durch den nächsten
Tastendruck, Gamepad-Knopf oder Achsenausschlag über 0,6. Bereits gehaltene
Gamepad-Eingaben müssen erst losgelassen werden. Escape bricht die Aufnahme ab;
außerhalb einer Aufnahme schließt Escape den Dialog. Eine neu zugeordnete
Bindung wird aus allen anderen Aktionen entfernt. Zum Fortsetzen „Weiterspielen“
drücken; Schließen alleine startet die Simulation nicht.

Benutzerbelegungen übersteuern Autoreneinstellungen und werden unter
`ai-engine:input:<Spieltitel>` in `localStorage` gespeichert (gleicher Ursprung
und gleicher Titel teilen Einstellungen). „Standard wiederherstellen“ löscht
die Benutzerbelegung und lädt die Autoreneinstellungen bzw. Engine-Standards.
Ohne verfügbaren Speicher bleibt die Belegung für diese Sitzung wirksam;
eine Warnung erscheint im Dialog und Protokoll. Es werden keine Daten versendet.

## Touch-Tasten und Vibration

`props.ai_touch` akzeptiert ein Objekt oder JSON-String:

```json
{"buttons":[
  {"action":"sprint","label":"Sprint","icon":"»","anchor":"bottom-left","size":"md","hold":true},
  {"action":"fire","label":"Feuer","anchor":"bottom-right","size":"lg","hold":true},
  {"action":"reload","label":"Laden","anchor":"right","hold":false}
],"joystick":{"side":"left","size":120},"hide_default_action":false}
```

Aktionen entsprechen exakt den oben genannten `InputSystem`-Aktionen. Labels
erhalten deutsche ARIA-Beschreibungen; fehlende Labels nutzen deutsche
Aktionsnamen. Anker: `bottom-right` (Standard), `right`, `bottom-left`,
`top-right`. Größen: `md` (56 px) und `lg` (72 px). `hold` ist standardmäßig
`true`: gehaltene Finger liefern die Aktion kontinuierlich; `false` liefert
einen Impuls für den nächsten Simulationstick. Auch sehr kurze Taps bleiben
bis zum Tick erhalten; Cancel, Pause, Fokusverlust und Neustart löschen sie.
Jeder Finger wird unabhängig erfasst. `look_x/y` wirken wie positive digitale
Look-Bindungen; Tastatur-/Gamepad-Remaps ändern die logischen Touch-Aktionen nicht.
Für `look_x/y` gilt immer `hold:true`; `hold:false` wird mit Warnung korrigiert.

Joystick-Seite: `left` (Standard) oder `right`. Größe: 80–180 px, alternativ
`md` = 120 und `lg` = 150; im Querformat maximal 100 px. Authored Controls
ordnen auch Standard-Aktion und FPS-Feuer/Nachladen kollisionsfrei neu an.
Safe-Area, ShooterHud, Mission/Ziel und Footer bleiben frei; benutzerdefinierte
HUDs berücksichtigen alle Touch-Tasten. FPS-Touch-Controls vermeiden das
ShooterHud auch ohne `ai_touch`. Auf Touch-Geräten steht die Hilfe im
Steuerungsdialog statt über dem HUD im Footer.
Maximal 12 Zusatztasten; ungültige Felder nutzen Standards und erzeugen Warnungen.
Bei zu kleinen Viewports werden nicht platzierbare Tasten mit Warnung verborgen.
`hide_default_action:true` verbirgt nur die normale Aktions-/Sprungtaste.

`ai_input.haptics`: `{"enabled":true,"strength":1}`. `enabled` ist boolesch,
`strength` liegt zwischen 0 und 1. „Vibration“ im Steuerungsdialog wird zusammen
mit Remaps pro Spieltitel gespeichert; Reset lädt die Autoreneinstellungen.
Sammeln/Sieg und WASM-FPS-Treffer-/Schadensereignisse lösen kurze Impulse aus:
aktives Gamepad via `vibrationActuator.playEffect('dual-rumble', ...)`, sonst
Touch-Gerät via `navigator.vibrate(ms)`. Stärke skaliert Rumble-Amplituden bzw.
Vibrationsdauer; 0 oder deaktiviert unterdrückt Impulse. Nicht unterstützte/
abgelehnte APIs werden ignoriert. Kein Vibrationsversprechen auf iOS/Safari.

DOM-freie Kerne: `touch.js` (Validierung, Layout, Pointer-Zustand, Haptik);
DOM: `touch-controls.js`. Tests: `tools/test_web_touch.mjs`, echter Browser:
`tools/verify_web_touch.mjs`; Bilder: `target/touch-shots/`.

## Module und Prüfungen

`input.js` ist DOM-frei (`InputSystem`, Konfigurationsparser, Totzone,
Gamepad-Sampling, Flankenerkennung und deutsche Binding-Namen).
`input-settings.js` enthält Dialog, Aufnahme und lokale Speicherung.
`player.js` mischt Quellen und liefert die bisherigen Input-Namen;
`fps.js` akkumuliert Look-Werte. Kurze Tastatur-Aktionsimpulse bleiben über
`ActionLatch` bis zum nächsten Simulationstick erhalten.

Tests: `node --test tools/test_web_*.mjs`,
`cargo test -p engine-tests --test web_export` sowie exportierte echte WASM-
Spiele und `tools/verify_web_release.mjs` gegen `cargo run --example web_parity`.
