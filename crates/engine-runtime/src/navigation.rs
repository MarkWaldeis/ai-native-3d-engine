use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use shared::{
    components::{BodyType, Collider},
    navigation::*,
    scene::{PrimitiveKind, Scene},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const MAX_POLYGONS: usize = 65_536;

fn fingerprint(bytes: &[u8]) -> String {
    let mut h = 0xcbf29ce484222325u64;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub fn validate_params(p: &NavParams) -> Result<()> {
    ensure!(
        [
            p.radius,
            p.height,
            p.step,
            p.max_slope_deg,
            p.cell_size,
            p.tile_size
        ]
        .iter()
        .all(|v| v.is_finite()),
        "navigation parameters must be finite"
    );
    ensure!(
        p.radius > 0.0 && p.radius <= 10.0 && p.height >= 2.0 * p.radius && p.height <= 20.0,
        "invalid navigation agent radius/height"
    );
    ensure!(
        (0.0..=2.0).contains(&p.step) && (0.0..=75.0).contains(&p.max_slope_deg),
        "invalid navigation step/slope"
    );
    ensure!(
        (0.1..=4.0).contains(&p.cell_size) && (p.cell_size..=1024.0).contains(&p.tile_size),
        "invalid navigation cell/tile size"
    );
    ensure!(
        (p.tile_size / p.cell_size - (p.tile_size / p.cell_size).round()).abs() < 1e-4,
        "tile_size must be a multiple of cell_size"
    );
    if let Some(b) = p.bounds {
        ensure!(
            b.iter().all(|v| v.is_finite() && v.abs() <= 1e6) && b[0] < b[2] && b[1] < b[3],
            "invalid navigation bounds [min_x,min_z,max_x,max_z]"
        );
    }
    Ok(())
}

pub fn validate(mesh: &NavMesh) -> Result<()> {
    validate_params(&mesh.params)?;
    ensure!(
        mesh.version == 1 && mesh.tiles.len() <= 1024,
        "unsupported/oversized navigation asset"
    );
    ensure!(
        mesh.tiles.iter().map(|t| t.polygons.len()).sum::<usize>() <= MAX_POLYGONS,
        "navigation polygon limit exceeded"
    );
    let mut seen = BTreeSet::new();
    for tile in &mesh.tiles {
        for p in &tile.polygons {
            ensure!(
                p.y.is_finite()
                    && p.corners.iter().all(|v| v.is_finite())
                    && p.cell.iter().all(|v| v.unsigned_abs() <= 10_000_000),
                "invalid navigation polygon"
            );
            ensure!(
                seen.insert((p.cell, (p.y * 1000.0).round() as i64)),
                "duplicate navigation polygon"
            );
            ensure!(
                p.cell.map(
                    |v| (v as f32 * mesh.params.cell_size / mesh.params.tile_size).floor() as i32
                ) == tile.region,
                "navigation polygon in wrong region"
            );
        }
    }
    Ok(())
}

pub fn bake(scene: &Scene, root: &Path, params: NavParams) -> Result<(NavMesh, Value)> {
    validate_params(&params)?;
    ensure!(
        !scene
            .entities
            .iter()
            .any(|e| e.props.get("ai_terrain").is_some() && e.props["nav_ignore"] != true),
        "heightfield navigation requires collider proxies in this version"
    );
    let mut geometry = Scene::default();
    let mut hashes = BTreeMap::new();
    let mut files: BTreeMap<std::path::PathBuf, String> = BTreeMap::new();
    for e in &scene.entities {
        if e.nav_agent.is_some()
            || e.props["nav_ignore"] == true
            || e.rigidbody
                .as_ref()
                .is_some_and(|b| b.kind != BodyType::Static)
        {
            continue;
        }
        let physics = crate::physics_config::Config::from_entity(e)?;
        if physics.sensor || physics.ragdoll.is_some()
            || (physics.collider.is_none() && e.collider.is_none() && e.props["nav_geometry"] != true) {
            continue;
        }
        let mut e = e.clone();
        if let Some(props) = e.props.as_object_mut() {
            props.remove("ai_physics");
        }
        let mut file_hash = String::new();
        if physics.collider.is_none() {
            e.collider.get_or_insert(Collider::Mesh);
            if let PrimitiveKind::Gltf { path } = &mut e.mesh {
                let full = engine_core::project_path(root, Path::new(path))?;
                file_hash = if let Some(h) = files.get(&full) {
                    h.clone()
                } else {
                    let h = fingerprint(&std::fs::read(&full)?);
                    files.insert(full.clone(), h.clone());
                    h
                };
                *path = full.to_string_lossy().into_owned();
            }
        }
        let shape = if let Some(collider) = physics.collider {
            let compound = matches!(&collider, crate::physics_config::Geometry::Compound { .. });
            e.props["ai_physics"] = json!({"collider":collider});
            if compound {
                json!([e.props["ai_physics"], "compound_part_seam_steps"])
            } else {
                e.props["ai_physics"].clone()
            }
        } else if matches!(e.mesh, PrimitiveKind::Gltf { .. }) {
            json!({"kind":"gltf","content":file_hash})
        } else {
            serde_json::to_value(&e.mesh)?
        };
        hashes.insert(
            e.id,
            fingerprint(&serde_json::to_vec(&json!([
                shape,
                e.transform,
                e.collider,
                e.props["web_mesh_bounds"]
            ]))?),
        );
        geometry.entities.push(e);
    }
    let mut physics = crate::physics::Physics::new(root.into());
    physics.reconcile(&geometry)?;
    physics.prepare_nav_queries();
    let boxes = physics.nav_bounds();
    let mut bounds = params.bounds.unwrap_or([
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ]);
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for (_, min, max) in &boxes {
        ensure!(
            min.iter()
                .chain(max.iter())
                .all(|v| v.is_finite() && v.abs() <= 1e6),
            "navigation geometry exceeds world bounds"
        );
        if params.bounds.is_none() {
            bounds[0] = bounds[0].min(min[0]);
            bounds[1] = bounds[1].min(min[2]);
            bounds[2] = bounds[2].max(max[0]);
            bounds[3] = bounds[3].max(max[2]);
        }
        low = low.min(min[1]);
        high = high.max(max[1]);
    }
    let all_hash = fingerprint(&serde_json::to_vec(&json!([params, hashes]))?);
    let mut mesh = NavMesh {
        version: 1,
        params: params.clone(),
        fingerprint: all_hash,
        tiles: vec![],
    };
    if boxes.is_empty() {
        return Ok((mesh, json!({"baked_tiles":0,"reused_tiles":0,"polygons":0})));
    }
    let s = params.cell_size;
    let start = [
        (bounds[0] / s).floor() as i32,
        (bounds[1] / s).floor() as i32,
    ];
    let end = [(bounds[2] / s).ceil() as i32, (bounds[3] / s).ceil() as i32];
    ensure!(
        i64::from(end[0] - start[0]) * i64::from(end[1] - start[1]) <= MAX_POLYGONS as i64,
        "bake exceeds 65536 cells; supply bounded region bounds or a larger cell_size"
    );
    let mut cells: BTreeMap<[i32; 2], Vec<[i32; 2]>> = BTreeMap::new();
    for x in start[0]..end[0] {
        for z in start[1]..end[1] {
            let cell = [x, z];
            let region = cell.map(|v| (v as f32 * s / params.tile_size).floor() as i32);
            cells.entry(region).or_default().push(cell);
        }
    }
    ensure!(cells.len() <= 1024, "navigation tile limit exceeded");
    let mut reused = 0;
    let mut baked = 0;
    let mut count = 0;
    let slope = params.max_slope_deg.to_radians().cos();
    for (region, cells) in cells {
        let margin = params.radius + s;
        let min = [
            region[0] as f32 * params.tile_size - margin,
            region[1] as f32 * params.tile_size - margin,
        ];
        let max = [
            min[0] + params.tile_size + 2.0 * margin,
            min[1] + params.tile_size + 2.0 * margin,
        ];
        let sources: Vec<_> = boxes
            .iter()
            .filter(|(_, a, b)| {
                a[0] <= max[0] && b[0] >= min[0] && a[2] <= max[1] && b[2] >= min[1]
            })
            .map(|(id, _, _)| (&hashes[id], id))
            .collect();
        let hash = fingerprint(&serde_json::to_vec(&json!([
            params,
            sources,
            region,
            cells.first(),
            cells.last()
        ]))?);
        if let Some(old) = scene
            .navmesh
            .as_ref()
            .filter(|m| m.params == params)
            .and_then(|m| {
                m.tiles
                    .iter()
                    .find(|t| t.region == region && t.fingerprint == hash)
            })
        {
            mesh.tiles.push(old.clone());
            reused += 1;
            count += old.polygons.len();
            continue;
        }
        let mut polygons = vec![];
        for cell in cells {
            let x = (cell[0] as f32 + 0.5) * s;
            let z = (cell[1] as f32 + 0.5) * s;
            let surfaces = physics.nav_surfaces(x, z, high + 1.0, low - 1.0, slope);
            let mut last = None;
            for surface in surfaces {
                let y = surface.y;
                if last.is_some_and(|v: f32| (v - y).abs() < 0.01) {
                    continue;
                }
                let mut supports = vec![surface.collider];
                let mut corners = [y; 4];
                let mut valid = true;
                for (i, [dx, dz]) in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
                    .into_iter()
                    .enumerate()
                {
                    let d = s * 0.5 + params.radius;
                    let hits =
                        physics.nav_surfaces(x + dx * d, z + dz * d, high + 1.0, low - 1.0, slope);
                    if let Some(corner) = hits
                        .into_iter()
                        .min_by(|a, b| (a.y - y).abs().total_cmp(&(b.y - y).abs()))
                        .filter(|corner| {
                            let rise = corner.y - y;
                            let step = params.step + 0.01;
                            let slope_limit = params.max_slope_deg.to_radians().tan() * d * 1.415 + 0.01;
                            if rise.abs() > step.max(slope_limit) {
                                return false;
                            }
                            if corner.collider == surface.collider {
                                return true;
                            }
                            physics.nav_seam_step(
                                [surface.collider, corner.collider],
                                [x, y, z],
                                [x + dx * d, corner.y, z + dz * d],
                                [high + 1.0, low - 1.0],
                                slope,
                            ).is_some_and(|seam| seam <= step)
                        })
                    {
                        supports.push(corner.collider);
                        corners[i] = y + (corner.y - y) * (s * 0.5 / d);
                    } else {
                        valid = false;
                        break;
                    }
                }
                if valid
                    && physics.nav_clear(x, y, z, s * 0.5 + params.radius, params.height, &supports)
                {
                    polygons.push(NavPoly { cell, y, corners });
                    last = Some(y);
                }
            }
        }
        count += polygons.len();
        ensure!(count <= MAX_POLYGONS, "navigation polygon limit exceeded");
        mesh.tiles.push(NavTile {
            region,
            fingerprint: hash,
            polygons,
        });
        baked += 1;
    }
    validate(&mesh)?;
    Ok((
        mesh,
        json!({"baked_tiles":baked,"reused_tiles":reused,"polygons":count}),
    ))
}

struct Graph<'a> {
    mesh: &'a NavMesh,
    polys: Vec<&'a NavPoly>,
    cells: BTreeMap<[i32; 2], Vec<usize>>,
}
impl<'a> Graph<'a> {
    fn new(mesh: &'a NavMesh) -> Self {
        let polys: Vec<_> = mesh.tiles.iter().flat_map(|t| t.polygons.iter()).collect();
        let mut cells: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for (i, p) in polys.iter().enumerate() {
            cells.entry(p.cell).or_default().push(i);
        }
        Self { mesh, polys, cells }
    }
    fn center(&self, i: usize) -> [f32; 3] {
        let p = self.polys[i];
        [
            (p.cell[0] as f32 + 0.5) * self.mesh.params.cell_size,
            p.y,
            (p.cell[1] as f32 + 0.5) * self.mesh.params.cell_size,
        ]
    }
    fn project(&self, p: [f32; 3]) -> Result<usize> {
        let cell = [
            (p[0] / self.mesh.params.cell_size).floor() as i32,
            (p[2] / self.mesh.params.cell_size).floor() as i32,
        ];
        self.cells
            .get(&cell)
            .into_iter()
            .flatten()
            .copied()
            .min_by(|a, b| {
                (self.polys[*a].y - p[1])
                    .abs()
                    .total_cmp(&(self.polys[*b].y - p[1]).abs())
                    .then(a.cmp(b))
            })
            .filter(|i| {
                (self.polys[*i].y - p[1]).abs()
                    <= self.mesh.params.step.max(
                        self.mesh.params.max_slope_deg.to_radians().tan()
                            * self.mesh.params.cell_size
                            * 0.7072,
                    ) + 0.05
            })
            .context("point is outside the walkable navmesh")
    }
    fn neighbors(&self, i: usize) -> Vec<usize> {
        let p = self.polys[i];
        let mut out = vec![];
        for [dx, dz] in [[-1, 0], [0, -1], [0, 1], [1, 0]] {
            for j in self
                .cells
                .get(&[p.cell[0] + dx, p.cell[1] + dz])
                .into_iter()
                .flatten()
            {
                if (self.polys[*j].y - p.y).abs()
                    <= self.mesh.params.step.max(
                        self.mesh.params.cell_size
                            * self.mesh.params.max_slope_deg.to_radians().tan(),
                    ) + 0.001
                {
                    out.push(*j);
                }
            }
        }
        out
    }
    fn visible(&self, a: [f32; 3], b: [f32; 3], corridor: &BTreeSet<usize>) -> bool {
        if (a[1] - b[1]).abs() > 0.001 {
            return false;
        }
        let s = self.mesh.params.cell_size;
        let n = (((b[0] - a[0]).abs().max((b[2] - a[2]).abs()) / (s * 0.2)).ceil() as usize).max(1);
        let mut previous: Option<usize> = None;
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let p = [a[0] + (b[0] - a[0]) * t, a[1], a[2] + (b[2] - a[2]) * t];
            let Ok(i) = self.project(p) else {
                return false;
            };
            if !corridor.contains(&i) || (self.polys[i].y - a[1]).abs() > 0.001 {
                return false;
            }
            if let Some(j) = previous {
                let c = self.polys[j].cell;
                let d = self.polys[i].cell;
                if c[0] != d[0] && c[1] != d[1] {
                    for cross in [[c[0], d[1]], [d[0], c[1]]] {
                        if !self
                            .cells
                            .get(&cross)
                            .is_some_and(|ids| ids.iter().any(|id| corridor.contains(id)))
                        {
                            return false;
                        }
                    }
                }
            }
            previous = Some(i);
        }
        true
    }
}

