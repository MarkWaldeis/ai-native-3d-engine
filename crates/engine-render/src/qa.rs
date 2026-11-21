use crate::{
    encode_png,
    inspection::{self, Bounds},
    Renderer,
};
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use shared::scene::Scene;

impl Renderer {
    pub fn screenshot_qa(
        &mut self,
        scene: &Scene,
        revision: u64,
        w: u32,
        h: u32,
        views: Option<&[String]>,
        overlay_ids: bool,
    ) -> Result<String> {
        if w < 128 || h < 96 || w > 2048 || h > 2048 {
            bail!("QA tile size must be 128..=2048 by 96..=2048");
        }
        if views.is_some_and(|v| v.is_empty() || v.len() > 7) {
            bail!("views must contain 1..=7 view names");
        }
        let fallback = vec!["current".to_string()];
        let names = views.unwrap_or(&fallback);
        let entities: Vec<_> = scene
            .entities
            .iter()
            .filter(|e| e.visible)
            .map(|e| Ok((e, inspection::bounds(e)?)))
            .collect::<Result<_>>()?;
        let bounds = entities
            .iter()
            .map(|(_, b)| *b)
            .reduce(Bounds::union)
            .unwrap_or(Bounds {
                min: [-1.0; 3],
                max: [1.0; 3],
            });
        let columns = (names.len() as f32).sqrt().ceil() as u32;
        let rows = (names.len() as u32).div_ceil(columns);
        if u64::from(columns * w) * u64::from(rows * h) > 20_000_000 {
            bail!("contact sheet exceeds 20 megapixels; reduce tile width/height");
        }
        let mut sheet = vec![24_u8; (columns * w * rows * h * 4) as usize];
        for pixel in sheet.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        for (index, name) in names.iter().enumerate() {
            let mut tile_scene = scene.clone();
            let (near, far) = if views.is_some() {
                tile_scene.camera =
                    inspection::framed_camera(bounds, scene.camera, name, w as f32 / h as f32)?;
                let distance = tile_scene
                    .camera
                    .position
                    .iter()
                    .zip(tile_scene.camera.target)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    .sqrt();
                (
                    0.001_f32.min(distance * 0.001).max(0.00001),
                    (distance * 3.0).max(1.0),
                )
            } else {
                (0.05, 500.0)
            };
            let mut tile =
                self.render_rgba_camera(&tile_scene, revision, w, h, near, far, views.is_some())?;
            if overlay_ids {
                let matrix = inspection::matrix(
                    &tile_scene.camera,
                    w as f32 / h as f32,
                    near,
                    far,
                    views.is_some(),
                );
                let mut occupied = vec![];
                for (entity, bounds) in &entities {
                    let p = inspection::transform(matrix, bounds.center());
                    if p[3] <= 0.0
                        || p[2] < 0.0
                        || p[2] > p[3]
                        || p[0].abs() > p[3]
                        || p[1].abs() > p[3]
                    {
                        continue;
                    }
                    let label = format!(
                        "{} {}",
                        entity.id,
                        entity.name.chars().take(12).collect::<String>()
                    );
                    let scale = if w >= 400 { 2 } else { 1 };
                    let label_width = label.chars().count() as i32 * 6 * scale;
                    let spacing = 7 * scale + 5;
                    let px = ((p[0] / p[3] + 1.0) * 0.5 * w as f32) as i32;
                    let py = ((1.0 - p[1] / p[3]) * 0.5 * h as f32) as i32;
                    let x = px.clamp(2, (w as i32 - label_width - 4).max(2));
                    let mut y = py.clamp(30, h as i32 - spacing);
                    for _ in 0..(h as i32 / spacing) {
                        if !occupied.iter().any(|&(ox, oy, ow): &(i32, i32, i32)| {
                            (x - ox).abs() < ow.max(label_width) && (y - oy).abs() < spacing
                        }) {
                            break;
                        }
                        y += spacing;
                        if y > h as i32 - spacing {
                            y = 30;
                        }
                    }
                    occupied.push((x, y, label_width));
                    let steps = (px - x).abs().max((py - y).abs()).max(1);
                    for n in 0..=steps {
                        put(
                            &mut tile,
                            w,
                            h,
                            px + (x - px) * n / steps,
                            py + (y - py) * n / steps,
                            [240, 240, 240, 255],
                        );
                    }
                    text(&mut tile, w, h, x, y, &label, scale);
                }
            }
            if views.is_some() {
                text(&mut tile, w, h, 8, 8, name, 2);
            }
            let ox = index as u32 % columns * w;
            let oy = index as u32 / columns * h;
            for y in 0..h {
                let source = (y * w * 4) as usize;
                let dest = (((oy + y) * columns * w + ox) * 4) as usize;
                sheet[dest..dest + (w * 4) as usize]
                    .copy_from_slice(&tile[source..source + (w * 4) as usize]);
            }
        }
        Ok(STANDARD.encode(encode_png(&sheet, columns * w, rows * h)?))
    }
}

fn text(pixels: &mut [u8], w: u32, h: u32, x: i32, y: i32, label: &str, scale: i32) {
    let width = label.chars().count() as i32 * 6 * scale + 4;
    for py in y - 2..y + 7 * scale + 2 {
        for px in x - 2..x + width {
            put(pixels, w, h, px, py, [18, 22, 28, 255]);
        }
    }
    for (n, c) in label.chars().enumerate() {
        for (row, bits) in glyph(c.to_ascii_uppercase()).iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            put(
                                pixels,
                                w,
                                h,
                                x + n as i32 * 6 * scale + col * scale + dx,
                                y + row as i32 * scale + dy,
                                [255, 245, 200, 255],
                            );
                        }
                    }
                }
            }
        }
    }
}
fn put(pixels: &mut [u8], w: u32, h: u32, x: i32, y: i32, color: [u8; 4]) {
    if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
        let i = ((y as u32 * w + x as u32) * 4) as usize;
        pixels[i..i + 4].copy_from_slice(&color);
    }
}

fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bitmap_labels_have_opaque_background_and_distinct_headless_glyphs() {
        let mut pixels = vec![255; 160 * 40 * 4];
        text(&mut pixels, 160, 40, 8, 8, "FRONT 12-A", 2);
        assert_eq!(
            &pixels[(6 * 160 + 6) * 4..(6 * 160 + 6) * 4 + 4],
            &[18, 22, 28, 255]
        );
        assert!(pixels.chunks_exact(4).any(|p| p == [255, 245, 200, 255]));
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_".chars() {
            assert_ne!(glyph(c), [0; 7]);
        }
        assert_ne!(glyph('F'), glyph('B'));
    }
}
