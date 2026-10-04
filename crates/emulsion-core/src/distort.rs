//! Perspective and envelope distortion of the selected pixels (or a whole
//! layer) in place, as one command so it previews live and undoes in one
//! step. Pixels resample through [`emulsion_raster::warp`]: four corners for
//! perspective, or a lattice of points (3×3 or 4×4 cells) for an envelope.
//! Vector stroke layers move their points through the same mapping.

use crate::command::Command;
use crate::document::Document;
use crate::node::{NodeId, NodeKind};
use emulsion_raster::paint::fill_pixels;
use emulsion_raster::warp;
use emulsion_raster::{IRect, Mask, Raster, color, select};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type Pt = (f64, f64);

/// Most cells along each side of an envelope lattice.
pub const MAX_ENVELOPE: usize = 8;

/// How the area is reshaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistortKind {
    /// Drag four corners; straight lines stay straight.
    Perspective,
    /// Drag the points of an n×n-cell lattice (n from 1 to 8).
    Envelope(usize),
}

/// A distortion in progress: the document-space rectangle being reshaped and
/// where its lattice points are now, row-major (corners TL, TR, BL, BR for
/// perspective, which is a 1×1 lattice).
#[derive(Clone, Debug, PartialEq)]
pub struct Distortion {
    pub source: IRect,
    pub kind: DistortKind,
    pub grid: Vec<Pt>,
}

impl Distortion {
    /// The undistorted lattice over `source`.
    pub fn new(source: IRect, kind: DistortKind) -> Self {
        let n = match kind {
            DistortKind::Perspective => 1,
            DistortKind::Envelope(n) => n.clamp(1, MAX_ENVELOPE),
        };
        let kind = match kind {
            DistortKind::Envelope(_) => DistortKind::Envelope(n),
            k => k,
        };
        let grid = (0..=n)
            .flat_map(|r| {
                (0..=n).map(move |c| {
                    (
                        source.x as f64 + source.w as f64 * c as f64 / n as f64,
                        source.y as f64 + source.h as f64 * r as f64 / n as f64,
                    )
                })
            })
            .collect();
        Self { source, kind, grid }
    }

    /// Cells along each side.
    pub fn cells(&self) -> usize {
        match self.kind {
            DistortKind::Perspective => 1,
            DistortKind::Envelope(n) => n,
        }
    }

    pub fn move_handle(&mut self, i: usize, p: Pt) {
        if p.0.is_finite()
            && p.1.is_finite()
            && let Some(q) = self.grid.get_mut(i)
        {
            *q = p;
        }
    }

    /// Whether any point has moved.
    pub fn is_identity(&self) -> bool {
        *self == Distortion::new(self.source, self.kind)
    }

    /// Lattice lines for the overlay.
    pub fn lines(&self) -> Vec<Vec<Pt>> {
        let n = self.cells();
        let at = |c: usize, r: usize| self.grid[r * (n + 1) + c];
        let mut out = Vec::new();
        for r in 0..=n {
            out.push((0..=n).map(|c| at(c, r)).collect());
        }
        for c in 0..=n {
            out.push((0..=n).map(|r| at(c, r)).collect());
        }
        out
    }

    /// The corners in warp order: TL, TR, BR, BL.
    fn quad(&self) -> [Pt; 4] {
        let n = self.cells();
        let at = |c: usize, r: usize| self.grid[r * (n + 1) + c];
        [at(0, 0), at(n, 0), at(n, n), at(0, n)]
    }

