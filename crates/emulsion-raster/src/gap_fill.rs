//! The bucket's gap closing and fill modes, and tracing a filled area into
//! polygons so a fill on a vector layer stays vector.
//!
//! Gap closing (Storyboard Pro's "close gap"): the line art is thickened by
//! half the gap size so openings narrower than the gap close, the click is
//! flooded inside the thickened art, and the result is grown back by the same
//! amount within the original area so the fill still meets the lines.

use crate::color;
use crate::geom::IRect;
use crate::image::{Mask, Raster};
use crate::paint::{fill_color, fill_pixels};
use crate::select::{self, Combine};
use serde::{Deserialize, Serialize};

/// Largest gap the bucket closes, in pixels.
pub const MAX_GAP: u32 = 64;

/// How the bucket combines its colour with what the layer already holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillMode {
    /// Paint over the area.
    #[default]
    Normal,
    /// Paint under the layer's existing pixels, showing only where they are
    /// transparent or translucent.
    Behind,
    /// Paint only pixels whose alpha is below the threshold, leaving
    /// painted pixels alone.
    Unpainted,
}

impl FillMode {
    pub const ALL: [FillMode; 3] = [FillMode::Normal, FillMode::Behind, FillMode::Unpainted];

    pub fn label(self) -> &'static str {
        match self {
            FillMode::Normal => "normal",
            FillMode::Behind => "behind",
            FillMode::Unpainted => "unpainted",
        }
    }
}

/// The area a bucket click at (x, y) fills in straight sRGBA8 `image`:
/// pixels within `tolerance` of the seed (see [`select::by_color`]), where
/// openings in the boundary up to `gap` pixels wide count as closed.
pub fn gap_region(
    image: &[u8],
    w: u32,
    h: u32,
    (x, y): (u32, u32),
    tolerance: u8,
    contiguous: bool,
    gap: u32,
) -> Mask {
    let flood = select::by_color(image, w, h, x, y, tolerance, contiguous);
    let gap = gap.min(MAX_GAP);
    if gap == 0 || !contiguous {
        return flood;
    }
    let similar = select::by_color(image, w, h, x, y, tolerance, false);
    let r = gap.div_ceil(2) as i32;
    let walls = select::grow(&select::invert(&similar), r);
    let open = select::combine(Some(&similar), &walls, Combine::Subtract);
    let core = select::connected(&open, x, y);
    if select::bounds(&core).is_empty() {
        // The click is in a corner narrower than the gap: fill as usual.
        return flood;
    }
    let reach = select::grow(&core, r + 1);
    select::combine(Some(&reach), &flood, Combine::Intersect)
}

/// Fill `region` of `base` with `color` (premultiplied linear) weighted by
/// `coverage`, combining as `mode` says. `threshold` is the alpha (0–255)
/// below which [`FillMode::Unpainted`] counts a pixel as unpainted.
pub fn fill(
    base: &Raster,
    region: IRect,
    coverage: &(dyn Fn(i32, i32) -> f32 + Sync),
    color: [f32; 4],
    mode: FillMode,
    threshold: u8,
) -> (Raster, IRect) {
    match mode {
        FillMode::Normal => fill_color(base, region, coverage, color),
        FillMode::Behind => fill_pixels(base, region, coverage, &|b, k| {
            let under = 1.0 - b[3];
            [0, 1, 2, 3].map(|c| b[c] + color[c] * k * under)
        }),
        FillMode::Unpainted => {
            let limit = threshold as f32 / 255.0;
            fill_pixels(base, region, coverage, &|b, k| {
                if b[3] < limit {
                    [0, 1, 2, 3].map(|c| color[c] * k + b[c] * (1.0 - k))
                } else {
                    b
                }
            })
        }
    }
}

/// Pixels of `image` (straight sRGBA8, `w` wide) whose alpha is below
/// `threshold`, as a mask: where [`FillMode::Unpainted`] may paint.
pub fn unpainted(image: &[u8], w: u32, h: u32, threshold: u8) -> Mask {
    let px: Vec<u8> = image
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| if p[3] < threshold { 255 } else { 0 })
        .collect();
    Mask::from_pixels(w, h, 0, &px)
}

