//! Procedural film and camera artefacts as pixel layers: light leaks,
//! dust and scratches, paper and instant-film frames, and the date stamp
//! text of a 1990s compact. All are deterministic for a given size and
//! seed so a recipe re-applies identically.

use emulsion_core::text::{Align, TextSpec};
use emulsion_raster::{Raster, color};

fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd825_5f9d) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

fn noise(x: f32, y: f32, scale: f32, seed: u32) -> f32 {
    let (fx, fy) = (x / scale, y / scale);
    let (ix, iy) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - ix, fy - iy);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (ix, iy) = (ix as i32 as u32, iy as i32 as u32);
    let a = hash(ix, iy, seed);
    let b = hash(ix.wrapping_add(1), iy, seed);
    let c = hash(ix, iy.wrapping_add(1), seed);
    let d = hash(ix.wrapping_add(1), iy.wrapping_add(1), seed);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

fn premul(rgb: [f32; 3], a: f32) -> [u16; 4] {
    let a = a.clamp(0.0, 1.0);
    color::f_to_px([
        color::srgb_to_linear(rgb[0]) * a,
        color::srgb_to_linear(rgb[1]) * a,
        color::srgb_to_linear(rgb[2]) * a,
        a,
    ])
}

/// Where a light leak enters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LeakSide {
    #[default]
    Right,
    Left,
    Top,
    Bottom,
}

/// A warm streak of light from one edge, to be blended with Screen.
/// `strength` 0–1 sets its reach and opacity; `color` is sRGB 0–255.
pub fn light_leak(
    w: u32,
    h: u32,
    strength: f32,
    color: [u8; 3],
    side: LeakSide,
    seed: u32,
) -> Raster {
    let s = strength.clamp(0.0, 1.0);
    let rgb = [
        color[0] as f32 / 255.0,
        color[1] as f32 / 255.0,
        color[2] as f32 / 255.0,
    ];
    let (fw, fh) = (w as f32, h as f32);
    Raster::from_fn(w, h, [0; 4], move |x, y| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        // Distance from the entry edge, 0 at the edge.
        let (d, along) = match side {
            LeakSide::Right => ((fw - px) / fw, py / fh),
            LeakSide::Left => (px / fw, py / fh),
            LeakSide::Top => (py / fh, px / fw),
            LeakSide::Bottom => ((fh - py) / fh, px / fw),
        };
        let reach = 0.10 + 0.28 * s;
        // Wobble the edge and vary the intensity along it.
        let wobble = (noise(along * fh, 0.0, fh * 0.35, seed) - 0.5) * 0.25;
        let band = ((reach + wobble - d) / reach).clamp(0.0, 1.0);
        let streak = 0.6 + 0.4 * noise(along * fh, d * fh, fh * 0.12, seed + 1);
        let a = band * band * streak * (0.22 + 0.4 * s);
        // Leaks go orange at the edge and redder as they fade.
        let fade = 1.0 - band;
        let rgb = [
            rgb[0],
            rgb[1] * (1.0 - 0.35 * fade),
            rgb[2] * (1.0 - 0.6 * fade),
        ];
        premul(rgb, a)
    })
}

