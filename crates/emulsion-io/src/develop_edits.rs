//! Content-addressed local edits. UI and MCP use the same assets and renderer.
use crate::{IoError, Result};
use emulsion_core::develop_edits::*;
use emulsion_raster::Raster;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
fn bad(s: impl ToString) -> IoError {
    IoError::Manifest(s.to_string())
}
pub fn path(digest: &[u8; 32]) -> std::path::PathBuf {
    crate::recent::data_dir()
        .join("develop-edits")
        .join(format!(
            "{}.json",
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
}
pub fn store(edits: &LocalEdits) -> Result<[u8; 32]> {
    edits.validate().map_err(bad)?;
    let bytes = serde_json::to_vec(edits).map_err(bad)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(bad("Local edit asset exceeds 4 MiB"));
    }
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let target = path(&digest);
    std::fs::create_dir_all(target.parent().unwrap())?;
    if !target.exists() {
        let mut file = tempfile::NamedTempFile::new_in(target.parent().unwrap())?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(&target) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.error.into()),
        }
    }
    load(&digest)?;
    Ok(digest)
}
pub fn load(digest: &[u8; 32]) -> Result<LocalEdits> {
    use std::io::Read;
    let mut bytes = vec![];
    std::fs::File::open(path(digest))?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 || <[u8; 32]>::from(Sha256::digest(&bytes)) != *digest {
        return Err(bad("Local edit asset is corrupt"));
    }
    let edits: LocalEdits = serde_json::from_slice(&bytes).map_err(bad)?;
    edits.validate().map_err(bad)?;
    Ok(edits)
}
fn feather(distance: f32, radius: f32, soft: f32) -> f32 {
    let t = ((radius - distance) / (radius * soft).max(1e-6)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1])
        / (d[0] * d[0] + d[1] * d[1]).max(1e-10))
    .clamp(0., 1.);
    ((p[0] - a[0] - t * d[0]).powi(2) + (p[1] - a[1] - t * d[1]).powi(2)).sqrt()
}
fn luma(p: [f32; 3]) -> f32 {
    p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722
}
pub fn weight(
    mask: &Mask,
    xy: [f32; 2],
    pixel: [f32; 3],
    bitmaps: &std::collections::HashMap<[u8; 32], emulsion_raster::Mask>,
) -> f32 {
    if !mask.enabled {
        return 0.;
    }
    let mut weight: f32 = 0.;
    for c in &mask.components {
        let v = match &c.shape {
            Shape::Brush {
                points,
                radius,
                feather: f,
            } => {
                let distance = if points.len() == 1 {
                    segment(xy, points[0], points[0])
                } else {
                    points
                        .windows(2)
                        .map(|p| segment(xy, p[0], p[1]))
                        .fold(f32::INFINITY, f32::min)
                };
                feather(distance, *radius, *f)
            }
            Shape::Radial {
                center,
                radius,
                feather: f,
            } => feather(
                (((xy[0] - center[0]) / radius[0]).powi(2)
                    + ((xy[1] - center[1]) / radius[1]).powi(2))
                .sqrt(),
                1.,
                *f,
            ),
            Shape::Linear { start, end } => {
                let d = [end[0] - start[0], end[1] - start[1]];
                (((xy[0] - start[0]) * d[0] + (xy[1] - start[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]))
                    .clamp(0., 1.)
            }
            Shape::Luminance { range, feather: f } => {
                let y = luma(pixel);
                let half = (range[1] - range[0]) * 0.5;
                feather((y - (range[0] + half)).abs(), half, *f)
            }
            Shape::Color {
                rgb,
                tolerance,
                feather: f,
            } => feather(
                pixel
                    .iter()
                    .zip(rgb)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    .sqrt(),
                *tolerance,
                *f,
            ),
            Shape::Bitmap { digest, inverted } => {
                let m = &bitmaps[digest];
                let v = m.get(
                    ((xy[0] * m.width() as f32) as u32).min(m.width() - 1),
                    ((xy[1] * m.height() as f32) as u32).min(m.height() - 1),
                ) as f32
                    / 255.;
                if *inverted { 1. - v } else { v }
            }
        };
        weight = match c.operation {
            Operation::Add => weight.max(v),
            Operation::Subtract => weight * (1. - v),
            Operation::Intersect => weight * v,
        };
    }
    weight
}
pub fn apply(input: Raster, edits: &LocalEdits, cancel: &AtomicBool) -> Result<Raster> {
    let (w, h) = (input.width(), input.height());
    let mut pixels = input.to_pixels();
    let original = pixels.clone();
    let mut bitmaps = std::collections::HashMap::new();
    for mask in &edits.masks {
        for c in &mask.components {
            if let Shape::Bitmap { digest, .. } = c.shape {
                bitmaps
                    .entry(digest)
                    .or_insert(crate::photo_develop::load_mask(&digest)?);
            }
        }
    }
    let sample = |p: [f32; 2]| {
        original[(p[1] * h as f32).clamp(0., h as f32 - 1.) as usize * w as usize
            + (p[0] * w as f32).clamp(0., w as f32 - 1.) as usize]
    };
    let spot_bounds: Vec<_> = edits
        .spots
        .iter()
        .map(|spot| {
            let points = if spot.stroke.is_empty() {
                std::slice::from_ref(&spot.target)
            } else {
                &spot.stroke
            };
            let mut b = [1f32, 1., 0., 0.];
            for p in points {
                b[0] = b[0].min(p[0] - spot.radius);
                b[1] = b[1].min(p[1] - spot.radius);
                b[2] = b[2].max(p[0] + spot.radius);
                b[3] = b[3].max(p[1] + spot.radius);
            }
            b
        })
        .collect();
    for y in 0..h {
        if cancel.load(Ordering::Relaxed) {
            return Err(bad("Development cancelled"));
        }
        for x in 0..w {
            let xy = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
            let i = (y * w + x) as usize;
            let mut p = [pixels[i][0], pixels[i][1], pixels[i][2]].map(|v| v as f32 / 65535.);
            for (spot, bounds) in edits.spots.iter().zip(&spot_bounds) {
                if xy[0] < bounds[0] || xy[1] < bounds[1] || xy[0] > bounds[2] || xy[1] > bounds[3]
                {
                    continue;
                }
                let distance = if spot.stroke.len() > 1 {
                    spot.stroke
                        .windows(2)
                        .map(|p| segment(xy, p[0], p[1]))
                        .fold(f32::INFINITY, f32::min)
                } else {
                    segment(xy, spot.target, spot.target)
                };
                let alpha = feather(distance, spot.radius, spot.feather) * spot.opacity;
                if alpha == 0. {
                    continue;
                }
                let at = [
                    spot.source[0] + xy[0] - spot.target[0],
                    spot.source[1] + xy[1] - spot.target[1],
                ];
                if at.iter().any(|v| !(0.0..=1.0).contains(v)) {
                    continue;
                }
                let s = sample(at);
                let sc = sample(spot.source);
                let tc = sample(spot.target);
                for c in 0..3 {
                    let correction = if spot.mode == SpotMode::Heal {
                        (tc[c] as f32 - sc[c] as f32) / 65535.
                    } else {
                        0.
                    };
                    p[c] = p[c] * (1. - alpha) + (s[c] as f32 / 65535. + correction) * alpha;
                }
            }
            let reference = p;
            for mask in &edits.masks {
                let a = weight(mask, xy, reference, &bitmaps);
                if a == 0. {
                    continue;
                }
                let gray = luma(p);
                for c in 0..3 {
                    let v = (gray + (p[c] - gray) * (1. + mask.saturation * a))
                        * 2f32.powf(mask.exposure * a);
                    p[c] = (v - 0.18) * (1. + mask.contrast * a) + 0.18;
                }
                p[0] *= 2f32.powf(mask.temperature * a * 0.7);
                p[2] *= 2f32.powf(-mask.temperature * a * 0.7);
                p[1] *= 2f32.powf(-mask.tint * a * 0.4);
            }
            for c in 0..3 {
                pixels[i][c] = (p[c].clamp(0., 1.) * 65535. + 0.5) as u16;
            }
        }
    }
    Ok(Raster::from_pixels(w, h, [0; 4], &pixels))
}

/// Magenta overlay on the untransformed preview; never baked into output.
pub fn overlay(
    bytes: &mut [u8],
    w: u32,
    h: u32,
    reference: &Raster,
    edits: &LocalEdits,
    selected: Option<u32>,
) -> Result<()> {
    overlay_oriented(bytes, w, h, reference, edits, selected, 0)
}
/// The reference and masks remain in source coordinates; display pixels may be quarter-turned.
pub fn overlay_oriented(
    bytes: &mut [u8],
    w: u32,
    h: u32,
    reference: &Raster,
    edits: &LocalEdits,
    selected: Option<u32>,
    rotation: u8,
) -> Result<()> {
    let mut bitmaps = std::collections::HashMap::new();
    for m in &edits.masks {
        for c in &m.components {
            if let Shape::Bitmap { digest, .. } = c.shape {
                bitmaps
                    .entry(digest)
                    .or_insert(crate::photo_develop::load_mask(&digest)?);
            }
        }
    }
    for (i, p) in bytes.chunks_exact_mut(4).enumerate() {
        let xy = [
            ((i as u32 % w) as f32 + 0.5) / w as f32,
            ((i as u32 / w) as f32 + 0.5) / h as f32,
        ];
        let [x, y] = xy;
        let xy = match rotation % 4 {
            1 => [y, 1. - x],
            2 => [1. - x, 1. - y],
            3 => [1. - y, x],
            _ => [x, y],
        };
        let sample = reference.get(
            (xy[0] * reference.width() as f32) as u32,
            (xy[1] * reference.height() as f32) as u32,
        );
        let rgb = [sample[0], sample[1], sample[2]].map(|v| v as f32 / 65535.);
        let alpha = edits
            .masks
            .iter()
            .filter(|m| selected.is_none_or(|id| id == m.id))
            .map(|m| weight(m, xy, rgb, &bitmaps))
            .fold(0., f32::max)
            * 0.5;
        for c in 0..3 {
            p[c] = (p[c] as f32 * (1. - alpha) + [255., 0., 255.][c] * alpha) as u8;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mask() -> Mask {
        Mask {
            id: 1,
            name: "Center".into(),
            enabled: true,
            components: vec![Component {
                operation: Operation::Add,
                shape: Shape::Radial {
                    center: [0.5, 0.5],
                    radius: [0.4, 0.4],
                    feather: 0.1,
                },
            }],
            exposure: 1.,
            contrast: 0.,
            saturation: 0.,
            temperature: 0.,
            tint: 0.,
        }
    }
    #[test]
    fn rotated_mask_overlay_follows_the_original_source_coordinates() {
        let reference = Raster::solid(30, 20, [0.2, 0.3, 0.4, 1.]);
        let mut m = mask();
        m.components[0].shape = Shape::Radial {
            center: [0.2, 0.3],
            radius: [0.15, 0.2],
            feather: 0.5,
        };
        let edits = LocalEdits {
            masks: vec![m],
            ..Default::default()
        };
        let mut before = reference.to_srgba8();
        overlay(&mut before, 30, 20, &reference, &edits, None).unwrap();
        let mut after = vec![0; before.len()];
        let base = reference.to_srgba8();
        for y in 0..20usize {
            for x in 0..30usize {
                let dest = (x * 20 + (19 - y)) * 4;
                let source = (y * 30 + x) * 4;
                after[dest..dest + 4].copy_from_slice(&base[source..source + 4]);
            }
        }
        overlay_oriented(&mut after, 20, 30, &reference, &edits, None, 1).unwrap();
        for y in 0..20usize {
            for x in 0..30usize {
                let dest = (x * 20 + (19 - y)) * 4;
                let source = (y * 30 + x) * 4;
                for c in 0..4 {
                    assert!((after[dest + c] as i16 - before[source + c] as i16).abs() <= 1);
                }
            }
        }
    }
    #[test]
    fn composition_preserves_excluded_pixels_and_original() {
        let original = Raster::solid(21, 21, [0.2, 0.2, 0.2, 1.]);
        let before = original.to_pixels();
        let mut m = mask();
        m.components.push(Component {
            operation: Operation::Subtract,
            shape: Shape::Brush {
                points: vec![[0.5, 0.5]],
                radius: 0.1,
                feather: 0.1,
            },
        });
        let edits = LocalEdits {
            masks: vec![m],
            ..Default::default()
        };
        let result = apply(original.clone(), &edits, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.get(10, 10), original.get(10, 10));
        assert!(result.get(14, 10)[0] > original.get(14, 10)[0]);
        assert_eq!(result.get(0, 0), original.get(0, 0));
        assert_eq!(before, original.to_pixels());
    }
    #[test]
    fn freehand_clone_covers_the_stroke_and_preserves_other_pixels() {
        let mut pixels = vec![[1000, 1000, 1000, 65535]; 100 * 100];
        for y in 10..35 {
            for x in 10..35 {
                pixels[y * 100 + x] = [30000, 2000, 1000, 65535];
            }
        }
        let input = Raster::from_pixels(100, 100, [0; 4], &pixels);
        let edits = LocalEdits {
            spots: vec![Spot {
                id: 1,
                source: [0.155, 0.155],
                target: [0.655, 0.655],
                stroke: vec![[0.655, 0.655], [0.755, 0.755]],
                radius: 0.025,
                feather: 0.2,
                opacity: 1.,
                mode: SpotMode::Clone,
            }],
            ..Default::default()
        };
        edits.validate().unwrap();
        let output = apply(input.clone(), &edits, &AtomicBool::new(false)).unwrap();
        assert_eq!(output.get(65, 65), input.get(15, 15));
        assert_eq!(output.get(75, 75), input.get(25, 25));
        assert_eq!(output.get(40, 40), input.get(40, 40));
    }
    #[test]
    fn clone_uses_source_coordinates_and_cancel_is_visible() {
        let mut px = vec![[1000, 1000, 1000, 65535]; 20 * 20];
        px[5 * 20 + 5] = [30000, 1000, 1000, 65535];
        let input = Raster::from_pixels(20, 20, [0; 4], &px);
        let edits = LocalEdits {
            spots: vec![Spot {
                id: 1,
                source: [0.275, 0.275],
                target: [0.775, 0.775],
                stroke: vec![],
                radius: 0.08,
                feather: 0.1,
                opacity: 1.,
                mode: SpotMode::Clone,
            }],
            ..Default::default()
        };
        let output = apply(input.clone(), &edits, &AtomicBool::new(false)).unwrap();
        assert_eq!(output.get(15, 15), input.get(5, 5));
        assert!(apply(input, &edits, &AtomicBool::new(true)).is_err());
    }
}

/// Preview-only high-pass view for finding small dust marks; alpha is preserved.
pub fn visualize_dust(bgra: &mut [u8], width: u32, height: u32) {
    if width == 0 || height == 0 || bgra.len() != width as usize * height as usize * 4 {
        return;
    }
    let luma: Vec<f32> = bgra
        .chunks_exact(4)
        .map(|p| (p[2] as f32 * 0.2126 + p[1] as f32 * 0.7152 + p[0] as f32 * 0.0722) / 255.)
        .collect();
    let (w, h) = (width as usize, height as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.;
            let mut count = 0.;
            for dy in [-3isize, 0, 3] {
                for dx in [-3isize, 0, 3] {
                    let sx = x.saturating_add_signed(dx).min(w - 1);
                    let sy = y.saturating_add_signed(dy).min(h - 1);
                    sum += luma[sy * w + sx];
                    count += 1.;
                }
            }
            let value = ((luma[y * w + x] - sum / count).abs() * 1800.).clamp(0., 255.) as u8;
            bgra[(y * w + x) * 4..(y * w + x) * 4 + 3].fill(value);
        }
    }
}