pub fn query(mesh: &NavMesh, start: [f32; 3], goal: [f32; 3]) -> Result<Vec<[f32; 3]>> {
    validate(mesh)?;
    ensure!(
        start
            .iter()
            .chain(goal.iter())
            .all(|v| v.is_finite() && v.abs() <= 1e6),
        "navigation points must be finite and bounded"
    );
    let g = Graph::new(mesh);
    let a = g.project(start)?;
    let b = g.project(goal)?;
    let heuristic = |i: usize| -> u64 {
        u64::from(
            g.polys[i].cell[0].abs_diff(g.polys[b].cell[0])
                + g.polys[i].cell[1].abs_diff(g.polys[b].cell[1]),
        ) * 1000
    };
    let mut open = BTreeSet::from([(heuristic(a), 0u64, a)]);
    let mut costs = BTreeMap::from([(a, 0u64)]);
    let mut parent = BTreeMap::new();
    let mut found = false;
    while let Some((_, cost, i)) = open.pop_first() {
        if costs.get(&i) != Some(&cost) {
            continue;
        }
        if i == b {
            found = true;
            break;
        }
        for j in g.neighbors(i) {
            let next = cost + 1000 + ((g.polys[j].y - g.polys[i].y).abs() * 1000.0).round() as u64;
            if costs.get(&j).is_none_or(|old| next < *old) {
                costs.insert(j, next);
                parent.insert(j, i);
                open.insert((next + heuristic(j), next, j));
            }
        }
    }
    ensure!(found, "unreachable navigation target");
    let mut ids = vec![b];
    while *ids.last().unwrap() != a {
        ids.push(parent[ids.last().unwrap()]);
    }
    ids.reverse();
    let corridor: BTreeSet<_> = ids.iter().copied().collect();
    let mut points = vec![[start[0], g.polys[a].y, start[2]]];
    points.extend(
        ids.iter()
            .skip(1)
            .take(ids.len().saturating_sub(2))
            .map(|i| g.center(*i)),
    );
    points.push([goal[0], g.polys[b].y, goal[2]]);
    let mut pulled = vec![points[0]];
    let mut i = 0;
    while i + 1 < points.len() {
        let mut j = i + 1;
        for k in ((i + 2)..points.len()).rev() {
            if g.visible(points[i], points[k], &corridor) {
                j = k;
                break;
            }
        }
        pulled.push(points[j]);
        i = j;
        ensure!(pulled.len() <= 1024, "navigation path exceeds 1024 corners");
    }
    Ok(pulled)
}

