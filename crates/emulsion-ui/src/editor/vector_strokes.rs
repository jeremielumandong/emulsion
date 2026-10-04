//! Vector drawing on stroke layers (Storyboard Pro's vector layers, D2, D3,
//! D6, D13, D14).
//!
//! When the active layer is a stroke layer, the Brush draws centreline
//! strokes through the brush's own input pipeline (stabilizer, pressure
//! curve, speed, QuickShape) with pressure as width and opacity dynamics as
//! per-point opacity, and the Eraser cuts strokes apart. The Vector tool
//! holds the Line, Rectangle, Ellipse and Polyline shapes (strokes on a
//! stroke layer, brush pixels elsewhere), the contour editor and pencil
//! retouch. Every gesture is one Undo step through `Command::SetStrokes`,
//! previewed once per frame while it lasts.

use super::tools::{BrushSlot, Overlay, ToolDrag, local_clip, premul, shape_rect};
use super::transform::Handle;
use super::*;
use crate::tablet::PenSample;
use crate::widgets::{chip_action, tip};
use emulsion_core::project::mileage::path_length;
use emulsion_raster::paint::{Brush, Ink, Sample, Stroke as BrushStroke};
use emulsion_raster::quickshape::{self, Shape};
use emulsion_raster::strokes::{Retouch, Stroke, StrokePoint, StrokeSet};
use glam::{DAffine2, dvec2};

type Pt = (f64, f64);

/// What the Vector tool does.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VectorMode {
    Line,
    Rectangle,
    Ellipse,
    Polyline,
    /// Select, move, scale and rotate strokes and drag their points.
    Contour,
    /// Brush over pencil lines to change their width, opacity or wobble.
    Retouch,
}

impl VectorMode {
    pub(crate) const SHAPES: [VectorMode; 4] = [
        VectorMode::Line,
        VectorMode::Rectangle,
        VectorMode::Ellipse,
        VectorMode::Polyline,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            VectorMode::Line => "Line",
            VectorMode::Rectangle => "Rectangle",
            VectorMode::Ellipse => "Ellipse",
            VectorMode::Polyline => "Polyline",
            VectorMode::Contour => "Contour editor",
            VectorMode::Retouch => "Pencil retouch",
        }
    }

    fn is_shape(self) -> bool {
        Self::SHAPES.contains(&self)
    }
}

/// Option sliders of the vector tools.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum VectorSlider {
    Width,
    OpacityPressure,
    OpacityTilt,
    OpacitySpeed,
    Fade,
    RetouchSize,
    RetouchAmount,
    Smooth,
    Optimize,
}

/// D14: how a vector pencil line's opacity follows the hand. Each amount
/// is 0–1; `fade` is the length in pixels over which a line fades out
/// (0 is off).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OpacityDynamics {
    pub pressure: f32,
    pub tilt: f32,
    pub speed: f32,
    pub fade: f32,
}

pub(crate) struct VectorUi {
    pub mode: VectorMode,
    /// Line width of shapes, in pixels.
    pub width: f32,
    pub dynamics: OpacityDynamics,
    pub retouch: Retouch,
    pub retouch_size: f32,
    pub retouch_amount: f32,
    /// Smooth strength, 0–1.
    pub smooth: f32,
    /// Optimize tolerance in pixels.
    pub optimize: f32,
    /// The stroke layer the contour selection belongs to.
    pub layer: Option<NodeId>,
    /// Selected strokes (indices into the layer's strokes).
    pub selection: Vec<usize>,
    /// Polyline vertices placed so far.
    pub polyline: Vec<Pt>,
    /// Strokes to show at the next frame: one `SetStrokes` per frame.
    pending: Option<(NodeId, Arc<StrokeSet>)>,
}

impl Default for VectorUi {
    fn default() -> Self {
        Self {
            mode: VectorMode::Line,
            width: 4.,
            dynamics: OpacityDynamics {
                pressure: 0.,
                tilt: 0.,
                speed: 0.,
                fade: 0.,
            },
            retouch: Retouch::Thicker,
            retouch_size: 30.,
            retouch_amount: 0.5,
            smooth: 0.5,
            optimize: 1.,
            layer: None,
            selection: Vec::new(),
            polyline: Vec::new(),
            pending: None,
        }
    }
}

/// What the contour editor's drag holds on to.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Grab {
    Move,
    /// A centreline point: stroke and point index.
    Point(usize, usize),
    /// A handle of the selection's box: its bounds when grabbed.
    Handle(Handle, (Pt, Pt)),
}

/// A vector gesture in progress.
pub(crate) enum VectorDrag {
    /// The Brush on a stroke layer.
    Draw {
        id: NodeId,
        input: Box<BrushStroke>,
        base: Arc<StrokeSet>,
        stroke: Stroke,
        /// QuickShape's fit, once the hand has held still.
        shape: Option<Shape>,
    },
    Erase {
        id: NodeId,
        set: StrokeSet,
        last: Pt,
        radius: f64,
    },
    Retouch {
        id: NodeId,
        set: StrokeSet,
        last: Pt,
    },
    Shape {
        start: Pt,
        end: Pt,
    },
    Marquee {
        start: Pt,
        end: Pt,
        add: bool,
    },
    Edit {
        id: NodeId,
        base: Arc<StrokeSet>,
        start: Pt,
        grab: Grab,
    },
}

