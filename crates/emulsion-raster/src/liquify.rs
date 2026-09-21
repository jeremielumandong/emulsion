//! Liquify: push, twirl, pinch and expand pixels under a soft brush, or
//! restore them toward the original. Each dab resamples the current
//! layer inside the brush footprint, so strokes accumulate the way
//! Photoshop's Forward Warp does: what is under the brush travels with it.

use crate::geom::IRect;
use crate::image::Raster;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Pixels move with the brush.
    Push,
    /// Rotate around the brush centre; clockwise when `cw`.
    Twirl { cw: bool },
    /// Pull pixels toward the centre (pucker).
    Pinch,
    /// Push pixels away from the centre (bloat).
    Expand,
    /// Blend back toward the original pixels.
    Restore,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Push => "push",
            Mode::Twirl { cw: true } => "twirl ↻",
            Mode::Twirl { cw: false } => "twirl ↺",
            Mode::Pinch => "pinch",
            Mode::Expand => "expand",
            Mode::Restore => "restore",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s.trim().to_lowercase().as_str() {
            "push" | "warp" | "forward" => Mode::Push,
            "twirl" | "twirl_cw" | "twirl-cw" | "cw" => Mode::Twirl { cw: true },
            "twirl_ccw" | "twirl-ccw" | "ccw" => Mode::Twirl { cw: false },
            "pinch" | "pucker" => Mode::Pinch,
            "expand" | "bloat" => Mode::Expand,
            "restore" | "reconstruct" => Mode::Restore,
            _ => return None,
        })
    }
}

/// Smooth falloff from 1 at the centre to 0 at the rim.
#[inline]
fn falloff(d: f32) -> f32 {
    if d >= 1.0 {
        0.0
    } else {
        let t = 1.0 - d * d;
        t * t
    }
}

/// Bilinear sample of a dense `rect`-sized block, clamped to its edges.
fn sample(px: &[[u16; 4]], rect: IRect, x: f32, y: f32) -> [u16; 4] {
    let fx = (x - rect.x as f32 - 0.5).clamp(0.0, (rect.w - 1) as f32);
    let fy = (y - rect.y as f32 - 0.5).clamp(0.0, (rect.h - 1) as f32);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = (
        (x0 + 1).min(rect.w as usize - 1),
        (y0 + 1).min(rect.h as usize - 1),
    );
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let w = rect.w as usize;
    let p = [
        px[y0 * w + x0],
        px[y0 * w + x1],
        px[y1 * w + x0],
        px[y1 * w + x1],
    ];
    let k = [
        (1.0 - tx) * (1.0 - ty),
        tx * (1.0 - ty),
        (1.0 - tx) * ty,
        tx * ty,
    ];
    let mut out = [0u16; 4];
    for (c, o) in out.iter_mut().enumerate() {
        let v: f32 = (0..4).map(|i| p[i][c] as f32 * k[i]).sum();
        *o = v.round().clamp(0.0, 65535.0) as u16;
    }
    out
}