/// Dust specks, hairs and scratches, to be blended with Screen (light
/// dust on a print) or Multiply (dirt); `amount` 0–1.
pub fn dust(w: u32, h: u32, amount: f32, seed: u32) -> Raster {
    let a = amount.clamp(0.0, 1.0);
    let (fw, fh) = (w as f32, h as f32);
    let cell = (fw.min(fh) / 18.0).max(8.0);
    // A few fine scratches at deterministic positions: (x, start, length, width).
    let scratches: Vec<(f32, f32, f32, f32)> = (0..(1.0 + a * 3.0) as u32)
        .map(|i| {
            (
                hash(i, 7, seed) * fw,
                hash(i, 10, seed) * 0.5,
                0.15 + 0.45 * hash(i, 8, seed),
                0.35 + 0.6 * hash(i, 9, seed),
            )
        })
        .collect();
    Raster::from_fn(w, h, [0; 4], move |x, y| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let mut v = 0.0f32;
        // Specks: one candidate per cell, present with probability ~amount.
        let (cx, cy) = ((px / cell).floor() as u32, (py / cell).floor() as u32);
        for dy in 0..3u32 {
            for dx in 0..3u32 {
                let (gx, gy) = (
                    cx.wrapping_add(dx).wrapping_sub(1),
                    cy.wrapping_add(dy).wrapping_sub(1),
                );
                if hash(gx, gy, seed + 3) > 0.35 + 0.6 * (1.0 - a) {
                    let ox = (gx as f32 + hash(gx, gy, seed + 4)) * cell;
                    let oy = (gy as f32 + hash(gx, gy, seed + 5)) * cell;
                    let r = 0.6 + 2.2 * hash(gx, gy, seed + 6) * a;
                    let d = ((px - ox).powi(2) + (py - oy).powi(2)).sqrt();
                    v = v.max((1.0 - d / r).clamp(0.0, 1.0));
                }
            }
        }
        for &(sx, start, len, width) in &scratches {
            let drift = (noise(0.0, py, fh * 0.12, seed + 11) - 0.5) * fw * 0.06;
            let d = (px - sx - drift).abs();
            let t = py / fh;
            if t > start && t < start + len {
                let along = (0.3 + 0.7 * noise(py, 0.0, 6.0, seed + 12)).powi(2);
                v = v.max(((1.0 - d / width) * along).clamp(0.0, 1.0));
            }
        }
        premul([1.0, 0.97, 0.9], v * (0.45 + 0.5 * a))
    })
}

/// Frame kinds a recipe can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Frame {
    #[default]
    None,
    /// Instant-film print: white border, deep at the bottom.
    Polaroid,
    /// 35 mm negative strip with sprocket holes.
    Film,
    /// Aged paper print with a thin white border and yellowed corners.
    Paper,
}

impl Frame {
    pub fn parse(s: &str) -> Option<Frame> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "none" | "" => Frame::None,
            "polaroid" | "instant" | "instax" => Frame::Polaroid,
            "film" | "negative" | "35mm" => Frame::Film,
            "paper" | "print" | "old" => Frame::Paper,
            _ => return None,
        })
    }
    pub fn key(self) -> &'static str {
        match self {
            Frame::None => "none",
            Frame::Polaroid => "polaroid",
            Frame::Film => "film",
            Frame::Paper => "paper",
        }
    }
}

/// A frame the size of the document, transparent where the picture shows.
pub fn frame(w: u32, h: u32, kind: Frame, seed: u32) -> Option<Raster> {
    let (fw, fh) = (w as f32, h as f32);
    let short = fw.min(fh);
    match kind {
        Frame::None => None,
        Frame::Polaroid => {
            let side = short * 0.055;
            let bottom = short * 0.22;
            let paper = [0.965, 0.955, 0.93];
            Some(Raster::from_fn(w, h, [0; 4], move |x, y| {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let inside = px > side && px < fw - side && py > side && py < fh - bottom;
                if inside {
                    return [0; 4];
                }
                // Slight paper texture and a shadow line at the window edge.
                let tex = 0.985 + 0.03 * noise(px, py, 3.0, seed);
                let edge = (px - side)
                    .abs()
                    .min((fw - side - px).abs())
                    .min((py - side).abs())
                    .min((fh - bottom - py).abs());
                let shade = 1.0 - 0.18 * (1.0 - (edge / 3.0).clamp(0.0, 1.0));
                premul(
                    [
                        paper[0] * tex * shade,
                        paper[1] * tex * shade,
                        paper[2] * tex * shade,
                    ],
                    1.0,
                )
            }))
        }
        Frame::Film => {
            let band = short * 0.11;
            let hole_w = band * 0.42;
            let hole_h = band * 0.32;
            let pitch = band * 0.75;
            Some(Raster::from_fn(w, h, [0; 4], move |x, y| {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let in_band = py < band || py > fh - band;
                if !in_band {
                    return [0; 4];
                }
                // Sprocket holes: rounded rectangles along each band.
                let cy = if py < band {
                    band * 0.5
                } else {
                    fh - band * 0.5
                };
                let cx = ((px / pitch).floor() + 0.5) * pitch;
                let dx = (px - cx).abs() - hole_w / 2.0;
                let dy = (py - cy).abs() - hole_h / 2.0;
                let hole = dx.max(dy) < 0.0;
                if hole {
                    return [0; 4];
                }
                let base = 0.07 + 0.02 * noise(px, py, 2.5, seed);
                // Edge print text band, faint.
                let mark =
                    if (px / (pitch * 4.0)).fract() < 0.18 && ((py - cy).abs() / hole_h) > 0.9 {
                        0.22
                    } else {
                        0.0
                    };
                premul(
                    [base + mark * 0.9, base + mark * 0.6, base + mark * 0.1],
                    1.0,
                )
            }))
        }
        Frame::Paper => {
            let border = short * 0.035;
            Some(Raster::from_fn(w, h, [0; 4], move |x, y| {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let inside = px > border && px < fw - border && py > border && py < fh - border;
                // Yellowed corners bleed a little onto the picture.
                let corner = ((px - fw * 0.5).abs() / fw + (py - fh * 0.5).abs() / fh - 0.62)
                    .clamp(0.0, 1.0)
                    * 2.4;
                if inside {
                    let a = corner * 0.35 * (0.7 + 0.3 * noise(px, py, 40.0, seed));
                    return premul([0.82, 0.68, 0.42], a);
                }
                let tex = 0.94 + 0.05 * noise(px, py, 2.0, seed + 1);
                let age = 0.35 + 0.5 * corner.min(1.0);
                premul(
                    [
                        0.96 * tex,
                        (0.94 - 0.07 * age) * tex,
                        (0.88 - 0.16 * age) * tex,
                    ],
                    1.0,
                )
            }))
        }
    }
}

