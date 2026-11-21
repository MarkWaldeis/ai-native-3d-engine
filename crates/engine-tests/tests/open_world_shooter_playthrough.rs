use engine_runtime::{Input, Simulation};
use serde_json::Value;
use shared::scene::{Entity, Scene};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

struct Obstacle {
    id: u64,
    name: String,
    min: [f32; 3],
    max: [f32; 3],
}
fn intersects(a: [f32; 3], b: [f32; 3], o: &Obstacle) -> bool {
    let (mut lo, mut hi) = (0.001_f32, 0.999_f32);
    for i in 0..3 {
        let d = b[i] - a[i];
        if d.abs() < 0.00001 {
            if a[i] < o.min[i] || a[i] > o.max[i] {
                return false;
            }
        } else {
            let x = (o.min[i] - a[i]) / d;
            let y = (o.max[i] - a[i]) / d;
            lo = lo.max(x.min(y));
            hi = hi.min(x.max(y));
        }
    }
    lo <= hi
}
fn obstacles(scene: &Scene) -> Vec<Obstacle> {
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/open_world_shooter/assets/manifest.json"))
            .unwrap(),
    )
    .unwrap();
    scene
        .entities
        .iter()
        .filter(|e| e.collider.is_some() && e.script.is_none() && e.name != "terrain")
        .filter_map(|e| {
            let value = serde_json::to_value(e).unwrap();
            let bounds = if value["kind"] == "gltf" {
                let stem = std::path::Path::new(value["path"].as_str().unwrap())
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap();
                manifest[stem]["bounds"].clone()
            } else if let Some(size) = value["size"].as_array() {
                serde_json::json!([
                    size.iter()
                        .map(|v| -v.as_f64().unwrap() / 2.0)
                        .collect::<Vec<_>>(),
                    size.iter()
                        .map(|v| v.as_f64().unwrap() / 2.0)
                        .collect::<Vec<_>>()
                ])
            } else {
                return None;
            };
            if !bounds.is_array() {
                return None;
            }
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            let yaw = e.transform.rotation_deg[1].to_radians();
            for corner in 0..8 {
                let p: [f32; 3] = std::array::from_fn(|i| {
                    bounds[(corner >> i) & 1][i].as_f64().unwrap() as f32 * e.transform.scale[i]
                });
                let q = [
                    p[0] * yaw.cos() + p[2] * yaw.sin(),
                    p[1],
                    -p[0] * yaw.sin() + p[2] * yaw.cos(),
                ];
                for i in 0..3 {
                    let v = q[i] + e.transform.position[i];
                    min[i] = min[i].min(v);
                    max[i] = max[i].max(v);
                }
            }
            Some(Obstacle {
                id: e.id,
                name: e.name.clone(),
                min,
                max,
            })
        })
        .collect()
}

