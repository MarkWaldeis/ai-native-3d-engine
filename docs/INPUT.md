# Eingabe / Gamepad

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
