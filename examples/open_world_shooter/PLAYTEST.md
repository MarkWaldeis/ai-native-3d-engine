# Operation Rio Verde: full mission route

Open the exported game at `/open_world_shooter/?speed=4` and start a new game.
The optional `speed` input scales **horizontal walking/sprinting only**, in Rhai;
it does not teleport or accelerate combat, reloads, jumping, AI or objectives.
It defaults to 1 and is bounded to 1–4. Change the URL and refresh to change it.

WASD moves, mouse looks, left mouse fires, R reloads, Shift sprints and Space
jumps. Stop to aim rather than sprinting into a whole outpost. Magazines hold
30 rounds; an empty magazine reloads automatically if reserve ammunition remains.
Walk over medkits (+40 health, capped at 100) and ammunition boxes (+75 reserve).

Coordinates below are `(x, y, z)`, with terrain/landmark height in `y`.
Intermediate road points are `(x, z)`. HUD bearings use **0° = −Z, 90° = +X**.
Landmark names such as `bridge-south` are authored names, not compass bearings.

| Objective | World coordinates | Path and action |
| --- | --- | --- |
| Echo spawn | `(-2688, 30.9, -2688)` | Follow the road ESE (about 125°) through `(-2350,-2450)` and `(-1900,-1950)` to the river approach `(-1339,-1700)`. |
| Rio Verde bridge | `bridge-marker`: `(-1228.9, 5.1, -1700)` | Take the ramps/deck **east (90°)**, staying near `z=-1700`. The objective advances near the marker, but finish crossing to `(-1119,-1700)` rather than turning off the bridge into the river. |
| Clear Outpost Alpha | `(-300, 26, -2100)` | Head ENE (about 60°) via `(-800,-1850)` and `(-430,-2100)`. Kill all six `alpha-soldier-*` guards (`grp_alpha`); no flag interaction is needed. Go around the sandbag ring, not through it. Supplies are near `(-306,-2103)` and `(-294,-2096)`. |
| San Aurelio intel | `intel`: `(904, 24, -397)` | Take the road SE (about 145°) via `(100,-1500)` and `(500,-900)` into the central crossroads `(900,-400)`. Skirt the sandbags/ruins at the town entrance. Walk within 3 m of the intel case to collect it automatically. Town guards/drone can be killed, but clearing the whole town is not the objective. |
| Destroy radio tower | `radio-tower`: `(-1692, 40, 1192)` | Go SW/WSW (roughly 220–250°) via `(300,300)`, `(-300,800)`, then the **other bridge**, centered at `(-592.5,900)`. Enter from `(-482.5,900)` and cross west to `(-702.5,900)`. Continue via `(-1100,1000)` toward `(-1560,1153)` on the Bravo road. Shoot the tower mast, above nearby cover: 9 rifle hits destroy its 300 HP. Bravo guards/drone are threats, not a separate clearance requirement. |
| Eliminate commander | Initial `commander`: `(2200, 78, 2194)` | Retrace the Bravo road and bridge to San Aurelio. Then follow the mesa road SSE (about 150°) through `(1300,300)`, `(1700,1100)`, `(2000,1700)`, `(2200,1960)`. Resupply near `(2170,2100)`, `(2200,2100)`, `(2230,2100)`; a medkit is at `(2220,2104)`. Approach the **open wall gap at `(2200,2130)`** from the road, heading +Z (180°). Enter through that gap, not a wall or keep collider. The commander patrols near the listed position in front of the keep; 8 rifle hits defeat his 250 HP. |
| Won | — | The victory overlay follows the commander's death after the preceding objectives. There is no final pickup or extraction trigger. |

Regular soldiers need 3 rifle hits. Use clear lines of sight, aim above sandbags,
and stop firing once a guard falls. Cover/buildings block both movement and shots;
the road polyline is a guide, not permission to walk straight through barricades.
There is no requirement to clear optional Outpost Charlie.

## Native end-to-end reproduction

```sh
cargo test --release -p engine-tests --test open_world_shooter_playthrough -- --nocapture
```

This loads the unmodified full generated scene and uses the browser's authored
active-region settings. A waypoint/input driver steers around static cover,
aims with look angles, fires, collects actual pickups and crosses the citadel
gate before shooting the commander. It never writes mission state, health,
enemy positions or player transforms. The only speed boost is `travel_speed=4`.
It asserts every mission stage, required kills, intel collection, victory and
no below-terrain fall, and prints ticks, health and mean/max simulation-step
milliseconds (CPU only; excluding loading, route planning and rendering).
The same test also runs in the default workspace suite; debug builds are slower.