type Cell = (i32, i32);
fn cell(p: [f32; 3]) -> Cell {
    (p[0].round() as i32, p[2].round() as i32)
}
fn cell_point(p: Cell) -> [f32; 3] {
    [p.0 as f32, 0., p.1 as f32]
}
fn walkable_line(a: [f32; 3], b: [f32; 3], blocked: &BTreeSet<Cell>) -> bool {
    let samples = (distance(a, b) * 2.).ceil().max(1.) as usize;
    (1..=samples).all(|i| {
        let t = i as f32 / samples as f32;
        !blocked.contains(&cell([
            a[0] + (b[0] - a[0]) * t,
            0.,
            a[2] + (b[2] - a[2]) * t,
        ]))
    })
}
fn walking_grid(obstacles: &[Obstacle]) -> BTreeSet<Cell> {
    let mut blocked = BTreeSet::new();
    for o in obstacles.iter().filter(|o| !o.name.starts_with("bridge-")) {
        for x in (o.min[0] - 1.).ceil() as i32..=(o.max[0] + 1.).floor() as i32 {
            for z in (o.min[2] - 1.).ceil() as i32..=(o.max[2] + 1.).floor() as i32 {
                blocked.insert((x, z));
            }
        }
    }
    blocked
}
fn walking_path(p: [f32; 3], goal: [f32; 3], blocked: &BTreeSet<Cell>) -> VecDeque<[f32; 3]> {
    if walkable_line(p, goal, blocked) {
        return VecDeque::from([goal]);
    }
    let start = cell(p);
    let end = cell(goal);
    let heuristic = |a: Cell| {
        let x = (a.0 - end.0).unsigned_abs();
        let z = (a.1 - end.1).unsigned_abs();
        14 * x.min(z) + 10 * (x.max(z) - x.min(z))
    };
    let mut open = BinaryHeap::from([Reverse((heuristic(start), Reverse(0), start))]);
    let mut costs = BTreeMap::from([(start, 0)]);
    let mut previous = BTreeMap::new();
    while let Some(Reverse((_, Reverse(cost), at))) = open.pop() {
        if at == end {
            let mut path = vec![goal];
            let mut next = at;
            while next != start {
                path.push(cell_point(next));
                next = previous[&next];
            }
            path.reverse();
            let mut result = VecDeque::new();
            let mut anchor = p;
            let mut first = 0;
            while first < path.len() {
                let last = (first..path.len())
                    .rev()
                    .find(|i| walkable_line(anchor, path[*i], blocked))
                    .unwrap_or(first);
                result.push_back(path[last]);
                anchor = path[last];
                first = last + 1;
            }
            return result;
        }
        if cost > costs[&at] {
            continue;
        }
        assert!(
            costs.len() < 100_000,
            "navigation search exhausted: {p:?} -> {goal:?}, blocked goal={}",
            blocked.contains(&end)
        );
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let next = (at.0 + dx, at.1 + dz);
                if blocked.contains(&next)
                    || (dx != 0
                        && dz != 0
                        && (blocked.contains(&(at.0 + dx, at.1))
                            || blocked.contains(&(at.0, at.1 + dz))))
                {
                    continue;
                }
                let next_cost = cost + if dx == 0 || dz == 0 { 10 } else { 14 };
                if next_cost < *costs.get(&next).unwrap_or(&u32::MAX) {
                    costs.insert(next, next_cost);
                    previous.insert(next, at);
                    open.push(Reverse((
                        next_cost + heuristic(next),
                        Reverse(next_cost),
                        next,
                    )));
                }
            }
        }
    }
    panic!("no walking route: {p:?} -> {goal:?}");
}

