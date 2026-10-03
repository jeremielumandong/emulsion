//! Reading drawn paper worksheets back from a photo or a flatbed scan:
//! find and read the sheet's QR code, find its four corner marks (adaptive
//! threshold and connected components, so turned, tilted and unevenly lit
//! photos work), map the sheet's millimetres onto the photo with a
//! homography, cut each frame out at the panel's resolution and clean it so
//! the paper turns white or transparent. Pure image work, no UI.
use super::worksheet::{SheetCode, Slot, project_key};
use anyhow::Context;
use emulsion_raster::warp::{self, Homography};
use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Long side of the copy that marks and codes are looked for in.
const DETECT_SIDE: u32 = 2400;
/// Paper left round each frame's edge (its printed outline), in millimetres.
const EDGE_MM: f64 = 0.8;
/// A frame with less ink than this share of its pixels is left blank.
const BLANK_INK: f32 = 0.0004;
/// Largest photo read, in pixels.
const MAX_PIXELS: u64 = 120_000_000;

/// How a drawing is cleaned up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clean {
    /// Paper becomes transparent; pencil and ink keep their colour.
    #[default]
    Transparent,
    /// Paper becomes even white, with levels and white balance.
    White,
    /// Strokes only, in black: paper, shading and specks are dropped.
    LineArt,
    /// The photo as it is, only straightened.
    Photo,
}

/// Why a photo could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("No worksheet code was found. Choose the layout the sheet was printed with.")]
    NoCode,
    #[error(
        "This sheet was printed from another storyboard (project {0}). Open that storyboard to import it."
    )]
    Foreign(String),
    #[error("{0}")]
    Failed(String),
}

impl From<anyhow::Error> for ScanError {
    fn from(e: anyhow::Error) -> Self {
        Self::Failed(e.to_string())
    }
}

/// One frame cut from a sheet.
#[derive(Clone, Debug)]
pub struct ScannedFrame {
    pub slot: Slot,
    /// The drawing at the panel's resolution, straight-alpha sRGB.
    pub image: RgbaImage,
    /// Share of the frame drawn on; 0 for an empty frame.
    pub ink: f32,
}

impl ScannedFrame {
    /// Whether anything was drawn in the frame.
    pub fn drawn(&self) -> bool {
        self.ink >= BLANK_INK
    }
}

/// A sheet read from a photo.
#[derive(Clone, Debug)]
pub struct ScannedSheet {
    pub code: SheetCode,
    /// Whether the code was read from the photo (otherwise it was chosen).
    pub code_read: bool,
    /// The corner marks' centres in the photo, top-left first, clockwise.
    pub corners: [(f64, f64); 4],
    pub frames: Vec<ScannedFrame>,
}

/// Read a photo or scan (any format File → Open reads) flattened to sRGBA.
pub fn load(path: &Path) -> anyhow::Result<RgbaImage> {
    let doc = crate::open(path)?;
    if u64::from(doc.width) * u64::from(doc.height) > MAX_PIXELS {
        anyhow::bail!("Photos are limited to 120 megapixels")
    }
    let doc = crate::export::develop_document(&doc)?;
    let r = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
    RgbaImage::from_raw(r.width(), r.height(), r.to_srgba8()).context("Cannot read the photo")
}

/// Luminance, 0–255.
fn luma(p: &[u8]) -> u8 {
    ((u32::from(p[0]) * 2126 + u32::from(p[1]) * 7152 + u32::from(p[2]) * 722) / 10_000) as u8
}

/// A grey copy of `image` with its long side at most `side`, and the scale
/// from it back to the photo.
fn grey(image: &RgbaImage, side: u32) -> (image::GrayImage, f64) {
    let long = image.width().max(image.height());
    let gray = image::GrayImage::from_fn(image.width(), image.height(), |x, y| {
        image::Luma([luma(&image.get_pixel(x, y).0)])
    });
    if long <= side {
        return (gray, 1.);
    }
    let scale = f64::from(long) / f64::from(side);
    let (w, h) = (
        (f64::from(image.width()) / scale).round().max(1.) as u32,
        (f64::from(image.height()) / scale).round().max(1.) as u32,
    );
    let small = image::imageops::resize(&gray, w, h, image::imageops::FilterType::Triangle);
    (small, scale)
}