/// The outlines of the selected (at least half) pixels of `m` as closed
/// polygons in pixel coordinates: outer edges run clockwise and holes
/// anticlockwise, so they fill correctly with the non-zero rule. Pixel
/// stairs are straightened to within `tolerance` pixels.
pub fn trace(m: &Mask, tolerance: f64) -> Vec<Vec<(f64, f64)>> {
    let e = select::extent(m);
    if e.is_empty() {
        return Vec::new();
    }
    let region = IRect::new(e.x - 1, e.y - 1, e.w + 2, e.h + 2).intersect(&m.bounds());
    let dense = m.read_rect(region);
    let inside = |x: i32, y: i32| -> bool {
        x >= region.x
            && y >= region.y
            && x < region.right()
            && y < region.bottom()
            && dense[((y - region.y) * region.w + x - region.x) as usize] >= 128
    };
    // Directed unit edges with the selected pixel on their right.
    let mut edges: Vec<((i32, i32), (i32, i32))> = Vec::new();
    for y in region.y..region.bottom() {
        for x in region.x..region.right() {
            if !inside(x, y) {
                continue;
            }
            if !inside(x, y - 1) {
                edges.push(((x, y), (x + 1, y)));
            }
            if !inside(x + 1, y) {
                edges.push(((x + 1, y), (x + 1, y + 1)));
            }
            if !inside(x, y + 1) {
                edges.push(((x + 1, y + 1), (x, y + 1)));
            }
            if !inside(x - 1, y) {
                edges.push(((x, y + 1), (x, y)));
            }
        }
    }
    let mut from: std::collections::HashMap<(i32, i32), Vec<usize>> =
        std::collections::HashMap::with_capacity(edges.len());
    for (i, (a, _)) in edges.iter().enumerate() {
        from.entry(*a).or_default().push(i);
    }
    let mut used = vec![false; edges.len()];
    let mut loops = Vec::new();
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let mut ring = Vec::new();
        let mut at = start;
        loop {
            used[at] = true;
            let (a, b) = edges[at];
            ring.push(a);
            // At a saddle two edges leave the vertex; turn right so the
            // diagonal pixels stay separate loops.
            let next = from.get(&b).and_then(|out| {
                let dir = (b.0 - a.0, b.1 - a.1);
                let right = (-dir.1, dir.0);
                out.iter().copied().filter(|i| !used[*i]).max_by_key(|i| {
                    let (c, d) = edges[*i];
                    let turn = (d.0 - c.0, d.1 - c.1);
                    turn.0 * right.0 + turn.1 * right.1
                })
            });
            match next {
                Some(n) => at = n,
                None => break,
            }
        }
        let ring = straighten(&ring, tolerance);
        if ring.len() >= 3 {
            loops.push(ring);
        }
    }
    loops
}

/// Drop points on straight runs, then simplify the closed ring.
fn straighten(ring: &[(i32, i32)], tolerance: f64) -> Vec<(f64, f64)> {
    let n = ring.len();
    let corners: Vec<(f32, f32)> = (0..n)
        .filter(|i| {
            let (p, c, q) = (ring[(i + n - 1) % n], ring[*i], ring[(i + 1) % n]);
            (c.0 - p.0, c.1 - p.1) != (q.0 - c.0, q.1 - c.1)
        })
        .map(|i| (ring[i].0 as f32, ring[i].1 as f32))
        .collect();
    if corners.len() < 3 || tolerance <= 0.0 {
        return corners
            .into_iter()
            .map(|(x, y)| (x as f64, y as f64))
            .collect();
    }
    let mut closed = corners.clone();
    closed.push(corners[0]);
    let mut out = crate::quickshape::simplify(&closed, tolerance as f32);
    out.pop();
    if out.len() < 3 {
        out = corners;
    }
    out.into_iter().map(|(x, y)| (x as f64, y as f64)).collect()
}

