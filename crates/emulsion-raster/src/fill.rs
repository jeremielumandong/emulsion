//! Content-aware fill and gradients.

use crate::color;
use crate::composite::{CompositeTree, region};
use crate::{IRect, Mask, Raster};

/// Content-aware fill of `selection` against the visible image, as a layer.
///
/// Returns the pixels to put in a new node at the returned rect's origin.
/// The node holds only the fill, so composited over the image it gives the
/// filled result, and hiding it restores the original. None when the
/// selection is empty.
pub fn content_aware_layer(tree: &CompositeTree, selection: &Mask) -> Option<(Raster, IRect)> {
    let b = crate::select::bounds(selection);
    if b.is_empty() {
        return None;
    }
    let canvas = IRect::new(0, 0, selection.width() as i32, selection.height() as i32);
    let margin = (b.w.max(b.h) / 2).max(48);
    let reg = IRect::new(
        b.x - margin,
        b.y - margin,
        b.w + 2 * margin,
        b.h + 2 * margin,
    )
    .intersect(&canvas);
    let comp = region(tree, reg);
    let hole: Vec<f32> = selection
        .read_rect(reg)
        .into_iter()
        .map(|v| v as f32 / 255.0)
        .collect();
    let out = content_aware(&comp, &hole, reg.w as usize, reg.h as usize, 0x5EED);
    // out = fill·k + comp·(1−k), so the fill alone is out − comp·(1−k).
    let px: Vec<[u16; 4]> = out
        .iter()
        .zip(&comp)
        .zip(&hole)
        .map(|((o, c), k)| color::f_to_px([0, 1, 2, 3].map(|i| (o[i] - c[i] * (1.0 - k)).max(0.0))))
        .collect();
    Some((
        Raster::from_pixels(reg.w as u32, reg.h as u32, [0; 4], &px),
        reg,
    ))
}

/// Fill the hole in a dense premultiplied-linear image from the rest of it.
///
/// Pixels are filled from the hole's edge inwards. Each takes the colour of
/// a known pixel whose 5×5 neighbourhood best matches its own, searched
/// among the offsets its filled neighbours used (coherence) and random
/// candidates narrowing around the best. `hole` is coverage 0–1; soft edges
/// blend the original with the fill.
pub fn content_aware(
    image: &[[f32; 4]],
    hole: &[f32],
    w: usize,
    h: usize,
    seed: u64,
) -> Vec<[f32; 4]> {
    assert_eq!(image.len(), w * h);
    assert_eq!(hole.len(), w * h);
    let is_hole: Vec<bool> = hole.iter().map(|v| *v > 0.02).collect();
    if !is_hole.iter().any(|b| *b) || is_hole.iter().all(|b| *b) {
        return image.to_vec();
    }
    let mut out = image.to_vec();
    let mut done: Vec<bool> = is_hole.iter().map(|h| !h).collect();
    let mut offset: Vec<Option<(i32, i32)>> = vec![None; w * h];
    let mut rng = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut rand = move |n: usize| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % n.max(1) as u64) as usize
    };
    let known: Vec<usize> = (0..w * h).filter(|i| !is_hole[*i]).collect();

    // Onion peel: breadth-first from the known boundary.
    let mut order = Vec::new();
    let mut queued = vec![false; w * h];
    let mut queue = std::collections::VecDeque::new();
    for i in 0..w * h {
        if is_hole[i] {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0
                    && ny >= 0
                    && (nx as usize) < w
                    && (ny as usize) < h
                    && !is_hole[ny as usize * w + nx as usize]
            });
            if edge {
                queued[i] = true;
                queue.push_back(i);
            }
        }
    }
    while let Some(i) = queue.pop_front() {
        order.push(i);
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                let j = ny as usize * w + nx as usize;
                if is_hole[j] && !queued[j] {
                    queued[j] = true;
                    queue.push_back(j);
                }
            }
        }
    }

    const R: i32 = 2;
    for &p in &order {
        let (px, py) = ((p % w) as i32, (p / w) as i32);
        let cost = |o: (i32, i32), out: &[[f32; 4]], done: &[bool]| -> f32 {
            let (qx, qy) = (px + o.0, py + o.1);
            if qx < 0
                || qy < 0
                || qx as usize >= w
                || qy as usize >= h
                || is_hole[qy as usize * w + qx as usize]
            {
                return f32::MAX;
            }
            let (mut sum, mut n) = (0.0, 0);
            for sy in -R..=R {
                for sx in -R..=R {
                    let (ax, ay, bx, by) = (px + sx, py + sy, qx + sx, qy + sy);
                    if ax < 0
                        || ay < 0
                        || bx < 0
                        || by < 0
                        || ax as usize >= w
                        || ay as usize >= h
                        || bx as usize >= w
                        || by as usize >= h
                    {
                        continue;
                    }
                    let (a, b) = (ay as usize * w + ax as usize, by as usize * w + bx as usize);
                    if !done[a] || is_hole[b] {
                        continue;
                    }
                    let d: f32 = (0..4).map(|c| (out[a][c] - image[b][c]).powi(2)).sum();
                    sum += d;
                    n += 1;
                }
            }
            if n == 0 {
                f32::MAX / 2.0
            } else {
                sum / n as f32
            }
        };
        let mut best = (0, 0);
        let mut best_cost = f32::MAX;
        let try_o = |o: (i32, i32), best: &mut (i32, i32), best_cost: &mut f32| {
            let c = cost(o, &out, &done);
            if c < *best_cost {
                *best_cost = c;
                *best = o;
            }
        };
        for (dx, dy) in [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, 1),
            (-1, 1),
            (1, -1),
        ] {
            let (nx, ny) = (px + dx, py + dy);
            if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                let n = ny as usize * w + nx as usize;
                // Coherence: reuse the offset a filled neighbour chose.
                let o = if is_hole[n] { offset[n] } else { None };
                if let Some(o) = o {
                    try_o(o, &mut best, &mut best_cost);
                }
            }
        }
        for _ in 0..8 {
            let k = known[rand(known.len())];
            try_o(
                ((k % w) as i32 - px, (k / w) as i32 - py),
                &mut best,
                &mut best_cost,
            );
        }
        let mut radius = w.max(h) as i32;
        while radius >= 1 && best_cost < f32::MAX {
            let o = (
                best.0 + rand((2 * radius + 1) as usize) as i32 - radius,
                best.1 + rand((2 * radius + 1) as usize) as i32 - radius,
            );
            try_o(o, &mut best, &mut best_cost);
            radius /= 2;
        }
        if best_cost == f32::MAX {
            continue;
        }
        let q = (py + best.1) as usize * w + (px + best.0) as usize;
        let k = hole[p].clamp(0.0, 1.0);
        out[p] = [0, 1, 2, 3].map(|c| image[q][c] * k + image[p][c] * (1.0 - k));
        done[p] = true;
        offset[p] = Some(best);
    }
    out
}