/// The first worksheet code in `gray`, with its corners (top-left of the
/// code first, clockwise). Tried as it is, softened and smaller (sensor
/// noise and paper grain can break the finder patterns at a few pixels a
/// module), then evened out by the adaptive threshold for shadowed or
/// unevenly lit photos.
fn read_code(gray: &image::GrayImage) -> Option<(SheetCode, [(f64, f64); 4])> {
    let at = |image: &image::GrayImage, scale: f64, offset: (u32, u32)| {
        let (w, h) = (image.width() as usize, image.height() as usize);
        let mut prepared =
            rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| image.as_raw()[y * w + x]);
        prepared.detect_grids().into_iter().find_map(|grid| {
            let (_, text) = grid.decode().ok()?;
            let code = SheetCode::decode(&text)?;
            let corner = |p: rqrr::Point| {
                (
                    f64::from(p.x) * scale + f64::from(offset.0),
                    f64::from(p.y) * scale + f64::from(offset.1),
                )
            };
            Some((code, grid.bounds.map(corner)))
        })
    };
    let decode = |image: &image::GrayImage, scale: f64| at(image, scale, (0, 0));
    decode(gray, 1.)
        .or_else(|| decode(&image::imageops::blur(gray, 1.), 1.))
        .or_else(|| {
            let (w, h) = (gray.width() * 3 / 4, gray.height() * 3 / 4);
            let small = image::imageops::resize(gray, w, h, image::imageops::FilterType::Triangle);
            decode(&small, f64::from(gray.width()) / f64::from(w.max(1)))
        })
        .or_else(|| {
            // Busy photos can hide the code's finders among other shapes:
            // look again in overlapping halves of the picture.
            let soft = image::imageops::blur(gray, 1.);
            let (w, h) = (gray.width() / 2, gray.height() / 2);
            (0..3).find_map(|row| {
                (0..3).find_map(|col| {
                    let offset = (col * w / 2, row * h / 2);
                    let tile = image::imageops::crop_imm(&soft, offset.0, offset.1, w, h);
                    at(&tile.to_image(), 1., offset)
                })
            })
        })
        .or_else(|| {
            let dark = threshold(gray);
            let binary = image::GrayImage::from_fn(gray.width(), gray.height(), |x, y| {
                let i = y as usize * gray.width() as usize + x as usize;
                image::Luma([if dark[i] { 0 } else { 255 }])
            });
            decode(&binary, 1.)
        })
}

/// A dark connected region of the thresholded photo.
#[derive(Clone, Copy, Debug)]
struct Blob {
    area: u32,
    sum: (f64, f64),
    min: (u32, u32),
    max: (u32, u32),
}

impl Blob {
    fn centre(&self) -> (f64, f64) {
        (
            self.sum.0 / f64::from(self.area),
            self.sum.1 / f64::from(self.area),
        )
    }
    fn size(&self) -> (f64, f64) {
        (
            f64::from(self.max.0 - self.min.0 + 1),
            f64::from(self.max.1 - self.min.1 + 1),
        )
    }
}

/// Dark pixels against their neighbourhood's mean (Bradley's adaptive
/// threshold), so shadows and uneven light do not matter.
fn threshold(gray: &image::GrayImage) -> Vec<bool> {
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    let mut integral = vec![0u64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0u64;
        for x in 0..w {
            row += u64::from(gray.as_raw()[y * w + x]);
            integral[(y + 1) * (w + 1) + x + 1] = integral[y * (w + 1) + x + 1] + row;
        }
    }
    let r = (w.min(h) / 32).max(7);
    let mut dark = vec![false; w * h];
    dark.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for (x, d) in out.iter_mut().enumerate() {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let sum = integral[y1 * (w + 1) + x1] + integral[y0 * (w + 1) + x0]
                - integral[y0 * (w + 1) + x1]
                - integral[y1 * (w + 1) + x0];
            let count = ((x1 - x0) * (y1 - y0)) as u64;
            let p = u64::from(gray.as_raw()[y * w + x]);
            *d = p * count * 100 < sum * 82;
        }
    });
    dark
}