/// The orange seven-segment-style date a compact camera burned into the
/// corner. `date` like "'98 12 24"; None uses today's date in that form.
pub fn date_stamp(w: u32, h: u32, date: Option<&str>) -> TextSpec {
    let text = match date {
        Some(d) if !d.trim().is_empty() => d.trim().to_string(),
        _ => today_stamp(),
    };
    let short = w.min(h) as f32;
    let size = (short * 0.062).clamp(12.0, 260.0);
    TextSpec {
        text,
        font: String::new(),
        size,
        line_height: 1.0,
        color: [255, 150, 40, 255],
        bold: true,
        italic: false,
        align: Align::Right,
        x: w as f32 * 0.04,
        y: h as f32 - size * 2.1,
        width: Some(w as f32 * 0.92),
        letter_spacing: size * 0.08,
        rotation: 0.0,
        ..Default::default()
    }
}

fn today_stamp() -> String {
    // Days since the epoch → civil date, without pulling in a time crate.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("'{:02} {} {}", y % 100, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_have_the_expected_shape() {
        let (w, h) = (200, 120);
        let leak = light_leak(w, h, 0.8, [255, 140, 60], LeakSide::Right, 1);
        assert!(
            leak.get(198, 60)[3] > leak.get(60, 60)[3],
            "leak strongest at its edge"
        );
        assert_eq!(leak.get(5, 60)[3], 0, "leak does not reach the far side");
        let d = dust(w, h, 0.6, 2);
        let specks = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| d.get(x, y)[3] > 0)
            .count();
        assert!(specks > 50 && specks < (w * h / 2) as usize, "{specks}");
        let p = frame(w, h, Frame::Polaroid, 0).unwrap();
        assert_eq!(p.get(100, 60), [0; 4], "window is clear");
        assert!(
            p.get(2, 60)[3] == 65535 && p.get(100, 115)[3] == 65535,
            "border and deep bottom"
        );
        let f = frame(w, h, Frame::Film, 0).unwrap();
        assert_eq!(f.get(100, 60), [0; 4]);
        assert!(f.get(3, 2)[3] == 65535, "band");
        assert!(frame(w, h, Frame::None, 0).is_none());
        assert_eq!(Frame::parse("Instax"), Some(Frame::Polaroid));
        let stamp = date_stamp(w, h, Some("'98 12 24"));
        assert_eq!(stamp.text, "'98 12 24");
        let today = date_stamp(w, h, None).text;
        assert!(
            today.starts_with('\'') && today.split(' ').count() == 3,
            "{today}"
        );
    }
}