/// The vector line through the brush's stamped samples: pressure narrows
/// it as the brush's Pressure → size setting asks, tapers thin its ends,
/// and the opacity dynamics fade it.
pub(crate) fn stroke_points(
    samples: &[Sample],
    brush: &Brush,
    dynamics: &OpacityDynamics,
    finished: bool,
) -> Vec<StrokePoint> {
    let mut along = Vec::with_capacity(samples.len());
    let mut length = 0f32;
    for (i, s) in samples.iter().enumerate() {
        if i > 0 {
            let p = samples[i - 1];
            length += (s.x - p.x).hypot(s.y - p.y);
        }
        along.push(length);
    }
    samples
        .iter()
        .zip(&along)
        .map(|(s, &dist)| {
            let mut taper = 1f32;
            if brush.taper_start > 0. {
                taper = taper.min(dist / brush.taper_start);
            }
            if finished && brush.taper_end > 0. {
                taper = taper.min((length - dist) / brush.taper_end);
            }
            let taper = 0.08 + 0.92 * taper.clamp(0., 1.);
            let pressure = s.pressure.clamp(0., 1.);
            let width = (1. - brush.size_pressure * (1. - pressure)) * taper;
            let tilt = (s.tilt.0.hypot(s.tilt.1) / 90.).min(1.);
            let fast = ((s.speed - 0.8) / 5.).clamp(0., 1.);
            let fade = if dynamics.fade > 0. {
                (1. - dist / dynamics.fade).max(0.)
            } else {
                1.
            };
            let opacity = brush.opacity
                * (1. - dynamics.pressure * (1. - pressure))
                * (1. - dynamics.tilt * tilt)
                * (1. - dynamics.speed * fast)
                * fade;
            StrokePoint {
                x: f64::from(s.x),
                y: f64::from(s.y),
                width: width.clamp(0., 8.),
                opacity: opacity.clamp(0., 1.),
            }
        })
        .collect()
}

/// The shape a Line, Rectangle or Ellipse drag from `a` to `b` draws;
/// `constrain` (Shift) keeps lines to 45° steps and boxes square.
pub(crate) fn drag_shape(mode: VectorMode, a: Pt, b: Pt, constrain: bool) -> Option<Shape> {
    let f = |p: Pt| (p.0 as f32, p.1 as f32);
    match mode {
        VectorMode::Line => {
            let b = if constrain { snap_45(a, b) } else { b };
            ((b.0 - a.0).hypot(b.1 - a.1) >= 1.).then(|| Shape::Line(f(a), f(b)))
        }
        VectorMode::Rectangle | VectorMode::Ellipse => {
            let (x, y, w, h) = shape_rect(a, b, constrain);
            if w < 1. || h < 1. {
                return None;
            }
            Some(if mode == VectorMode::Rectangle {
                Shape::Polygon(vec![
                    f((x, y)),
                    f((x + w, y)),
                    f((x + w, y + h)),
                    f((x, y + h)),
                ])
            } else {
                Shape::Ellipse {
                    center: f((x + w / 2., y + h / 2.)),
                    radii: ((w / 2.) as f32, (h / 2.) as f32),
                    angle: 0.,
                }
            })
        }
        _ => None,
    }
}

/// `b` moved onto the nearest 45° direction from `a`.
fn snap_45(a: Pt, b: Pt) -> Pt {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let step = std::f64::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    let len = dx.hypot(dy);
    (a.0 + len * angle.cos(), a.1 + len * angle.sin())
}

/// The transform a contour-editor handle drag makes: corners scale about
/// the opposite corner (proportionally unless `free`), edges along one
/// axis, and the rotate zone turns about the centre (15° steps when
/// `free`).
pub(crate) fn handle_transform(
    handle: Handle,
    (lo, hi): (Pt, Pt),
    start: Pt,
    now: Pt,
    free: bool,
) -> DAffine2 {
    let corners = [lo, (hi.0, lo.1), hi, (lo.0, hi.1)];
    let center = ((lo.0 + hi.0) / 2., (lo.1 + hi.1) / 2.);
    let about = |p: Pt, m: DAffine2| {
        DAffine2::from_translation(dvec2(p.0, p.1))
            * m
            * DAffine2::from_translation(dvec2(-p.0, -p.1))
    };
    let ratio = |v: f64, h: f64, o: f64| {
        if (h - o).abs() < 1e-9 {
            1.
        } else {
            (v - o) / (h - o)
        }
    };
    match handle {
        Handle::Rotate => {
            let a0 = (start.1 - center.1).atan2(start.0 - center.0);
            let a1 = (now.1 - center.1).atan2(now.0 - center.0);
            let mut angle = (a1 - a0).to_degrees();
            if free {
                angle = (angle / 15.).round() * 15.;
            }
            about(center, DAffine2::from_angle(angle.to_radians()))
        }
        Handle::Corner(i) => {
            let (h, o) = (corners[i], corners[(i + 2) % 4]);
            let (mut sx, mut sy) = (ratio(now.0, h.0, o.0), ratio(now.1, h.1, o.1));
            if !free {
                let s = if sx.abs() > sy.abs() { sx } else { sy };
                (sx, sy) = (s, s);
            }
            about(o, DAffine2::from_scale(dvec2(sx, sy)))
        }
        Handle::Edge(i) => {
            // 0 top, 1 right, 2 bottom, 3 left; the opposite edge stays.
            let m = match i {
                0 => DAffine2::from_scale(dvec2(1., ratio(now.1, lo.1, hi.1))),
                1 => DAffine2::from_scale(dvec2(ratio(now.0, hi.0, lo.0), 1.)),
                2 => DAffine2::from_scale(dvec2(1., ratio(now.1, hi.1, lo.1))),
                _ => DAffine2::from_scale(dvec2(ratio(now.0, lo.0, hi.0), 1.)),
            };
            let o = match i {
                0 => (lo.0, hi.1),
                1 => lo,
                2 => lo,
                _ => (hi.0, lo.1),
            };
            about(o, m)
        }
    }
}

impl EditorView {
    // ── Targets ─────────────────────────────────────────────────────────