/// The 8-connected regions of `mask`.
fn blobs(mask: &[bool], w: usize, h: usize) -> Vec<Blob> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut blob = Blob {
            area: 0,
            sum: (0., 0.),
            min: (u32::MAX, u32::MAX),
            max: (0, 0),
        };
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            blob.area += 1;
            blob.sum.0 += x as f64 + 0.5;
            blob.sum.1 += y as f64 + 0.5;
            blob.min = (blob.min.0.min(x as u32), blob.min.1.min(y as u32));
            blob.max = (blob.max.0.max(x as u32), blob.max.1.max(y as u32));
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    if mask[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        out.push(blob);
    }
    out
}

/// A corner mark found in the photo.
#[derive(Clone, Copy, Debug)]
struct Mark {
    centre: (f64, f64),
    /// The ring's larger side, in pixels.
    size: f64,
}

/// Square rings with a small dark centre: the corner marks.
fn marks(gray: &image::GrayImage) -> Vec<Mark> {
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    let mut all = blobs(&threshold(gray), w, h);
    all.retain(|b| b.area >= 6);
    all.sort_by(|a, b| a.centre().0.total_cmp(&b.centre().0));
    let xs: Vec<f64> = all.iter().map(|b| b.centre().0).collect();
    let mut out = Vec::new();
    for ring in &all {
        let (rw, rh) = ring.size();
        let fill = f64::from(ring.area) / (rw * rh);
        if ring.area < 40 || rw < 7. || rh < 7. || !(0.2..=0.75).contains(&fill) {
            continue;
        }
        let side = rw.max(rh);
        if rw.min(rh) / side < 0.4 {
            continue;
        }
        let c = ring.centre();
        let reach = side * 0.15;
        let from = xs.partition_point(|x| *x < c.0 - reach);
        let centre = all[from..]
            .iter()
            .take_while(|b| b.centre().0 <= c.0 + reach)
            .find(|b| {
                let (bw, bh) = b.size();
                let (bx, by) = b.centre();
                let ratio = f64::from(b.area) / f64::from(ring.area);
                (bx - c.0).hypot(by - c.1) <= reach
                    && b.min.0 > ring.min.0
                    && b.min.1 > ring.min.1
                    && b.max.0 < ring.max.0
                    && b.max.1 < ring.max.1
                    // Corner marks (centre 0.3 of the ring, 0.25 of its
                    // area), not QR finders (0.43 and 0.38).
                    && (0.1..=0.33).contains(&ratio)
                    && (0.18..=0.38).contains(&(bw.max(bh) / side))
            });
        if let Some(centre) = centre {
            out.push(Mark {
                centre: centre.centre(),
                size: side,
            });
        }
    }
    out
}

/// The four marks nearest where `predicted` puts them, each within a
/// quarter of the shortest side.
fn nearest(found: &[Mark], predicted: [(f64, f64); 4]) -> Option<[(f64, f64); 4]> {
    let short = (0..4)
        .map(|i| {
            let (a, b) = (predicted[i], predicted[(i + 1) % 4]);
            (a.0 - b.0).hypot(a.1 - b.1)
        })
        .fold(f64::MAX, f64::min);
    let mut out = [(0., 0.); 4];
    for (slot, p) in out.iter_mut().zip(predicted) {
        let best = found
            .iter()
            .map(|m| (m, (m.centre.0 - p.0).hypot(m.centre.1 - p.1)))
            .filter(|(_, d)| *d <= short * 0.25)
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        *slot = best.0.centre;
    }
    Some(out)
}

/// The four largest marks spanning the largest quadrilateral, top-left
/// first and clockwise, for a sheet whose code cannot be read (assumed
/// upright).
fn largest_quad(found: &[Mark]) -> Option<[(f64, f64); 4]> {
    let mut found = found.to_vec();
    found.sort_by(|a, b| b.size.total_cmp(&a.size));
    let biggest = found.first()?.size;
    found.retain(|m| m.size >= biggest * 0.5);
    found.truncate(12);
    let n = found.len();
    let mut best: Option<(f64, [(f64, f64); 4])> = None;
    for a in 0..n {
        for b in a + 1..n {
            for c in b + 1..n {
                for d in c + 1..n {
                    let quad = order([a, b, c, d].map(|i| found[i].centre));
                    let area = area(&quad);
                    if best.is_none_or(|(best, _)| area > best) {
                        best = Some((area, quad));
                    }
                }
            }
        }
    }
    best.map(|(_, quad)| quad)
}

