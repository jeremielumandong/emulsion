//! Drawing guides (Procreate's Drawing Guide) over the canvas: a 2D grid,
//! an isometric grid, one-, two- or three-point perspective, four- or
//! five-point curvilinear perspective, and a straight-edge ruler. Drawing
//! Assist locks each stroke to the nearest guide line or arc; a stroke that
//! starts near the ruler follows its edge. Handles are dragged on the
//! canvas. The guides are stored with the document (see
//! `emulsion_core::drawing_guides`), so each storyboard panel keeps its own.

use super::*;
use emulsion_core::drawing_guides::{AssistCurve, clip_line};

#[derive(Clone, Debug, Default)]
pub struct GuideState {
    /// Drawing Assist: strokes follow the guides' lines and arcs.
    pub assist: bool,
    /// The line or arc the current stroke is locked to. `None` until the
    /// hand has moved far enough to tell (or the stroke began on the ruler).
    pub lock: Option<AssistCurve>,
    /// Where the current stroke began, for choosing the lock.
    pub start: Option<(f64, f64)>,
}

/// A document-space polyline.
pub(crate) type Polyline = Vec<(f64, f64)>;

/// Screen pixels within which a handle can be grabbed.
const HANDLE_PX: f64 = 10.0;
/// How far (screen px) the hand moves before a stroke's direction is chosen.
const LOCK_PX: f64 = 8.0;
/// Strokes starting within this many screen pixels of the ruler follow it.
const RULER_PX: f64 = 24.0;

impl EditorView {
    /// Begin a stroke at `d` (document pixels): remember where, so the
    /// assist can pick a direction once the hand commits to one. Returns
    /// where the stroke really starts: on the ruler's edge when it began
    /// near the ruler.
    pub(crate) fn assist_begin(&mut self, d: (f64, f64)) -> (f64, f64) {
        let guides = &self.editor.doc.drawing_guides;
        let ruler = guides
            .ruler
            .and_then(|r| r.snap(d, RULER_PX / self.view.zoom.max(1e-6)));
        let assist = self.tools.guide.assist && guides.active().next().is_some();
        let g = &mut self.tools.guide;
        g.lock = ruler;
        g.start = (ruler.is_some() || assist).then_some(d);
        match ruler {
            Some(line) => line.project(d),
            None => d,
        }
    }

    /// Where a stroke point lands with Drawing Assist or the ruler: on the
    /// locked line or arc once the hand has shown a direction, else where
    /// it is.
    pub(crate) fn assist_point(&mut self, d: (f64, f64)) -> (f64, f64) {
        let Some(start) = self.tools.guide.start else {
            return d;
        };
        if self.tools.guide.lock.is_none() {
            let moved = (d.0 - start.0).hypot(d.1 - start.1) * self.view.zoom;
            if moved < LOCK_PX {
                return start;
            }
            let curves = self.editor.doc.drawing_guides.curves(start);
            match AssistCurve::best(&curves, start, (d.0 - start.0, d.1 - start.1)) {
                Some(curve) => self.tools.guide.lock = Some(curve),
                None => return d,
            }
        }
        self.tools.guide.lock.map_or(d, |c| c.project(d))
    }

    pub(crate) fn assist_end(&mut self) {
        self.tools.guide.lock = None;
        self.tools.guide.start = None;
    }

    /// Every draggable guide point: vanishing points and curvilinear
    /// centres, the ruler's ends, then a selection distortion's lattice.
    fn guide_handles(&self) -> Vec<(f64, f64)> {
        let mut v = self.editor.doc.drawing_guides.handles();
        v.extend(self.distort_handles());
        v
    }

    /// The guide handle under `pos`, if there is one.
    pub(crate) fn vanishing_hit(&self, pos: Point<Pixels>) -> Option<usize> {
        let b = self.canvas_bounds()?;
        if !b.contains(&pos) {
            return None;
        }
        let (sx, sy) = (f32::from(pos.x) as f64, f32::from(pos.y) as f64);
        self.guide_handles().iter().position(|p| {
            let s = self.view.doc_to_screen(*p, &b);
            (s.0 - sx).hypot(s.1 - sy) <= HANDLE_PX
        })
    }

    pub(crate) fn move_vanishing(&mut self, i: usize, d: (f64, f64), cx: &mut Context<Self>) {
        let n = self.editor.doc.drawing_guides.handles().len();
        if i < n {
            self.editor.doc.drawing_guides.move_handle(i, d);
        } else {
            self.move_distort_handle(i - n, d, cx);
        }
    }

    /// Guide geometry for the overlay: lines and handles.
    pub(crate) fn guide_overlay(&self) -> (Vec<Polyline>, Vec<(f64, f64)>) {
        let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let guides = &self.editor.doc.drawing_guides;
        let mut lines: Vec<Polyline> = guides.active().flat_map(|g| g.lines(w, h)).collect();
        if let Some(r) = guides.ruler.filter(|r| r.enabled) {
            lines.push(vec![r.a, r.b]);
        }
        lines.extend(self.distort_lines());
        // Symmetry axes show while mirroring or radial symmetry is on.
        if self.tools.mirror_x {
            lines.push(vec![(w / 2.0, 0.0), (w / 2.0, h)]);
        }
        if self.tools.mirror_y {
            lines.push(vec![(0.0, h / 2.0), (w, h / 2.0)]);
        }
        if self.tools.symmetry >= 2 {
            let n = self.tools.symmetry;
            let r = w.hypot(h);
            for k in 0..n {
                let a = k as f64 / n as f64 * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
                let l = clip_line(
                    (w / 2.0, h / 2.0),
                    (w / 2.0 + a.cos() * r, h / 2.0 + a.sin() * r),
                    w,
                    h,
                );
                if l.len() == 2 {
                    lines.push(l);
                }
            }
        }
        (lines, self.guide_handles())
    }
}