/// Straight sRGBA8 pixels of a raster's `w` × `h` top-left area.
pub fn srgba8(raster: &Raster, w: u32, h: u32) -> Vec<u8> {
    raster
        .read_rect(IRect::new(0, 0, w as i32, h as i32))
        .into_iter()
        .flat_map(|p| color::premul_to_srgba8(color::px_to_f(p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::fill_coverage;

    /// A white image with a black square outline from (10, 10) to (49, 49),
    /// two pixels thick, with an opening `gap` pixels wide in its top side.
    fn outline_with_gap(gap: u32) -> (Vec<u8>, u32, u32) {
        let (w, h) = (60u32, 60u32);
        let mut img = vec![255u8; (w * h * 4) as usize];
        for y in 10..50 {
            for x in 10..50 {
                let border = !(12..48).contains(&x) || !(12..48).contains(&y);
                let opening = y < 12 && (28..28 + gap).contains(&x);
                if border && !opening {
                    let i = ((y * w + x) * 4) as usize;
                    img[i..i + 3].copy_from_slice(&[0, 0, 0]);
                }
            }
        }
        (img, w, h)
    }

    fn count(m: &Mask) -> usize {
        m.to_gray8().iter().filter(|v| **v >= 128).count()
    }

    #[test]
    fn gap_closing_stops_the_leak_and_still_reaches_the_lines() {
        let (img, w, h) = outline_with_gap(4);
        let leaky = gap_region(&img, w, h, (30, 30), 10, true, 0);
        assert!(leaky.get(2, 2) >= 128, "without gap closing the fill leaks");
        let closed = gap_region(&img, w, h, (30, 30), 10, true, 6);
        assert!(closed.get(2, 2) < 128, "the 4 px opening is closed");
        // It still fills right up to the inside of the outline.
        for (x, y) in [(12, 30), (47, 30), (30, 47), (12, 12), (47, 47)] {
            assert!(closed.get(x, y) >= 128, "({x}, {y}) is filled");
        }
        // The inside, plus at most the mouth of the opening.
        let n = count(&closed);
        assert!((36 * 36..=36 * 36 + 8).contains(&n), "{n} pixels");
        // A gap wider than the setting still leaks.
        let (wide, w, h) = outline_with_gap(12);
        assert!(gap_region(&wide, w, h, (30, 30), 10, true, 6).get(2, 2) >= 128);
        // Non-contiguous fills ignore gaps.
        assert_eq!(
            count(&gap_region(&img, w, h, (30, 30), 10, false, 6)),
            count(&select::by_color(&img, w, h, 30, 30, 10, false))
        );
    }

    #[test]
    fn traced_regions_fill_back_to_the_same_pixels() {
        // A ring (square with a square hole) and a separate diagonal pair.
        let (w, h) = (40u32, 40u32);
        let mut px = vec![0u8; (w * h) as usize];
        for y in 5..25 {
            for x in 5..25 {
                if !(10..20).contains(&x) || !(10..20).contains(&y) {
                    px[(y * w + x) as usize] = 255;
                }
            }
        }
        px[(30 * w + 30) as usize] = 255;
        px[(31 * w + 31) as usize] = 255;
        let m = Mask::from_pixels(w, h, 0, &px);
        let loops = trace(&m, 0.0);
        assert_eq!(loops.len(), 4, "outer, hole and two diagonal pixels");
        let back = fill_coverage(&loops, w, h);
        assert_eq!(back.to_gray8(), m.to_gray8());
        // Simplified, the outline keeps its corners: a square is 4 points.
        let square = trace(&select::rect(w, h, 3.0, 3.0, 10.0, 10.0), 0.75);
        assert_eq!(square.len(), 1);
        assert_eq!(square[0].len(), 4);
    }

    #[test]
    fn fill_modes_respect_existing_pixels() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let half = [0.0, 0.0, 0.5, 0.5];
        let base = Raster::from_pixels(
            3,
            1,
            [0; 4],
            &[[0.0; 4], half, [0.0, 0.0, 1.0, 1.0]].map(color::f_to_px),
        );
        let all = |_: i32, _: i32| 1.0;
        let px = |r: &Raster, x: u32| color::px_to_f(r.get(x, 0));
        let (normal, _) = fill(&base, base.bounds(), &all, red, FillMode::Normal, 0);
        assert!((0..3).all(|x| (px(&normal, x)[0] - 1.0).abs() < 1e-3));
        let (behind, _) = fill(&base, base.bounds(), &all, red, FillMode::Behind, 0);
        assert!((px(&behind, 0)[0] - 1.0).abs() < 1e-3, "empty pixel filled");
        let mid = px(&behind, 1);
        assert!((mid[0] - 0.5).abs() < 1e-3 && (mid[2] - 0.5).abs() < 1e-3);
        assert!((mid[3] - 1.0).abs() < 1e-3, "translucent pixel backed");
        assert!(px(&behind, 2)[0] < 1e-3, "opaque pixel untouched");
        let (kept, _) = fill(&base, base.bounds(), &all, red, FillMode::Unpainted, 100);
        assert!((px(&kept, 0)[0] - 1.0).abs() < 1e-3);
        assert!(px(&kept, 1)[0] < 1e-3, "alpha 128 counts as painted");
        assert!(px(&kept, 2)[0] < 1e-3);
        let (loose, _) = fill(&base, base.bounds(), &all, red, FillMode::Unpainted, 200);
        assert!(
            (px(&loose, 1)[0] - 1.0).abs() < 1e-3,
            "below 200 is unpainted"
        );
        let m = unpainted(&srgba8(&base, 3, 1), 3, 1, 100);
        assert_eq!(m.to_gray8(), vec![255, 0, 0]);
    }
}