/// Points clockwise (in image axes) from the one nearest the top left.
fn order(mut points: [(f64, f64); 4]) -> [(f64, f64); 4] {
    let c = (
        points.iter().map(|p| p.0).sum::<f64>() / 4.,
        points.iter().map(|p| p.1).sum::<f64>() / 4.,
    );
    points.sort_by(|a, b| {
        (a.1 - c.1)
            .atan2(a.0 - c.0)
            .total_cmp(&(b.1 - c.1).atan2(b.0 - c.0))
    });
    let first = (0..4)
        .min_by(|&i, &j| (points[i].0 + points[i].1).total_cmp(&(points[j].0 + points[j].1)))
        .unwrap_or(0);
    points.rotate_left(first);
    points
}

/// Signed area of a quadrilateral (shoelace); positive when clockwise in
/// image axes.
fn area(q: &[(f64, f64); 4]) -> f64 {
    (0..4)
        .map(|i| {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f64>()
        / 2.
}

/// Sample `image` at `(x, y)` (pixel centres at .5) with bilinear filtering,
/// as linear-ish 0–1 RGB; outside the photo reads as black.
pub(super) fn sample(image: &RgbaImage, x: f64, y: f64) -> [f32; 3] {
    let (w, h) = (image.width() as i64, image.height() as i64);
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
    let at = |x: i64, y: i64| -> [f32; 3] {
        if x < 0 || y < 0 || x >= w || y >= h {
            return [0.; 3];
        }
        let p = image.get_pixel(x as u32, y as u32).0;
        [p[0], p[1], p[2]].map(|v| f32::from(v) / 255.)
    };
    let (a, b, c, d) = (
        at(x0, y0),
        at(x0 + 1, y0),
        at(x0, y0 + 1),
        at(x0 + 1, y0 + 1),
    );
    std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bottom = c[i] + (d[i] - c[i]) * tx;
        top + (bottom - top) * ty
    })
}

/// Cut `rect` (sheet millimetres) out of the photo through `h` at `size`
/// pixels, averaging several samples a pixel when the photo is larger.
fn warp_frame(
    image: &RgbaImage,
    h: &Homography,
    rect: crate::printing::Rect,
    size: (u32, u32),
) -> Vec<[f32; 3]> {
    let (w, ht) = (size.0 as usize, size.1 as usize);
    // Photo pixels per panel pixel, measured along the frame's top edge.
    let a = warp::apply(h, (rect.x, rect.y));
    let b = warp::apply(h, (rect.x + rect.w, rect.y));
    let ratio = (b.0 - a.0).hypot(b.1 - a.1) / size.0 as f64;
    let k = ratio.ceil().clamp(1., 4.) as usize;
    let mut out = vec![[0f32; 3]; w * ht];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate() {
            let mut acc = [0f32; 3];
            for sy in 0..k {
                for sx in 0..k {
                    let u = (x as f64 + (sx as f64 + 0.5) / k as f64) / size.0 as f64;
                    let v = (y as f64 + (sy as f64 + 0.5) / k as f64) / size.1 as f64;
                    let p = warp::apply(h, (rect.x + u * rect.w, rect.y + v * rect.h));
                    let s = sample(image, p.0, p.1);
                    for i in 0..3 {
                        acc[i] += s[i];
                    }
                }
            }
            *px = acc.map(|v| v / (k * k) as f32);
        }
    });
    out
}

fn lum(p: [f32; 3]) -> f32 {
    p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722
}

