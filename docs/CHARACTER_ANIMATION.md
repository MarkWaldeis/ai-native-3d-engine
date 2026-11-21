# Charakteranimation: Blender → deterministische Simulation → Web

Sky Gardens Milo ist ein echtes skinned GLB mit einem Armature und den drei
Actions `idle`, `walk`, `jump`. Seine Capsule, Physik und Bewegungsregeln bleiben
unverändert. `examples/sky_garden/milo_rig.blend` ist die bearbeitbare Quelle.
Das kleine Testmodell ist bewusst kein Rendering-Qualitätsprojekt.

## Blender

Reproduzierbarer Bau mit echtem Blender (lokal mit 5.3 geprüft):

```sh
blender --background --factory-startup --python tools/sky_garden/rigged_character.py -- --out target/milo
```

Das Skript erzeugt Knochen, Gewichte und drei in-place Actions; der normale
Bridge-Exporter schreibt das GLB. `blender_sync` exportiert Mesh + zugehöriges
Armature zusammen, mit Skins und mehreren Actions statt einer statisch
evaluierten Mesh-Kopie. Clipnamen bleiben beim Content-Hash-Dedup erhalten.
Die Bind-Pose ist zentriert, damit die bestehende bounds-basierte
Collider-Zentrierung Sky Gardens Bewegung und Win-Ticks nicht verändert.
Das Armature muss ein Root-Objekt ohne gescherten World-Transform sein.
Der Asset-Transform wird auf diesen Root bezogen. Auswahl und World-Transforms
werden auch bei Exportfehlern wiederhergestellt. Vorhandene Bridge-Prozesse
müssen nach einem Engine-Update neu gestartet werden.

Bei einem neuen animierten Import liest die Engine Clipnamen und Laufzeiten
direkt aus dem GLB. Sie erzeugt zunächst einen `default`-State für den ersten
Clip. Optional enthält die Mesh-Custom-Property `ai_animator` die State-Machine
als JSON. Ein vorhandener Animator bleibt bei erneutem Sync erhalten; die
Clip-Liste wird aktualisiert. Verlorene/referenzierte Clips führen zu einem
Fehler statt einem halben Szenenimport.

## MCP und Datenvertrag

```json
{"tool":"animation_clips","arguments":{"id":20}}
{"tool":"animation_clips","arguments":{"path":"assets/milo.glb"}}
{"tool":"animator_set","arguments":{"id":20,"agent":"animator","expected_revision":42,"speed":1,"blend_time":0.15,"machine":{
  "initial":"idle","parameters":{"speed":0},
  "states":{"idle":{"clip":"idle"},"walk":{"clip":"walk","speed":1,"looping":true}},
  "transitions":[
    {"from":"idle","to":"walk","conditions":[{"parameter":"speed","op":"gt","value":0.1}]},
    {"from":"walk","to":"idle","conditions":[{"parameter":"speed","op":"le","value":0.1}]}
  ]
}}}
```

`animator_validate {id,machine?,speed?,blend_time?}` prüft ohne Mutation die
vorgeschlagene oder bereits gespeicherte Machine gegen die **echten** Asset-Clips.
Unbekannte Clips, Parameter und States sowie strukturell unerreichbare States
sind Fehler. Die Erreichbarkeitsprüfung ist eine Graphprüfung, keine Lösung
numerischer Bedingungen. Ein leerer/ungültiger Animator wird abgelehnt; eine
Szene ohne Animatoren bleibt gültig. Pfade sind auf das Projekt beschränkt;
externe GLTF-Buffer werden nicht geladen.

`animator_set` akzeptiert keine erfundene Clip-Liste. Es beachtet Entity-Locks,
CAS, `apply_batch` mit `mode:"atomic"`, Rollback und Projekt-Checkpoints.
Während Play sind Authoring-Änderungen gesperrt. Prefab-Overrides können das
`animator`-Feld enthalten. `query_entities {component:"animator"}` und die
Szenenzusammenfassung erkennen die neue Komponente.

Der minimale gemeinsame Vertragszusatz ist `Entity.animator` (optional, alte
Szenen bleiben gültig) plus die typisierten Animationsdaten in
`shared/src/animation.rs`. `clips`, `machine`, `speed`, `blend_time` sind
Authoring-Daten; `current_state`, `time`, `values`, Vorgänger-State und Blenduhr
sind Simulationszustand. Ein neuer Play-/WASM-Load setzt diese Laufzeitwerte
immer auf Initial-State/Parameterdefaults zurück.

## Rhai und Tick-Semantik

```rhai
if ctx.has_animator() {
    let velocity = ctx.velocity();
    ctx.animator_set("speed", velocity[0].abs() + velocity[2].abs());
    ctx.animator_set("grounded", ctx.grounded());
}
```

Parameter sind deklarierte endliche Zahlen; boolesche Rhai-Werte werden auf
`0`/`1` abgebildet. Ein unbekannter Parameter oder ein Setter ohne Animator ist
ein Scriptfehler. Operatoren: `eq`, `ne`, `gt`, `ge`, `lt`, `le`.

Nach den Rhai-Hooks und dem Physikschritt erfolgt je aktivem Entity genau eine
Animationsauswertung pro festem Simulationstick. Alle Bedingungen einer
Transition müssen erfüllt sein. Die **erste passende Transition in Listenreihenfolge**
gewinnt; höchstens ein Wechsel pro Tick. Zielzeit startet bei null und wird
im selben Tick fortgeschrieben. Clipzeit = Tickdauer × globaler Speed ×
State-Speed; Loops verwenden Modulo, nicht-loopende Clips klemmen am Ende.
Die Blenduhr läuft in Simulationssekunden. Inaktive Streaming-Entities frieren
auch ihre Animation ein. Simulationspause bedeutet keine fortschreitende Pose.

## Web-Playback und Grenzen

Native Runtime und WASM benutzen denselben Rust-Code. Snapshots enthalten pro
Animator aktuellen/vorherigen State, Clipzeiten, Blendzeit und Parameterwerte,
nicht erneut die vollständige Machine. Der Webplayer klont Skeletons samt
Bone-Referenzen pro Instanz, umgeht statisches Instancing/Impostors und sampelt
`AnimationMixer`-Actions mit diesen Zeiten und linearen Crossfade-Gewichten.
Render-FPS oder Wanduhr treiben die Animation **nicht**. Asset-Laden darf
asynchron sein: der letzte Simulationszustand wird nach dem Laden angewendet.

Kein Root-Motion-Transfer in die Physik, kein IK/Retargeting, kein Blend-Tree und
keine skinned LODs. Rasche Folgetransitionen blenden vom vorherigen State,
nicht von einem gespeicherten mehrstufigen Blend-Ergebnis. Der native
Headless-Renderer zeichnet weiterhin die Bind-Pose; die native **Simulation**
ist animationsfähig. GPU-Skinning läuft im three.js-Webplayer. Materialien,
Beleuchtung, Schatten und Post-Processing wurden nicht verändert.

## Regressionen

```sh
cargo test --test animation -- --nocapture
node --test tools/test_web_animation.mjs
```

Die Release-Parität vergleicht zusätzlich zu Gameplay-Zuständen alle 30 Ticks
die Animationszustände einschließlich Clip-/Blendzeit zwischen nativer
Simulation und frisch exportiertem WASM. Die normalen Chromium-/WebKit-
Releasechecks behalten Win/Restart für alle drei Spiele bei.