/// One dab of the liquify brush at `center` (layer pixels) with
/// `radius`; `strength` 0–1 scales the effect; `delta` is how far the
/// pointer moved since the last dab (Push follows it). `original` is the
/// layer before the tool started (for Restore). Returns the new layer
/// and the rectangle that changed; unchanged when the footprint misses
/// the layer.
pub fn dab(
    current: &Raster,
    original: &Raster,
    mode: Mode,
    center: (f32, f32),
    radius: f32,
    strength: f32,
    delta: (f32, f32),
) -> (Raster, IRect) {
    let r = radius.max(1.0);
    let s = strength.clamp(0.0, 1.0);
    let foot = IRect::new(
        (center.0 - r).floor() as i32,
        (center.1 - r).floor() as i32,
        (2.0 * r).ceil() as i32 + 2,
        (2.0 * r).ceil() as i32 + 2,
    )
    .intersect(&current.bounds());
    if foot.is_empty() || s <= 0.0 {
        return (current.clone(), IRect::default());
    }
    // The furthest any pixel is fetched from, so the source block covers it.
    let reach = match mode {
        Mode::Push => delta.0.hypot(delta.1) * s,
        Mode::Twirl { .. } => r * 0.25 * s,
        Mode::Pinch | Mode::Expand => r * 0.12 * s,
        Mode::Restore => 0.0,
    }
    .ceil() as i32
        + 2;
    let src_rect = IRect::new(
        foot.x - reach,
        foot.y - reach,
        foot.w + 2 * reach,
        foot.h + 2 * reach,
    );
    let src = current.read_rect(src_rect);
    let orig = if mode == Mode::Restore {
        current
            .read_rect(foot)
            .into_iter()
            .zip(original.read_rect(foot))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut out = Vec::with_capacity((foot.w * foot.h) as usize);
    for y in foot.y..foot.bottom() {
        for x in foot.x..foot.right() {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (dx, dy) = (px - center.0, py - center.1);
            let d = dx.hypot(dy) / r;
            let w = falloff(d) * s;
            if w <= 0.0 {
                out.push(sample(&src, src_rect, px, py));
                continue;
            }
            let p = match mode {
                Mode::Push => sample(&src, src_rect, px - delta.0 * w, py - delta.1 * w),
                Mode::Twirl { cw } => {
                    let a = 0.25 * w * if cw { -1.0 } else { 1.0 };
                    let (sa, ca) = a.sin_cos();
                    sample(
                        &src,
                        src_rect,
                        center.0 + dx * ca - dy * sa,
                        center.1 + dx * sa + dy * ca,
                    )
                }
                Mode::Pinch => {
                    let k = 1.0 + 0.12 * w;
                    sample(&src, src_rect, center.0 + dx * k, center.1 + dy * k)
                }
                Mode::Expand => {
                    let k = 1.0 - 0.12 * w;
                    sample(&src, src_rect, center.0 + dx * k, center.1 + dy * k)
                }
                Mode::Restore => {
                    let i = ((y - foot.y) * foot.w + (x - foot.x)) as usize;
                    let (cur, org) = orig[i];
                    let t = (0.35 * w).min(1.0);
                    let mut o = [0u16; 4];
                    for c in 0..4 {
                        o[c] = (cur[c] as f32 + (org[c] as f32 - cur[c] as f32) * t).round() as u16;
                    }
                    o
                }
            };
            out.push(p);
        }
    }
    (current.write_rect(foot, &out), foot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker() -> Raster {
        let mut pixels = vec![[0; 4]; 65 * 65];
        for y in 31..=33 {
            for x in 41..=43 {
                pixels[y * 65 + x] = [65535, 0, 0, 65535];
            }
        }
        Raster::from_pixels(65, 65, [0; 4], &pixels)
    }

    fn red_centroid(image: &Raster) -> (f64, f64) {
        let mut weighted = (0.0, 0.0);
        let mut total = 0.0;
        for (index, pixel) in image.to_pixels().iter().enumerate() {
            let weight = f64::from(pixel[0]);
            weighted.0 += (index % image.width() as usize) as f64 * weight;
            weighted.1 += (index / image.width() as usize) as f64 * weight;
            total += weight;
        }
        assert!(total > 0.0, "the marker must remain visible");
        (weighted.0 / total, weighted.1 / total)
    }

    #[test]
    fn twirl_directions_move_a_marker_clockwise_and_counterclockwise() {
        let original = marker();
        for clockwise in [true, false] {
            let mut image = original.clone();
            for _ in 0..8 {
                image = dab(
                    &image,
                    &original,
                    Mode::Twirl { cw: clockwise },
                    (32.5, 32.5),
                    24.0,
                    1.0,
                    (0.0, 0.0),
                )
                .0;
            }
            let (x, y) = red_centroid(&image);
            // Image Y grows downward: a marker to the right must turn down
            // clockwise and up counterclockwise, remaining on the right.
            assert!(x > 32.0 && x < 41.0, "marker rotated inward in X: {x}, {y}");
            if clockwise {
                assert!(y > 35.0, "clockwise moved down: {x}, {y}");
            } else {
                assert!(y < 29.0, "counterclockwise moved up: {x}, {y}");
            }
        }
    }

    #[test]
    fn pinch_and_expand_move_a_marker_in_opposite_radial_directions() {
        let original = marker();
        for mode in [Mode::Pinch, Mode::Expand] {
            let mut image = original.clone();
            for _ in 0..8 {
                image = dab(&image, &original, mode, (32.5, 32.5), 24.0, 1.0, (0.0, 0.0)).0;
            }
            let (x, y) = red_centroid(&image);
            assert!(
                (y - 32.0).abs() < 0.01,
                "radial operation must not rotate the marker"
            );
            match mode {
                Mode::Pinch => assert!(x > 32.0 && x < 39.0, "pinch pulled inward: {x}"),
                Mode::Expand => assert!(x > 45.0, "expand pushed outward: {x}"),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn push_at_canvas_edge_pulls_transparency_without_color_fringes() {
        let red = [65535, 0, 0, 65535];
        let original = Raster::from_pixels(16, 16, [0; 4], &[red; 256]);
        let (image, dirty) = dab(
            &original,
            &original,
            Mode::Push,
            (1.5, 8.5),
            6.0,
            1.0,
            (4.0, 0.0),
        );
        assert_eq!(
            image.get(1, 8),
            [0; 4],
            "off-canvas source pixels must be transparent"
        );
        assert_eq!(
            image.get(15, 8),
            red,
            "pixels beyond the brush remain untouched"
        );
        assert_eq!(dirty.intersect(&original.bounds()), dirty);
        for pixel in image.to_pixels() {
            assert_eq!(
                pixel[0], pixel[3],
                "resampling must preserve premultiplied red"
            );
            assert_eq!([pixel[1], pixel[2]], [0, 0]);
        }
    }

    #[test]
    fn restore_recovers_original_color_and_alpha_only_under_the_brush() {
        let target = [16000, 0, 0, 16000];
        let blue = [0, 0, 65535, 65535];
        let original = Raster::from_pixels(16, 16, [0; 4], &[target; 256]);
        let mut image = Raster::from_pixels(16, 16, [0; 4], &[blue; 256]);
        let mut last = blue;
        for _ in 0..12 {
            image = dab(
                &image,
                &original,
                Mode::Restore,
                (8.5, 8.5),
                5.0,
                1.0,
                (0.0, 0.0),
            )
            .0;
            let next = image.get(8, 8);
            assert!(next[0] >= last[0] && next[0] <= target[0]);
            assert!(next[2] <= last[2] && next[3] <= last[3] && next[3] >= target[3]);
            assert_eq!(
                image.get(0, 0),
                blue,
                "restore must not replace the whole layer"
            );
            last = next;
        }
        for (actual, expected) in last.into_iter().zip(target) {
            assert!(
                actual.abs_diff(expected) < 500,
                "restored color and opacity converge"
            );
        }
    }

    #[test]
    fn every_mode_is_a_noop_at_zero_strength_or_outside_the_layer() {
        let original = marker();
        for mode in [
            Mode::Push,
            Mode::Twirl { cw: true },
            Mode::Twirl { cw: false },
            Mode::Pinch,
            Mode::Expand,
            Mode::Restore,
        ] {
            for (center, strength) in [((32.5, 32.5), 0.0), ((-100.0, -100.0), 1.0)] {
                let (image, dirty) = dab(
                    &original,
                    &original,
                    mode,
                    center,
                    8.0,
                    strength,
                    (5.0, 2.0),
                );
                assert!(dirty.is_empty(), "{mode:?} should not invalidate pixels");
                assert_eq!(image.to_pixels(), original.to_pixels(), "{mode:?}");
            }
        }
    }

    fn stripes() -> Raster {
        let mut px = vec![[0u16, 0, 0, 65535]; 100 * 100];
        for y in 0..100 {
            for x in 0..100 {
                if x >= 50 {
                    px[y * 100 + x] = [65535, 65535, 65535, 65535];
                }
            }
        }
        Raster::transparent(100, 100).write_rect(IRect::new(0, 0, 100, 100), &px)
    }

    #[test]
    fn push_moves_the_edge_and_restore_brings_it_back() {
        let base = stripes();
        let mut cur = base.clone();
        // Push the black/white edge to the right by dragging across it.
        for i in 0..10 {
            let c = (44.0 + i as f32 * 2.0, 50.0);
            let (r, dirty) = dab(&cur, &base, Mode::Push, c, 20.0, 1.0, (2.0, 0.0));
            assert!(!dirty.is_empty());
            cur = r;
        }
        assert!(
            cur.get(52, 50)[0] < 20000,
            "black pushed right over x=52: {:?}",
            cur.get(52, 50)
        );
        assert_eq!(cur.get(52, 5)[0], 65535, "far rows untouched");
        for _ in 0..30 {
            cur = dab(
                &cur,
                &base,
                Mode::Restore,
                (50.0, 50.0),
                30.0,
                1.0,
                (0.0, 0.0),
            )
            .0;
        }
        assert!(
            cur.get(52, 50)[0] > 60000,
            "restored: {:?}",
            cur.get(52, 50)
        );

        // Pinch pulls white in from the right; expand pushes black out.
        // Centred left of the edge: pinch pulls the edge toward the centre
        // (white reaches x = 49), expand pushes it away (black reaches 50).
        let p = dab(
            &base,
            &base,
            Mode::Pinch,
            (40.0, 50.0),
            20.0,
            1.0,
            (0.0, 0.0),
        )
        .0;
        assert!(p.get(49, 50)[0] > 30000, "{:?}", p.get(49, 50));
        let e = dab(
            &base,
            &base,
            Mode::Expand,
            (40.0, 50.0),
            20.0,
            1.0,
            (0.0, 0.0),
        )
        .0;
        assert!(e.get(50, 50)[0] < 30000, "{:?}", e.get(50, 50));
        let t = dab(
            &base,
            &base,
            Mode::Twirl { cw: true },
            (50.0, 50.0),
            20.0,
            1.0,
            (0.0, 0.0),
        )
        .0;
        assert_ne!(t.get(50, 40), t.get(50, 60), "twirl breaks the symmetry");
        assert!(
            dab(
                &base,
                &base,
                Mode::Push,
                (-500.0, 0.0),
                5.0,
                1.0,
                (1.0, 0.0)
            )
            .1
            .is_empty()
        );
        assert_eq!(Mode::parse("bloat"), Some(Mode::Expand));
    }
}