/// The paper's colour around each pixel: the brightest pixels of each
/// block, smoothly blended between blocks. Blocks covered in ink take the
/// paper of the whole frame.
fn paper(pixels: &[[f32; 3]], w: usize, h: usize) -> Vec<[f32; 3]> {
    let n = 12usize;
    let (bw, bh) = (w.div_ceil(n).max(1), h.div_ceil(n).max(1));
    let (cols, rows) = (w.div_ceil(bw), h.div_ceil(bh));
    let mut blocks = Vec::with_capacity(cols * rows);
    for by in 0..rows {
        for bx in 0..cols {
            let mut values: Vec<[f32; 3]> = (by * bh..((by + 1) * bh).min(h))
                .flat_map(|y| (bx * bw..((bx + 1) * bw).min(w)).map(move |x| (x, y)))
                .map(|(x, y)| pixels[y * w + x])
                .collect();
            values.sort_by(|a, b| lum(*a).total_cmp(&lum(*b)));
            let top = &values[values.len() * 4 / 5..];
            let mean = top.iter().fold([0f32; 3], |acc, p| {
                [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]]
            });
            blocks.push(mean.map(|v| v / top.len().max(1) as f32));
        }
    }
    let mut lums: Vec<f32> = blocks.iter().map(|p| lum(*p)).collect();
    lums.sort_by(f32::total_cmp);
    let median = lums[lums.len() / 2];
    let whole = blocks
        .iter()
        .filter(|p| lum(**p) >= median)
        .fold(([0f32; 3], 0), |(acc, n), p| {
            ([acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]], n + 1)
        });
    let whole = whole.0.map(|v| v / whole.1.max(1) as f32);
    for block in &mut blocks {
        if lum(*block) < lum(whole) * 0.7 {
            *block = whole;
        }
    }
    let at = |bx: usize, by: usize| blocks[by.min(rows - 1) * cols + bx.min(cols - 1)];
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let fx = ((x as f32 + 0.5) / bw as f32 - 0.5).max(0.);
            let fy = ((y as f32 + 0.5) / bh as f32 - 0.5).max(0.);
            let (x0, y0) = (fx as usize, fy as usize);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let (a, b, c, d) = (
                at(x0, y0),
                at(x0 + 1, y0),
                at(x0, y0 + 1),
                at(x0 + 1, y0 + 1),
            );
            std::array::from_fn(|i| {
                let top = a[i] + (b[i] - a[i]) * tx;
                let bottom = c[i] + (d[i] - c[i]) * tx;
                (top + (bottom - top) * ty).max(0.05)
            })
        })
        .collect()
}

/// Clean a cut-out frame: even out the light and the paper's colour, set
/// levels, then make the paper white or transparent, or keep only strokes.
/// `edge` pixels round the border are cleared of the printed outline.
fn clean(pixels: Vec<[f32; 3]>, size: (u32, u32), edge: u32, mode: Clean) -> (RgbaImage, f32) {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let paper = paper(&pixels, w, h);
    let flat: Vec<[f32; 3]> = pixels
        .iter()
        .zip(&paper)
        .map(|(p, q)| std::array::from_fn(|i| (p[i] / q[i]).clamp(0., 1.)))
        .collect();
    // Levels: the paper at white, the darkest strokes at black.
    let mut lums: Vec<f32> = flat.iter().map(|p| lum(*p)).collect();
    lums.sort_by(f32::total_cmp);
    let black = lums[lums.len() / 400].min(0.35);
    let white = 0.9f32;
    let level = |v: f32| ((v - black) / (white - black)).clamp(0., 1.);
    let inside = |x: usize, y: usize| {
        let e = edge as usize;
        x >= e && y >= e && x + e < w && y + e < h
    };
    let mut ink = 0usize;
    let mut out = RgbaImage::new(size.0, size.1);
    let mut keep = vec![false; w * h];
    if mode == Clean::LineArt {
        let strokes: Vec<bool> = flat
            .iter()
            .enumerate()
            .map(|(i, p)| inside(i % w, i / w) && 1. - level(lum(*p)) > 0.35)
            .collect();
        // Specks of dust and paper grain are dropped.
        let min = ((w * h) as f64 * 0.00002).max(4.) as u32;
        for blob in blobs(&strokes, w, h) {
            if blob.area >= min {
                for y in blob.min.1..=blob.max.1 {
                    for x in blob.min.0..=blob.max.0 {
                        let i = y as usize * w + x as usize;
                        keep[i] = strokes[i];
                    }
                }
            }
        }
    }
    for (i, p) in flat.iter().enumerate() {
        let (x, y) = (i % w, i / w);
        let leveled = p.map(level);
        let dark = 1. - lum(leveled);
        let pixel = match mode {
            Clean::Photo => {
                let raw = pixels[i].map(|v| (v * 255.).round() as u8);
                [raw[0], raw[1], raw[2], 255]
            }
            _ if !inside(x, y) => match mode {
                Clean::White => [255; 4],
                _ => [255, 255, 255, 0],
            },
            Clean::White => {
                let c = leveled.map(|v| (v * 255.).round() as u8);
                [c[0], c[1], c[2], 255]
            }
            Clean::Transparent => {
                let alpha = ((dark - 0.06) / 0.5).clamp(0., 1.);
                if alpha <= 0. {
                    [255, 255, 255, 0]
                } else {
                    // The ink colour that, over white, gives the pixel.
                    let c = leveled
                        .map(|v| (((v - (1. - alpha)) / alpha).clamp(0., 1.) * 255.).round() as u8);
                    [c[0], c[1], c[2], (alpha * 255.).round() as u8]
                }
            }
            Clean::LineArt => {
                if keep[i] {
                    let alpha = ((dark - 0.2) / 0.4).clamp(0., 1.);
                    [0, 0, 0, (alpha * 255.).round() as u8]
                } else {
                    [255, 255, 255, 0]
                }
            }
        };
        if inside(x, y) && dark > 0.3 {
            ink += 1;
        }
        out.put_pixel(x as u32, y as u32, image::Rgba(pixel));
    }
    (out, ink as f32 / (w * h).max(1) as f32)
}

