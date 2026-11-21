# Waldwächter

512 × 512 m Blender-Wald. Fünf Relikte am goldenen Pfad sammeln, Abstand zu
den drei patrouillierenden Wächtern halten. Erwischt → letzter Wald-Anker;
Relikte bleiben erhalten. Ab dem dritten Relikt wird der Anker aktualisiert.
WASD/Pfeile oder Bildschirm-Joystick, Aktion/Leertaste zum Springen.

Reproduktion in ein **neues** Projekt: `cargo build --example waldwaechter_bake`
und `cargo build`, dann `python3 tools/waldwaechter/reproduce.py NEUES_PROJEKT
--work NEUES_MESSVERZEICHNIS` aus dem Repo. Vier deterministisch geskriptete
MCP-Rollen, keine unabhängigen KI-Modelle. Blender-Quelle erhalten.
Navigation wird mit der bestehenden nativen Bake-API als Inhaltsrezept erzeugt;
der HTTP-Endpunkt verweigert `nav_bake` derzeit. Keine Runtime-Anpassung.

`scene.json` ist portabel, Skripte sind zusätzlich mit unveränderlicher Quelle
eingefroren. Änderungen an Rezept, `scripts/` und Szenenquelle gemeinsam halten.
GLBs werden nach dem Bundle im Inhaltsrezept über SHA256 kanonisiert, einschließlich
der LOD-Verweise. Zwischenstände und lokale Checkpoints nicht ins Repository.
Prefab-Instanzen bleiben ausdrücklich materialisiert; kein neuer Runtime-Vertrag.

Export: `cargo run -- --project examples/waldwaechter --export-web scene.json web`.
Regressionen: `cargo test --test waldwaechter`; native Referenz:
`cargo run --example waldwaechter_parity`. Web-Prüfung siehe
`tools/waldwaechter/browser.mjs`. Messgrenzen: `docs/SHOWCASE_REPORT.md`.