    /// The active stroke layer and its strokes.
    pub(crate) fn vector_layer(&self) -> Option<(NodeId, Arc<StrokeSet>)> {
        let id = self.selected?;
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Strokes { strokes, .. } => Some((id, strokes.clone())),
            _ => None,
        }
    }

    /// The active stroke layer, when it may be edited; otherwise says why
    /// on the status line, as the other tools do.
    fn vector_target(&mut self, cx: &mut Context<Self>) -> Option<(NodeId, Arc<StrokeSet>)> {
        let Some((id, set)) = self.vector_layer() else {
            self.set_status(
                "Select a vector layer, or add one with Layer › New Vector Layer.",
                false,
                cx,
            );
            return None;
        };
        let refusal = if self.editor.is_read_only() {
            Some(emulsion_core::command::CommandError::ReadOnly.to_string())
        } else if self.editor.doc.locked_ancestor(id).is_some() {
            Some("That layer is locked.".into())
        } else if self.editor.doc.layer_locks(id).pixels {
            Some("That layer's drawing is locked.".into())
        } else {
            None
        };
        if let Some(reason) = refusal {
            self.set_status(reason, true, cx);
            return None;
        }
        if self.vector.layer != Some(id) {
            self.vector.layer = Some(id);
            self.vector.selection.clear();
        }
        let len = set.strokes.len();
        self.vector.selection.retain(|i| *i < len);
        Some((id, set))
    }

    /// Add an empty vector stroke layer above the selection and select it.
    pub(crate) fn new_vector_layer(&mut self, cx: &mut Context<Self>) -> Option<NodeId> {
        let k = self
            .editor
            .doc
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Strokes { .. }))
            .count()
            + 1;
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let node = Node::strokes(
            0,
            format!("Vector {k}"),
            Arc::new(StrokeSet::default()),
            w,
            h,
        );
        let slot = self.insertion_slot();
        let id = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        )?;
        self.set_layer_selection(vec![id], Some(id));
        Some(id)
    }

    /// Apply strokes at once, as one step labelled `label`. Errors (a
    /// locked layer or panel) go to the status line.
    fn set_strokes(
        &mut self,
        id: NodeId,
        strokes: StrokeSet,
        label: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor.begin(label);
        let ok = self.stroke_command(id, Arc::new(strokes), cx);
        self.editor.end();
        ok
    }

    fn stroke_command(
        &mut self,
        id: NodeId,
        strokes: Arc<StrokeSet>,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.editor.execute(Command::SetStrokes { id, strokes }) {
            Ok(_) => {
                self.after_change(cx);
                true
            }
            Err(e) => {
                self.set_status(e.to_string(), true, cx);
                false
            }
        }
    }

    /// Show the strokes waiting for this frame. A refused edit ends the
    /// gesture and leaves the layer as it was.
    pub(crate) fn flush_vector_preview(&mut self, cx: &mut Context<Self>) {
        let Some((id, strokes)) = self.vector.pending.take() else {
            return;
        };
        if !self.stroke_command(id, strokes, cx) {
            if matches!(self.drag, Some(Drag::Tool(ToolDrag::Vector(_)))) {
                self.drag = None;
            }
            self.editor.cancel();
            self.after_change(cx);
        }
    }

    fn stage(&mut self, id: NodeId, strokes: StrokeSet, cx: &mut Context<Self>) {
        self.vector.pending = Some((id, Arc::new(strokes)));
        cx.notify();
    }

    fn put_back(&mut self, drag: VectorDrag) {
        self.drag = Some(Drag::Tool(ToolDrag::Vector(Box::new(drag))));
    }

    /// The Brush slot's brush, whichever tool is active.
    fn paint_brush(&self) -> Brush {
        let slot = BrushSlot::Paint(PaintKind::Brush);
        if BrushSlot::of(self.tool, self.tools.paint) == Some(slot) {
            self.tools.brush
        } else {
            self.tools
                .kits
                .get(&slot)
                .map(|kit| kit.0)
                .unwrap_or_else(|| slot.default_brush())
        }
    }

    // ── Brush and Eraser on a stroke layer ──────────────────────────────

    /// The Brush tool went down. On a stroke layer the Brush draws a
    /// vector line and the Eraser cuts lines; pixel-only kinds refuse.
    /// Returns whether the press was handled here.
    pub(crate) fn vector_brush_down(
        &mut self,
        d: Pt,
        pen: Option<PenSample>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.tools.mask_edit_target.is_mask()
            || self.tools.quick_mask
            || self.vector_layer().is_none()
        {
            return false;
        }
        match self.tools.paint {
            PaintKind::Brush | PaintKind::Eraser => {}
            // Vector fills belong to the bucket.
            PaintKind::Bucket => return false,
            PaintKind::Smudge | PaintKind::Gradient | PaintKind::Liquify => {
                self.set_status(
                    "Smudge, Gradient and Liquify work on pixel layers. Choose Rasterize Layer first.",
                    false,
                    cx,
                );
                return true;
            }
        }
        let Some((id, set)) = self.vector_target(cx) else {
            return true;
        };
        self.tools.stroke_started = Some(Instant::now());
        // The ruler and Drawing Assist hold vector strokes like pixel ones.
        let d = self.assist_begin(d);
        if self.tools.paint == PaintKind::Eraser {
            self.editor.begin("Erase");
            let radius = f64::from(self.tools.brush.size / 2.).max(0.5);
            let mut set = (*set).clone();
            if set.erase(d, radius) {
                self.stage(id, set.clone(), cx);
            }
            self.put_back(VectorDrag::Erase {
                id,
                set,
                last: d,
                radius,
            });
            return true;
        }
        crate::tablet::start();
        let brush = self.tools.brush;
        let mut input = BrushStroke::new(
            Arc::new(Raster::transparent(1, 1)),
            brush,
            Ink::Color([0., 0., 0., 1.]),
            None,
        );
        input.point_full(
            d.0 as f32,
            d.1 as f32,
            pen.map(|p| p.pressure),
            pen.map(|p| p.tilt),
            Some(0.),
        );
        self.editor.begin("Vector stroke");
        self.put_back(VectorDrag::Draw {
            id,
            input: Box::new(input),
            base: set,
            stroke: Stroke::new(self.tools.fg, brush.size.min(2000.)),
            shape: None,
        });
        self.stage_drawing(false, cx);
        if self.tools.quick_shape {
            self.watch_vector_quick_shape(cx);
        }
        true
    }

    /// Put the line drawn so far into the pending strokes.
    fn stage_drawing(&mut self, finished: bool, cx: &mut Context<Self>) {
        let dynamics = self.vector.dynamics;
        let Some(Drag::Tool(ToolDrag::Vector(drag))) = &self.drag else {
            return;
        };
        let VectorDrag::Draw {
            id,
            input,
            base,
            stroke,
            shape,
        } = drag.as_ref()
        else {
            return;
        };
        let mut line = match shape {
            Some(shape) => Stroke {
                width: stroke.width
                    * input.path().first().map_or(1., |s| {
                        1. - input.brush.size_pressure * (1. - s.pressure.clamp(0., 1.))
                    }),
                ..Stroke::from_shape(shape, stroke.color, stroke.width)
            },
            None => Stroke {
                points: stroke_points(input.path(), &input.brush, &dynamics, finished),
                ..stroke.clone()
            },
        };
        if line.points.is_empty() {
            return;
        }
        if finished {
            // Drop points that add nothing, as Optimize would.
            line.simplify(0.2);
        }
        let mut set = (**base).clone();
        set.strokes.push(line);
        let id = *id;
        self.stage(id, set, cx);
    }

    fn watch_vector_quick_shape(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
                let go_on = this.update(cx, |this, cx| this.vector_quick_shape_tick(cx));
                if !matches!(go_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// QuickShape for vector lines: holding still at the end of a line
    /// turns it into the line, polygon or ellipse it was aiming for, with
    /// clean vertices. Returns whether to keep watching.
    pub(crate) fn vector_quick_shape_tick(&mut self, cx: &mut Context<Self>) -> bool {
        const HOLD_MS: f64 = 450.;
        let now_ms = self
            .tools
            .stroke_started
            .map(|s| s.elapsed().as_secs_f64() * 1000.)
            .unwrap_or(0.);
        let radius = (3. / self.view.zoom.max(0.05)) as f32;
        let Some(Drag::Tool(ToolDrag::Vector(drag))) = &mut self.drag else {
            return false;
        };
        let VectorDrag::Draw { input, shape, .. } = drag.as_mut() else {
            return false;
        };
        if shape.is_some() {
            return false;
        }
        let raw = input.raw_points();
        if raw.len() < 8 || input.held_ms(now_ms, radius) < HOLD_MS {
            return true;
        }
        let pts: Vec<(f32, f32)> = raw.iter().map(|r| (r.0, r.1)).collect();
        let Some(fit) = quickshape::fit(&pts) else {
            return false;
        };
        let what = super::tools::quick_shape_name(&fit);
        *shape = Some(fit);
        self.stage_drawing(true, cx);
        self.set_status(t!("editor.tools.quickshape", shape = what), false, cx);
        false
    }

    // ── Gestures ────────────────────────────────────────────────────────

    /// The Vector tool went down at `d` (document pixels).
    pub(crate) fn vector_down(
        &mut self,
        d: Pt,
        modifiers: Modifiers,
        click_count: usize,
        cx: &mut Context<Self>,
    ) {
        match self.vector.mode {
            VectorMode::Polyline => self.polyline_click(d, modifiers.shift, click_count, cx),
            mode if mode.is_shape() => self.put_back(VectorDrag::Shape { start: d, end: d }),
            VectorMode::Retouch => {
                let Some((id, set)) = self.vector_target(cx) else {
                    return;
                };
                self.editor.begin("Pencil retouch");
                let mut set = (*set).clone();
                if self.retouch_dab(&mut set, d) {
                    self.stage(id, set.clone(), cx);
                }
                self.put_back(VectorDrag::Retouch { id, set, last: d });
            }
            _ => self.contour_down(d, modifiers.shift, cx),
        }
        cx.notify();
    }

    pub(crate) fn vector_move(&mut self, d: Pt, pen: Option<PenSample>, cx: &mut Context<Self>) {
        let Some(Drag::Tool(ToolDrag::Vector(mut drag))) = self.drag.take() else {
            return;
        };
        let shift = self.drag_shift;
        match drag.as_mut() {
            VectorDrag::Draw { input, shape, .. } => {
                if shape.is_none() {
                    let t = self
                        .tools
                        .stroke_started
                        .map(|s| s.elapsed().as_secs_f64() * 1000.);
                    input.point_full(
                        d.0 as f32,
                        d.1 as f32,
                        pen.map(|p| p.pressure),
                        pen.map(|p| p.tilt),
                        t,
                    );
                }
                self.drag = Some(Drag::Tool(ToolDrag::Vector(drag)));
                self.stage_drawing(false, cx);
                return;
            }
            VectorDrag::Erase {
                id,
                set,
                last,
                radius,
            } => {
                let mut changed = false;
                for p in dabs(*last, d, *radius / 2.) {
                    changed |= set.erase(p, *radius);
                }
                *last = d;
                if changed {
                    self.vector.pending = Some((*id, Arc::new(set.clone())));
                }
            }
            VectorDrag::Retouch { id, set, last } => {
                let mut changed = false;
                let step = f64::from(self.vector.retouch_size) / 6.;
                for p in dabs(*last, d, step.max(0.5)) {
                    changed |= self.retouch_dab(set, p);
                }
                *last = d;
                if changed {
                    self.vector.pending = Some((*id, Arc::new(set.clone())));
                }
            }
            VectorDrag::Shape { end, .. } | VectorDrag::Marquee { end, .. } => *end = d,
            VectorDrag::Edit {
                id,
                base,
                start,
                grab,
            } => {
                let mut set = (**base).clone();
                let (mut dx, mut dy) = (d.0 - start.0, d.1 - start.1);
                match *grab {
                    Grab::Move => {
                        if shift {
                            // Shift keeps the move horizontal or vertical.
                            if dx.abs() > dy.abs() {
                                dy = 0.;
                            } else {
                                dx = 0.;
                            }
                        }
                        let m = DAffine2::from_translation(dvec2(dx, dy));
                        for &i in &self.vector.selection {
                            if let Some(s) = set.strokes.get_mut(i) {
                                s.transform(m);
                            }
                        }
                    }
                    Grab::Point(i, j) => {
                        if let Some(p) = set.strokes.get_mut(i).and_then(|s| s.points.get_mut(j)) {
                            p.x += dx;
                            p.y += dy;
                        }
                    }
                    Grab::Handle(handle, bounds) => {
                        let m = handle_transform(handle, bounds, *start, d, shift);
                        for &i in &self.vector.selection {
                            if let Some(s) = set.strokes.get_mut(i) {
                                s.transform(m);
                            }
                        }
                    }
                }
                self.vector.pending = Some((*id, Arc::new(set)));
            }
        }
        self.drag = Some(Drag::Tool(ToolDrag::Vector(drag)));
        cx.notify();
    }

    pub(crate) fn vector_up(&mut self, mut drag: VectorDrag, cx: &mut Context<Self>) {
        let mut ink = 0.;
        if matches!(drag, VectorDrag::Draw { .. } | VectorDrag::Erase { .. }) {
            self.assist_end();
        }
        match &mut drag {
            VectorDrag::Draw { input, .. } => {
                // Catch the stabilizer up and taper the end, then show the
                // finished line.
                input.finish();
                ink = super::storyboard_extras::ink_of(input.path().iter().map(|s| (s.x, s.y)));
                self.put_back(drag);
                self.stage_drawing(true, cx);
                self.drag = None;
            }
            VectorDrag::Shape { start, end } => {
                if let Some(shape) = drag_shape(self.vector.mode, *start, *end, self.drag_shift) {
                    self.commit_shape(&shape, self.vector.mode.label(), cx);
                }
            }
            VectorDrag::Marquee { start, end, add } => {
                if let Some((_, set)) = self.vector_layer() {
                    let hits = set.in_rect(*start, *end);
                    if !*add {
                        self.vector.selection.clear();
                    }
                    for i in hits {
                        if !self.vector.selection.contains(&i) {
                            self.vector.selection.push(i);
                        }
                    }
                }
            }
            VectorDrag::Erase { .. } | VectorDrag::Retouch { .. } | VectorDrag::Edit { .. } => {}
        }
        self.flush_vector_preview(cx);
        if self.editor.in_transaction() {
            self.editor.end();
        }
        self.note_ink(ink, cx);
        self.tools.stroke_started = None;
        cx.notify();
    }

    /// One retouch dab at `p`; returns whether a stroke changed.
    fn retouch_dab(&self, set: &mut StrokeSet, p: Pt) -> bool {
        set.retouch(
            p,
            f64::from(self.vector.retouch_size / 2.),
            self.vector.retouch,
            self.vector.retouch_amount * 0.2,
        )
    }

    // ── Shapes ──────────────────────────────────────────────────────────

    fn polyline_click(&mut self, d: Pt, shift: bool, click_count: usize, cx: &mut Context<Self>) {
        let tolerance = 8. / self.view.zoom.max(0.01);
        let pts = &self.vector.polyline;
        let d = match pts.last() {
            Some(&last) if shift => snap_45(last, d),
            _ => d,
        };
        let closes = pts.len() >= 3
            && pts
                .first()
                .is_some_and(|p| (p.0 - d.0).hypot(p.1 - d.1) <= tolerance);
        if closes {
            self.finish_polyline(true, cx);
        } else if click_count >= 2 && pts.len() >= 2 {
            self.finish_polyline(false, cx);
        } else {
            self.vector.polyline.push(d);
        }
    }

    /// Draw the polyline placed so far (Enter or a double-click), closed
    /// when it ended on its first point.
    pub(crate) fn finish_polyline(&mut self, closed: bool, cx: &mut Context<Self>) {
        let pts = std::mem::take(&mut self.vector.polyline);
        if pts.len() < 2 {
            return;
        }
        let v: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 as f32, p.1 as f32)).collect();
        let shape = if closed && v.len() >= 3 {
            Shape::Polygon(v)
        } else {
            Shape::Polyline(v)
        };
        self.commit_shape(&shape, "Polyline", cx);
        cx.notify();
    }

    /// Draw `shape`: an editable stroke on a stroke layer, otherwise brush
    /// pixels replayed along it as QuickShape does.
    pub(crate) fn commit_shape(&mut self, shape: &Shape, label: &str, cx: &mut Context<Self>) {
        if self.vector_layer().is_some() {
            let Some((id, set)) = self.vector_target(cx) else {
                return;
            };
            let mut set = (*set).clone();
            let line = Stroke::from_shape(shape, self.tools.fg, self.vector.width);
            let ink = path_length(line.points.iter().map(|p| (p.x, p.y)));
            set.strokes.push(line);
            if self.set_strokes(id, set, label, cx) {
                self.note_ink(ink, cx);
            }
            return;
        }
        let Some(id) = self.paint_target(cx) else {
            return;
        };
        let Some((raster, to_doc)) = self.target_raster(id) else {
            return;
        };
        let to_local = to_doc.inverse();
        let scale = to_doc.matrix2.determinant().abs().sqrt().max(1e-6);
        let mut brush = self.paint_brush();
        brush.size = (f64::from(self.vector.width) / scale).max(1.) as f32;
        let clip = self
            .editor
            .doc
            .selection
            .clone()
            .map(|m| local_clip(m, to_doc));
        let mut stroke = BrushStroke::new(
            raster.clone(),
            brush,
            Ink::Color(premul(self.tools.fg)),
            clip,
        );
        let step = (brush.size * brush.spacing * 0.5).max(1.);
        let path: Vec<(f32, f32)> = shape
            .outline(step, (0., 0.))
            .into_iter()
            .map(|(x, y)| {
                let p = to_local.transform_point2(dvec2(f64::from(x), f64::from(y)));
                (p.x as f32, p.y as f32)
            })
            .collect();
        stroke.replay(&path, 1.);
        let (r, dirty) = stroke.render(&raster);
        let revision = self.editor.revision;
        self.commit_stroke(id, r, dirty, label, tools::PaintTarget::Content, cx);
        if self.editor.revision != revision {
            self.note_ink(super::storyboard_extras::ink_of(path), cx);
        }
    }

    // ── Contour editor ──────────────────────────────────────────────────

    /// The selection's box handle under `d`, in screen-sized tolerances.
    fn contour_handle(&self, d: Pt, bounds: (Pt, Pt)) -> Option<Handle> {
        let z = self.view.zoom.max(0.01);
        let (grab, rotate) = (7. / z, 26. / z);
        let (lo, hi) = bounds;
        let corners = [lo, (hi.0, lo.1), hi, (lo.0, hi.1)];
        let edges = [
            ((lo.0 + hi.0) / 2., lo.1),
            (hi.0, (lo.1 + hi.1) / 2.),
            ((lo.0 + hi.0) / 2., hi.1),
            (lo.0, (lo.1 + hi.1) / 2.),
        ];
        let dist = |p: Pt| (p.0 - d.0).hypot(p.1 - d.1);
        if let Some(i) = (0..4).find(|&i| dist(corners[i]) <= grab) {
            return Some(Handle::Corner(i));
        }
        if let Some(i) = (0..4).find(|&i| dist(edges[i]) <= grab) {
            return Some(Handle::Edge(i));
        }
        let inside = d.0 >= lo.0 && d.0 <= hi.0 && d.1 >= lo.1 && d.1 <= hi.1;
        (!inside && corners.iter().any(|c| dist(*c) <= rotate)).then_some(Handle::Rotate)
    }

    fn contour_down(&mut self, d: Pt, shift: bool, cx: &mut Context<Self>) {
        let Some((id, set)) = self.vector_target(cx) else {
            return;
        };
        let tolerance = 6. / self.view.zoom.max(0.01);
        let selection = self.vector.selection.clone();
        let grab = if let Some(bounds) = set
            .centreline_bounds(&selection)
            .filter(|_| !shift)
            .and_then(|b| Some((self.contour_handle(d, b)?, b)))
        {
            Some((Grab::Handle(bounds.0, bounds.1), "Transform strokes"))
        } else if let Some((i, j)) = set.point_near(d, tolerance, &selection) {
            Some((Grab::Point(i, j), "Move point"))
        } else if let Some(i) = set.hit(d, tolerance) {
            if shift {
                if let Some(k) = self.vector.selection.iter().position(|s| *s == i) {
                    self.vector.selection.remove(k);
                } else {
                    self.vector.selection.push(i);
                }
            } else if !self.vector.selection.contains(&i) {
                self.vector.selection = vec![i];
            }
            Some((Grab::Move, "Move strokes"))
        } else {
            None
        };
        match grab {
            Some((grab, label)) => {
                self.editor.begin(label);
                self.put_back(VectorDrag::Edit {
                    id,
                    base: set,
                    start: d,
                    grab,
                });
            }
            None => self.put_back(VectorDrag::Marquee {
                start: d,
                end: d,
                add: shift,
            }),
        }
    }

    /// Delete the contour editor's selected strokes. Returns whether the
    /// Delete key was used here.
    pub(crate) fn vector_delete(&mut self, cx: &mut Context<Self>) -> bool {
        if self.tool != Tool::Vector
            || self.vector.mode != VectorMode::Contour
            || self.vector.selection.is_empty()
        {
            return false;
        }
        let Some((id, set)) = self.vector_target(cx) else {
            return true;
        };
        let mut set = (*set).clone();
        let mut chosen = std::mem::take(&mut self.vector.selection);
        chosen.sort_unstable();
        chosen.dedup();
        for i in chosen.into_iter().rev() {
            set.strokes.remove(i);
        }
        self.set_strokes(id, set, "Delete strokes", cx);
        true
    }

    // ── Stroke tools (D6) ───────────────────────────────────────────────

    /// The strokes a stroke tool works on: the contour selection, or every
    /// stroke of the layer.
    fn stroke_tool_targets(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(NodeId, StrokeSet, Vec<usize>)> {
        let (id, set) = self.vector_target(cx)?;
        let chosen = if self.tool == Tool::Vector && !self.vector.selection.is_empty() {
            self.vector.selection.clone()
        } else {
            (0..set.strokes.len()).collect()
        };
        Some((id, (*set).clone(), chosen))
    }

    pub(crate) fn smooth_strokes(&mut self, cx: &mut Context<Self>) {
        let Some((id, mut set, chosen)) = self.stroke_tool_targets(cx) else {
            return;
        };
        for &i in &chosen {
            set.strokes[i].smooth(f64::from(self.vector.smooth), 2);
        }
        if self.set_strokes(id, set, "Smooth strokes", cx) {
            self.set_status(format!("Smoothed {} strokes.", chosen.len()), false, cx);
        }
    }

    pub(crate) fn optimize_strokes(&mut self, cx: &mut Context<Self>) {
        let Some((id, mut set, chosen)) = self.stroke_tool_targets(cx) else {
            return;
        };
        let before = set.point_count();
        for &i in &chosen {
            set.strokes[i].simplify(f64::from(self.vector.optimize));
        }
        let removed = before - set.point_count();
        if self.set_strokes(id, set, "Optimize strokes", cx) {
            self.set_status(format!("Optimize removed {removed} points."), false, cx);
        }
    }

    /// Convert pencil lines to brush shapes: filled outlines.
    pub(crate) fn outline_selected_strokes(&mut self, cx: &mut Context<Self>) {
        let Some((id, mut set, chosen)) = self.stroke_tool_targets(cx) else {
            return;
        };
        set.outline_strokes(&chosen);
        if self.set_strokes(id, set, "Lines to shapes", cx) {
            self.vector.selection.clear();
            self.set_status(
                format!("Converted {} lines to shapes.", chosen.len()),
                false,
                cx,
            );
        }
    }

    // ── Tool state ──────────────────────────────────────────────────────

    pub(crate) fn set_vector_mode(&mut self, mode: VectorMode, cx: &mut Context<Self>) {
        if self.tool != Tool::Vector {
            self.set_tool(Tool::Vector, cx);
        }
        if mode != VectorMode::Polyline {
            self.vector.polyline.clear();
        }
        self.vector.mode = mode;
        cx.notify();
    }

    /// N: the shape tools, stepping Line → Rectangle → Ellipse → Polyline
    /// when one is already active.
    pub(crate) fn cycle_vector_shape(&mut self, cx: &mut Context<Self>) {
        let next = match self.vector.mode {
            mode if self.tool == Tool::Vector && mode.is_shape() => {
                let shapes = VectorMode::SHAPES;
                let i = shapes.iter().position(|m| *m == mode).unwrap_or(0);
                shapes[(i + 1) % shapes.len()]
            }
            mode if mode.is_shape() => mode,
            _ => VectorMode::Line,
        };
        self.set_vector_mode(next, cx);
    }

    /// Escape: drop a polyline being placed.
    pub(crate) fn vector_cancel(&mut self) -> bool {
        self.vector.pending = None;
        !std::mem::take(&mut self.vector.polyline).is_empty()
    }

    pub(crate) fn set_vector_slider(&mut self, key: VectorSlider, v: f32, cx: &mut Context<Self>) {
        let squared = |v: f32, max: f32| {
            let f = ((v - 1.) / (max - 1.)).clamp(0., 1.);
            (1. + f * f * (max - 1.)).round()
        };
        let ui = &mut self.vector;
        match key {
            VectorSlider::Width => ui.width = squared(v, 200.),
            VectorSlider::OpacityPressure => ui.dynamics.pressure = v / 100.,
            VectorSlider::OpacityTilt => ui.dynamics.tilt = v / 100.,
            VectorSlider::OpacitySpeed => ui.dynamics.speed = v / 100.,
            VectorSlider::Fade => {
                let f = (v / 2000.).clamp(0., 1.);
                ui.dynamics.fade = (f * f * 2000.).round();
            }
            VectorSlider::RetouchSize => ui.retouch_size = squared(v, 500.),
            VectorSlider::RetouchAmount => ui.retouch_amount = v / 100.,
            VectorSlider::Smooth => ui.smooth = v / 100.,
            VectorSlider::Optimize => ui.optimize = v,
        }
        cx.notify();
    }

    // ── Overlay ─────────────────────────────────────────────────────────

    /// Previews of shapes and polylines, the contour selection with its
    /// points and box, and the retouch brush.
    pub(crate) fn vector_overlay(&self, o: &mut Overlay) {
        let pointer = self.tools.pointer.and_then(|p| self.doc_point(p));
        if let Some(Drag::Tool(ToolDrag::Vector(drag))) = &self.drag {
            match drag.as_ref() {
                VectorDrag::Shape { start, end } => {
                    if let Some(shape) = drag_shape(self.vector.mode, *start, *end, self.drag_shift)
                    {
                        let closed = !matches!(shape, Shape::Line(..));
                        let pts = shape
                            .outline(2., (start.0 as f32, start.1 as f32))
                            .into_iter()
                            .map(|(x, y)| (f64::from(x), f64::from(y)))
                            .collect();
                        o.lines.push((pts, closed));
                    }
                }
                VectorDrag::Marquee { start, end, .. } => o
                    .lines
                    .push((vec![*start, (end.0, start.1), *end, (start.0, end.1)], true)),
                _ => {}
            }
        }
        if self.tool != Tool::Vector {
            return;
        }
        if !self.vector.polyline.is_empty() {
            let mut pts = self.vector.polyline.clone();
            if self.drag.is_none()
                && let Some(p) = pointer
            {
                pts.push(p);
            }
            o.lines.push((pts, false));
        }
        match self.vector.mode {
            VectorMode::Retouch => {
                if let Some(p) = self.tools.pointer {
                    o.cursor = Some((
                        p,
                        (f64::from(self.vector.retouch_size) / 2. * self.view.zoom) as f32,
                    ));
                }
            }
            VectorMode::Contour => {
                let Some((id, set)) = self.vector_layer() else {
                    return;
                };
                if self.vector.layer != Some(id) {
                    return;
                }
                let chosen: Vec<usize> = self
                    .vector
                    .selection
                    .iter()
                    .copied()
                    .filter(|i| *i < set.strokes.len())
                    .collect();
                let mut pen = super::pen::PenOverlay::default();
                for &i in &chosen {
                    let s = &set.strokes[i];
                    pen.curves
                        .push((s.points.iter().map(|p| (p.x, p.y)).collect(), s.closed));
                    pen.anchors
                        .extend(s.points.iter().map(|p| ((p.x, p.y), false, true)));
                }
                o.pen = Some(pen);
                if let Some((lo, hi)) = set.centreline_bounds(&chosen) {
                    o.transform = Some([lo, (hi.0, lo.1), hi, (lo.0, hi.1)]);
                }
            }
            _ => {}
        }
    }

    // ── Options bar ─────────────────────────────────────────────────────

    fn vector_slider(
        &mut self,
        key: VectorSlider,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = &self.vector;
        let sqrt = |v: f32, max: f32| ((v - 1.) / (max - 1.)).clamp(0., 1.).sqrt();
        let (name, display, norm, spec) = match key {
            VectorSlider::Width => (
                "width",
                format!("{:.0}px", ui.width),
                sqrt(ui.width, 200.),
                (1., 200., 1.),
            ),
            VectorSlider::OpacityPressure => (
                "pressure",
                format!("{:.0}%", ui.dynamics.pressure * 100.),
                ui.dynamics.pressure,
                (0., 100., 1.),
            ),
            VectorSlider::OpacityTilt => (
                "tilt",
                format!("{:.0}%", ui.dynamics.tilt * 100.),
                ui.dynamics.tilt,
                (0., 100., 1.),
            ),
            VectorSlider::OpacitySpeed => (
                "speed",
                format!("{:.0}%", ui.dynamics.speed * 100.),
                ui.dynamics.speed,
                (0., 100., 1.),
            ),
            VectorSlider::Fade => (
                "fade",
                if ui.dynamics.fade > 0. {
                    format!("{:.0}px", ui.dynamics.fade)
                } else {
                    "off".into()
                },
                (ui.dynamics.fade / 2000.).sqrt(),
                (0., 2000., 1.),
            ),
            VectorSlider::RetouchSize => (
                "size",
                format!("{:.0}px", ui.retouch_size),
                sqrt(ui.retouch_size, 500.),
                (1., 500., 1.),
            ),
            VectorSlider::RetouchAmount => (
                "amount",
                format!("{:.0}%", ui.retouch_amount * 100.),
                ui.retouch_amount,
                (1., 100., 1.),
            ),
            VectorSlider::Smooth => (
                "strength",
                format!("{:.0}%", ui.smooth * 100.),
                ui.smooth,
                (0., 100., 1.),
            ),
            VectorSlider::Optimize => (
                "tolerance",
                format!("{:.1}px", ui.optimize),
                (ui.optimize - 0.1) / 9.9,
                (0.1, 10., 0.1),
            ),
        };
        self.opt_slider(SliderKey::Vector(key), name, display, norm, spec, p, cx)
    }

    /// Opacity dynamics, shown with the Brush on a stroke layer.
    pub(crate) fn vector_brush_options(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.tool != Tool::Brush
            || self.tools.paint != PaintKind::Brush
            || self.vector_layer().is_none()
        {
            return Vec::new();
        }
        let mut v = vec![self.group("vector opacity", p)];
        for key in [
            VectorSlider::OpacityPressure,
            VectorSlider::OpacityTilt,
            VectorSlider::OpacitySpeed,
            VectorSlider::Fade,
        ] {
            v.push(self.vector_slider(key, p, cx));
        }
        v
    }

    #[allow(clippy::too_many_arguments)] // a UI row: each argument is one visible property
    fn vector_action(
        &self,
        id: &'static str,
        text: &'static str,
        help: &'static str,
        enabled: bool,
        p: &Palette,
        cx: &mut Context<Self>,
        run: fn(&mut EditorView, &mut Context<EditorView>),
    ) -> AnyElement {
        tip(
            chip_action(
                id,
                text,
                false,
                enabled,
                p,
                cx.listener(move |this, _, _, cx| run(this, cx)),
            )
            .test_support(),
            help,
        )
        .into_any_element()
    }

    pub(crate) fn vector_tool_options(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v = Vec::new();
        let mode = self.vector.mode;
        let layer = self.vector_layer();
        if mode.is_shape() {
            for (id, m) in [
                ("vec-line", VectorMode::Line),
                ("vec-rect", VectorMode::Rectangle),
                ("vec-ellipse", VectorMode::Ellipse),
                ("vec-polyline", VectorMode::Polyline),
            ] {
                v.push(self.mode_chip(
                    id,
                    match m {
                        VectorMode::Line => "line",
                        VectorMode::Rectangle => "rect",
                        VectorMode::Ellipse => "ellipse",
                        _ => "polyline",
                    },
                    m,
                    mode,
                    p,
                    cx,
                    EditorView::set_vector_mode,
                ));
            }
            v.push(self.vector_slider(VectorSlider::Width, p, cx));
            if mode == VectorMode::Polyline {
                let ready = self.vector.polyline.len() >= 2;
                v.push(self.vector_action(
                    "vec-polyline-finish",
                    "Finish",
                    "Draw the polyline (Enter or double-click); click the first point to close it",
                    ready,
                    p,
                    cx,
                    |this, cx| this.finish_polyline(false, cx),
                ));
            }
            v.push(
                div()
                    .flex_none()
                    .child(if layer.is_some() {
                        "Shift constrains · draws editable strokes"
                    } else {
                        "Shift constrains · draws pixels; a vector layer keeps strokes"
                    })
                    .into_any_element(),
            );
            return v;
        }
        if layer.is_none() {
            v.push(self.vector_action(
                "vec-new-layer",
                "New vector layer",
                "Add an empty vector layer to draw editable strokes on",
                true,
                p,
                cx,
                |this, cx| {
                    this.new_vector_layer(cx);
                },
            ));
            return v;
        }
        if mode == VectorMode::Retouch {
            for (id, text, r) in [
                ("rt-thicker", "thicker", Retouch::Thicker),
                ("rt-thinner", "thinner", Retouch::Thinner),
                ("rt-opaquer", "opaquer", Retouch::Opaquer),
                ("rt-fainter", "fainter", Retouch::Fainter),
                ("rt-smooth", "smooth", Retouch::Smooth),
            ] {
                v.push(
                    self.mode_chip(id, text, r, self.vector.retouch, p, cx, |this, r, cx| {
                        this.vector.retouch = r;
                        cx.notify();
                    }),
                );
            }
            v.push(self.vector_slider(VectorSlider::RetouchSize, p, cx));
            v.push(self.vector_slider(VectorSlider::RetouchAmount, p, cx));
            return v;
        }
        let selected = self.vector.selection.len();
        v.push(
            mono(
                if selected == 0 {
                    "all strokes".to_string()
                } else {
                    format!("{selected} selected")
                },
                9.5,
                p.muted,
            )
            .flex_none()
            .into_any_element(),
        );
        v.push(self.vector_slider(VectorSlider::Smooth, p, cx));
        v.push(self.vector_action(
            "vec-smooth",
            "Smooth",
            "Smooth the selected strokes, or all strokes on the layer",
            true,
            p,
            cx,
            EditorView::smooth_strokes,
        ));
        v.push(self.vector_slider(VectorSlider::Optimize, p, cx));
        v.push(self.vector_action(
            "vec-optimize",
            "Optimize",
            "Remove points that do not change the line's shape, width or opacity",
            true,
            p,
            cx,
            EditorView::optimize_strokes,
        ));
        v.push(self.vector_action(
            "vec-outline",
            "Lines → shapes",
            "Convert pencil lines to filled brush shapes",
            true,
            p,
            cx,
            EditorView::outline_selected_strokes,
        ));
        v.push(self.vector_action(
            "vec-delete",
            "Delete",
            "Delete the selected strokes (Delete)",
            selected > 0,
            p,
            cx,
            |this, cx| {
                this.vector_delete(cx);
            },
        ));
        v
    }
}

/// Points from `a` (excluded) to `b` (included), at most `step` apart, so a
/// fast drag erases or retouches without gaps.
fn dabs(a: Pt, b: Pt, step: f64) -> Vec<Pt> {
    let len = (b.0 - a.0).hypot(b.1 - a.1);
    let n = ((len / step.max(0.25)).ceil() as usize).clamp(1, 4096);
    (1..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        })
        .collect()
}

#[cfg(test)]
#[path = "vector_strokes_tests.rs"]
mod vector_strokes_tests;