/// Read one sheet from `image`. `project` is the open storyboard's project
/// ID; a code from another storyboard is refused. `layout` is the code to
/// use when the photo's own cannot be read. Frames come out at `size`.
pub fn scan(
    image: &RgbaImage,
    project: &str,
    layout: Option<&SheetCode>,
    size: (u32, u32),
    mode: Clean,
    cancel: &AtomicBool,
) -> Result<ScannedSheet, ScanError> {
    if image.width() < 64 || image.height() < 64 {
        return Err(ScanError::Failed("The photo is too small to read.".into()));
    }
    let canceled = || {
        if cancel.load(Ordering::Relaxed) {
            Err(ScanError::Failed("Canceled".into()))
        } else {
            Ok(())
        }
    };
    let (gray, scale) = grey(image, DETECT_SIDE);
    let mut read = read_code(&gray).map(|(c, b)| (c, b, scale));
    if read.is_none() && scale > 1. {
        // Small codes may only read at full size.
        canceled()?;
        read = read_code(&grey(image, u32::MAX).0).map(|(c, b)| (c, b, 1.));
    }
    canceled()?;
    let found = marks(&gray);
    let to_photo = |p: (f64, f64)| (p.0 * scale, p.1 * scale);
    let (code, code_read, corners) = match read {
        Some((code, bounds, at)) => {
            if project_key(&code.project) != project_key(project) {
                return Err(ScanError::Foreign(code.project));
            }
            // Where the code's square is, the corner marks follow.
            let (x, y, s) = code.code;
            let square = [(x, y), (x + s, y), (x + s, y + s), (x, y + s)];
            let bounds = bounds.map(|(bx, by)| (bx * at / scale, by * at / scale));
            let h = warp::homography(square, bounds)
                .ok_or_else(|| ScanError::Failed("The code is too distorted.".into()))?;
            let (mw, mh) = code.marks;
            let predicted = [(0., 0.), (mw, 0.), (mw, mh), (0., mh)].map(|p| warp::apply(&h, p));
            let corners = nearest(&found, predicted).ok_or_else(|| {
                ScanError::Failed(
                    "The four corner marks were not all found. Photograph the whole sheet, flat and in focus.".into(),
                )
            })?;
            (code, true, corners)
        }
        None => {
            let code = layout.ok_or(ScanError::NoCode)?.clone();
            let corners = largest_quad(&found).ok_or_else(|| {
                ScanError::Failed(
                    "The four corner marks were not found. Photograph the whole sheet, flat and in focus.".into(),
                )
            })?;
            (code, false, corners)
        }
    };
    if area(&corners) <= 0. {
        return Err(ScanError::Failed(
            "The corner marks are out of order; the photo may be mirrored.".into(),
        ));
    }
    let corners = corners.map(to_photo);
    let (mw, mh) = code.marks;
    let h = warp::homography([(0., 0.), (mw, 0.), (mw, mh), (0., mh)], corners)
        .ok_or_else(|| ScanError::Failed("The sheet is too distorted to read.".into()))?;
    let mut frames = Vec::new();
    for (slot, corner) in &code.frames {
        canceled()?;
        let rect = code.frame_rect(*corner);
        let pixels = warp_frame(image, &h, rect, size);
        let edge = (EDGE_MM / rect.w * f64::from(size.0)).ceil() as u32;
        let (image, ink) = clean(pixels, size, edge, mode);
        frames.push(ScannedFrame {
            slot: *slot,
            image,
            ink,
        });
    }
    Ok(ScannedSheet {
        code,
        code_read,
        corners,
        frames,
    })
}