    /// Where the document point `p` (inside the source) lands.
    pub fn map(&self, p: Pt) -> Option<Pt> {
        let s = self.source;
        let local = (p.0 - s.x as f64, p.1 - s.y as f64);
        match self.kind {
            DistortKind::Perspective => {
                let (w, h) = (s.w as f64, s.h as f64);
                let m = warp::homography([(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], self.quad())?;
                Some(warp::apply(&m, local))
            }
            DistortKind::Envelope(n) => {
                warp::mesh_point(&self.grid, n, n, (s.w as f64, s.h as f64), local)
            }
        }
    }

    /// Warp `src` (the source rectangle's pixels) to its new shape: the
    /// pixels and their document-space bounds.
    fn warp_pixels(&self, src: &Raster) -> Option<(Raster, IRect)> {
        match self.kind {
            DistortKind::Perspective => warp::warp(src, self.quad(), [0; 4]),
            DistortKind::Envelope(n) => warp::warp_mesh(src, &self.grid, n, n, [0; 4]),
        }
    }
}

/// What a distortion of layer `id` starts from: the selection's bounds, or
/// the layer's content when nothing is selected, within the canvas.
pub fn distort_area(doc: &Document, id: NodeId, selection: Option<&Mask>) -> Option<IRect> {
    let canvas = IRect::new(0, 0, doc.width as i32, doc.height as i32);
    let area = match selection {
        Some(s) => select::bounds(s),
        None => match &doc.node(id)?.kind {
            NodeKind::Raster { raster, placement } => {
                let b = raster.coverage_bounds();
                let o = placement
                    .to_doc(raster.width(), raster.height())
                    .translation;
                IRect::new(b.x + o.x.round() as i32, b.y + o.y.round() as i32, b.w, b.h)
            }
            NodeKind::Strokes { strokes, .. } => strokes.bounds()?,
            _ => return None,
        },
    };
    let area = area.intersect(&canvas);
    (!area.is_empty()).then_some(area)
}

/// The layer's whole-pixel offset, when it is only moved (not scaled,
/// rotated or flipped).
fn pixel_offset(raster: &Raster, placement: &emulsion_raster::Placement) -> Option<(i32, i32)> {
    let m = placement.to_doc(raster.width(), raster.height());
    let t = m.translation;
    let plain = (m.matrix2.x_axis.x - 1.0).abs() < 1e-9
        && (m.matrix2.y_axis.y - 1.0).abs() < 1e-9
        && m.matrix2.x_axis.y.abs() < 1e-9
        && m.matrix2.y_axis.x.abs() < 1e-9
        && (t.x - t.x.round()).abs() < 1e-6
        && (t.y - t.y.round()).abs() < 1e-6;
    plain.then(|| (t.x.round() as i32, t.y.round() as i32))
}

/// The command that applies `d` to the selected part (`selection`) of
/// layer `id`, or to all of the source rectangle without a selection.
pub fn distort_command(
    doc: &Document,
    id: NodeId,
    selection: Option<&Mask>,
    d: &Distortion,
) -> Result<Command, String> {
    let node = doc.node(id).ok_or("No such layer")?;
    if crate::smart_filter_mask::descriptor(node).is_some() {
        return Err("Rasterize the Smart Object before distorting its filter mask".into());
    }
    if node.vector_mask.is_some() {
        return Err("Rasterize or delete the vector mask before distorting".into());
    }
    let n = d.cells();
    if d.grid.len() != (n + 1) * (n + 1) || d.source.is_empty() {
        return Err("The distortion lattice is malformed".into());
    }
    let selected = |x: f64, y: f64| -> f32 {
        match selection {
            None => 1.0,
            Some(s) if x >= 0.0 && y >= 0.0 && x < s.width() as f64 && y < s.height() as f64 => {
                s.get(x as u32, y as u32) as f32 / 255.0
            }
            Some(_) => 0.0,
        }
    };
    match &node.kind {
        NodeKind::Raster { raster, placement } => {
            if node.mask.is_some() {
                return Err("Apply or delete the layer mask before distorting".into());
            }
            let (ox, oy) = pixel_offset(raster, placement)
                .ok_or("Distort works on layers that are not scaled, rotated or flipped")?;
            let s = d.source;
            let local = IRect::new(s.x - ox, s.y - oy, s.w, s.h);
            // Selected pixels of the source rectangle, outside the layer
            // transparent.
            let inside = local.intersect(&raster.bounds());
            let px: Vec<[u16; 4]> = raster
                .read_rect(local)
                .into_iter()
                .enumerate()
                .map(|(i, p)| {
                    let (x, y) = (s.x + i as i32 % s.w, s.y + i as i32 / s.w);
                    let (lx, ly) = (x - ox, y - oy);
                    if lx < inside.x
                        || ly < inside.y
                        || lx >= inside.right()
                        || ly >= inside.bottom()
                    {
                        return [0; 4];
                    }
                    let k = selected(x as f64 + 0.5, y as f64 + 0.5);
                    p.map(|v| (v as f32 * k).round() as u16)
                })
                .collect();
            let src = Raster::from_pixels(s.w as u32, s.h as u32, [0; 4], &px);
            let (warped, b) = d
                .warp_pixels(&src)
                .ok_or("That shape folds over itself or is too large")?;
            let coverage = |x: i32, y: i32| selected((x + ox) as f64 + 0.5, (y + oy) as f64 + 0.5);
            let (cleared, dirty) =
                fill_pixels(raster, local, &coverage, &|p, k| p.map(|v| v * (1.0 - k)));
            // Lay the warped pixels over what is left, within the layer.
            let dest = IRect::new(b.x - ox, b.y - oy, b.w, b.h).intersect(&raster.bounds());
            let out = if dest.is_empty() {
                cleared
            } else {
                let under = cleared.read_rect(dest);
                let over = warped.read_rect(IRect::new(
                    dest.x + ox - b.x,
                    dest.y + oy - b.y,
                    dest.w,
                    dest.h,
                ));
                let px: Vec<[u16; 4]> = under
                    .iter()
                    .zip(&over)
                    .map(|(u, o)| {
                        let (u, o) = (color::px_to_f(*u), color::px_to_f(*o));
                        color::f_to_px([0, 1, 2, 3].map(|c| o[c] + u[c] * (1.0 - o[3])))
                    })
                    .collect();
                cleared.write_rect(dest, &px)
            };
            let dirty = if dest.is_empty() {
                dirty
            } else if dirty.is_empty() {
                dest
            } else {
                dirty.union(&dest)
            };
            Ok(Command::ReplacePixels {
                id,
                raster: Arc::new(out),
                dirty,
                label: "Distort".into(),
            })
        }
        NodeKind::Strokes { strokes, .. } => {
            let s = d.source;
            let moves = |p: Pt| {
                p.0 >= s.x as f64
                    && p.1 >= s.y as f64
                    && p.0 <= s.right() as f64
                    && p.1 <= s.bottom() as f64
                    && selected(p.0, p.1) >= 0.5
            };
            let map = |p: Pt| -> Result<Pt, String> {
                if moves(p) {
                    d.map(p)
                        .ok_or_else(|| "That shape folds over itself".to_string())
                } else {
                    Ok(p)
                }
            };
            let mut set = (**strokes).clone();
            for stroke in &mut set.strokes {
                for q in &mut stroke.points {
                    (q.x, q.y) = map((q.x, q.y))?;
                }
            }
            for fill in &mut set.fills {
                for outline in &mut fill.outlines {
                    for q in outline.iter_mut() {
                        *q = map(*q)?;
                    }
                }
            }
            Ok(Command::SetStrokes {
                id,
                strokes: Arc::new(set),
            })
        }
        _ => Err("Distort works on pixel and vector stroke layers".into()),
    }
}