#[test]
#[ignore = "full campaign; run with --release --ignored"]
fn full_generated_mission_is_winnable_using_only_player_inputs() {
    let scene: Scene = serde_json::from_str(
        &std::fs::read_to_string(root().join("examples/open_world_shooter/scene.json")).unwrap(),
    )
    .unwrap();
    let point = |name: &str| {
        scene
            .entities
            .iter()
            .find(|e| e.name == name)
            .unwrap()
            .transform
            .position
    };
    let south = point("bridge-south-0");
    let north = point("bridge-north-0");
    let intel = point("intel");
    let tower = point("radio-tower");
    let routes: Vec<Vec<[f32; 3]>> = vec![
        vec![
            [-2350., 0., -2450.],
            [-1900., 0., -1950.],
            [south[0] - 110., 0., south[2]],
            [south[0] + 110., 0., south[2]],
            [-800., 0., -1850.],
            [-430., 0., -2100.],
        ],
        vec![
            [south[0] + 110., 0., south[2]],
            [-800., 0., -1850.],
            [-430., 0., -2100.],
            [-300., 0., -2100.],
        ],
        vec![
            [-306., 0., -2103.],
            [100., 0., -1500.],
            [500., 0., -900.],
            [900., 0., -400.],
            intel,
        ],
        vec![
            [300., 0., 300.],
            [-300., 0., 800.],
            [north[0] + 110., 0., 900.],
            [north[0] - 110., 0., 900.],
            [-1100., 0., 1000.],
            [-1560., 0., 1153.],
        ],
        vec![
            [-1560., 0., 1153.],
            [-1100., 0., 1000.],
            [north[0] - 110., 0., 900.],
            [north[0] + 110., 0., 900.],
            [-300., 0., 800.],
            [300., 0., 300.],
            [900., 0., -400.],
            [1300., 0., 300.],
            [1700., 0., 1100.],
            [2000., 0., 1700.],
            [2200., 0., 1960.],
            [2200., 0., 2130.],
            [2200., 0., 2185.],
        ],
    ];
    let obstacles = obstacles(&scene);
    let blocked = walking_grid(&obstacles);
    let terrain_entity = scene.entities.iter().find(|e| e.name == "terrain").unwrap();
    let terrain = engine_terrain::Terrain::load(
        &root().join(terrain_entity.props["ai_terrain"].as_str().unwrap()),
    )
    .unwrap();
    let origin = terrain_entity.transform.position;
    let player = scene.entities.iter().find(|e| e.name == "player").unwrap();
    let radius = player.props["ai_world"]["sim_radius"].as_f64().unwrap() as f32;
    let hysteresis = player.props["ai_world"]["sim_hysteresis"].as_f64().unwrap() as f32;
    let required_kills: BTreeSet<_> = scene
        .entities
        .iter()
        .filter(|e| {
            e.name == "commander"
                || e.name == "radio-tower"
                || e.tags.iter().any(|t| t == "grp_alpha")
        })
        .map(|e| e.id)
        .collect();
    assert_eq!(required_kills.len(), 8);
    let mut sim = Simulation::new(scene, &root()).unwrap();
    let mut dead = BTreeSet::new();
    let mut stages = Vec::new();
    let mut stage = -1;
    let mut route = VecDeque::new();
    let mut elapsed = Duration::ZERO;
    let mut max_tick = Duration::ZERO;
    let mut last_position = [0.; 3];
    let mut stuck = 0;
    let mut navigation_goal = [f32::INFINITY; 3];
    let mut navigation_path = VecDeque::new();
    let mut entered_citadel = false;
    for _ in 0..16000 {
        let summary = sim.state_summary();
        let state = &summary["state"];
        assert_ne!(state["lost"], true, "lost at tick {}: {summary}", sim.tick);
        if state["won"] == true {
            assert_eq!(stages, [0, 1, 2, 3, 4]);
            assert!(required_kills.is_subset(&dead));
            assert!(entered_citadel);
            assert!(state["health"].as_f64().unwrap() > 0.);
            let mut intel_left = false;
            sim.visit_entities(|e| {
                intel_left |= e.name == "intel";
            });
            assert!(!intel_left);
            println!(
                "FULL MISSION WON: ticks={}, health={}, kills={}, mean_ms={:.3}, max_ms={:.3}",
                sim.tick,
                state["health"],
                state["kills"],
                elapsed.as_secs_f64() * 1000. / sim.tick as f64,
                max_tick.as_secs_f64() * 1000.
            );
            return;
        }
        let current = state["_stage"].as_i64().unwrap();
        if current != stage {
            stage = current;
            stages.push(stage);
            route = routes[stage as usize].clone().into();
            println!(
                "MISSION stage={stage} tick={} health={}",
                sim.tick, state["health"]
            );
        }
        let mut actors = Vec::new();
        let mut p = [0.; 3];
        sim.visit_entities(|e| {
            if e.name == "player" {
                p = e.transform.position;
            } else if e.tags.iter().any(|t| t == "enemy" || t == "pickup")
                || e.name == "radio-tower"
            {
                actors.push(e.clone());
            }
        });
        let ground = terrain
            .sample((p[0] - origin[0]) as f64, (p[2] - origin[2]) as f64)
            .unwrap();
        assert!(
            p[1] >= ground - 0.5,
            "fell below terrain: tick={} position={p:?} ground={ground}",
            sim.tick
        );
        if stage == 4 && p[2] > 2134. && p[2] < 2195. && (p[0] - 2200.).abs() < 9. {
            entered_citadel = true;
        }
        sim.set_active_region(p[0], p[2], radius, hysteresis)
            .unwrap();
        if sim.tick % 1200 == 0 {
            println!(
                "PROGRESS tick={} stage={stage} position={p:?} health={} ammo={}/{} goal={:?}",
                sim.tick,
                state["health"],
                state["ammo"],
                state["ammo_reserve"],
                route.front()
            );
        }
        while route.front().is_some_and(|q| distance(p, *q) < 2.0) {
            route.pop_front();
        }
        let eye = [p[0], p[1] + 0.75, p[2]];
        let aim_point = |e: &Entity| {
            let mut q = e.transform.position;
            q[1] += if e.name == "radio-tower" {
                12.0
            } else if e.tags.iter().any(|t| t == "drone") {
                0.2
            } else {
                1.4
            };
            q
        };
        let visible = |e: &Entity| {
            let q = aim_point(e);
            let samples = distance(eye, q).ceil() as usize;
            let clear_ground = (1..samples).all(|i| {
                let t = i as f32 / samples as f32;
                let at: [f32; 3] =
                    std::array::from_fn(|axis| eye[axis] + (q[axis] - eye[axis]) * t);
                at[1]
                    > terrain
                        .sample((at[0] - origin[0]) as f64, (at[2] - origin[2]) as f64)
                        .unwrap()
                        + 0.15
            });
            clear_ground
                && !obstacles
                    .iter()
                    .any(|o| o.id != e.id && intersects(eye, aim_point(e), o))
        };
        let target = actors
            .iter()
            .filter(|e| {
                e.tags.iter().any(|t| t == "enemy")
                    && !dead.contains(&e.id)
                    && (e.name != "commander" || entered_citadel)
                    && distance(p, e.transform.position) < 160.
                    && visible(e)
            })
            .min_by(|a, b| {
                distance(p, a.transform.position).total_cmp(&distance(p, b.transform.position))
            })
            .or_else(|| {
                if stage == 3 {
                    actors.iter().find(|e| {
                        e.name == "radio-tower" && distance(p, tower) < 180. && visible(e)
                    })
                } else {
                    None
                }
            });
        let mut goal = route.front().copied().unwrap_or(p);
        if route.is_empty() {
            if stage == 1 {
                if let Some(e) = actors
                    .iter()
                    .find(|e| e.tags.iter().any(|t| t == "grp_alpha") && !dead.contains(&e.id))
                {
                    goal = e.transform.position;
                }
            }
            if stage == 4 {
                if let Some(e) = actors.iter().find(|e| e.name == "commander") {
                    goal = e.transform.position;
                }
            }
        }
        if target.is_none() {
            if let Some(e) = actors
                .iter()
                .filter(|e| {
                    distance(p, e.transform.position) < 35.
                        && ((state["health"].as_f64().unwrap() < 80.
                            && e.tags.iter().any(|t| t == "pickup_health"))
                            || (state["ammo_reserve"].as_i64().unwrap() < 60
                                && e.tags.iter().any(|t| t == "pickup_ammo")))
                })
                .min_by(|a, b| {
                    distance(p, a.transform.position).total_cmp(&distance(p, b.transform.position))
                })
            {
                goal = e.transform.position;
            }
        }
        if target.is_none() && distance(goal, navigation_goal) > 0.5 {
            navigation_path = walking_path(p, goal, &blocked);
            navigation_goal = goal;
        }
        while navigation_path
            .front()
            .is_some_and(|q| distance(p, *q) < 1.8)
        {
            navigation_path.pop_front();
        }
        let movement_goal = navigation_path.front().copied().unwrap_or(goal);
        let aim = target.map(aim_point).unwrap_or(movement_goal);
        let dx = (aim[0] - p[0]) as f64;
        let dz = (aim[2] - p[2]) as f64;
        let yaw = (-dx).atan2(-dz);
        let pitch = if target.is_some() {
            ((aim[1] - eye[1]) as f64).atan2(dx.hypot(dz))
        } else {
            0.
        };
        let mut mx = 0.;
        let mut mz = 0.;
        if target.is_none() && distance(p, movement_goal) > 1.5 {
            let dx = (movement_goal[0] - p[0]) as f64;
            let dz = (movement_goal[2] - p[2]) as f64;
            let d = dx.hypot(dz);
            mx = (dx * yaw.cos() - dz * yaw.sin()) / d;
            mz = (dx * yaw.sin() + dz * yaw.cos()) / d;
        }
        if target.is_none() && distance(p, goal) > 3. && distance(p, last_position) < 0.1 {
            stuck += 6;
        } else {
            stuck = 0;
        }
        if stuck > 240 {
            let near: Vec<_> = obstacles
                .iter()
                .filter(|o| {
                    p[0] > o.min[0] - 3.
                        && p[0] < o.max[0] + 3.
                        && p[2] > o.min[2] - 3.
                        && p[2] < o.max[2] + 3.
                })
                .map(|o| &o.name)
                .collect();
            panic!(
                "stuck tick={} stage={stage} pos={p:?} goal={goal:?} near={near:?}",
                sim.tick
            );
        }
        last_position = p;
        let inputs: Vec<_> = [
            ("move_x", mx),
            ("move_z", mz),
            ("look_yaw", yaw),
            ("look_pitch", pitch),
            ("fire", if target.is_some() { 1. } else { 0. }),
            ("reload", 0.),
            ("sprint", 1.),
            ("jump", if stuck > 30 { 1. } else { 0. }),
            (
                "travel_speed",
                if distance(p, movement_goal) < 6. {
                    1.
                } else {
                    4.
                },
            ),
        ]
        .into_iter()
        .map(|(action, value)| Input {
            tick: sim.tick,
            action: action.into(),
            value,
        })
        .collect();
        for n in 0..6 {
            let start = Instant::now();
            sim.step(1, if n == 0 { &inputs } else { &[] }).unwrap();
            let dt = start.elapsed();
            elapsed += dt;
            max_tick = max_tick.max(dt);
            let output = sim.take_events();
            for event in output["events"].as_array().unwrap() {
                if event["type"] == "kill" {
                    dead.insert(event["target"].as_u64().unwrap());
                }
            }
        }
    }
    panic!("mission timed out: {}", sim.state_summary());
}
