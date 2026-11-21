//! Deterministic, independently addressable CPU heightfield tiles.
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use shared::mesh::{MeshData, Vertex};
use std::{fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TerrainConfig {
    pub size_m: [f64; 2],
    pub tile_m: f64,
    pub samples: usize,
    pub height_scale: f64,
    pub seed: u64,
}
impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            size_m: [44000000.0_f64.sqrt(); 2],
            tile_m: 256.0,
            samples: 129,
            height_scale: 200.0,
            seed: 1,
        }
    }
}
impl TerrainConfig {
    pub fn preset_44km2() -> Self {
        Self::default()
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.size_m.iter().all(|v| v.is_finite() && *v > 0.0)
                && self.tile_m.is_finite()
                && self.tile_m > 0.0,
            "invalid terrain dimensions"
        );
        ensure!(
            (2..=4097).contains(&self.samples)
                && self.height_scale.is_finite()
                && self.height_scale >= 0.0,
            "invalid samples/height scale"
        );
        ensure!(
            self.grid().iter().all(|n| *n <= 4096),
            "terrain grid too large"
        );
        Ok(())
    }
    pub fn grid(&self) -> [u32; 2] {
        self.size_m.map(|v| (v / self.tile_m).ceil() as u32)
    }
    pub fn spacing(&self) -> f64 {
        self.tile_m / (self.samples - 1) as f64
    }
    pub fn height(&self, x: f64, z: f64) -> f32 {
        let mut sum = 0.0;
        let mut weight = 0.5;
        let mut frequency = 1.0 / 1024.0;
        for octave in 0..6 {
            let n = noise(x * frequency, z * frequency, self.seed.wrapping_add(octave));
            sum += (1.0 - n.abs()) * weight;
            weight *= 0.5;
            frequency *= 2.0;
        }
        (sum * self.height_scale) as f32
    }
    pub fn generate_tile(&self, x: u32, z: u32) -> Result<Tile> {
        self.validate()?;
        ensure!(
            x < self.grid()[0] && z < self.grid()[1],
            "tile outside grid"
        );
        let n = self.samples;
        // A one-sample halo preserves central differences even when loaded alone.
        let mut heights = Vec::with_capacity((n + 2) * (n + 2));
        for j in -1..=n as i64 {
            for i in -1..=n as i64 {
                let gx = x as i64 * (n - 1) as i64 + i;
                let gz = z as i64 * (n - 1) as i64 + j;
                heights.push(self.height(gx as f64 * self.spacing(), gz as f64 * self.spacing()));
            }
        }
        Ok(Tile { x, z, heights })
    }
}
fn noise(x: f64, z: f64, seed: u64) -> f64 {
    let hash = |x: i64, z: i64| {
        let mut h = (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
            ^ (z as u64).wrapping_mul(0xbf58476d1ce4e5b9)
            ^ seed;
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((h ^ (h >> 31)) >> 11) as f64 / ((1u64 << 53) as f64) * 2.0 - 1.0
    };
    let ix = x.floor() as i64;
    let iz = z.floor() as i64;
    let smooth = |v: f64| v * v * (3.0 - 2.0 * v);
    let u = smooth(x - x.floor());
    let v = smooth(z - z.floor());
    let a = hash(ix, iz) * (1.0 - u) + hash(ix + 1, iz) * u;
    let b = hash(ix, iz + 1) * (1.0 - u) + hash(ix + 1, iz + 1) * u;
    a * (1.0 - v) + b * v
}
#[derive(Clone, Debug)]
pub struct Tile {
    pub x: u32,
    pub z: u32,
    pub heights: Vec<f32>,
}
impl Tile {
    pub fn height(&self, config: &TerrainConfig, x: usize, z: usize) -> f32 {
        self.heights[(z + 1) * (config.samples + 2) + x + 1]
    }
    pub fn mesh(&self, config: &TerrainConfig) -> MeshData {
        let n = config.samples;
        let stride = n + 2;
        let d = config.spacing() as f32;
        let mut mesh = MeshData::new();
        for z in 0..n {
            for x in 0..n {
                let k = (z + 1) * stride + x + 1;
                let normal = [
                    -(self.heights[k + 1] - self.heights[k - 1]) / (2.0 * d),
                    1.0,
                    -(self.heights[k + stride] - self.heights[k - stride]) / (2.0 * d),
                ];
                let len = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
                mesh.vertices.push(Vertex::new(
                    [x as f32 * d, self.heights[k], z as f32 * d],
                    normal.map(|v| v / len),
                    [x as f32 / (n - 1) as f32, z as f32 / (n - 1) as f32],
                ));
                if x + 1 < n && z + 1 < n {
                    let a = (z * n + x) as u32;
                    let b = a + n as u32;
                    mesh.indices
                        .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
                }
            }
        }
        mesh
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TileFile {
    pub x: u32,
    pub z: u32,
    pub file: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub config: TerrainConfig,
    pub grid: [u32; 2],
    pub height_range: [f32; 2],
    pub tiles: Vec<TileFile>,
}
#[derive(Clone, Debug)]
pub struct Terrain {
    pub manifest: Manifest,
    pub tiles: Vec<Tile>,
}
impl Terrain {
    pub fn generate(config: TerrainConfig) -> Result<Self> {
        config.validate()?;
        let grid = config.grid();
        let mut tiles = Vec::new();
        for z in 0..grid[1] {
            for x in 0..grid[0] {
                tiles.push(config.generate_tile(x, z)?);
            }
        }
        Ok(Self::from_tiles(config, tiles))
    }
    fn from_tiles(config: TerrainConfig, tiles: Vec<Tile>) -> Self {
        let mut range = [f32::INFINITY, f32::NEG_INFINITY];
        for t in &tiles {
            for h in &t.heights {
                range[0] = range[0].min(*h);
                range[1] = range[1].max(*h);
            }
        }
        let files = tiles
            .iter()
            .map(|t| TileFile {
                x: t.x,
                z: t.z,
                file: format!("tile_{}_{}.f32", t.x, t.z),
            })
            .collect();
        Self {
            manifest: Manifest {
                version: 1,
                grid: config.grid(),
                config,
                height_range: range,
                tiles: files,
            },
            tiles,
        }
    }
    /// Input rows run from minimum to maximum world Z. Raw R16 is little endian.
    pub fn import(
        path: &Path,
        config: TerrainConfig,
        raw_dimensions: Option<[usize; 2]>,
    ) -> Result<Self> {
        config.validate()?;
        let (width, height, values) = if path.extension().is_some_and(|e| e == "png") {
            let mut reader = png::Decoder::new(fs::File::open(path)?).read_info()?;
            let mut bytes = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut bytes)?;
            ensure!(
                info.bit_depth == png::BitDepth::Sixteen
                    && info.color_type == png::ColorType::Grayscale,
                "expected 16-bit grayscale PNG"
            );
            (
                info.width as usize,
                info.height as usize,
                bytes[..info.buffer_size()]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
        } else {
            let [w, h] = raw_dimensions
                .ok_or_else(|| anyhow::anyhow!("raw r16 requires width and height"))?;
            let bytes = fs::read(path)?;
            ensure!(
                w.checked_mul(h).and_then(|n| n.checked_mul(2)) == Some(bytes.len()),
                "raw size mismatch"
            );
            (
                w,
                h,
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect(),
            )
        };
        ensure!(width >= 2 && height >= 2, "heightmap must be at least 2x2");
        let sample = |x: f64, z: f64| {
            let u = (x / config.size_m[0]).clamp(0.0, 1.0) * (width - 1) as f64;
            let v = (z / config.size_m[1]).clamp(0.0, 1.0) * (height - 1) as f64;
            let i = u.floor() as usize;
            let j = v.floor() as usize;
            let a = u - i as f64;
            let b = v - j as f64;
            let at =
                |i: usize, j: usize| values[j.min(height - 1) * width + i.min(width - 1)] as f64;
            (((at(i, j) * (1.0 - a) + at(i + 1, j) * a) * (1.0 - b)
                + (at(i, j + 1) * (1.0 - a) + at(i + 1, j + 1) * a) * b)
                / 65535.0
                * config.height_scale) as f32
        };
        let mut tiles = Vec::new();
        let n = config.samples;
        for z in 0..config.grid()[1] {
            for x in 0..config.grid()[0] {
                let mut heights = Vec::new();
                for j in -1..=n as i64 {
                    for i in -1..=n as i64 {
                        heights.push(sample(
                            (x as i64 * (n - 1) as i64 + i) as f64 * config.spacing(),
                            (z as i64 * (n - 1) as i64 + j) as f64 * config.spacing(),
                        ));
                    }
                }
                tiles.push(Tile { x, z, heights });
            }
        }
        Ok(Self::from_tiles(config, tiles))
    }
    pub fn save(&self, directory: &Path) -> Result<()> {
        ensure!(
            !directory.exists() || directory.read_dir()?.next().is_none(),
            "output directory must be empty"
        );
        fs::create_dir_all(directory)?;
        for (tile, file) in self.tiles.iter().zip(&self.manifest.tiles) {
            fs::write(
                directory.join(&file.file),
                tile.heights
                    .iter()
                    .flat_map(|h| h.to_le_bytes())
                    .collect::<Vec<_>>(),
            )?;
        }
        fs::write(
            directory.join("terrain.json"),
            serde_json::to_vec_pretty(&self.manifest)?,
        )?;
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let manifest: Manifest = serde_json::from_slice(&fs::read(path)?)?;
        manifest.config.validate()?;
        ensure!(
            manifest.version == 1 && manifest.grid == manifest.config.grid(),
            "unsupported terrain manifest"
        );
        ensure!(
            manifest.tiles.len() == manifest.grid[0] as usize * manifest.grid[1] as usize,
            "incomplete terrain grid"
        );
        let root = path.parent().unwrap_or(Path::new(".")).canonicalize()?;
        let mut tiles = Vec::new();
        for (index, file) in manifest.tiles.iter().enumerate() {
            ensure!(
                file.x == index as u32 % manifest.grid[0]
                    && file.z == index as u32 / manifest.grid[0],
                "invalid tile order"
            );
            let path = root.join(&file.file).canonicalize()?;
            ensure!(path.starts_with(&root), "tile path escape");
            let bytes = fs::read(path)?;
            ensure!(
                bytes.len() == (manifest.config.samples + 2).pow(2) * 4,
                "invalid tile length"
            );
            let heights: Vec<_> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            ensure!(heights.iter().all(|h| h.is_finite()), "nonfinite height");
            tiles.push(Tile {
                x: file.x,
                z: file.z,
                heights,
            });
        }
        Ok(Self { manifest, tiles })
    }
    pub fn sample(&self, x: f64, z: f64) -> Result<f32> {
        let c = &self.manifest.config;
        if !x.is_finite()
            || !z.is_finite()
            || x < 0.0
            || z < 0.0
            || x > c.size_m[0]
            || z > c.size_m[1]
        {
            bail!("position outside terrain");
        }
        let tx = ((x / c.tile_m).floor() as u32).min(c.grid()[0] - 1);
        let tz = ((z / c.tile_m).floor() as u32).min(c.grid()[1] - 1);
        let t = &self.tiles[(tz * c.grid()[0] + tx) as usize];
        let u = (x - tx as f64 * c.tile_m) / c.spacing();
        let v = (z - tz as f64 * c.tile_m) / c.spacing();
        let i = (u.floor() as usize).min(c.samples - 2);
        let j = (v.floor() as usize).min(c.samples - 2);
        let a = (u - i as f64) as f32;
        let b = (v - j as f64) as f32;
        Ok(if a + b <= 1.0 {
            t.height(c, i, j) * (1.0 - a - b)
                + t.height(c, i + 1, j) * a
                + t.height(c, i, j + 1) * b
        } else {
            t.height(c, i + 1, j) * (1.0 - b)
                + t.height(c, i, j + 1) * (1.0 - a)
                + t.height(c, i + 1, j + 1) * (a + b - 1.0)
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldPos(pub [f64; 3]);
impl WorldPos {
    pub fn relative_to(self, origin: Self) -> [f32; 3] {
        std::array::from_fn(|i| (self.0[i] - origin.0[i]) as f32)
    }
    pub fn from_relative(local: [f32; 3], origin: Self) -> Self {
        Self(std::array::from_fn(|i| origin.0[i] + local[i] as f64))
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FloatingOrigin {
    pub origin: WorldPos,
    pub threshold: f64,
    pub tile_m: f64,
}
impl FloatingOrigin {
    pub fn rebase(&mut self, camera: WorldPos) -> bool {
        let distance = (0..3)
            .map(|i| (camera.0[i] - self.origin.0[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        if distance <= self.threshold {
            return false;
        }
        self.origin = WorldPos(camera.0.map(|v| (v / self.tile_m).round() * self.tile_m));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_and_empty_config() {
        let a = TerrainConfig::default();
        let b = TerrainConfig {
            seed: a.seed + 1,
            ..a.clone()
        };
        assert_ne!(a.height(123.0, 456.0), b.height(123.0, 456.0));
        assert!(TerrainConfig {
            size_m: [0.0, 1.0],
            ..a.clone()
        }
        .validate()
        .is_err());
        assert!(TerrainConfig {
            tile_m: f64::NAN,
            ..a
        }
        .validate()
        .is_err());
    }

    #[test]
    fn mesh_is_local_and_faces_up() {
        let config = TerrainConfig {
            samples: 3,
            ..Default::default()
        };
        let tile = config.generate_tile(25, 25).unwrap();
        let mesh = tile.mesh(&config);
        assert_eq!(mesh.vertices.len(), 9);
        assert_eq!(mesh.indices.len(), 24);
        assert_eq!(mesh.vertices[0].position[0], 0.0);
        assert_eq!(mesh.vertices[8].position[2], 256.0);
        assert!(mesh.vertices.iter().all(|v| v.normal[1] > 0.0));
    }

    #[test]
    fn sample_interpolates_the_mesh_triangles_not_a_bilinear_patch() {
        let config = TerrainConfig {
            size_m: [256.0; 2],
            samples: 2,
            height_scale: 0.0,
            ..Default::default()
        };
        let mut terrain = Terrain::generate(config).unwrap();
        terrain.tiles[0].heights[10] = 200.0;
        for (x, z, expected) in [
            (0.0, 0.0, 0.0),
            (256.0, 0.0, 0.0),
            (0.0, 256.0, 0.0),
            (256.0, 256.0, 200.0),
            (128.0, 128.0, 0.0),
            (64.0, 64.0, 0.0),
            (192.0, 192.0, 100.0),
            (128.0, 256.0, 100.0),
        ] {
            assert_eq!(terrain.sample(x, z).unwrap(), expected);
        }
    }
}