/// A two-colour gradient over a `w × h` region; `a` and `b` in its pixels.
/// Colours are straight sRGBA8; interpolation runs in linear light.
pub fn gradient(
    w: usize,
    h: usize,
    a: (f32, f32),
    b: (f32, f32),
    c0: [u8; 4],
    c1: [u8; 4],
    radial: bool,
) -> Vec<[f32; 4]> {
    let (p0, p1) = (color::srgba8_to_premul(c0), color::srgba8_to_premul(c1));
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    let len = len2.sqrt();
    (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let (px, py) = (x as f32 + 0.5 - a.0, y as f32 + 0.5 - a.1);
                let t = if radial {
                    (px * px + py * py).sqrt() / len
                } else {
                    (px * dx + py * dy) / len2
                };
                let t = t.clamp(0.0, 1.0);
                [0, 1, 2, 3].map(|c| p0[c] + (p1[c] - p0[c]) * t)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_a_hole_in_stripes_with_stripe_colours() {
        // Vertical stripes 4 px wide; a square hole in the middle.
        let (w, h) = (64, 64);
        let img: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                if (i % w) / 4 % 2 == 0 {
                    [0.9, 0.1, 0.1, 1.0]
                } else {
                    [0.1, 0.1, 0.9, 1.0]
                }
            })
            .collect();
        let hole: Vec<f32> = (0..w * h)
            .map(|i| {
                if (24..40).contains(&(i % w)) && (24..40).contains(&(i / w)) {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();
        let mut damaged = img.clone();
        for (p, h) in damaged.iter_mut().zip(&hole) {
            if *h > 0.0 {
                *p = [0.0, 1.0, 0.0, 1.0];
            }
        }
        let out = content_aware(&damaged, &hole, w, h, 7);
        let green = out.iter().filter(|p| p[1] > 0.5).count();
        assert_eq!(green, 0, "no hole colour survives");
        // Most filled pixels match the stripe that belongs there.
        let right = (0..w * h)
            .filter(|i| hole[*i] > 0.0 && (out[*i][0] - img[*i][0]).abs() < 0.05)
            .count();
        assert!(right as f32 / 256.0 > 0.7, "{right}/256 match the stripes");
    }

    #[test]
    fn gradient_endpoints() {
        let g = gradient(
            100,
            1,
            (0.0, 0.0),
            (100.0, 0.0),
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            false,
        );
        assert!(g[0][0] < 0.01 && g[99][0] > 0.98);
    }
}