/// Most photos one import reads.
pub const MAX_PHOTOS: usize = 50;

/// Read each photo as a sheet, in order; `progress(done, total)` hears each
/// one start. A photo that fails keeps its reason.
pub fn scan_files(
    paths: &[std::path::PathBuf],
    project: &str,
    layout: Option<&SheetCode>,
    size: (u32, u32),
    mode: Clean,
    cancel: &AtomicBool,
    progress: impl Fn(usize, usize),
) -> Vec<Result<ScannedSheet, ScanError>> {
    let mut out = Vec::new();
    for (index, path) in paths.iter().take(MAX_PHOTOS).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(index, paths.len());
        out.push(
            load(path)
                .map_err(ScanError::from)
                .and_then(|image| scan(&image, project, layout, size, mode, cancel)),
        );
    }
    out
}

/// Where the drawings of read sheets go.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// Drawings for existing panels.
    pub drawings: Vec<(emulsion_core::project::PageId, RgbaImage)>,
    /// Drawings that become new panels, in print order, with their names.
    pub new_panels: Vec<(String, RgbaImage)>,
    /// Frames left empty.
    pub blank: usize,
    /// What was skipped or changed, for the user.
    pub notes: Vec<String>,
}

/// Sort the drawn frames of `sheets` onto `panels` (the board's panels);
/// new-panel slots, and frames of panels no longer on the board, become new
/// panels. A sheet photographed twice counts once.
pub fn plan(sheets: &[ScannedSheet], panels: &[emulsion_core::project::PageId]) -> Plan {
    let mut out = Plan::default();
    let mut seen = std::collections::HashSet::new();
    let mut new = Vec::new();
    for (index, sheet) in sheets.iter().enumerate() {
        // Sheets read with a chosen layout follow the printed ones, each in
        // photo order.
        let group = if sheet.code_read { 0 } else { index as u32 + 1 };
        if sheet.code_read && !seen.insert(sheet.code.sheet.clone()) {
            out.notes.push(format!(
                "Sheet {} was photographed more than once; the first photo is used.",
                sheet.code.sheet
            ));
            continue;
        }
        for frame in &sheet.frames {
            if !frame.drawn() {
                out.blank += 1;
                continue;
            }
            match frame.slot {
                Slot::Panel(id) if panels.contains(&id) => {
                    out.drawings.push((id, frame.image.clone()));
                }
                Slot::Panel(id) => {
                    out.notes.push(format!(
                        "Panel {id} is no longer on the board; its drawing becomes a new panel."
                    ));
                    new.push(((u32::MAX, 0), frame.image.clone()));
                }
                Slot::New(n) => new.push(((group, n), frame.image.clone())),
            }
        }
    }
    // New panels follow the print's numbering.
    new.sort_by_key(|(n, _)| *n);
    out.new_panels = new
        .into_iter()
        .enumerate()
        .map(|(i, (_, image))| (format!("Paper panel {}", i + 1), image))
        .collect();
    out
}

/// The name of a layer holding a paper drawing imported today.
pub fn layer_name() -> String {
    format!(
        "{} ({})",
        emulsion_core::project::PAPER_LAYER_PREFIX,
        super::today()
    )
}

/// Put `plan`'s drawings on their panels and add its new panels after
/// `after`, as one Undo step. With `replace`, a panel's earlier paper
/// drawing layers are removed.
pub fn place(
    editor: &mut emulsion_core::project::ProjectEditor,
    plan: Plan,
    after: Option<emulsion_core::project::PageId>,
    replace: bool,
) -> Result<emulsion_core::project::PaperPlaced, String> {
    let raster = |image: RgbaImage| {
        std::sync::Arc::new(emulsion_raster::Raster::from_srgba8(
            image.width(),
            image.height(),
            image.as_raw(),
        ))
    };
    let drawings = plan
        .drawings
        .into_iter()
        .map(|(id, image)| (id, raster(image)))
        .collect();
    let new_panels = plan
        .new_panels
        .into_iter()
        .map(|(name, image)| (name, raster(image)))
        .collect();
    editor.place_paper_drawings(drawings, new_panels, after, &layer_name(), replace)
}