pub fn set_target(
    mesh: &NavMesh,
    position: [f32; 3],
    agent: &mut NavAgent,
    target: [f32; 3],
) -> Result<bool> {
    ensure!(
        target.iter().all(|v| v.is_finite() && v.abs() <= 1e6),
        "navigation target must be finite and bounded"
    );
    if agent.planned_target == Some(target) {
        return Ok(agent.status != "unreachable");
    }
    let foot = [position[0], position[1] - agent.offset_y, position[2]];
    agent.target = Some(target);
    agent.planned_target = Some(target);
    agent.next = 1;
    match query(mesh, foot, target) {
        Ok(path) => {
            agent.path = path;
            agent.status = "moving".into();
            Ok(true)
        }
        Err(_) => {
            agent.path.clear();
            agent.next = 0;
            agent.status = "unreachable".into();
            Ok(false)
        }
    }
}

pub fn step(scene: &mut Scene, inactive: &BTreeSet<u64>, dt: f32) -> Result<BTreeSet<u64>> {
    let mut dirty = BTreeSet::new();
    let Some(mesh) = &scene.navmesh else {
        return Ok(dirty);
    };
    for e in &mut scene.entities {
        if inactive.contains(&e.id) {
            continue;
        }
        let Some(a) = &mut e.nav_agent else {
            continue;
        };
        if let Some(target) = a.target {
            set_target(mesh, e.transform.position, a, target)?;
        } else {
            a.path.clear();
            a.planned_target = None;
            a.status = "idle".into();
            continue;
        }
        let mut remaining = a.speed * dt;
        while a.next < a.path.len() && remaining > 0.0 {
            let p = e.transform.position;
            let t = a.path[a.next];
            let delta = [t[0] - p[0], t[1] + a.offset_y - p[1], t[2] - p[2]];
            let distance = (delta.iter().map(|v| v * v).sum::<f32>()).sqrt();
            if distance <= remaining.max(1e-6) {
                e.transform.position = [t[0], t[1] + a.offset_y, t[2]];
                remaining = (remaining - distance).max(0.0);
                a.next += 1;
            } else {
                for (i, d) in delta.into_iter().enumerate() {
                    e.transform.position[i] += d * (remaining / distance);
                }
                remaining = 0.0;
            }
            dirty.insert(e.id);
        }
        if !a.path.is_empty() && a.next == a.path.len() {
            a.status = "arrived".into();
        }
    }
    Ok(dirty)
}
