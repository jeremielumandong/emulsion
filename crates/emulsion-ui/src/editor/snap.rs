//! Ruler guides and snapping.
//!
//! Drag from a ruler to make a guide; drag a guide back onto a ruler (or
//! off the canvas) to remove it. Guides live in the document, so they are
//! saved and undoable. Moving a node snaps its edges and centre to the
//! canvas edges and centre, to guides, and to other nodes, within a few
//! screen pixels. Hold Ctrl while dragging to move freely.

use super::*;
use emulsion_core::document::Guide;

/// A full-length line: (vertical, position in document pixels).
pub(crate) type Line = (bool, f64);

/// Snap distance in screen pixels.
const SNAP_PX: f64 = 6.0;
/// Grab distance for an existing guide, in screen pixels.
const GRAB_PX: f64 = 4.0;
/// Ruler thickness; matches the viewport's.
const RULER_PX: f32 = 16.0;

impl EditorView {
    /// Top-left of the canvas area, for tests that aim at the rulers.
    #[cfg(test)]
    pub(crate) fn canvas_origin(&self) -> Option<Point<Pixels>> {
        self.canvas_bounds().map(|b| b.origin)
    }

    fn rulers_shown(&self) -> bool {
        self.rulers && self.view.rotation.rem_euclid(360.0) == 0.0
    }

    /// Which ruler `pos` is over: Some(true) for the left (vertical guides),
    /// Some(false) for the top (horizontal guides).
    pub(crate) fn ruler_hit(&self, pos: Point<Pixels>) -> Option<bool> {
        let b = self.canvas_bounds()?;
        if !self.rulers_shown() || !b.contains(&pos) {
            return None;
        }
        let (x, y) = (pos.x - b.origin.x, pos.y - b.origin.y);
        if y < px(RULER_PX) && x >= px(RULER_PX) {
            Some(false)
        } else if x < px(RULER_PX) && y >= px(RULER_PX) {
            Some(true)
        } else {
            None
        }
    }

    /// The guide under `pos`, if any.
    pub(crate) fn guide_hit(&self, pos: Point<Pixels>) -> Option<usize> {
        let b = self.canvas_bounds()?;
        if self.view.rotation.rem_euclid(360.0) != 0.0 {
            return None;
        }
        let (sx, sy) = (f32::from(pos.x) as f64, f32::from(pos.y) as f64);
        self.editor.doc.guides.iter().position(|g| {
            let s = if g.vertical {
                self.view.doc_to_screen((g.pos, 0.0), &b).0 - sx
            } else {
                self.view.doc_to_screen((0.0, g.pos), &b).1 - sy
            };
            s.abs() <= GRAB_PX
        })
    }

    /// Where a guide being dragged would land, or None to remove it.
    pub(crate) fn guide_position(&self, vertical: bool, pos: Point<Pixels>) -> Option<f64> {
        let b = self.canvas_bounds()?;
        if !b.contains(&pos) || self.ruler_hit(pos).is_some() {
            return None;
        }
        let d = self.doc_point(pos)?;
        let v = if vertical { d.0 } else { d.1 };
        // Whole pixels unless zoomed in far enough to place finer.
        Some(if self.view.zoom >= 8.0 {
            (v * 4.0).round() / 4.0
        } else {
            v.round()
        })
    }

    /// Finish a guide drag.
    pub(crate) fn drop_guide(
        &mut self,
        vertical: bool,
        pos: Option<f64>,
        existing: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let mut guides = self.editor.doc.guides.clone();
        match (existing, pos) {
            (Some(i), Some(p)) if i < guides.len() => guides[i].pos = p,
            (Some(i), None) if i < guides.len() => {
                guides.remove(i);
            }
            (None, Some(p)) => guides.push(Guide { vertical, pos: p }),
            _ => return,
        }
        if guides != self.editor.doc.guides {
            self.execute(Command::SetGuides { guides }, cx);
        }
    }

    pub fn clear_guides(&mut self, cx: &mut Context<Self>) {
        if !self.editor.doc.guides.is_empty() {
            self.execute(Command::SetGuides { guides: Vec::new() }, cx);
        }
    }

    /// Adjust a move of node `id` by (dx, dy) so an edge or the centre
    /// lands on something nearby. Records the lines it snapped to.
    pub(crate) fn snap_move(
        &mut self,
        id: NodeId,
        start: &Placement,
        dx: f64,
        dy: f64,
    ) -> (f64, f64) {
        self.snap_lines.clear();
        if !self.snap || self.snap_bypass {
            return (dx, dy);
        }
        let doc = &self.editor.doc;
        let Some(NodeKind::Raster { raster, .. }) = doc.node(id).map(|n| &n.kind) else {
            return (dx, dy);
        };
        let b = start.doc_bounds(raster.width(), raster.height());
        let (mut tx, mut ty) = (
            vec![0.0, doc.width as f64 / 2.0, doc.width as f64],
            vec![0.0, doc.height as f64 / 2.0, doc.height as f64],
        );
        for g in &doc.guides {
            if g.vertical {
                tx.push(g.pos)
            } else {
                ty.push(g.pos)
            }
        }
        for n in &doc.nodes {
            if n.id == id || !n.visible {
                continue;
            }
            if let NodeKind::Raster { raster, placement } = &n.kind {
                let o = placement.doc_bounds(raster.width(), raster.height());
                tx.extend([o.x as f64, o.x as f64 + o.w as f64 / 2.0, o.right() as f64]);
                ty.extend([o.y as f64, o.y as f64 + o.h as f64 / 2.0, o.bottom() as f64]);
            }
        }
        let limit = SNAP_PX / self.view.zoom;
        let best = |edges: [f64; 3], targets: &[f64], d: f64| -> Option<(f64, f64)> {
            let mut best: Option<(f64, f64)> = None;
            for e in edges {
                for t in targets {
                    let diff = t - (e + d);
                    if diff.abs() <= limit && best.is_none_or(|(b, _)| diff.abs() < b.abs()) {
                        best = Some((diff, *t));
                    }
                }
            }
            best
        };
        let xs = [b.x as f64, b.x as f64 + b.w as f64 / 2.0, b.right() as f64];
        let ys = [b.y as f64, b.y as f64 + b.h as f64 / 2.0, b.bottom() as f64];
        let (mut dx, mut dy) = (dx, dy);
        if let Some((diff, t)) = best(xs, &tx, dx) {
            dx += diff;
            self.snap_lines.push((true, t));
        }
        if let Some((diff, t)) = best(ys, &ty, dy) {
            dy += diff;
            self.snap_lines.push((false, t));
        }
        (dx, dy)
    }

    /// Guides and snap lines for the overlay, including one being dragged.
    pub(crate) fn guide_lines(&self) -> (Vec<Line>, Vec<Line>) {
        let mut guides: Vec<Line> = self
            .editor
            .doc
            .guides
            .iter()
            .enumerate()
            .filter(|(i, _)| !matches!(&self.drag, Some(Drag::Guide { existing: Some(e), .. }) if e == i))
            .map(|(_, g)| (g.vertical, g.pos))
            .collect();
        if let Some(Drag::Guide {
            vertical,
            pos: Some(p),
            ..
        }) = &self.drag
        {
            guides.push((*vertical, *p));
        }
        (guides, self.snap_lines.clone())
    }
}
