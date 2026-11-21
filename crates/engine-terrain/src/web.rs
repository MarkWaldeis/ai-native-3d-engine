//! Compact, globally quantized terrain transport and partial residency.
use crate::{Terrain, TerrainConfig, Tile};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

pub const DEFAULT_COARSE_STEP: f64 = 16.0;
const HEADER: usize = 12;

fn encode(values: &[f32], range: [f32; 2], dims: [u32; 2], magic: &[u8; 4]) -> Vec<u8> {
    let mut bytes = magic.to_vec();
    bytes.extend_from_slice(&dims[0].to_le_bytes());
    bytes.extend_from_slice(&dims[1].to_le_bytes());
    let span = range[1] as f64 - range[0] as f64;
    for h in values {
        let q = if span > 0.0 {
            ((*h as f64 - range[0] as f64) / span * 65535.0)
                .round()
                .clamp(0.0, 65535.0) as u16
        } else {
            0
        };
        bytes.extend_from_slice(&q.to_le_bytes());
    }
    bytes
}
fn decode(bytes: &[u8], range: [f32; 2], dims: [u32; 2], magic: &[u8; 4]) -> Result<Vec<f32>> {
    let count = (dims[0] as usize)
        .checked_mul(dims[1] as usize)
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(HEADER));
    ensure!(
        count == Some(bytes.len()) && bytes.len() >= HEADER,
        "invalid quantized heightfield length"
    );
    ensure!(
        &bytes[..4] == magic
            && bytes[4..8] == dims[0].to_le_bytes()
            && bytes[8..12] == dims[1].to_le_bytes(),
        "invalid heightfield header"
    );
    ensure!(
        range.iter().all(|v| v.is_finite()) && range[0] <= range[1],
        "invalid height range"
    );
    Ok(bytes[HEADER..]
        .chunks_exact(2)
        .map(|b| {
            (range[0] as f64
                + u16::from_le_bytes([b[0], b[1]]) as f64 / 65535.0
                    * (range[1] as f64 - range[0] as f64)) as f32
        })
        .collect())
}
pub fn encode_tile(tile: &Tile, samples: usize, range: [f32; 2]) -> Vec<u8> {
    encode(&tile.heights, range, [(samples + 2) as u32; 2], b"AITT")
}
pub fn decode_tile(x: u32, z: u32, bytes: &[u8], samples: usize, range: [f32; 2]) -> Result<Tile> {
    Ok(Tile {
        x,
        z,
        heights: decode(bytes, range, [(samples + 2) as u32; 2], b"AITT")?,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoarseInfo {
    pub step: f64,
    pub dims: [u32; 2],
    pub file: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebManifest {
    pub version: u32,
    pub config: TerrainConfig,
    pub grid: [u32; 2],
    pub height_range: [f32; 2],
    pub coarse: CoarseInfo,
    pub tiles: String,
}
#[derive(Clone, Debug)]
pub struct CoarseGrid {
    pub step: f64,
    pub dims: [u32; 2],
    pub size_m: [f64; 2],
    pub heights: Vec<f32>,
}
impl CoarseGrid {
    pub fn from_terrain(terrain: &Terrain, step: f64) -> Result<Self> {
        ensure!(step.is_finite() && step > 0.0, "invalid coarse step");
        let size_m = terrain.manifest.config.size_m;
        let dims = size_m.map(|s| (s / step).ceil() as u32 + 1);
        ensure!(
            (dims[0] as u64) * (dims[1] as u64) <= 16_777_216,
            "coarse grid too large"
        );
        let mut heights = Vec::new();
        for z in 0..dims[1] {
            for x in 0..dims[0] {
                heights.push(terrain.sample(
                    (x as f64 * step).min(size_m[0]),
                    (z as f64 * step).min(size_m[1]),
                )?);
            }
        }
        Ok(Self {
            step,
            dims,
            size_m,
            heights,
        })
    }
    pub fn encode(&self, range: [f32; 2]) -> Vec<u8> {
        encode(&self.heights, range, self.dims, b"AITC")
    }
    pub fn decode(
        bytes: &[u8],
        info: &CoarseInfo,
        size_m: [f64; 2],
        range: [f32; 2],
    ) -> Result<Self> {
        ensure!(
            info.step.is_finite()
                && info.step > 0.0
                && info.dims == size_m.map(|s| (s / info.step).ceil() as u32 + 1),
            "invalid coarse dimensions"
        );
        Ok(Self {
            step: info.step,
            dims: info.dims,
            size_m,
            heights: decode(bytes, range, info.dims, b"AITC")?,
        })
    }
    pub fn sample(&self, x: f64, z: f64) -> f32 {
        let p = [x, z];
        let i: [usize; 2] = std::array::from_fn(|a| {
            ((p[a].clamp(0.0, self.size_m[a]) / self.step).floor() as usize)
                .min(self.dims[a] as usize - 2)
        });
        let t: [f32; 2] = std::array::from_fn(|a| {
            ((p[a].clamp(0.0, self.size_m[a]) - i[a] as f64 * self.step)
                / (((i[a] + 1) as f64 * self.step).min(self.size_m[a]) - i[a] as f64 * self.step))
                as f32
        });
        let h = |dx, dz| self.heights[(i[1] + dz) * self.dims[0] as usize + i[0] + dx];
        (h(0, 0) * (1.0 - t[0]) + h(1, 0) * t[0]) * (1.0 - t[1])
            + (h(0, 1) * (1.0 - t[0]) + h(1, 1) * t[0]) * t[1]
    }
}
#[derive(Clone, Debug)]
pub struct StreamedTerrain {
    pub config: TerrainConfig,
    pub height_range: [f32; 2],
    pub coarse: Option<CoarseGrid>,
    pub tiles: BTreeMap<(u32, u32), Tile>,
}
impl StreamedTerrain {
    pub fn from_terrain(terrain: &Terrain) -> Self {
        Self {
            config: terrain.manifest.config.clone(),
            height_range: terrain.manifest.height_range,
            coarse: None,
            tiles: terrain
                .tiles
                .iter()
                .map(|t| ((t.x, t.z), t.clone()))
                .collect(),
        }
    }
    pub fn from_manifest(manifest: &WebManifest, coarse_bytes: &[u8]) -> Result<Self> {
        manifest.config.validate()?;
        ensure!(
            manifest.version == 1 && manifest.grid == manifest.config.grid(),
            "invalid web terrain manifest"
        );
        Ok(Self {
            config: manifest.config.clone(),
            height_range: manifest.height_range,
            coarse: Some(CoarseGrid::decode(
                coarse_bytes,
                &manifest.coarse,
                manifest.config.size_m,
                manifest.height_range,
            )?),
            tiles: BTreeMap::new(),
        })
    }
    pub fn insert_tile(&mut self, x: u32, z: u32, bytes: &[u8]) -> Result<()> {
        ensure!(
            x < self.config.grid()[0] && z < self.config.grid()[1],
            "tile outside grid"
        );
        self.tiles.insert(
            (x, z),
            decode_tile(x, z, bytes, self.config.samples, self.height_range)?,
        );
        Ok(())
    }
    pub fn sample(&self, x: f64, z: f64) -> f32 {
        let c = &self.config;
        let x = x.clamp(0.0, c.size_m[0]);
        let z = z.clamp(0.0, c.size_m[1]);
        let tx = ((x / c.tile_m).floor() as u32).min(c.grid()[0] - 1);
        let tz = ((z / c.tile_m).floor() as u32).min(c.grid()[1] - 1);
        let Some(t) = self.tiles.get(&(tx, tz)) else {
            return self.coarse.as_ref().map_or(0.0, |g| g.sample(x, z));
        };
        let u = (x - tx as f64 * c.tile_m) / c.spacing();
        let v = (z - tz as f64 * c.tile_m) / c.spacing();
        let i = (u.floor() as usize).min(c.samples - 2);
        let j = (v.floor() as usize).min(c.samples - 2);
        let a = (u - i as f64) as f32;
        let b = (v - j as f64) as f32;
        if a + b <= 1.0 {
            t.height(c, i, j) * (1.0 - a - b)
                + t.height(c, i + 1, j) * a
                + t.height(c, i, j + 1) * b
        } else {
            t.height(c, i + 1, j) * (1.0 - b)
                + t.height(c, i, j + 1) * (1.0 - a)
                + t.height(c, i + 1, j + 1) * (a + b - 1.0)
        }
    }
}
pub fn write_web_bundle(terrain: &Terrain, dir: &Path, coarse_step: f64) -> Result<()> {
    let coarse = CoarseGrid::from_terrain(terrain, coarse_step)?;
    let manifest = WebManifest {
        version: 1,
        config: terrain.manifest.config.clone(),
        grid: terrain.manifest.grid,
        height_range: terrain.manifest.height_range,
        coarse: CoarseInfo {
            step: coarse.step,
            dims: coarse.dims,
            file: "coarse.bin".into(),
        },
        tiles: "tiles/{x}_{z}.bin".into(),
    };
    fs::create_dir_all(dir.join("tiles"))?;
    fs::write(
        dir.join("terrain.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::write(dir.join("coarse.bin"), coarse.encode(manifest.height_range))?;
    for tile in &terrain.tiles {
        fs::write(
            dir.join(format!("tiles/{}_{}.bin", tile.x, tile.z)),
            encode_tile(tile, manifest.config.samples, manifest.height_range),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantized_roundtrip_streaming_and_clamping() {
        let terrain = Terrain::generate(TerrainConfig {
            size_m: [512.0; 2],
            samples: 33,
            ..Default::default()
        })
        .unwrap();
        let range = terrain.manifest.height_range;
        let tolerance = (range[1] - range[0]) / 65535.0;
        let mut streamed = StreamedTerrain::from_terrain(&terrain);
        for tile in &terrain.tiles {
            let bytes = encode_tile(tile, 33, range);
            assert_eq!(bytes.len(), 12 + 35 * 35 * 2);
            let decoded = decode_tile(tile.x, tile.z, &bytes, 33, range).unwrap();
            for (a, b) in tile.heights.iter().zip(decoded.heights) {
                assert!((a - b).abs() <= tolerance);
            }
            assert!(decode_tile(0, 0, &bytes[..20], 33, range).is_err());
            streamed.insert_tile(tile.x, tile.z, &bytes).unwrap();
        }
        for (x, z) in [(0.0, 0.0), (11.3, 201.2), (256.0, 256.0), (512.0, 512.0)] {
            assert!((streamed.sample(x, z) - terrain.sample(x, z).unwrap()).abs() <= tolerance);
        }
        assert_eq!(streamed.sample(-20.0, 900.0), streamed.sample(0.0, 512.0));
        streamed.tiles.clear();
        assert_eq!(streamed.sample(10.0, 10.0), 0.0);
        let coarse = CoarseGrid::from_terrain(&terrain, 17.0).unwrap();
        let info = CoarseInfo {
            step: 17.0,
            dims: coarse.dims,
            file: String::new(),
        };
        let decoded = CoarseGrid::decode(&coarse.encode(range), &info, [512.0; 2], range).unwrap();
        assert!(
            (decoded.sample(512.0, 512.0) - terrain.sample(512.0, 512.0).unwrap()).abs()
                <= tolerance
        );
        let expected = (decoded.heights[0]
            + decoded.heights[1]
            + decoded.heights[decoded.dims[0] as usize]
            + decoded.heights[decoded.dims[0] as usize + 1])
            / 4.0;
        assert!((decoded.sample(8.5, 8.5) - expected).abs() < 0.0001);
        streamed.coarse = Some(decoded);
        assert_eq!(streamed.sample(-1.0, 1000.0), streamed.sample(0.0, 512.0));
        assert!(streamed.insert_tile(2, 0, &[]).is_err());
        assert!(CoarseGrid::from_terrain(&terrain, 0.0).is_err());
        assert!(CoarseGrid::decode(&[], &info, [512.0; 2], range).is_err());
        let flat = Tile {
            x: 0,
            z: 0,
            heights: vec![7.0; 35 * 35],
        };
        assert_eq!(
            decode_tile(0, 0, &encode_tile(&flat, 33, [7.0; 2]), 33, [7.0; 2])
                .unwrap()
                .heights,
            flat.heights
        );
    }
}
