//! Interactive tools: selection, painting, healing, cloning, crop, shapes,
//! and the colour picker. Every result lands through the Command API, so it
//! is one undo step and visible to the assistant.

use super::crop::{CropOptions, crop_rect};
use super::*;
use crate::widgets::{chip_action, tip};
use emulsion_raster::composite::{PixelSampler, region};
use emulsion_raster::paint::{Brush, BrushBlend, Clip, GrainKind, Ink, Stroke, fill_color};
use emulsion_raster::select::{self, Combine};
use emulsion_raster::{IRect, Mask, fill};
use glam::{DAffine2, dvec2};
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

#[path = "shape_fill.rs"]
mod shape_fill;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BrushSettingsSection {
    Presets,
    #[default]
    Tip,
    Texture,
    Dynamics,
    Drawing,
}

pub(super) fn zoom_out(modifiers: Modifiers) -> bool {
    modifiers.shift || modifiers.alt
}

/// Marching-ants outline segments: (x0, y0, x1, y1) in document pixels.
pub(crate) type Segments = Arc<Vec<(f32, f32, f32, f32)>>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectShape {
    Rect,
    Ellipse,
    Lasso,
    Polygon,
    Wand,
    /// Brush over an area; the selection grows through similar colour.
    Quick,
    /// Click anchors; the outline snaps to edges between them.
    Magnetic,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PaintKind {
    Brush,
    Eraser,
    /// Drag the colour already on the layer.
    Smudge,
    Bucket,
    Gradient,
    /// Push, twirl, pinch and expand the pixels themselves.
    Liquify,
}

/// Which tool a brush setting belongs to. Each keeps its own size,
/// hardness and the rest, so changing the eraser never changes the brush.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BrushSlot {
    Paint(PaintKind),
    Heal,
    Clone,
    Mask,
}

impl BrushSlot {
    pub fn of(tool: Tool, paint: PaintKind) -> Option<BrushSlot> {
        match tool {
            Tool::Brush => Some(BrushSlot::Paint(paint)),
            Tool::Heal => Some(BrushSlot::Heal),
            Tool::Clone => Some(BrushSlot::Clone),
            Tool::Mask => Some(BrushSlot::Mask),
            _ => None,
        }
    }

    /// A sensible starting brush for the slot.
    pub fn default_brush(self) -> Brush {
        let mut b = Brush::default();
        match self {
            BrushSlot::Paint(PaintKind::Eraser) => {
                b.size = 40.0;
                b.hardness = 0.9;
            }
            BrushSlot::Paint(PaintKind::Smudge) => {
                b.size = 60.0;
                b.hardness = 0.3;
            }
            BrushSlot::Paint(PaintKind::Liquify) => {
                b.size = 80.0;
                b.flow = 0.6;
            }
            BrushSlot::Mask => {
                b.size = 60.0;
                b.hardness = 0.5;
            }
            BrushSlot::Heal | BrushSlot::Clone => {
                b.size = 30.0;
                b.hardness = 0.6;
            }
            _ => {}
        }
        b
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind {
    Rect,
    Ellipse,
}

pub struct ToolState {
    pub transform_lift: Option<super::clipboard::TransformLift>,
    pub rotate_view: bool,
    pub select: SelectShape,
    pub combine: Combine,
    pub feather: f32,
    pub tolerance: u8,
    pub contiguous: bool,
    pub paint: PaintKind,
    pub brush: Brush,
    pub radial: bool,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
    pub shape: ShapeKind,
    pub clone_source: Option<(f64, f64)>,
    pub clone_offset: Option<(f64, f64)>,
    pub polygon: Vec<(f64, f64)>,
    polygon_combine: Combine,
    /// Pending crop rectangle in document pixels, until Enter.
    pub crop: Option<(f64, f64, f64, f64)>,
    pub crop_options: CropOptions,
    pub straighten: f32,
    /// Crops grow from their centre.
    pub crop_centered: bool,
    /// Fill canvas that a crop or canvas-size change adds, from the image.
    pub fill_edges: bool,
    /// Crop cuts pixel layers down to the canvas (Photoshop's "delete
    /// cropped pixels"); off keeps them whole beyond the edge.
    pub crop_delete: bool,
    /// Mirror strokes across the canvas centre.
    pub mirror_x: bool,
    pub mirror_y: bool,
    /// Rotational symmetry around the canvas centre (0 or 1 = off).
    pub symmetry: u32,
    /// Alpha lock: paint only where the layer already has pixels.
    pub alpha_lock: bool,
    /// Interactive wet paint and smudge sample the visible canvas by default.
    /// MCP's separate sample_merged option samples lower layers instead.
    pub sample_merged: bool,
    /// Drawing guide and assist.
    pub guide: super::guides::GuideState,
    /// What the Liquify brush does.
    pub liquify: emulsion_raster::liquify::Mode,
    /// Original and latest raster for consecutive liquify strokes on one layer.
    liquify_session: Option<(NodeId, Arc<Raster>, Arc<Raster>)>,
    /// Saved brush settings (and preset name) for the slots not in use.
    pub kits: std::collections::HashMap<BrushSlot, (Brush, Option<String>)>,
    /// Mask tool: painting reveals (white) or hides (black).
    pub mask_reveal: bool,
    /// When the current stroke started, for speed dynamics.
    pub stroke_started: Option<Instant>,
    pub(crate) stroke_preview_pending: bool,
    /// QuickShape: holding the pointer still at the end of a stroke snaps
    /// it to the line, polygon, circle or ellipse it was aiming for.
    pub quick_shape: bool,
    pub pen: super::pen::PenState,
    /// Brush and eraser paint the selected node's mask instead of pixels.
    pub mask_edit: bool,
    pub pointer: Option<Point<Pixels>>,
    pub ants_phase: bool,
    pub picker: bool,
    /// Hue kept separately so greys do not lose it.
    pub hue: f32,
    ants: Option<(usize, u32, Segments)>,
    /// Selection bounds by selection identity, for the Info panel.
    pub(crate) sel_bounds: Option<(usize, IRect)>,
    /// Whether the SAM model is installed, checked at most every 2 s (it
    /// is a filesystem stat and the options bar asks every frame).
    sam_ok: Option<(Instant, bool)>,
    /// Magnetic lasso: the edge-following path from the last anchor to the pointer.
    pub magnetic_live: Vec<(f64, f64)>,
    /// Edge map of the composite for the magnetic lasso, by revision.
    edges: Option<(u64, Arc<Vec<f32>>)>,
    edges_loading: Option<u64>,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            transform_lift: None,
            rotate_view: false,
            select: SelectShape::Rect,
            combine: Combine::Replace,
            feather: 0.0,
            tolerance: 32,
            contiguous: true,
            paint: PaintKind::Brush,
            brush: Brush::default(),
            radial: false,
            fg: [10, 10, 11, 255],
            bg: [255, 255, 255, 255],
            shape: ShapeKind::Rect,
            clone_source: None,
            clone_offset: None,
            polygon: Vec::new(),
            polygon_combine: Combine::Replace,
            crop: None,
            crop_options: CropOptions::default(),
            straighten: 0.0,
            crop_centered: false,
            fill_edges: false,
            crop_delete: true,
            mirror_x: false,
            mirror_y: false,
            symmetry: 0,
            alpha_lock: false,
            sample_merged: true,
            guide: Default::default(),
            liquify: emulsion_raster::liquify::Mode::Push,
            liquify_session: None,
            kits: Default::default(),
            mask_reveal: true,
            stroke_started: None,
            stroke_preview_pending: false,
            quick_shape: true,
            pen: super::pen::PenState::fresh(),
            mask_edit: false,
            pointer: None,
            ants_phase: false,
            picker: false,
            hue: 0.0,
            ants: None,
            sel_bounds: None,
            sam_ok: None,
            magnetic_live: Vec::new(),
            edges: None,
            edges_loading: None,
        }
    }
}

pub enum ToolDrag {
    Stroke {
        id: NodeId,
        stroke: Box<Stroke>,
        to_local: DAffine2,
        heal: bool,
        label: &'static str,
        /// The stroke paints the node's mask; the raster is a grey view of it.
        mask: bool,
        /// That grey view, kept up to date as the stroke renders, so the
        /// whole mask is not re-converted on every pointer move.
        mask_raster: Option<Arc<Raster>>,
    },
    /// Liquify: the layer as it was when the tool went down (for Restore)
    /// and the last dab position in layer pixels.
    Liquify {
        id: NodeId,
        original: Arc<Raster>,
        to_local: DAffine2,
        last: (f32, f32),
    },
    Marquee {
        start: (f64, f64),
        end: (f64, f64),
        combine: Combine,
        ellipse: bool,
    },
    Lasso {
        pts: Vec<(f64, f64)>,
        combine: Combine,
    },
    Gradient {
        start: (f64, f64),
        end: (f64, f64),
    },
    Crop {
        start: (f64, f64),
        end: (f64, f64),
        /// Grow from the start point in both directions (Alt, or the chip).
        symmetric: bool,
    },
    Shape {
        start: (f64, f64),
        end: (f64, f64),
        ellipse: bool,
    },
    PickSv {
        track: TrackBounds,
    },
    /// Dragging inside the selection moves it.
    MoveSelection {
        start: (f64, f64),
        orig: Arc<Mask>,
    },
    Quick {
        pts: Vec<(f64, f64)>,
        combine: Combine,
    },
    Pen(super::pen::PenDrag),
    PickHue {
        track: TrackBounds,
    },
}

/// A mask as grey pixels, so the brush engine can paint it.
pub(crate) fn mask_to_raster(m: &Mask) -> Raster {
    let b = m.bounds();
    let px: Vec<[u16; 4]> = m
        .read_rect(b)
        .into_iter()
        .map(|v| {
            let l = color::f_to_u16(color::srgb_to_linear(v as f32 / 255.0));
            [l, l, l, 65535]
        })
        .collect();
    Raster::from_pixels(m.width(), m.height(), [0; 4], &px)
}

fn combine_for(m: &Modifiers, default: Combine) -> Combine {
    match (m.shift, m.alt) {
        (true, true) => Combine::Intersect,
        (true, false) => Combine::Add,
        (false, true) => Combine::Subtract,
        _ => default,
    }
}

fn norm(a: (f64, f64), b: (f64, f64)) -> (f64, f64, f64, f64) {
    (
        a.0.min(b.0),
        a.1.min(b.1),
        (a.0 - b.0).abs(),
        (a.1 - b.1).abs(),
    )
}

/// Keep the raw pointer endpoint so releasing Shift restores the free aspect.
pub(super) fn shape_rect(
    start: (f64, f64),
    end: (f64, f64),
    constrain: bool,
) -> (f64, f64, f64, f64) {
    if !constrain {
        return norm(start, end);
    }
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let side = dx.abs().max(dy.abs());
    let end = (
        start.0 + if dx < 0.0 { -side } else { side },
        start.1 + if dy < 0.0 { -side } else { side },
    );
    norm(start, end)
}

pub(crate) fn premul(c: [u8; 4]) -> [f32; 4] {
    color::srgba8_to_premul(c)
}

/// Selection coverage for a layer pixel, through the layer's placement.
pub(crate) fn local_clip(mask: Arc<Mask>, to_doc: DAffine2) -> Clip {
    Arc::new(move |x, y| {
        let p = to_doc.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
        if p.x < 0.0 || p.y < 0.0 || p.x >= mask.width() as f64 || p.y >= mask.height() as f64 {
            return 0.0;
        }
        mask.get(p.x as u32, p.y as u32) as f32 / 255.0
    })
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h6 = (h.rem_euclid(1.0)) * 6.0;
    let c = v * s;
    let x = c * (1.0 - ((h6 % 2.0) - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r, g, b].map(|u| ((u + m) * 255.0).round() as u8)
}

pub fn rgb_to_hsv(c: [u8; 4]) -> (f32, f32, f32) {
    let [r, g, b] = [c[0], c[1], c[2]].map(|u| u as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d < 1e-6 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if mx > 0.0 { d / mx } else { 0.0 }, mx)
}

impl EditorView {
    pub(crate) fn doc_point(&self, pos: Point<Pixels>) -> Option<(f64, f64)> {
        let b = self.canvas_bounds()?;
        Some(
            self.view
                .screen_to_doc((f32::from(pos.x) as f64, f32::from(pos.y) as f64), &b),
        )
    }

    /// Window position of a document point, used by the headless tests.
    pub(crate) fn doc_to_window(&self, d: (f64, f64)) -> Option<Point<Pixels>> {
        let b = self.canvas_bounds()?;
        let s = self.view.doc_to_screen(d, &b);
        Some(point(px(s.0 as f32), px(s.1 as f32)))
    }

    /// Resolve the old tool before its controls and preview disappear.
    pub(super) fn finish_tool_interaction(&mut self, cx: &mut Context<Self>) {
        self.finish_shape_color_edit(cx);
        self.tools.transform_lift = None;
        if matches!(self.drag, Some(Drag::Pan { .. } | Drag::RotateView { .. })) {
            self.drag = None;
        }
        if matches!(self.drag, Some(Drag::Move(_) | Drag::Transform(_))) {
            self.drag = None;
            self.snap_lines.clear();
            self.editor.end();
        } else if matches!(self.drag, Some(Drag::Tool(_))) {
            let Some(Drag::Tool(drag)) = self.drag.take() else {
                unreachable!()
            };
            // These gestures have already modified the document. Finish their
            // history step; discard previews which have not changed anything.
            if matches!(
                drag,
                ToolDrag::Stroke { .. }
                    | ToolDrag::Liquify { .. }
                    | ToolDrag::MoveSelection { .. }
                    | ToolDrag::PickSv { .. }
                    | ToolDrag::PickHue { .. }
                    | ToolDrag::Pen(_)
            ) {
                self.tool_up(drag, cx);
            }
        }
        if matches!(self.drag, Some(Drag::Warp(_) | Drag::Distort { .. })) {
            self.drag = None;
        }
        if self.warp.is_some() {
            self.cancel_warp(cx);
        }
        self.tools.polygon.clear();
        self.tools.magnetic_live.clear();
        self.tools.crop = None;
        self.tools.straighten = 0.0;
        self.pen_cancel();
        self.close_text_field(cx);
    }

    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if tool != self.tool {
            self.finish_tool_interaction(cx);
            self.type_tool.selection = None;
        }
        let from = BrushSlot::of(self.tool, self.tools.paint);
        self.tool = tool;
        self.switch_slot(from, BrushSlot::of(tool, self.tools.paint), cx);
        if self.sidebar_tab == SidebarTab::BrushSettings && !self.brushy() {
            self.select_sidebar(SidebarTab::History, cx);
        }
        self.tools.mask_edit = tool == Tool::Mask
            || (self.tools.mask_edit
                && matches!(tool, Tool::Move | Tool::Brush | Tool::Clone | Tool::Heal));
        if tool == Tool::Grade {
            // Open the selected adjustment for editing, or offer new layers.
            let tab = if self
                .selected
                .and_then(|id| self.editor.doc.node(id))
                .is_some_and(|node| matches!(node.kind, NodeKind::Adjust(_)))
            {
                SidebarTab::Properties
            } else {
                SidebarTab::Adjustments
            };
            self.select_sidebar(tab, cx);
        }
        if tool == Tool::Shape {
            self.select_sidebar(SidebarTab::Properties, cx);
        }
        if tool == Tool::Mask {
            // The tool needs a mask to paint; a node without one gets a
            // fully revealing mask now.
            if self.mask_target(cx).is_none() {
                self.tool = Tool::Brush;
                self.tools.mask_edit = false;
            }
        }
        cx.notify();
    }

    /// Put the current brush away under `from` and take out the one saved
    /// for `to` (or its default). Nothing happens when they are the same.
    fn switch_slot(
        &mut self,
        from: Option<BrushSlot>,
        to: Option<BrushSlot>,
        cx: &mut Context<Self>,
    ) {
        if from == to {
            return;
        }
        self.remember_brush_for_slot(from, cx);
        if let Some(f) = from {
            self.presets.ids.insert(f, self.presets.current_id.clone());
            self.presets.definitions.insert(f, self.presets.definition);
            self.tools
                .kits
                .insert(f, (self.tools.brush, self.presets.current.clone()));
        }
        if let Some(t) = to {
            self.presets.current_id = self.presets.ids.get(&t).cloned().flatten();
            self.presets.definition = self.presets.definitions.get(&t).copied().flatten();
            let previous_definition = self.presets.definition;
            let (b, name) = self
                .tools
                .kits
                .get(&t)
                .cloned()
                .unwrap_or_else(|| (t.default_brush(), None));
            self.tools.brush = b;
            self.presets.current = name;
            if let Some(id) = self.presets.current_id.as_deref()
                && let Some(library) = &self.presets.library
            {
                if let Some(definition) = library.read(cx).catalog.brush(id) {
                    let old = self.presets.definition;
                    if old != Some(definition.brush) {
                        let size = old.filter(|old| b.size != old.size).map(|_| b.size);
                        let opacity = old
                            .filter(|old| b.opacity != old.opacity)
                            .map(|_| b.opacity);
                        self.tools.brush = definition.brush;
                        if let Some(size) = size {
                            self.tools.brush.size = size;
                        }
                        if let Some(opacity) = opacity {
                            self.tools.brush.opacity = opacity;
                        }
                    }
                    self.presets.current = Some(definition.name.clone());
                    self.presets.definition = Some(definition.brush);
                } else {
                    self.presets.current_id = None;
                    self.presets.current = None;
                    self.presets.definition = None;
                }
            }
            self.restore_brush_slot_memory(previous_definition, cx);
        }
    }

    pub fn paint_kind(&self) -> PaintKind {
        self.tools.paint
    }

    pub fn set_mask_edit(&mut self, on: bool, cx: &mut Context<Self>) {
        self.tools.mask_edit = on;
        cx.notify();
    }

    pub fn set_mirror(&mut self, x: bool, y: bool, cx: &mut Context<Self>) {
        self.tools.mirror_x = x;
        self.tools.mirror_y = y;
        cx.notify();
    }

    pub fn set_fg(&mut self, rgba: [u8; 4], cx: &mut Context<Self>) {
        self.tools.hue = rgb_to_hsv(rgba).0;
        self.apply_foreground(rgba, cx);
    }

    pub(crate) fn set_type_mode(&mut self, vertical: bool, cx: &mut Context<Self>) {
        self.finish_tool_interaction(cx);
        self.close_text_field(cx);
        self.set_tool(Tool::Type, cx);
        self.type_tool.spec.vertical = vertical;
        cx.notify();
    }

    fn foreground_edits_text(&self) -> bool {
        matches!(self.tool, Tool::Type | Tool::Move)
            && self
                .text_target()
                .is_some_and(|(id, _)| self.editor.doc.locked_ancestor(id).is_none())
    }

    // HSV drags preserve the chosen hue even at zero saturation/value.
    fn apply_foreground(&mut self, rgba: [u8; 4], cx: &mut Context<Self>) {
        self.tools.fg = rgba;
        if self.foreground_edits_text() {
            self.restyle_text(|spec| spec.color = rgba, cx);
        }
        cx.notify();
    }

    fn begin_colour_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        window.focus(&self.canvas_focus, cx);
        if self.foreground_edits_text() {
            self.editor.begin("Text colour");
        }
    }

    pub(crate) fn open_text_colour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        if let Some((_, spec)) = self.text_target() {
            self.tools.fg = self
                .text_style_range()
                .map_or(spec.color, |range| spec.style_at(range.start).color);
        }
        self.tools.hue = rgb_to_hsv(self.tools.fg).0;
        self.tools.picker = true;
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }

    pub fn set_paint(&mut self, kind: PaintKind, cx: &mut Context<Self>) {
        // Choosing a different brush or preset while explicitly editing a mask
        // must keep painting that mask. Bucket also keeps the mask selected
        // through the dedicated Mask tool or the Layers thumbnail.
        let mask_edit = self.tools.mask_edit;
        if self.tool != Tool::Brush || kind != self.tools.paint {
            self.finish_tool_interaction(cx);
        }
        if kind != PaintKind::Liquify {
            self.tools.liquify_session = None;
        }
        let from = BrushSlot::of(self.tool, self.tools.paint);
        self.tool = Tool::Brush;
        self.tools.mask_edit = mask_edit;
        self.tools.paint = kind;
        self.switch_slot(from, Some(BrushSlot::Paint(kind)), cx);
        if self.sidebar_tab == SidebarTab::BrushSettings && !self.brushy() {
            self.select_sidebar(SidebarTab::History, cx);
        }
        cx.notify();
    }

    pub fn select_shape(&self) -> SelectShape {
        self.tools.select
    }

    pub fn set_select(&mut self, shape: SelectShape, cx: &mut Context<Self>) {
        if self.tool == Tool::Select && self.tools.select == shape {
            return;
        }
        self.finish_tool_interaction(cx);
        self.selection_request = self.selection_request.wrapping_add(1);
        self.set_tool(Tool::Select, cx);
        self.tools.select = shape;
        self.tools.polygon.clear();
        self.tools.magnetic_live.clear();
        if shape == SelectShape::Magnetic {
            self.ensure_edges(cx);
        }
        cx.notify();
    }

    pub fn swap_colors(&mut self, cx: &mut Context<Self>) {
        std::mem::swap(&mut self.tools.fg, &mut self.tools.bg);
        self.set_fg(self.tools.fg, cx);
    }

    pub fn default_colors(&mut self, cx: &mut Context<Self>) {
        self.tools.bg = [255, 255, 255, 255];
        self.set_fg([10, 10, 11, 255], cx);
    }

    pub fn brush_size(&mut self, larger: bool, cx: &mut Context<Self>) {
        let s = self.tools.brush.size;
        let step = if s < 10.0 {
            1.0
        } else if s < 50.0 {
            5.0
        } else if s < 200.0 {
            10.0
        } else {
            50.0
        };
        self.tools.brush.size = (if larger { s + step } else { s - step }).clamp(1.0, 2000.0);
        self.remember_active_brush(cx);
        cx.notify();
    }

    /// The pixel node strokes go into: the selected one, or a new empty
    /// layer above the selection when that is not a pixel node.
    /// Is the SAM quick-select model installed? Cached briefly.
    pub(crate) fn sam_available(&mut self) -> bool {
        if let Some((t, ok)) = self.tools.sam_ok
            && t.elapsed().as_secs_f32() < 2.0
        {
            return ok;
        }
        let ok = emulsion_ai::sam::available().is_some();
        self.tools.sam_ok = Some((Instant::now(), ok));
        ok
    }

    /// Bounds of the current selection, cached by its identity.
    pub(crate) fn selection_bounds(&mut self) -> Option<IRect> {
        let s = self.editor.doc.selection.as_ref()?;
        let key = Arc::as_ptr(s) as usize;
        if let Some((k, b)) = self.tools.sel_bounds
            && k == key
        {
            return Some(b);
        }
        let b = select::bounds(s);
        self.tools.sel_bounds = Some((key, b));
        Some(b)
    }

    pub(crate) fn paint_target(&mut self, cx: &mut Context<Self>) -> Option<NodeId> {
        if let Some(id) = self.selected
            && let Some(n) = self.editor.doc.node(id)
        {
            if self.editor.doc.locked_ancestor(id).is_some() {
                self.set_status("That layer is locked.", true, cx);
                return None;
            }
            if matches!(n.kind, NodeKind::Raster { .. }) {
                return Some(id);
            }
        }
        self.new_empty_layer(cx)
    }

    /// Add an empty, transparent pixel layer the size of the canvas above
    /// the selected layer (or on top), select it, and return its id: what
    /// "+ Layer › Empty layer" and Ctrl-Shift-N do, and what painting does
    /// when nothing paintable is selected.
    pub(crate) fn new_empty_layer(&mut self, cx: &mut Context<Self>) -> Option<NodeId> {
        let k = self
            .editor
            .doc
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Raster { .. }))
            .count()
            + 1;
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let node = Node::raster(
            0,
            format!("Layer {k}"),
            Arc::new(Raster::transparent(w, h)),
            Placement::default(),
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

    pub(crate) fn target_raster(&self, id: NodeId) -> Option<(Arc<Raster>, DAffine2)> {
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Raster { raster, placement } => Some((
                raster.clone(),
                placement.to_doc(raster.width(), raster.height()),
            )),
            _ => None,
        }
    }

    pub(crate) fn apply_selection(&mut self, new: Mask, combine: Combine, cx: &mut Context<Self>) {
        let feather = self.tools.feather;
        if feather > 0.5 {
            // Feathering a big selection takes a moment: off the UI thread.
            let existing = self.editor.doc.selection.clone();
            let ticket = self.selection_ticket();
            cx.spawn(async move |this, cx| {
                let selection = cx
                    .background_spawn(async move {
                        let soft = select::feather(&new, feather);
                        let combined = select::combine(existing.as_deref(), &soft, combine);
                        (!select::bounds(&combined).is_empty()).then(|| Arc::new(combined))
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if !this.selection_is_current(ticket) {
                        return;
                    }
                    if selection.is_some() && this.selected.is_none() {
                        let selected = this.editor.doc.nodes.last().map(|node| node.id);
                        this.set_layer_selection(selected.into_iter().collect(), selected);
                    }
                    this.execute(Command::SetSelection { selection }, cx);
                })
                .ok();
            })
            .detach();
            return;
        }
        let combined = select::combine(self.editor.doc.selection.as_deref(), &new, combine);
        let selection = (!select::bounds(&combined).is_empty()).then(|| Arc::new(combined));
        if selection.is_some() && self.selected.is_none() {
            let selected = self.editor.doc.nodes.last().map(|node| node.id);
            self.set_layer_selection(selected.into_iter().collect(), selected);
        }
        self.execute(Command::SetSelection { selection }, cx);
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        // Restore an active layer after clicking empty space, so Select All
        // followed by Copy works just as it does when a document first opens.
        if self.selected.is_none() {
            let selected = self.editor.doc.nodes.last().map(|node| node.id);
            self.set_layer_selection(selected.into_iter().collect(), selected);
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        self.execute(
            Command::SetSelection {
                selection: Some(Arc::new(select::all(w, h))),
            },
            cx,
        );
    }

    pub fn deselect(&mut self, cx: &mut Context<Self>) {
        self.execute(Command::SetSelection { selection: None }, cx);
    }

    pub fn invert_selection(&mut self, cx: &mut Context<Self>) {
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let inv = match &self.editor.doc.selection {
            Some(s) => select::invert(s),
            None => select::all(w, h),
        };
        let selection = (!select::bounds(&inv).is_empty()).then(|| Arc::new(inv));
        self.execute(Command::SetSelection { selection }, cx);
    }

    pub fn modify_selection(&mut self, grow: i32, feather: f32, cx: &mut Context<Self>) {
        let Some(s) = self.editor.doc.selection.clone() else {
            self.set_status("Nothing is selected.", false, cx);
            return;
        };
        self.set_status("Modifying the selection…", false, cx);
        let ticket = self.selection_ticket();
        cx.spawn(async move |this, cx| {
            let selection = cx
                .background_spawn(async move {
                    let mut m = (*s).clone();
                    if grow != 0 {
                        m = select::grow(&m, grow);
                    }
                    if feather > 0.0 {
                        m = select::feather(&m, feather);
                    }
                    (!select::bounds(&m).is_empty()).then(|| Arc::new(m))
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.selection_is_current(ticket) {
                    return;
                }
                this.status = None;
                this.execute(Command::SetSelection { selection }, cx);
            })
            .ok();
        })
        .detach();
    }

    // ── Pointer ─────────────────────────────────────────────────────────

    pub(crate) fn tool_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.doc_point(e.position) else {
            return;
        };
        match self.tool {
            Tool::Select => {
                self.selection_request = self.selection_request.wrapping_add(1);
                let combine = combine_for(&e.modifiers, self.tools.combine);
                let plain = combine == Combine::Replace
                    && matches!(
                        self.tools.select,
                        SelectShape::Rect | SelectShape::Ellipse | SelectShape::Lasso
                    );
                if plain
                    && let Some(sel) = self.editor.doc.selection.clone()
                    && d.0 >= 0.0
                    && d.1 >= 0.0
                    && d.0 < sel.width() as f64
                    && d.1 < sel.height() as f64
                    && sel.get(d.0 as u32, d.1 as u32) > 127
                {
                    self.editor.begin("Move selection");
                    self.drag = Some(Drag::Tool(ToolDrag::MoveSelection {
                        start: d,
                        orig: sel,
                    }));
                    return;
                }
                match self.tools.select {
                    SelectShape::Quick => {
                        self.drag = Some(Drag::Tool(ToolDrag::Quick {
                            pts: vec![d],
                            combine,
                        }));
                    }
                    SelectShape::Magnetic => self.magnetic_click(d, combine, e.click_count, cx),
                    SelectShape::Rect | SelectShape::Ellipse => {
                        let ellipse = self.tools.select == SelectShape::Ellipse;
                        self.drag = Some(Drag::Tool(ToolDrag::Marquee {
                            start: d,
                            end: d,
                            combine,
                            ellipse,
                        }));
                    }
                    SelectShape::Lasso => {
                        self.drag = Some(Drag::Tool(ToolDrag::Lasso {
                            pts: vec![d],
                            combine,
                        }))
                    }
                    SelectShape::Polygon => {
                        let pts = &self.tools.polygon;
                        let near_start = pts.first().is_some_and(|p0| {
                            let z = self.view.zoom;
                            ((p0.0 - d.0) * z).hypot((p0.1 - d.1) * z) < 8.0
                        });
                        if pts.len() >= 3 && (near_start || e.click_count >= 2) {
                            self.commit_polygon(cx);
                        } else {
                            if pts.is_empty() {
                                self.tools.polygon_combine = combine;
                            }
                            self.tools.polygon.push(d);
                        }
                    }
                    SelectShape::Wand => self.wand(d, combine, cx),
                }
            }
            Tool::Brush => {
                if e.modifiers.alt {
                    self.eyedropper(d, cx);
                    return;
                }
                match self.tools.paint {
                    PaintKind::Brush => self.start_stroke(
                        d,
                        Ink::Color(premul(self.tools.fg)),
                        false,
                        "Brush stroke",
                        cx,
                    ),
                    PaintKind::Eraser => self.start_stroke(d, Ink::Erase, false, "Erase", cx),
                    PaintKind::Smudge => self.start_stroke(d, Ink::Smudge, false, "Smudge", cx),
                    PaintKind::Bucket => self.bucket(d, cx),
                    PaintKind::Gradient => {
                        self.drag = Some(Drag::Tool(ToolDrag::Gradient { start: d, end: d }))
                    }
                    PaintKind::Liquify => self.start_liquify(d, cx),
                }
            }
            Tool::Mask => {
                self.tools.mask_edit = true;
                let ink = Ink::Color(premul(self.tools.fg));
                self.start_stroke(d, ink, false, "Paint mask", cx);
            }
            Tool::Heal => self.start_stroke(
                d,
                Ink::Color([0.45, 0.05, 0.03, 0.5]),
                true,
                "Spot heal",
                cx,
            ),
            Tool::Clone => {
                if e.modifiers.alt {
                    self.tools.clone_source = Some(d);
                    self.tools.clone_offset = None;
                    self.set_status("Clone source set. Paint to copy from it.", false, cx);
                    return;
                }
                let Some(src) = self.tools.clone_source else {
                    self.set_status("Alt-click to choose where to clone from.", false, cx);
                    return;
                };
                // Aligned: the offset set by the first stroke stays for later ones.
                let off = *self
                    .tools
                    .clone_offset
                    .get_or_insert((src.0 - d.0, src.1 - d.1));
                // start_stroke converts this document-space offset before its first dab.
                self.start_stroke(
                    d,
                    Ink::Clone {
                        dx: off.0 as f32,
                        dy: off.1 as f32,
                    },
                    false,
                    "Clone",
                    cx,
                );
            }
            Tool::Crop => {
                if !self.tools.crop_options.valid {
                    self.set_status(
                        "Enter valid crop dimensions before drawing a crop.",
                        false,
                        cx,
                    );
                    return;
                }
                let symmetric = e.modifiers.alt || self.tools.crop_centered;
                self.drag = Some(Drag::Tool(ToolDrag::Crop {
                    start: d,
                    end: d,
                    symmetric,
                }))
            }
            Tool::Pen => self.pen_down(d, e, cx),
            Tool::Type => self.type_down(d, window, cx),
            Tool::Eyedropper => {
                if e.modifiers.alt {
                    let fg = self.tools.fg;
                    let hue = self.tools.hue;
                    if self.eyedropper(d, cx) {
                        self.tools.bg = self.tools.fg;
                    }
                    self.tools.fg = fg;
                    self.tools.hue = hue;
                    cx.notify();
                } else {
                    self.eyedropper(d, cx);
                }
            }
            Tool::Zoom => {
                if let Some(b) = self.canvas_bounds() {
                    let anchor = (
                        f32::from(e.position.x) as f64,
                        f32::from(e.position.y) as f64,
                    );
                    if e.click_count >= 2 {
                        self.zoom_100(cx);
                    } else {
                        let f = if zoom_out(e.modifiers) { 0.5 } else { 2.0 };
                        self.view.zoom_at(f, anchor, &b);
                    }
                    cx.notify();
                }
            }
            Tool::Shape => {
                let ellipse = self.tools.shape == ShapeKind::Ellipse;
                self.drag = Some(Drag::Tool(ToolDrag::Shape {
                    start: d,
                    end: d,
                    ellipse,
                }));
            }
            _ => {}
        }
        cx.notify();
    }

    fn start_stroke(
        &mut self,
        d: (f64, f64),
        ink: Ink,
        heal: bool,
        label: &'static str,
        cx: &mut Context<Self>,
    ) {
        if heal && self.editor.in_transaction() {
            self.set_status("Finish the current edit before healing.", false, cx);
            return;
        }
        if heal && self.tools.mask_edit {
            self.set_status(
                "Use Brush, Eraser, Smudge, or Clone to edit the layer mask.",
                false,
                cx,
            );
            return;
        }
        let mask_mode = self.tools.mask_edit;
        let (id, raster, to_doc, ink) = if mask_mode {
            let Some((id, m, to_doc)) = self.mask_target(cx) else {
                return;
            };
            // White reveals, black hides; the eraser hides.
            let ink = match ink {
                Ink::Erase => Ink::Color([0.0, 0.0, 0.0, 1.0]),
                Ink::Color(c) => {
                    let gray = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                    Ink::Color([gray, gray, gray, c[3]])
                }
                other => other,
            };
            (id, Arc::new(mask_to_raster(&m)), to_doc, ink)
        } else {
            let Some(id) = self.paint_target(cx) else {
                return;
            };
            let Some((raster, to_doc)) = self.target_raster(id) else {
                return;
            };
            (id, raster, to_doc, ink)
        };
        let to_local = to_doc.inverse();
        let scale = to_doc.matrix2.determinant().abs().sqrt().max(1e-6);
        let mut brush = self.tools.brush;
        brush.size = (brush.size as f64 / scale) as f32;
        let clip = self
            .editor
            .doc
            .selection
            .clone()
            .map(|m| local_clip(m, to_doc));
        let ink = match ink {
            Ink::Clone { dx, dy } => {
                let offset = to_local.transform_vector2(dvec2(dx as f64, dy as f64));
                Ink::Clone {
                    dx: offset.x as f32,
                    dy: offset.y as f32,
                }
            }
            ink => ink,
        };
        self.remember_active_brush(cx);
        let secondary = self.presets.current_id.as_ref().and_then(|id| {
            self.presets.library.as_ref().and_then(|library| {
                let definition = library.read(cx).catalog.brush(id)?;
                definition
                    .secondary
                    .map(|brush| (brush, definition.combine_mode))
            })
        });
        let needs_backdrop = |brush: &Brush| {
            brush.wetness > 0.0
                || brush.advanced.wet.dilution > 0.0
                || brush.advanced.wet.pull > 0.0
        };
        let wet = needs_backdrop(&brush)
            || secondary.is_some_and(|(brush, _)| needs_backdrop(&brush))
            || matches!(ink, Ink::Smudge);
        let mut stroke = Stroke::new(raster.clone(), brush, ink, clip);
        if let Some((mut secondary, mode)) = secondary {
            secondary.size = (secondary.size as f64 / scale) as f32;
            stroke.set_secondary(secondary, mode);
        }
        stroke.set_alpha_lock(self.tools.alpha_lock && !mask_mode);
        if wet && !mask_mode && self.tools.sample_merged {
            // Wet media mix with what shows under this layer, not only with it.
            let backdrop = PixelSampler::new(self.tree.clone());
            let td = to_doc;
            stroke.set_backdrop(Arc::new(move |x: i32, y: i32| {
                let p = td.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
                let (dx, dy) = (p.x.floor() as i32, p.y.floor() as i32);
                backdrop.get(dx, dy)
            }));
        }
        let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        stroke.set_symmetry_space(to_doc);
        stroke.set_mirror(
            self.tools.mirror_x.then_some(w as f32 / 2.0),
            self.tools.mirror_y.then_some(h as f32 / 2.0),
        );
        if self.tools.symmetry >= 2 {
            stroke.set_radial((w as f32 / 2.0, h as f32 / 2.0), self.tools.symmetry);
        }
        crate::tablet::start();
        self.tools.stroke_started = Some(Instant::now());
        self.tools.stroke_preview_pending = false;
        self.assist_begin(d);
        let p = to_local.transform_point2(dvec2(d.0, d.1));
        let pen = crate::tablet::sample();
        stroke.point_full(
            p.x as f32,
            p.y as f32,
            pen.map(|sample| sample.pressure),
            pen.map(|sample| sample.tilt),
            Some(0.0),
        );
        let label = if mask_mode { "Paint mask" } else { label };
        self.editor.begin(label);
        let (r, dirty) = stroke.render(&raster);
        let mask_raster = mask_mode.then(|| Arc::new(r.clone()));
        self.commit_stroke(id, r, dirty, label, mask_mode, cx);
        self.drag = Some(Drag::Tool(ToolDrag::Stroke {
            id,
            stroke: Box::new(stroke),
            to_local,
            heal,
            label,
            mask: mask_mode,
            mask_raster,
        }));
        if self.tools.quick_shape && !heal {
            self.watch_quick_shape(cx);
        }
    }

    fn start_liquify(&mut self, d: (f64, f64), cx: &mut Context<Self>) {
        if self.tools.mask_edit {
            self.set_status("Use Brush or Smudge to edit the layer mask.", false, cx);
            return;
        }
        let Some(id) = self.paint_target(cx) else {
            return;
        };
        let Some((raster, to_doc)) = self.target_raster(id) else {
            return;
        };
        let to_local = to_doc.inverse();
        let p = to_local.transform_point2(dvec2(d.0, d.1));
        let original = match &self.tools.liquify_session {
            Some((target, original, latest)) if *target == id && Arc::ptr_eq(latest, &raster) => {
                original.clone()
            }
            _ => raster.clone(),
        };
        self.tools.liquify_session = Some((id, original.clone(), raster));
        self.editor.begin("Liquify");
        self.drag = Some(Drag::Tool(ToolDrag::Liquify {
            id,
            original,
            to_local,
            last: (p.x as f32, p.y as f32),
        }));
        // Twirl, pinch, expand and restore act on the press too.
        if self.tools.liquify != emulsion_raster::liquify::Mode::Push {
            self.liquify_to(d, cx);
        }
    }

    /// One Liquify dab at document point `d`.
    fn liquify_to(&mut self, d: (f64, f64), cx: &mut Context<Self>) {
        let Some(Drag::Tool(ToolDrag::Liquify {
            id,
            original,
            to_local,
            last,
        })) = &mut self.drag
        else {
            return;
        };
        let (id, original) = (*id, original.clone());
        let p = to_local.transform_point2(dvec2(d.0, d.1));
        let p = (p.x as f32, p.y as f32);
        let delta = (p.0 - last.0, p.1 - last.1);
        *last = p;
        let scale = to_local.matrix2.determinant().abs().sqrt().max(1e-6);
        let to_doc = to_local.inverse();
        let Some(NodeKind::Raster { raster, .. }) = self.editor.doc.node(id).map(|n| &n.kind)
        else {
            return;
        };
        let radius = self.tools.brush.size * scale as f32 / 2.0;
        let (r, dirty) = emulsion_raster::liquify::dab(
            raster,
            &original,
            self.tools.liquify,
            p,
            radius,
            self.tools.brush.flow,
            delta,
        );
        let r = if let Some(selection) = self.editor.doc.selection.clone() {
            let clip = local_clip(selection, to_doc);
            let prior = raster.read_rect(dirty);
            let pixels: Vec<[u16; 4]> = r
                .read_rect(dirty)
                .into_iter()
                .zip(&prior)
                .enumerate()
                .map(|(i, (painted, base))| {
                    let x = dirty.x + (i % dirty.w as usize) as i32;
                    let y = dirty.y + (i / dirty.w as usize) as i32;
                    let coverage = clip(x, y).clamp(0.0, 1.0);
                    [0, 1, 2, 3].map(|channel| {
                        (base[channel] as f32
                            + (painted[channel] as f32 - base[channel] as f32) * coverage)
                            .round() as u16
                    })
                })
                .collect();
            if pixels == prior {
                return;
            }
            raster.write_rect(dirty, &pixels)
        } else {
            r
        };
        self.commit_stroke(id, r, dirty, "Liquify", false, cx);
        if let Some(NodeKind::Raster { raster, .. }) =
            self.editor.doc.node(id).map(|node| &node.kind)
        {
            self.tools.liquify_session = Some((id, original, raster.clone()));
        }
    }

    /// Poll the live stroke for a rest at its end; snap it when found.
    /// Stops when the stroke ends or has been snapped.
    fn watch_quick_shape(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
                let go_on = this.update(cx, |this, cx| this.quick_shape_tick(cx));
                if !matches!(go_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One QuickShape poll. Returns whether to keep watching.
    fn quick_shape_tick(&mut self, cx: &mut Context<Self>) -> bool {
        const HOLD_MS: f64 = 450.0;
        let now_ms = self
            .tools
            .stroke_started
            .map(|s| s.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        // Rest radius: a few screen pixels, in layer pixels.
        let radius = (3.0 / self.view.zoom.max(0.05)) as f32;
        let Some(Drag::Tool(ToolDrag::Stroke {
            id,
            stroke,
            heal: false,
            label,
            mask,
            mask_raster,
            ..
        })) = &mut self.drag
        else {
            return false;
        };
        let mask_current = mask_raster.clone();
        if stroke.is_finished() {
            return false;
        }
        let raw = stroke.raw_points();
        if raw.len() < 8 || stroke.held_ms(now_ms, radius) < HOLD_MS {
            return true;
        }
        let pts: Vec<(f32, f32)> = raw.iter().map(|r| (r.0, r.1)).collect();
        let Some(shape) = emulsion_raster::quickshape::fit(&pts) else {
            // Not a shape; leave the hand's line alone and stop asking.
            return false;
        };
        let pressure = raw.iter().map(|r| r.3).sum::<f32>() / raw.len() as f32;
        let step = (stroke.brush.size * stroke.brush.spacing * 0.5).max(1.0);
        let path = shape.outline(step, pts[0]);
        stroke.replay(&path, pressure);
        let (id, label, mask) = (*id, *label, *mask);
        let current = if mask {
            match mask_current {
                Some(m) => m,
                None => return false,
            }
        } else {
            match self.editor.doc.node(id).map(|n| &n.kind) {
                Some(NodeKind::Raster { raster, .. }) => raster.clone(),
                _ => return false,
            }
        };
        let Some(Drag::Tool(ToolDrag::Stroke { stroke, .. })) = &mut self.drag else {
            return false;
        };
        let (r, dirty) = stroke.render(&current);
        self.commit_stroke(id, r, dirty, label, mask, cx);
        let what = match shape {
            emulsion_raster::quickshape::Shape::Line(..) => "line",
            emulsion_raster::quickshape::Shape::Polyline(_) => "polyline",
            emulsion_raster::quickshape::Shape::Polygon(ref v) => match v.len() {
                3 => "triangle",
                4 => "quadrilateral",
                _ => "polygon",
            },
            emulsion_raster::quickshape::Shape::Circle { .. } => "circle",
            emulsion_raster::quickshape::Shape::Ellipse { .. } => "ellipse",
        };
        self.set_status(format!("QuickShape: {what}"), false, cx);
        cx.notify();
        false
    }

    /// Put a rendered stroke into the document: pixels, or the mask it
    /// stands for.
    pub(crate) fn commit_stroke(
        &mut self,
        id: NodeId,
        r: Raster,
        dirty: IRect,
        label: &str,
        mask: bool,
        cx: &mut Context<Self>,
    ) {
        if dirty.is_empty() {
            return;
        }
        if mask {
            let Some(old) = self.editor.doc.node(id).and_then(|n| n.mask.clone()) else {
                return;
            };
            let px: Vec<u8> = r
                .read_rect(dirty)
                .into_iter()
                .map(|p| (color::linear_to_srgb(color::u16_to_f(p[0])) * 255.0).round() as u8)
                .collect();
            let m = old.write_rect(dirty, &px);
            self.execute(
                Command::SetMask {
                    id,
                    mask: Some(Arc::new(m)),
                },
                cx,
            );
        } else {
            self.note_painted_color();
            self.execute(
                Command::ReplacePixels {
                    id,
                    raster: Arc::new(r),
                    dirty,
                    label: label.into(),
                },
                cx,
            );
        }
    }

    /// The selected node's mask and the mask-space → document transform.
    /// A node without a mask gets a fully revealing one first.
    fn mask_target(&mut self, cx: &mut Context<Self>) -> Option<(NodeId, Arc<Mask>, DAffine2)> {
        let id = self.selected?;
        let n = self.editor.doc.node(id)?;
        if self.editor.doc.locked_ancestor(id).is_some() {
            self.set_status("That layer is locked.", true, cx);
            return None;
        }
        let (w, h, to_doc) = match &n.kind {
            NodeKind::Raster { raster, placement }
            | NodeKind::Smart {
                source: raster,
                placement,
                ..
            } => (
                raster.width(),
                raster.height(),
                placement.to_doc(raster.width(), raster.height()),
            ),
            _ => (
                self.editor.doc.width,
                self.editor.doc.height,
                DAffine2::IDENTITY,
            ),
        };
        let to_doc = if n.mask.is_some() {
            to_doc * DAffine2::from_cols_array(&n.mask_transform)
        } else {
            to_doc
        };
        let mask = match &n.mask {
            Some(m) => m.clone(),
            None => {
                let m = Arc::new(Mask::white(w, h));
                self.execute(
                    Command::SetMask {
                        id,
                        mask: Some(m.clone()),
                    },
                    cx,
                );
                m
            }
        };
        Some((id, mask, to_doc))
    }

    // ── Mask operations ─────────────────────────────────────────────────

    /// Add a mask from the selection (or one that reveals everything).
    pub fn add_mask(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let (w, h, to_doc) = match &n.kind {
            NodeKind::Raster { raster, placement }
            | NodeKind::Smart {
                source: raster,
                placement,
                ..
            } => (
                raster.width(),
                raster.height(),
                Some(placement.to_doc(raster.width(), raster.height())),
            ),
            _ => (self.editor.doc.width, self.editor.doc.height, None),
        };
        let m = match self.editor.doc.selection.clone() {
            Some(sel) => match to_doc {
                Some(td)
                    if td == glam::DAffine2::IDENTITY
                        && (w, h) == (sel.width(), sel.height())
                        && sel.fill() == 0 =>
                {
                    sel
                }
                // Raster masks live in the node's pixel space.
                Some(td) => Arc::new(Mask::from_fn(w, h, 0, |x, y| {
                    let p = td.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
                    if p.x < 0.0
                        || p.y < 0.0
                        || p.x >= sel.width() as f64
                        || p.y >= sel.height() as f64
                    {
                        0
                    } else {
                        sel.get(p.x as u32, p.y as u32)
                    }
                })),
                None => sel,
            },
            None => Arc::new(Mask::white(w, h)),
        };
        let commands = vec![
            Command::SetMask { id, mask: None },
            Command::SetMask { id, mask: Some(m) },
        ];
        if self
            .execute_layer_commands("Mask from selection", commands, cx)
            .is_none()
        {
            return;
        }
        self.set_status(
            "Mask added. Turn on \"edit mask\" to paint it: white reveals, black hides.",
            false,
            cx,
        );
    }

    pub fn remove_mask(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.execute(Command::SetMask { id, mask: None }, cx);
            self.tools.mask_edit = false;
        }
    }

    pub fn invert_mask(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected
            && let Some(m) = self.editor.doc.node(id).and_then(|n| n.mask.clone())
        {
            self.execute(
                Command::SetMask {
                    id,
                    mask: Some(Arc::new(select::invert(&m))),
                },
                cx,
            );
        }
    }

    pub fn feather_mask(&mut self, radius: f32, cx: &mut Context<Self>) {
        if let Some(id) = self.selected
            && let Some(m) = self.editor.doc.node(id).and_then(|n| n.mask.clone())
        {
            self.execute(
                Command::SetMask {
                    id,
                    mask: Some(Arc::new(select::feather(&m, radius))),
                },
                cx,
            );
        }
    }

    /// Load the mask as the selection.
    pub fn mask_to_selection(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.selected.and_then(|id| self.editor.doc.node(id)) else {
            return;
        };
        let Some(mask) = node.mask.clone() else {
            return;
        };
        let inverse = emulsion_core::transform::mask_to_document(node).inverse();
        let selection = Mask::from_fn(self.editor.doc.width, self.editor.doc.height, 0, |x, y| {
            let point = inverse.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
            if point.x < 0.0
                || point.y < 0.0
                || point.x >= mask.width() as f64
                || point.y >= mask.height() as f64
            {
                0
            } else {
                emulsion_core::transform::sample_mask(&mask, point)
            }
        });
        self.apply_selection(selection, self.tools.combine, cx);
    }

    /// Publish accumulated brush samples at the frame boundary.
    pub(crate) fn flush_live_stroke(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.tools.stroke_preview_pending) {
            return;
        }
        let Some(Drag::Tool(ToolDrag::Stroke {
            id,
            stroke,
            label,
            mask,
            mask_raster,
            ..
        })) = &mut self.drag
        else {
            return;
        };
        let current = if *mask {
            mask_raster.clone()
        } else {
            match self.editor.doc.node(*id).map(|n| &n.kind) {
                Some(NodeKind::Raster { raster, .. }) => Some(raster.clone()),
                _ => None,
            }
        };
        let Some(current) = current else { return };
        let started = Instant::now();
        let (r, dirty) = stroke.render(&current);
        if *mask {
            *mask_raster = Some(Arc::new(r.clone()));
        }
        let (id, label, mask) = (*id, *label, *mask);
        self.commit_stroke(id, r, dirty, label, mask, cx);
        tracing::debug!(target: "emulsion_ui::paint_timing", elapsed_us = started.elapsed().as_micros() as u64, "brush preview published");
    }

    pub(crate) fn tool_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(d) = self.doc_point(pos) else { return };
        let d = if matches!(self.drag, Some(Drag::Tool(ToolDrag::Stroke { .. }))) {
            self.assist_point(d)
        } else {
            d
        };
        let Some(Drag::Tool(t)) = &mut self.drag else {
            return;
        };
        match t {
            ToolDrag::Stroke {
                stroke, to_local, ..
            } => {
                let p = to_local.transform_point2(dvec2(d.0, d.1));
                let t = self
                    .tools
                    .stroke_started
                    .map(|s| s.elapsed().as_secs_f64() * 1000.0);
                // Keep every input sample, but compose tiles only once per displayed frame.
                let started = Instant::now();
                let pen = crate::tablet::sample();
                stroke.point_full(
                    p.x as f32,
                    p.y as f32,
                    pen.map(|sample| sample.pressure),
                    pen.map(|sample| sample.tilt),
                    t,
                );
                tracing::debug!(target: "emulsion_ui::paint_timing", elapsed_us = started.elapsed().as_micros() as u64, size = stroke.brush.size, wetness = stroke.brush.wetness, "brush input processed");
                self.tools.stroke_preview_pending = true;
                cx.notify();
            }
            ToolDrag::Liquify { .. } => self.liquify_to(d, cx),
            ToolDrag::Marquee { end, .. }
            | ToolDrag::Gradient { end, .. }
            | ToolDrag::Crop { end, .. }
            | ToolDrag::Shape { end, .. } => {
                *end = d;
                cx.notify();
            }
            ToolDrag::Lasso { pts, .. } => {
                let last = *pts.last().unwrap_or(&d);
                if ((last.0 - d.0) * self.view.zoom).hypot((last.1 - d.1) * self.view.zoom) >= 2.0 {
                    pts.push(d);
                    cx.notify();
                }
            }
            ToolDrag::PickSv { track } => {
                let track = track.clone();
                self.pick_sv(&track, pos, cx);
            }
            ToolDrag::MoveSelection { start, orig } => {
                let (dx, dy) = ((d.0 - start.0).round(), (d.1 - start.1).round());
                let moved = select::transform(orig, DAffine2::from_translation(dvec2(dx, dy)));
                let selection = (!select::bounds(&moved).is_empty()).then(|| Arc::new(moved));
                self.execute(Command::SetSelection { selection }, cx);
            }
            ToolDrag::Pen(pd) => {
                let pd = pd.clone();
                self.pen_move(d, pd, cx);
            }
            ToolDrag::Quick { pts, .. } => {
                let last = *pts.last().unwrap_or(&d);
                if ((last.0 - d.0) * self.view.zoom).hypot((last.1 - d.1) * self.view.zoom) >= 2.0 {
                    pts.push(d);
                    cx.notify();
                }
            }
            ToolDrag::PickHue { track } => {
                let track = track.clone();
                self.pick_hue(&track, pos, cx);
            }
        }
    }

    pub(crate) fn tool_up(&mut self, t: ToolDrag, cx: &mut Context<Self>) {
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        match t {
            ToolDrag::Stroke {
                id,
                mut stroke,
                heal,
                label,
                mask,
                mask_raster,
                ..
            } => {
                // Catch the stabilizer up and taper the end.
                let finished = stroke.finish();
                let pending = std::mem::take(&mut self.tools.stroke_preview_pending);
                if finished || pending {
                    let current = if mask {
                        mask_raster
                    } else {
                        match self.editor.doc.node(id).map(|n| &n.kind) {
                            Some(NodeKind::Raster { raster, .. }) => Some(raster.clone()),
                            _ => None,
                        }
                    };
                    if let Some(current) = current {
                        let (r, dirty) = stroke.render(&current);
                        self.commit_stroke(id, r, dirty, label, mask, cx);
                    }
                }
                self.tools.stroke_started = None;
                self.tools.stroke_preview_pending = false;
                self.assist_end();
                if heal {
                    self.finish_heal(id, *stroke, cx);
                } else if self.editor.in_transaction() {
                    self.editor.end();
                }
            }
            ToolDrag::Liquify { .. } => {
                if self.editor.in_transaction() {
                    self.editor.end();
                }
            }
            ToolDrag::Marquee {
                start,
                end,
                combine,
                ellipse,
            } => {
                let (x, y, rw, rh) = norm(start, end);
                if rw * self.view.zoom < 2.0 && rh * self.view.zoom < 2.0 {
                    // A click without a drag: deselect (Photoshop).
                    if combine == Combine::Replace && self.editor.doc.selection.is_some() {
                        self.deselect(cx);
                    }
                    return;
                }
                let m = if ellipse {
                    select::ellipse(w, h, x as f32, y as f32, rw as f32, rh as f32)
                } else {
                    select::rect(w, h, x as f32, y as f32, rw as f32, rh as f32)
                };
                self.apply_selection(m, combine, cx);
            }
            ToolDrag::Lasso { pts, combine } => {
                if pts.len() >= 3 {
                    let p: Vec<(f32, f32)> =
                        pts.iter().map(|(x, y)| (*x as f32, *y as f32)).collect();
                    self.apply_selection(select::polygon(w, h, &p), combine, cx);
                }
            }
            ToolDrag::Gradient { start, end } => self.make_gradient(start, end, cx),
            ToolDrag::Crop {
                start,
                end,
                symmetric,
            } => {
                let (x, y, rw, rh) = crop_rect(
                    start,
                    end,
                    symmetric,
                    self.crop_constraint(),
                    self.drag_shift,
                );
                self.tools.crop = (rw >= 1.0 && rh >= 1.0).then_some((x, y, rw, rh));
                cx.notify();
            }
            ToolDrag::Shape {
                start,
                end,
                ellipse,
            } => {
                let bounds = self.shape_drag_rect(start, end, self.drag_shift);
                self.finish_shape(bounds, ellipse, cx);
            }
            ToolDrag::PickSv { .. } | ToolDrag::PickHue { .. } => {
                if self.editor.in_transaction() {
                    self.editor.end();
                }
            }
            ToolDrag::MoveSelection { .. } => {
                if self.editor.in_transaction() {
                    self.editor.end();
                }
            }
            ToolDrag::Quick { pts, combine } => {
                if self.ai.ai_select && self.sam_available() {
                    self.sam_select(pts, combine, cx)
                } else {
                    self.quick_select(pts, combine, cx)
                }
            }
            ToolDrag::Pen(pd) => self.pen_up(pd, cx),
        }
        cx.notify();
    }

    pub fn commit_polygon(&mut self, cx: &mut Context<Self>) {
        if self.tools.polygon.len() + self.tools.magnetic_live.len() < 3 {
            return;
        }
        let mut pts = std::mem::take(&mut self.tools.polygon);
        pts.append(&mut self.tools.magnetic_live);
        if pts.len() >= 3 {
            let (w, h) = (self.editor.doc.width, self.editor.doc.height);
            let p: Vec<(f32, f32)> = pts.iter().map(|(x, y)| (*x as f32, *y as f32)).collect();
            let combine = self.tools.polygon_combine;
            self.apply_selection(select::polygon(w, h, &p), combine, cx);
        }
        cx.notify();
    }

    /// Enter: commit whatever the tool has pending.
    pub fn tool_commit(&mut self, cx: &mut Context<Self>) {
        if self.warp.is_none() {
            self.tools.transform_lift = None;
        }
        match self.tool {
            Tool::Move if self.warp.is_some() => self.finish_warp(cx),
            Tool::Pen => self.pen_finish(cx),
            Tool::Select if !self.tools.polygon.is_empty() => self.commit_polygon(cx),
            Tool::Crop => {
                if !self.tools.crop_options.valid {
                    self.set_status(
                        "Enter valid crop dimensions before applying the crop.",
                        false,
                        cx,
                    );
                    return;
                }
                if let Some((x, y, w, h)) = self.tools.crop {
                    if ![x, y, w, h].iter().all(|v| v.is_finite())
                        || x < i32::MIN as f64
                        || y < i32::MIN as f64
                        || x + w > i32::MAX as f64
                        || y + h > i32::MAX as f64
                        || emulsion_io::import::check_size(w.round() as u32, h.round() as u32)
                            .is_err()
                    {
                        self.set_status(
                            "Crop dimensions exceed the supported canvas size.",
                            true,
                            cx,
                        );
                        return;
                    }
                    self.tools.crop = None;
                    let rect = IRect::new(
                        x.round() as i32,
                        y.round() as i32,
                        (w.round() as i32).max(1),
                        (h.round() as i32).max(1),
                    );
                    let rotation = self.tools.straighten as f64;
                    self.tools.straighten = 0.0;
                    let delete = self.tools.crop_delete;
                    self.crop_canvas(rect, rotation, self.tools.fill_edges, delete, cx);
                }
            }
            _ => {}
        }
    }

    /// Escape: cancel the active gesture and all pending tool previews.
    pub fn tool_cancel(&mut self, cx: &mut Context<Self>) -> bool {
        if self.raw_cancel_interaction(cx) {
            return true;
        }
        if self.cancel_text_pointer(cx) {
            return true;
        }
        if let Some(Drag::RotateView { rotation, .. }) = self.drag.as_ref() {
            self.view.rotation = *rotation;
            self.drag = None;
            cx.notify();
            return true;
        }
        if self.rail.flyout.take().is_some() {
            cx.notify();
            return true;
        }
        let mut had = self.cancel_move(cx);
        if matches!(self.drag, Some(Drag::Transform(_))) {
            self.drag = None;
            self.snap_lines.clear();
            self.invalidate_pending_edits();
            self.editor.cancel();
            self.after_change(cx);
            had = true;
        }
        if matches!(self.drag, Some(Drag::Warp(_) | Drag::Distort { .. })) {
            // These drags only update preview geometry. Removing the drag also
            // prevents mouse-up from launching a resampling job.
            self.drag = None;
            had = true;
        }
        if self.warp.is_some() {
            self.cancel_warp(cx);
            had = true;
        }
        if matches!(self.drag, Some(Drag::Tool(_))) {
            let Some(Drag::Tool(drag)) = self.drag.take() else {
                unreachable!()
            };
            had = true;
            if matches!(
                drag,
                ToolDrag::Stroke { .. }
                    | ToolDrag::Liquify { .. }
                    | ToolDrag::MoveSelection { .. }
                    | ToolDrag::PickSv { .. }
                    | ToolDrag::PickHue { .. }
                    | ToolDrag::Pen(
                        super::pen::PenDrag::Anchor { .. } | super::pen::PenDrag::Handle { .. }
                    )
            ) {
                self.invalidate_pending_edits();
                self.editor.cancel();
                self.after_change(cx);
            }
            self.tools.stroke_started = None;
            self.tools.stroke_preview_pending = false;
            self.assist_end();
        }
        let pen_pending = self.pen_cancel();
        had |= self.cancel_transform_lift(cx);
        had |= !self.tools.polygon.is_empty()
            || self.tools.crop.is_some()
            || self.tools.straighten != 0.0
            || self.tools.picker
            || pen_pending;
        self.tools.polygon.clear();
        self.tools.magnetic_live.clear();
        self.tools.crop = None;
        self.tools.straighten = 0.0;
        self.tools.picker = false;
        cx.notify();
        had
    }

    // ── One-shot operations ─────────────────────────────────────────────

    fn eyedropper(&mut self, d: (f64, f64), cx: &mut Context<Self>) -> bool {
        let px = region(
            &self.tree,
            IRect::new(d.0.floor() as i32, d.1.floor() as i32, 1, 1),
        );
        let c = color::premul_to_srgba8(px[0]);
        if c[3] > 0 {
            self.tools.fg = [c[0], c[1], c[2], 255];
            self.tools.hue = rgb_to_hsv(self.tools.fg).0;
            cx.notify();
            true
        } else {
            false
        }
    }

    /// The composite as straight sRGBA8, off the main thread.
    fn composite_srgb8(&self) -> impl std::future::Future<Output = Vec<u8>> + use<> {
        let doc = self.editor.doc.clone();
        async move {
            let tree = doc.composite_tree();
            let full = region(
                &tree,
                IRect::new(0, 0, tree.width as i32, tree.height as i32),
            );
            full.into_iter().flat_map(color::premul_to_srgba8).collect()
        }
    }

    /// Load the selected node's pixels (or its mask) as the selection.
    pub fn select_from_node(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            self.set_status("Select a layer first.", false, cx);
            return;
        };
        match self.editor.doc.node_coverage(id) {
            Some(m) => {
                let combine = self.tools.combine;
                self.apply_selection(m, combine, cx);
            }
            None => self.set_status("That layer covers nothing to select.", false, cx),
        }
    }

    /// Move, scale (about the centre) and rotate the selection.
    pub fn transform_selection(
        &mut self,
        dx: f64,
        dy: f64,
        scale: f64,
        degrees: f64,
        cx: &mut Context<Self>,
    ) {
        let Some(sel) = self.editor.doc.selection.clone() else {
            self.set_status("Nothing is selected.", false, cx);
            return;
        };
        let b = select::bounds(&sel);
        let c = dvec2(b.x as f64 + b.w as f64 / 2.0, b.y as f64 + b.h as f64 / 2.0);
        let a = DAffine2::from_translation(c + dvec2(dx, dy))
            * DAffine2::from_angle(degrees.to_radians())
            * DAffine2::from_scale(dvec2(scale, scale))
            * DAffine2::from_translation(-c);
        let m = select::transform(&sel, a);
        let selection = (!select::bounds(&m).is_empty()).then(|| Arc::new(m));
        self.execute(Command::SetSelection { selection }, cx);
    }

    fn quick_select(&mut self, pts: Vec<(f64, f64)>, combine: Combine, cx: &mut Context<Self>) {
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let r = (self.tools.brush.size / 2.0).max(2.0) as f64;
        let mut seeds = Vec::new();
        for (x, y) in &pts {
            let step = (r / 3.0).max(1.0);
            let mut oy = -r;
            while oy <= r {
                let mut ox = -r;
                while ox <= r {
                    let (sx, sy) = (x + ox, y + oy);
                    if ox * ox + oy * oy <= r * r
                        && sx >= 0.0
                        && sy >= 0.0
                        && sx < w as f64
                        && sy < h as f64
                    {
                        seeds.push((sx as u32, sy as u32));
                    }
                    ox += step;
                }
                oy += step;
            }
        }
        if seeds.is_empty() {
            return;
        }
        let strength = (self.tools.tolerance as f32 / 255.0 * 100.0).max(1.0);
        let ticket = self.selection_ticket();
        let img = self.composite_srgb8();
        self.set_status("Selecting…", false, cx);
        cx.spawn(async move |this, cx| {
            let m = cx
                .background_spawn(async move {
                    let img = img.await;
                    select::quick_select(&img, w, h, &seeds, strength)
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.selection_is_current(ticket) {
                    return;
                }
                this.status = None;
                this.apply_selection(m, combine, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Compute the edge map the magnetic lasso follows, once per revision.
    fn ensure_edges(&mut self, cx: &mut Context<Self>) {
        let rev = self.operation_epoch;
        if self.tools.edges.as_ref().is_some_and(|(r, _)| *r == rev)
            || self.tools.edges_loading == Some(rev)
        {
            return;
        }
        self.tools.edges_loading = Some(rev);
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let img = self.composite_srgb8();
        cx.spawn(async move |this, cx| {
            let e = cx
                .background_spawn(async move { Arc::new(select::edges(&img.await, w, h)) })
                .await;
            this.update(cx, |this, _| {
                if this.tools.edges_loading == Some(rev) {
                    this.tools.edges_loading = None;
                }
                if this.operation_epoch == rev
                    && (this.editor.doc.width, this.editor.doc.height) == (w, h)
                {
                    this.tools.edges = Some((rev, e));
                }
            })
            .ok();
        })
        .detach();
    }

    fn magnetic_click(
        &mut self,
        d: (f64, f64),
        combine: Combine,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        self.ensure_edges(cx);
        let pts = &self.tools.polygon;
        let near_start = pts.first().is_some_and(|p0| {
            let z = self.view.zoom;
            ((p0.0 - d.0) * z).hypot((p0.1 - d.1) * z) < 8.0
        });
        if pts.len() >= 3 && (near_start || clicks >= 2) {
            self.commit_polygon(cx);
            return;
        }
        if pts.is_empty() {
            self.tools.polygon_combine = combine;
        }
        let live = std::mem::take(&mut self.tools.magnetic_live);
        self.tools.polygon.extend(live);
        self.tools.polygon.push(d);
        cx.notify();
    }

    /// Pointer moved with the magnetic lasso open: snap the live segment to
    /// edges, and drop an anchor when it grows long.
    pub(crate) fn magnetic_track(&mut self, d: (f64, f64), cx: &mut Context<Self>) {
        if self.tool != Tool::Select || self.tools.select != SelectShape::Magnetic {
            return;
        }
        let Some(&last) = self.tools.polygon.last() else {
            return;
        };
        let Some((epoch, edges)) = self.tools.edges.clone() else {
            self.ensure_edges(cx);
            return;
        };
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        if epoch != self.operation_epoch || edges.len() != w as usize * h as usize {
            self.tools.magnetic_live.clear();
            self.ensure_edges(cx);
            return;
        }
        let clamp = |p: (f64, f64)| {
            (
                p.0.clamp(0.0, w as f64 - 1.0) as u32,
                p.1.clamp(0.0, h as f64 - 1.0) as u32,
            )
        };
        let margin = (12.0 / self.view.zoom).clamp(4.0, 40.0) as u32;
        let path = select::live_wire(&edges, w, h, clamp(last), clamp(d), margin);
        let mut live: Vec<(f64, f64)> = path
            .iter()
            .skip(1)
            .map(|(x, y)| (*x as f64 + 0.5, *y as f64 + 0.5))
            .collect();
        // Fix the older half as the segment grows, so it stays stable.
        let anchor_every = (60.0 / self.view.zoom).max(8.0) as usize;
        if live.len() > anchor_every * 2 {
            let fixed: Vec<_> = live.drain(..anchor_every).collect();
            self.tools.polygon.extend(fixed);
        }
        self.tools.magnetic_live = live;
        cx.notify();
    }

    fn wand(&mut self, d: (f64, f64), combine: Combine, cx: &mut Context<Self>) {
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        if d.0 < 0.0 || d.1 < 0.0 || d.0 >= w as f64 || d.1 >= h as f64 {
            return;
        }
        let (tol, contiguous) = (self.tools.tolerance, self.tools.contiguous);
        let img = self.composite_srgb8();
        let ticket = self.selection_ticket();
        self.set_status("Selecting…", false, cx);
        cx.spawn(async move |this, cx| {
            let m = cx
                .background_spawn(async move {
                    let img = img.await;
                    select::by_color(&img, w, h, d.0 as u32, d.1 as u32, tol, contiguous)
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.selection_is_current(ticket) {
                    return;
                }
                this.status = None;
                this.apply_selection(m, combine, cx);
            })
            .ok();
        })
        .detach();
    }

    fn bucket(&mut self, d: (f64, f64), cx: &mut Context<Self>) {
        self.fill_at(d, self.tools.fg, "Fill", cx);
    }

    /// ColorDrop: the swatch was dropped on the canvas at window `pos`;
    /// fill the similar-coloured area there with that colour.
    pub(crate) fn color_drop(
        &mut self,
        color: [u8; 4],
        pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.doc_point(pos) else {
            return;
        };
        self.fill_at(d, color, "ColorDrop", cx);
    }

    /// Flood-fill the area of similar colour at document point `d` on the
    /// paint target, within the selection.
    fn fill_at(
        &mut self,
        d: (f64, f64),
        color: [u8; 4],
        label: &'static str,
        cx: &mut Context<Self>,
    ) {
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        if d.0 < 0.0 || d.1 < 0.0 || d.0 >= w as f64 || d.1 >= h as f64 {
            return;
        }
        if self.fill_object_or_mask(Some(d), color, cx) {
            return;
        }
        let color = premul(color);
        let Some(id) = self.paint_target(cx) else {
            return;
        };
        let Some((raster, to_doc)) = self.target_raster(id) else {
            return;
        };
        let (tol, contiguous) = (self.tools.tolerance, self.tools.contiguous);
        let sel = self.editor.doc.selection.clone();
        let img = self.composite_srgb8();
        self.set_status("Filling…", false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let img = img.await;
                    let mut m =
                        select::by_color(&img, w, h, d.0 as u32, d.1 as u32, tol, contiguous);
                    if let Some(s) = &sel {
                        m = select::combine(Some(&m), s, Combine::Intersect);
                    }
                    let clip = local_clip(Arc::new(m), to_doc);
                    fill_color(&raster, raster.bounds(), &|x, y| clip(x, y), color)
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Fill", cx) {
                    return;
                }
                this.status = None;
                let (r, dirty) = result;
                this.execute(
                    Command::ReplacePixels {
                        id,
                        raster: Arc::new(r),
                        dirty,
                        label: label.into(),
                    },
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// Fill the selection (or everything) on the target layer with the
    /// foreground colour.
    pub fn fill_selection(&mut self, cx: &mut Context<Self>) {
        if self.fill_object_or_mask(None, self.tools.fg, cx) {
            return;
        }
        let Some(id) = self.paint_target(cx) else {
            return;
        };
        let Some((raster, to_doc)) = self.target_raster(id) else {
            return;
        };
        let color = premul(self.tools.fg);
        let sel = self.editor.doc.selection.clone();
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let (r, dirty) = cx
                .background_spawn(async move {
                    match sel {
                        Some(s) => {
                            let clip = local_clip(s, to_doc);
                            fill_color(&raster, raster.bounds(), &|x, y| clip(x, y), color)
                        }
                        None => fill_color(&raster, raster.bounds(), &|_, _| 1.0, color),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Fill", cx) {
                    return;
                }
                this.execute(
                    Command::ReplacePixels {
                        id,
                        raster: Arc::new(r),
                        dirty,
                        label: "Fill".into(),
                    },
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// Fill the selection from its surroundings into a new node.
    pub fn content_aware_fill(&mut self, cx: &mut Context<Self>) {
        let Some(sel) = self.editor.doc.selection.clone() else {
            self.set_status("Select the area to fill first.", false, cx);
            return;
        };
        let doc = self.editor.doc.clone();
        self.set_status("Filling from the surroundings…", false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let Some((layer, reg)) = cx
                .background_spawn(
                    async move { fill::content_aware_layer(&doc.composite_tree(), &sel) },
                )
                .await
            else {
                return;
            };
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Fill", cx) {
                    return;
                }
                this.status = None;
                let node = Node::raster(
                    0,
                    "Content-aware fill",
                    Arc::new(layer),
                    Placement::at(reg.x as f64, reg.y as f64),
                );
                let slot = this.insertion_slot();
                if let Some(id) = this.execute(
                    Command::AddNode {
                        node: Box::new(node),
                        slot,
                    },
                    cx,
                ) {
                    this.set_layer_selection(vec![id], Some(id));
                    this.set_status("Filled into a new layer. Hide it to compare.", false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_heal(&mut self, id: NodeId, stroke: Stroke, cx: &mut Context<Self>) {
        let hole = stroke.coverage();
        let base = stroke.base().clone();
        let alpha_lock = self.tools.alpha_lock;
        let blend = stroke.brush.blend;
        // The live colored overlay belongs only to this stroke. Restore it
        // before dispatch so the worker never owns an open UI transaction.
        self.editor.cancel();
        self.after_change(cx);
        let epoch = self.operation_epoch;
        let b = select::bounds(&hole);
        if b.is_empty() {
            return;
        }
        let margin = (stroke.brush.size as i32 * 2).max(32);
        let reg = IRect::new(
            b.x - margin,
            b.y - margin,
            b.w + 2 * margin,
            b.h + 2 * margin,
        )
        .intersect(&base.bounds());
        let original = base.clone();
        cx.spawn(async move |this, cx| {
            let (raster, dirty) = cx
                .background_spawn(async move {
                    let img: Vec<[f32; 4]> = base
                        .read_rect(reg)
                        .into_iter()
                        .map(color::px_to_f)
                        .collect();
                    let h: Vec<f32> = hole
                        .read_rect(reg)
                        .into_iter()
                        .map(|v| v as f32 / 255.0)
                        .collect();
                    let out = fill::content_aware(&img, &h, reg.w as usize, reg.h as usize, 0x4EA1);
                    let px: Vec<[u16; 4]> = out.into_iter().zip(&img).zip(&h).enumerate().map(|(index, ((mut pixel, prior), coverage))| {
                        let coverage = coverage.clamp(0.0, 1.0);
                        pixel = match blend {
                            BrushBlend::Clear => prior.map(|value| value * (1.0 - coverage)),
                            BrushBlend::Behind => {
                                let amount = coverage * (1.0 - prior[3]);
                                emulsion_raster::blend::blend_px(
                                    emulsion_raster::BlendMode::Normal,
                                    emulsion_raster::blend::BlendSpace::Linear,
                                    *prior,
                                    pixel.map(|value| value * amount),
                                    index as f32,
                                )
                            }
                            _ => emulsion_raster::blend::blend_px(
                                blend.blend_mode(),
                                emulsion_raster::blend::BlendSpace::Linear,
                                *prior,
                                pixel.map(|value| value * coverage),
                                index as f32,
                            ),
                        };
                        if alpha_lock {
                            if pixel[3] > 0.0 {
                                let scale = prior[3] / pixel[3];
                                for channel in pixel.iter_mut().take(3) { *channel *= scale; }
                            } else { pixel = *prior; }
                            pixel[3] = prior[3];
                        }
                        color::f_to_px(pixel)
                    }).collect();
                    (base.write_rect(reg, &px), reg)
                })
                .await;
            this.update(cx, |this, cx| {
                let unchanged = this.editor.doc.node(id).is_some_and(|node| {
                    matches!(&node.kind, NodeKind::Raster { raster, .. } if Arc::ptr_eq(raster, &original))
                });
                if this.operation_epoch != epoch || this.editor.in_transaction() || !unchanged {
                    this.set_status("Heal cancelled because the document changed. Paint the area again to retry.", false, cx);
                    return;
                }
                this.execute(
                    Command::ReplacePixels {
                        id,
                        raster: Arc::new(raster),
                        dirty,
                        label: "Spot heal".into(),
                    },
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    fn make_gradient(&mut self, a: (f64, f64), b: (f64, f64), cx: &mut Context<Self>) {
        if ((a.0 - b.0) * self.view.zoom).hypot((a.1 - b.1) * self.view.zoom) < 3.0 {
            return;
        }
        if self.tools.mask_edit {
            let Some((id, mask, to_doc)) = self.mask_target(cx) else {
                return;
            };
            let gray =
                |c: [u8; 4]| 0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64;
            let (from, to) = (gray(self.tools.fg), gray(self.tools.bg));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = (dx * dx + dy * dy).max(1e-6);
            let radial = self.tools.radial;
            let selection = self.editor.doc.selection.clone();
            let result = Mask::from_fn(mask.width(), mask.height(), mask.fill(), |x, y| {
                let point = to_doc.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
                let (px, py) = (point.x - a.0, point.y - a.1);
                let t = if radial {
                    (px * px + py * py).sqrt() / len2.sqrt()
                } else {
                    (px * dx + py * dy) / len2
                }
                .clamp(0.0, 1.0);
                let coverage = selection.as_ref().map_or(1.0, |selection| {
                    if point.x < 0.0
                        || point.y < 0.0
                        || point.x >= selection.width() as f64
                        || point.y >= selection.height() as f64
                    {
                        0.0
                    } else {
                        selection.get(point.x as u32, point.y as u32) as f64 / 255.0
                    }
                });
                let old = mask.get(x, y) as f64;
                (old + ((from + (to - from) * t) - old) * coverage)
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
            self.execute(
                Command::SetMask {
                    id,
                    mask: Some(Arc::new(result)),
                },
                cx,
            );
            return;
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let (c0, c1) = (premul(self.tools.fg), premul(self.tools.bg));
        let radial = self.tools.radial;
        let sel = self.editor.doc.selection.clone();
        let (dx, dy) = ((b.0 - a.0) as f32, (b.1 - a.1) as f32);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let r = Raster::from_fn(w, h, [0; 4], |x, y| {
            let (px, py) = (x as f32 + 0.5 - a.0 as f32, y as f32 + 0.5 - a.1 as f32);
            let t = if radial {
                (px * px + py * py).sqrt() / len2.sqrt()
            } else {
                (px * dx + py * dy) / len2
            }
            .clamp(0.0, 1.0);
            let k = sel.as_ref().map_or(1.0, |s| s.get(x, y) as f32 / 255.0);
            color::f_to_px([0, 1, 2, 3].map(|i| (c0[i] + (c1[i] - c0[i]) * t) * k))
        });
        let node = Node::raster(
            0,
            if radial {
                "Radial gradient"
            } else {
                "Gradient"
            },
            Arc::new(r),
            Placement::default(),
        );
        let slot = self.insertion_slot();
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        ) {
            self.set_layer_selection(vec![id], Some(id));
        }
    }

    // ── Colour picker ───────────────────────────────────────────────────

    fn pick_sv(&mut self, track: &TrackBounds, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(b) = track.get() else { return };
        let s = (f32::from(pos.x - b.origin.x) / f32::from(b.size.width)).clamp(0.0, 1.0);
        let v = 1.0 - (f32::from(pos.y - b.origin.y) / f32::from(b.size.height)).clamp(0.0, 1.0);
        let [r, g, bl] = hsv_to_rgb(self.tools.hue, s, v);
        self.apply_foreground([r, g, bl, 255], cx);
    }

    fn pick_hue(&mut self, track: &TrackBounds, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(f) = track_fraction(track, pos.x) else {
            return;
        };
        self.tools.hue = f.min(0.9999);
        let (_, s, v) = rgb_to_hsv(self.tools.fg);
        let [r, g, b] = hsv_to_rgb(self.tools.hue, s.max(0.05), v.max(0.05));
        self.apply_foreground([r, g, b, 255], cx);
    }

    /// Marching-ants segments for the current selection, cached.
    pub(crate) fn ants(&mut self, level: u32) -> Option<Segments> {
        let sel = self.editor.doc.selection.clone()?;
        let key = Arc::as_ptr(&sel) as usize;
        // Keep the outline cheap: never finer than ~2048 px across.
        let mut level = level;
        while sel.level_size(level).0.max(sel.level_size(level).1) > 2048 {
            level += 1;
        }
        if let Some((k, l, segs)) = &self.tools.ants
            && *k == key
            && *l == level
        {
            return Some(segs.clone());
        }
        let segs = Arc::new(select::outline(&sel, level));
        self.tools.ants = Some((key, level, segs.clone()));
        Some(segs)
    }
}

/// What the canvas draws over the image this frame.
#[derive(Clone, Default)]
pub struct Overlay {
    pub ants: Option<Segments>,
    pub phase: bool,
    /// Document-space polylines (closed when the flag is set).
    pub lines: Vec<(Vec<(f64, f64)>, bool)>,
    pub crop: Option<(f64, f64, f64, f64)>,
    pub cursor: Option<(Point<Pixels>, f32)>,
    pub marker: Option<(f64, f64)>,
    /// Free Transform box of the selected node.
    pub transform: Option<[(f64, f64); 4]>,
    /// Dashed outline of the selected layer's bounds (GIMP's layer
    /// boundary), shown by every tool except Move, which has its box.
    pub layer: Option<[(f64, f64); 4]>,
    /// The assistant's brush while it paints: screen position and radius.
    pub ghost: Option<(Point<Pixels>, f32)>,
    pub pen: Option<super::pen::PenOverlay>,
    /// Guides and snap lines: (vertical, position in document pixels).
    pub guides: Vec<(bool, f64)>,
    pub snaps: Vec<(bool, f64)>,
    /// Drawing guide (grid, isometric, perspective) polylines and the
    /// vanishing points to show as handles, in document pixels.
    pub assist: Vec<Vec<(f64, f64)>>,
    pub vanishing: Vec<(f64, f64)>,
}

impl EditorView {
    pub(crate) fn overlay(&mut self, scale_factor: f32) -> Overlay {
        let level = self.view.level(scale_factor, 12);
        let (guides, snaps) = self.guide_lines();
        let (mut assist, mut vanishing) = self.guide_overlay();
        let (wl, wp) = self.warp_overlay();
        assist.extend(wl);
        vanishing.extend(wp);
        let mut o = Overlay {
            ants: self.ants(level),
            phase: self.tools.ants_phase,
            guides,
            snaps,
            assist,
            vanishing,
            transform: self.transform_box(),
            layer: self.layer_outline(),
            pen: self.pen_overlay(),
            ghost: self.ghost_brush().and_then(|(d, size)| {
                let p = self.doc_to_window(d)?;
                Some((p, (size as f64 * self.view.zoom / 2.0) as f32))
            }),
            ..Default::default()
        };
        let ellipse_pts = |x: f64, y: f64, w: f64, h: f64| -> Vec<(f64, f64)> {
            (0..64)
                .map(|i| {
                    let t = i as f64 / 64.0 * std::f64::consts::TAU;
                    (
                        x + w / 2.0 + w / 2.0 * t.cos(),
                        y + h / 2.0 + h / 2.0 * t.sin(),
                    )
                })
                .collect()
        };
        let rect_pts =
            |x: f64, y: f64, w: f64, h: f64| vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        if let Some(Drag::Tool(t)) = &self.drag {
            match t {
                ToolDrag::Marquee {
                    start,
                    end,
                    ellipse,
                    ..
                }
                | ToolDrag::Shape {
                    start,
                    end,
                    ellipse,
                } => {
                    let (x, y, w, h) = if matches!(t, ToolDrag::Shape { .. }) {
                        self.shape_drag_rect(*start, *end, self.drag_shift)
                    } else {
                        norm(*start, *end)
                    };
                    o.lines.push((
                        if *ellipse {
                            ellipse_pts(x, y, w, h)
                        } else {
                            rect_pts(x, y, w, h)
                        },
                        true,
                    ));
                }
                ToolDrag::Lasso { pts, .. } => o.lines.push((pts.clone(), false)),
                ToolDrag::Gradient { start, end } => o.lines.push((vec![*start, *end], false)),
                ToolDrag::Crop {
                    start,
                    end,
                    symmetric,
                } => {
                    o.crop = Some(crop_rect(
                        *start,
                        *end,
                        *symmetric,
                        self.crop_constraint(),
                        self.drag_shift,
                    ))
                }
                _ => {}
            }
        }
        if let Some(Drag::Tool(ToolDrag::Quick { pts, .. })) = &self.drag {
            o.lines.push((pts.clone(), false));
        }
        if !self.tools.polygon.is_empty() {
            let mut pts = self.tools.polygon.clone();
            if self.tools.select == SelectShape::Magnetic && !self.tools.magnetic_live.is_empty() {
                pts.extend(self.tools.magnetic_live.iter().copied());
            } else if let Some(p) = self.tools.pointer.and_then(|p| self.doc_point(p)) {
                pts.push(p);
            }
            o.lines.push((pts, false));
        }
        if o.crop.is_none() {
            o.crop = self.tools.crop;
        }
        // Straightening: show the region that will be kept, turned back
        // into the unrotated image, with a rule-of-thirds grid.
        if let Some((x, y, w, h)) = o.crop
            && self.tools.straighten != 0.0
        {
            o.crop = None;
            let (dw, dh) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
            let c = dvec2(dw / 2.0, dh / 2.0);
            let back = DAffine2::from_translation(c)
                * DAffine2::from_angle(-(self.tools.straighten as f64).to_radians())
                * DAffine2::from_translation(-c);
            let map = |px: f64, py: f64| {
                let q = back.transform_point2(dvec2(px, py));
                (q.x, q.y)
            };
            o.lines.push((
                vec![map(x, y), map(x + w, y), map(x + w, y + h), map(x, y + h)],
                true,
            ));
            for k in [1.0, 2.0] {
                let (gx, gy) = (x + w * k / 3.0, y + h * k / 3.0);
                o.lines.push((vec![map(gx, y), map(gx, y + h)], false));
                o.lines.push((vec![map(x, gy), map(x + w, gy)], false));
            }
        }
        let brushy = matches!(self.tool, Tool::Heal | Tool::Clone | Tool::Mask)
            || (self.tool == Tool::Brush
                && matches!(
                    self.tools.paint,
                    PaintKind::Brush | PaintKind::Eraser | PaintKind::Smudge | PaintKind::Liquify
                ));
        if brushy && let Some(p) = self.tools.pointer {
            o.cursor = Some((
                p,
                (self.tools.brush.size as f64 / 2.0 * self.view.zoom) as f32,
            ));
        }
        if self.tool == Tool::Clone {
            o.marker = self.tools.clone_source;
        }
        o
    }
}

pub(crate) fn paint_overlay(
    o: &Overlay,
    view: &View,
    bounds: Bounds<Pixels>,
    accent: Hsla,
    window: &mut Window,
) {
    let to_screen = |p: (f64, f64)| {
        let s = view.doc_to_screen(p, &bounds);
        point(px(s.0 as f32), px(s.1 as f32))
    };
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        let full_line = |vertical: bool, pos: f64, color: Hsla, window: &mut Window| {
            let s = to_screen(if vertical { (pos, 0.0) } else { (0.0, pos) });
            let q = if vertical {
                Bounds::new(
                    point(s.x.floor(), bounds.origin.y),
                    size(px(1.), bounds.size.height),
                )
            } else {
                Bounds::new(
                    point(bounds.origin.x, s.y.floor()),
                    size(bounds.size.width, px(1.)),
                )
            };
            window.paint_quad(fill(q, color));
        };
        let guide: Hsla = rgb(0x1FB5FF).into();
        if !o.assist.is_empty() {
            let mut pb = PathBuilder::stroke(px(1.));
            for l in &o.assist {
                if l.len() < 2 {
                    continue;
                }
                pb.move_to(to_screen(l[0]));
                for p in &l[1..] {
                    pb.line_to(to_screen(*p));
                }
            }
            if let Ok(p) = pb.build() {
                window.paint_path(p, guide.opacity(0.45));
            }
        }
        for v in &o.vanishing {
            let c = to_screen(*v);
            if !bounds.contains(&c) {
                continue;
            }
            let r = px(6.);
            window.paint_quad(fill(
                Bounds::new(point(c.x - r, c.y - r), size(r * 2., r * 2.)),
                gpui_kit::white().opacity(0.9),
            ));
            window.paint_quad(outline(
                Bounds::new(point(c.x - r, c.y - r), size(r * 2., r * 2.)),
                guide,
                BorderStyle::Solid,
            ));
        }
        for (v, p) in &o.guides {
            full_line(*v, *p, guide, window);
        }
        for (v, p) in &o.snaps {
            full_line(*v, *p, accent, window);
        }
        if let Some(q) = o.transform {
            super::transform::paint_box(q, view, bounds, rgb(0x1FB5FF).into(), window);
        }
        if let Some(q) = o.layer {
            let pts: Vec<Point<Pixels>> = q.iter().map(|p| to_screen(*p)).collect();
            // Light underlay so the dashes read on any picture, then dashes.
            let mut under = PathBuilder::stroke(px(1.));
            under.add_polygon(&pts, true);
            if let Ok(p) = under.build() {
                window.paint_path(p, gpui_kit::white().opacity(0.7));
            }
            let mut dashed = PathBuilder::stroke(px(1.)).dash_array(&[px(6.), px(4.)]);
            dashed.add_polygon(&pts, true);
            if let Ok(p) = dashed.build() {
                window.paint_path(p, accent);
            }
        }
        if let Some(segs) = &o.ants
            && !segs.is_empty()
        {
            let mut solid = PathBuilder::stroke(px(1.));
            let mut dashed = PathBuilder::stroke(px(1.)).dash_array(&[px(4.), px(4.)]);
            for (x0, y0, x1, y1) in segs.iter() {
                let (a, b) = (
                    to_screen((*x0 as f64, *y0 as f64)),
                    to_screen((*x1 as f64, *y1 as f64)),
                );
                solid.move_to(a);
                solid.line_to(b);
                dashed.move_to(a);
                dashed.line_to(b);
            }
            let (base, dash) = if o.phase {
                (gpui_kit::black(), gpui_kit::white())
            } else {
                (gpui_kit::white(), gpui_kit::black())
            };
            if let Ok(p) = solid.build() {
                window.paint_path(p, base);
            }
            if let Ok(p) = dashed.build() {
                window.paint_path(p, dash);
            }
        }
        if let Some((x, y, w, h)) = o.crop {
            let a = to_screen((x, y));
            let b = to_screen((x + w, y + h));
            let shade = gpui_kit::black().opacity(0.45);
            let (l, t, r, btm) = (
                bounds.origin.x,
                bounds.origin.y,
                bounds.origin.x + bounds.size.width,
                bounds.origin.y + bounds.size.height,
            );
            window.paint_quad(fill(
                Bounds::from_corners(point(l, t), point(r, a.y)),
                shade,
            ));
            window.paint_quad(fill(
                Bounds::from_corners(point(l, b.y), point(r, btm)),
                shade,
            ));
            window.paint_quad(fill(
                Bounds::from_corners(point(l, a.y), point(a.x, b.y)),
                shade,
            ));
            window.paint_quad(fill(
                Bounds::from_corners(point(b.x, a.y), point(r, b.y)),
                shade,
            ));
            window.paint_quad(outline(
                Bounds::from_corners(a, b),
                accent,
                BorderStyle::Solid,
            ));
        }
        for (pts, closed) in &o.lines {
            if pts.len() < 2 {
                continue;
            }
            for (width, color) in [(px(3.), gpui_kit::white().opacity(0.8)), (px(1.), accent)] {
                let mut pb = PathBuilder::stroke(width);
                pb.add_polygon(
                    &pts.iter().map(|p| to_screen(*p)).collect::<Vec<_>>(),
                    *closed,
                );
                if let Ok(p) = pb.build() {
                    window.paint_path(p, color);
                }
            }
        }
        if let Some((c, r)) = o.cursor {
            let r = r.max(2.0);
            let pts: Vec<Point<Pixels>> = (0..48)
                .map(|i| {
                    let t = i as f32 / 48.0 * std::f32::consts::TAU;
                    c + point(px(r * t.cos()), px(r * t.sin()))
                })
                .collect();
            for (width, color) in [
                (px(3.), gpui_kit::black().opacity(0.5)),
                (px(1.), gpui_kit::white()),
            ] {
                let mut pb = PathBuilder::stroke(width);
                pb.add_polygon(&pts, true);
                if let Ok(p) = pb.build() {
                    window.paint_path(p, color);
                }
            }
        }
        if let Some(pen) = &o.pen {
            let blue: Hsla = rgb(0x1FB5FF).into();
            for (pts, closed) in &pen.curves {
                if pts.len() < 2 {
                    continue;
                }
                let mut pb = PathBuilder::stroke(px(1.5));
                pb.move_to(to_screen(pts[0]));
                for q in &pts[1..] {
                    pb.line_to(to_screen(*q));
                }
                if *closed {
                    pb.line_to(to_screen(pts[0]));
                }
                if let Ok(p) = pb.build() {
                    window.paint_path(p, blue);
                }
            }
            for (a, h) in &pen.handles {
                let (sa, sh) = (to_screen(*a), to_screen(*h));
                let mut pb = PathBuilder::stroke(px(1.));
                pb.move_to(sa);
                pb.line_to(sh);
                if let Ok(p) = pb.build() {
                    window.paint_path(p, blue.opacity(0.8));
                }
                window.paint_quad(
                    fill(
                        Bounds::new(sh - point(px(3.), px(3.)), size(px(6.), px(6.))),
                        gpui_kit::white(),
                    )
                    .border_widths(px(1.))
                    .border_color(blue)
                    .corner_radii(px(3.)),
                );
            }
            for (a, selected, smooth) in &pen.anchors {
                let s = to_screen(*a);
                let q = Bounds::new(s - point(px(3.5), px(3.5)), size(px(7.), px(7.)));
                let bg = if *selected { blue } else { gpui_kit::white() };
                let quad = fill(q, bg).border_widths(px(1.)).border_color(blue);
                window.paint_quad(if *smooth {
                    quad.corner_radii(px(3.5))
                } else {
                    quad
                });
            }
        }
        if let Some((c, r)) = o.ghost {
            let r = r.max(3.0);
            let pts: Vec<Point<Pixels>> = (0..48)
                .map(|i| {
                    let t = i as f32 / 48.0 * std::f32::consts::TAU;
                    c + point(px(r * t.cos()), px(r * t.sin()))
                })
                .collect();
            let mut pb = PathBuilder::stroke(px(2.));
            pb.add_polygon(&pts, true);
            if let Ok(p) = pb.build() {
                window.paint_path(p, accent);
            }
            window.paint_quad(fill(
                Bounds::new(c - point(px(2.), px(2.)), size(px(4.), px(4.))),
                accent,
            ));
            // A square tag beside it, so the eye can follow it.
            window.paint_quad(fill(
                Bounds::new(c + point(px(r + 4.), px(-8.)), size(px(6.), px(6.))),
                accent,
            ));
        }
        if let Some(m) = o.marker {
            let c = to_screen(m);
            window.paint_quad(fill(
                Bounds::new(c - point(px(6.), px(0.5)), size(px(12.), px(1.))),
                accent,
            ));
            window.paint_quad(fill(
                Bounds::new(c - point(px(0.5), px(6.)), size(px(1.), px(12.))),
                accent,
            ));
        }
    });
}

// ── Tool options and colour picker ──────────────────────────────────────

impl EditorView {
    #[allow(clippy::too_many_arguments)] // a UI row: each argument is one visible property
    pub(crate) fn opt_slider(
        &mut self,
        key: SliderKey,
        name: &str,
        display: String,
        norm: f32,
        spec: (f32, f32, f32),
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let track = self.tracks.entry(key).or_default().clone();
        let compact = crate::app_state::settings(cx).compact_chrome;
        let quick_brush = key.is_quick_brush();
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .flex_none()
            .child(
                div()
                    .when(quick_brush, |d| d.w(rems(4.5)))
                    .child(name.to_string()),
            )
            .child(
                div()
                    .w(if quick_brush {
                        rems(7.5)
                    } else if compact {
                        rems(4.)
                    } else {
                        rems(5.25)
                    })
                    .child(
                        slider(
                            SharedString::from(format!("{key:?}")),
                            norm,
                            track,
                            p,
                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                this.slider_down(key, spec, e, cx)
                            }),
                        )
                        .tab_index(0)
                        .key_context("Slider")
                        .role(Role::Slider)
                        .aria_label(name.to_string())
                        .aria_value(display.clone())
                        .aria_min_numeric_value(spec.0 as f64)
                        .aria_max_numeric_value(spec.1 as f64)
                        .aria_description(
                            "Arrow keys adjust; Shift adjusts faster; Home and End go to limits",
                        )
                        .focus_visible(|s| s.bg(p.accent.opacity(0.2)))
                        .on_key_down(cx.listener(move |this, e, _, cx| {
                            this.slider_key(key, norm, spec, e, cx)
                        }))
                        .test_support(),
                    ),
            )
            .child(
                div()
                    .w(if quick_brush {
                        rems(3.5)
                    } else if compact {
                        rems(2.)
                    } else {
                        rems(2.5)
                    })
                    .text_color(p.ink)
                    .child(display),
            )
            .into_any_element()
    }

    /// A labelled divider between groups of options.
    pub(crate) fn group(&self, label: &'static str, p: &Palette) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .flex_none()
            .child(div().w(px(1.)).h(px(14.)).bg(p.line))
            .child(mono(label, 9.5, p.muted))
            .into_any_element()
    }

    /// Size, hardness, opacity and flow for the current brush slot.
    fn brush_sliders(&mut self, v: &mut Vec<AnyElement>, p: &Palette, cx: &mut Context<Self>) {
        let b = self.tools.brush;
        v.push(self.opt_slider(
            SliderKey::ToolSize,
            "size",
            format!("{:.0}px", b.size),
            ((b.size - 1.0) / 499.0).sqrt(),
            (1.0, 500.0, 1.0),
            p,
            cx,
        ));
        v.push(self.opt_slider(
            SliderKey::ToolHardness,
            "hard",
            format!("{:.0}%", b.hardness * 100.0),
            b.hardness,
            (0.0, 100.0, 1.0),
            p,
            cx,
        ));
        v.push(self.opt_slider(
            SliderKey::ToolOpacity,
            "opacity",
            format!("{:.0}%", b.opacity * 100.0),
            b.opacity,
            (1.0, 100.0, 1.0),
            p,
            cx,
        ));
        v.push(self.opt_slider(
            SliderKey::ToolFlow,
            "flow",
            format!("{:.0}%", b.flow * 100.0),
            b.flow,
            (1.0, 100.0, 1.0),
            p,
            cx,
        ));
    }

    #[allow(clippy::too_many_arguments)] // a UI row: each argument is one visible property
    fn mode_chip_tip<T: PartialEq + Copy + 'static>(
        &self,
        id: &'static str,
        text: &'static str,
        help: &'static str,
        value: T,
        current: T,
        p: &Palette,
        cx: &mut Context<Self>,
        set: fn(&mut EditorView, T, &mut Context<EditorView>),
    ) -> AnyElement {
        tip(
            chip(id, text, value == current, p)
                .on_click(cx.listener(move |this, _, _, cx| set(this, value, cx))),
            help,
        )
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)] // a UI row: each argument is one visible property
    fn mode_chip<T: PartialEq + Copy + 'static>(
        &self,
        id: &'static str,
        text: &'static str,
        value: T,
        current: T,
        p: &Palette,
        cx: &mut Context<Self>,
        set: fn(&mut EditorView, T, &mut Context<EditorView>),
    ) -> AnyElement {
        chip(id, text, value == current, p)
            .on_click(cx.listener(move |this, _, _, cx| set(this, value, cx)))
            .into_any_element()
    }

    pub(crate) fn tool_options(&mut self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        let b = self.tools.brush;
        if self.brushy() {
            let editor = cx.weak_entity();
            v.push(
                div()
                    .id("brush-settings")
                    .test_support()
                    .child(
                        Button::new("brush-settings-trigger")
                            .label("Brush settings ▾")
                            .small()
                            .tooltip("Adjust your brush here or right-click the canvas. [ and ] change size.")
                            .dropdown_menu(move |menu, _, cx| {
                                let Some(editor) = editor.upgrade() else {
                                    return menu;
                                };
                                super::brush_quick::menu(menu, &editor, cx)
                            }),
                    )
                    .into_any_element(),
            );
        }
        match self.tool {
            Tool::Grade => {
                v.push(self.group("add adjustment", p));
                for (id, title, key) in [
                    ("grade-exposure", "Exposure", "exposure"),
                    ("grade-curves", "Curves", "curves"),
                    ("grade-color-balance", "Color balance", "color_balance"),
                    ("grade-hsl", "Hue / Saturation", "hue_saturation"),
                ] {
                    v.push(
                        chip(id, title, false, p)
                            .on_click(cx.listener(move |this, _, _, cx| this.quick_adjust(key, cx)))
                            .test_support()
                            .into_any_element(),
                    );
                }
                v.push(
                    chip(
                        "grade-adjustments",
                        "All adjustments",
                        self.sidebar_tab == SidebarTab::Adjustments,
                        p,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.select_sidebar(SidebarTab::Adjustments, cx)
                    }))
                    .test_support()
                    .into_any_element(),
                );
            }
            Tool::Mask => {
                let reveal = self.tools.mask_reveal;
                for (id, t, help, r) in [
                    ("mk-reveal", "reveal", "Painting shows the layer here", true),
                    ("mk-hide", "hide", "Painting hides the layer here", false),
                ] {
                    v.push(
                        self.mode_chip_tip(id, t, help, r, reveal, p, cx, |e, r, cx| {
                            e.tools.mask_reveal = r;
                            e.tools.fg = if r { [255; 4] } else { [0, 0, 0, 255] };
                            cx.notify();
                        }),
                    );
                }
                v.push(self.group("brush", p));
                self.brush_sliders(&mut v, p, cx);
                v.push(self.group("mask", p));
                let has_sel = self.editor.doc.selection.is_some();
                v.push(
                    chip("mk-inv", "invert", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.invert_mask(cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("mk-feather", "feather 6", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.feather_mask(6.0, cx)))
                        .into_any_element(),
                );
                if has_sel {
                    v.push(
                        chip("mk-from", "from selection", false, p)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.remove_mask(cx);
                                this.add_mask(cx);
                                this.tools.mask_edit = true;
                                this.tool = Tool::Mask;
                            }))
                            .into_any_element(),
                    );
                }
                v.push(
                    chip("mk-sel", "to selection", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.mask_to_selection(cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("mk-del", "− mask", false, p)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.remove_mask(cx);
                            this.set_tool(Tool::Brush, cx);
                        }))
                        .into_any_element(),
                );
                v.push(
                    div()
                        .flex_none()
                        .child(if reveal {
                            "painting reveals the layer; hide paints it away"
                        } else {
                            "painting hides the layer; reveal brings it back"
                        })
                        .into_any_element(),
                );
            }
            Tool::Select => {
                v.push(
                    chip("sel-all", "Select all", false, p)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.select_all(cx);
                            window.focus(&this.canvas_focus, cx);
                        }))
                        .test_support()
                        .into_any_element(),
                );
                v.push(
                    chip("sel-none", "Deselect", false, p)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.deselect(cx);
                            window.focus(&this.canvas_focus, cx);
                        }))
                        .test_support()
                        .into_any_element(),
                );
                let cur = self.tools.select;
                for (id, t, s) in [
                    ("sel-rect", "Rectangle", SelectShape::Rect),
                    ("sel-ell", "ellipse", SelectShape::Ellipse),
                    ("sel-lasso", "lasso", SelectShape::Lasso),
                    ("sel-poly", "polygon", SelectShape::Polygon),
                    ("sel-mag", "magnetic", SelectShape::Magnetic),
                    ("sel-wand", "wand", SelectShape::Wand),
                    ("sel-quick", "quick", SelectShape::Quick),
                ] {
                    v.push(self.mode_chip(id, t, s, cur, p, cx, |e, s, cx| e.set_select(s, cx)));
                }
                let cm = self.tools.combine;
                for (id, t, c) in [
                    ("cm-new", "new", Combine::Replace),
                    ("cm-add", "add", Combine::Add),
                    ("cm-sub", "subtract", Combine::Subtract),
                    ("cm-int", "intersect", Combine::Intersect),
                ] {
                    v.push(self.mode_chip(id, t, c, cm, p, cx, |e, c, cx| {
                        e.tools.combine = c;
                        cx.notify();
                    }));
                }
                if cur == SelectShape::Quick {
                    let sam_ok = self.sam_available();
                    let ai_on = self.ai.ai_select && sam_ok;
                    v.push(
                        chip(
                            "sel-ai",
                            if sam_ok { "AI" } else { "AI (install)" },
                            ai_on,
                            p,
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if sam_ok {
                                this.ai.ai_select = !this.ai.ai_select;
                                cx.notify();
                            } else {
                                window.dispatch_action(Box::new(crate::actions::ShowSettings), cx);
                            }
                        }))
                        .into_any_element(),
                    );
                    if ai_on {
                        v.push(
                            mono("click a thing, or drag a box around it", 10., p.muted)
                                .into_any_element(),
                        );
                    }
                }
                v.push(
                    chip("sel-subject", "subject (AI)", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.select_subject(cx)))
                        .into_any_element(),
                );
                if let Some(r) = self.ai.refine.clone() {
                    v.push(mono("refine", 9., p.muted).into_any_element());
                    v.push(self.opt_slider(
                        SliderKey::RefineHi,
                        "in above",
                        format!("{:.0}", r.hi),
                        r.hi / 255.0,
                        (1.0, 255.0, 1.0),
                        p,
                        cx,
                    ));
                    v.push(self.opt_slider(
                        SliderKey::RefineLo,
                        "out below",
                        format!("{:.0}", r.lo),
                        r.lo / 255.0,
                        (0.0, 254.0, 1.0),
                        p,
                        cx,
                    ));
                    v.push(self.opt_slider(
                        SliderKey::RefineGrow,
                        "grow",
                        format!("{:+.0} px", r.grow),
                        (r.grow + 40.0) / 80.0,
                        (-40.0, 40.0, 1.0),
                        p,
                        cx,
                    ));
                    v.push(self.opt_slider(
                        SliderKey::RefineFeather,
                        "feather",
                        format!("{:.0} px", r.feather),
                        (r.feather / 60.0).sqrt(),
                        (0.0, 60.0, 0.5),
                        p,
                        cx,
                    ));
                    if let Some(why) = self.refine_why() {
                        v.push(mono(why, 9.5, p.muted).into_any_element());
                    }
                    v.push(
                        chip("sel-refine-done", "done", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.refine_done(cx)))
                            .into_any_element(),
                    );
                }
                if cur == SelectShape::Quick && !(self.ai.ai_select && self.sam_available()) {
                    let t = self.tools.tolerance as f32;
                    v.push(self.opt_slider(
                        SliderKey::Tolerance,
                        "spread",
                        format!("{:.0}", t / 255.0 * 100.0),
                        t / 255.0,
                        (0.0, 255.0, 1.0),
                        p,
                        cx,
                    ));
                    let s = self.tools.brush.size;
                    v.push(self.opt_slider(
                        SliderKey::ToolSize,
                        "brush",
                        format!("{s:.0}px"),
                        ((s - 1.0) / 499.0).sqrt(),
                        (1.0, 500.0, 1.0),
                        p,
                        cx,
                    ));
                } else if cur == SelectShape::Wand {
                    let t = self.tools.tolerance as f32;
                    v.push(self.opt_slider(
                        SliderKey::Tolerance,
                        "tolerance",
                        format!("{t:.0}"),
                        t / 255.0,
                        (0.0, 255.0, 1.0),
                        p,
                        cx,
                    ));
                    let c = self.tools.contiguous;
                    v.push(
                        chip("contig", "contiguous", c, p)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.tools.contiguous = !c;
                                cx.notify();
                            }))
                            .into_any_element(),
                    );
                } else {
                    let f = self.tools.feather;
                    v.push(self.opt_slider(
                        SliderKey::Feather,
                        "feather",
                        format!("{f:.0}px"),
                        f / 100.0,
                        (0.0, 100.0, 1.0),
                        p,
                        cx,
                    ));
                }
                if self.editor.doc.selection.is_some() {
                    v.push(
                        chip("sel-transform-pixels", "transform pixels", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.transform_pixels(cx)))
                            .into_any_element(),
                    );
                }
                v.push(
                    chip("sel-inv", "invert", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.invert_selection(cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("sel-grow", "grow 5", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.modify_selection(5, 0.0, cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("sel-shrink", "shrink 5", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.modify_selection(-5, 0.0, cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("sel-feather", "soften 10", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.modify_selection(0, 10.0, cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("sel-node", "from layer", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.select_from_node(cx)))
                        .into_any_element(),
                );
                for (id, t, scale, turn) in [
                    ("sel-smaller", "−10%", 0.9, 0.0),
                    ("sel-larger", "+10%", 1.1, 0.0),
                    ("sel-rotate", "↻15°", 1.0, 15.0),
                ] {
                    v.push(
                        chip(id, t, false, p)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.transform_selection(0.0, 0.0, scale, turn, cx)
                            }))
                            .into_any_element(),
                    );
                }
                v.push(
                    chip("sel-caf", "content-aware fill", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.content_aware_fill(cx)))
                        .into_any_element(),
                );
                v.push(
                    tip(
                        chip("sel-aifill", "AI fill", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.ai_fill(cx))),
                        "Fill the selection from its surroundings with the local LaMa model (no prompt)",
                    )
                    .into_any_element(),
                );
                v.extend(self.generate_row(p, cx));
            }
            Tool::Brush | Tool::Heal | Tool::Clone => {
                let open = self.presets.open;
                if self.brushy() {
                    v.push(
                        tip(
                            chip(
                                "presets",
                                self.presets
                                    .current
                                    .clone()
                                    .unwrap_or_else(|| "Brush presets".into()),
                                open,
                                p,
                            )
                            .test_support()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_presets(cx))),
                            "Brush library: pencils, inks, paints, erasers and imported brushes",
                        )
                        .into_any_element(),
                    );
                }
                if self.tool == Tool::Brush {
                    let cur = self.tools.paint;
                    if cur == PaintKind::Liquify {
                        use emulsion_raster::liquify::Mode;
                        let m = self.tools.liquify;
                        v.push(self.group("mode", p));
                        for (id, k) in [
                            ("lq-push", Mode::Push),
                            ("lq-cw", Mode::Twirl { cw: true }),
                            ("lq-ccw", Mode::Twirl { cw: false }),
                            ("lq-pinch", Mode::Pinch),
                            ("lq-expand", Mode::Expand),
                            ("lq-restore", Mode::Restore),
                        ] {
                            v.push(
                                chip(id, k.label(), m == k, p)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.tools.liquify = k;
                                        cx.notify();
                                    }))
                                    .into_any_element(),
                            );
                        }
                        let b = self.tools.brush;
                        v.push(self.opt_slider(
                            SliderKey::ToolSize,
                            "size",
                            format!("{:.0}px", b.size),
                            ((b.size - 1.0) / 499.0).sqrt(),
                            (1.0, 500.0, 1.0),
                            p,
                            cx,
                        ));
                        v.push(self.opt_slider(
                            SliderKey::ToolFlow,
                            "strength",
                            format!("{:.0}%", b.flow * 100.0),
                            b.flow,
                            (1.0, 100.0, 1.0),
                            p,
                            cx,
                        ));
                    }
                }
                if self.brushy() {
                    let current_blend = b.blend;
                    let editor = cx.weak_entity();
                    v.push(
                        Button::new("brush-blend-mode")
                            .label(format!("{} ▾", current_blend.label()))
                            .small()
                            .bg(p.soft_bg)
                            .text_color(p.ink)
                            .dropdown_menu(move |mut menu, _, _| {
                                for mode in BrushBlend::MENU.iter().copied() {
                                    let editor = editor.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(mode.label())
                                            .checked(mode == current_blend)
                                            .on_click(move |_, _, cx| {
                                                editor
                                                    .update(cx, |this, cx| {
                                                        this.tools.brush.blend = mode;
                                                        cx.notify();
                                                    })
                                                    .ok();
                                            }),
                                    );
                                }
                                menu
                            })
                            .into_any_element(),
                    );
                    // In the floating bar, spend available space on brush
                    // values before commands that open other panels.
                    let panel_commands = if crate::app_state::settings(cx).compact_chrome {
                        std::mem::take(&mut v)
                    } else {
                        Vec::new()
                    };
                    v.push(self.opt_slider(
                        SliderKey::ToolSize,
                        "size",
                        format!("{:.0}", b.size),
                        ((b.size - 1.0) / 499.0).sqrt(),
                        (1.0, 500.0, 1.0),
                        p,
                        cx,
                    ));
                    v.push(self.opt_slider(
                        SliderKey::ToolHardness,
                        "hard",
                        format!("{:.0}%", b.hardness * 100.0),
                        b.hardness,
                        (0.0, 100.0, 1.0),
                        p,
                        cx,
                    ));
                    if self.tool != Tool::Heal {
                        v.push(self.opt_slider(
                            SliderKey::ToolOpacity,
                            "opacity",
                            format!("{:.0}%", b.opacity * 100.0),
                            b.opacity,
                            (1.0, 100.0, 1.0),
                            p,
                            cx,
                        ));
                    }
                    v.extend(panel_commands);
                    // What is switched on in the advanced row, at a glance.
                    let mut on: Vec<&str> = Vec::new();
                    if self.tools.mirror_x || self.tools.mirror_y {
                        on.push("mirror");
                    }
                    if self.tools.symmetry >= 2 {
                        on.push("radial");
                    }
                    if self.tools.guide.kind != super::guides::GuideKind::Off {
                        on.push("guide");
                    }
                    if self.tools.alpha_lock {
                        on.push("alpha lock");
                    }
                    if !on.is_empty() {
                        v.push(
                            mono(format!("on: {}", on.join(", ")), 9.5, p.muted)
                                .flex_none()
                                .into_any_element(),
                        );
                    }
                } else if self.tools.paint == PaintKind::Bucket {
                    let t = self.tools.tolerance as f32;
                    v.push(self.opt_slider(
                        SliderKey::Tolerance,
                        "tolerance",
                        format!("{t:.0}"),
                        t / 255.0,
                        (0.0, 255.0, 1.0),
                        p,
                        cx,
                    ));
                } else if self.tools.paint == PaintKind::Gradient {
                    let r = self.tools.radial;
                    v.push(
                        chip("g-lin", "linear", !r, p)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tools.radial = false;
                                cx.notify();
                            }))
                            .into_any_element(),
                    );
                    v.push(
                        chip("g-rad", "radial", r, p)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tools.radial = true;
                                cx.notify();
                            }))
                            .into_any_element(),
                    );
                }
                let hint = match self.tool {
                    Tool::Clone if self.tools.clone_source.is_none() => "alt-click sets the source",
                    Tool::Clone => "alt-click to move the source",
                    Tool::Heal => "paint over a blemish",
                    _ if self.tools.paint == PaintKind::Smudge => {
                        "drag to smear the colour under the brush"
                    }
                    _ if self.tools.paint == PaintKind::Liquify => {
                        "drag to move the pixels; restore paints them back"
                    }
                    _ if self.tools.paint == PaintKind::Bucket => "click an area to fill it",
                    _ if self.tools.paint == PaintKind::Gradient => {
                        "drag from one colour to the other"
                    }
                    _ if self.tools.mask_edit => {
                        "painting the mask: white reveals, black hides, gray partially reveals"
                    }
                    _ => "alt-click picks a colour",
                };
                if matches!(self.tool, Tool::Clone | Tool::Heal) || self.tools.mask_edit {
                    v.push(div().flex_none().child(hint).into_any_element());
                }
            }
            Tool::Crop => {
                v.push(
                    tip(
                        chip_action(
                            "crop-apply",
                            "Apply",
                            self.tools.crop.is_some(),
                            self.tools.crop.is_some() && self.tools.crop_options.valid,
                            p,
                            cx.listener(|this, _, _, cx| this.tool_commit(cx)),
                        )
                        .test_support(),
                        "Draw a crop rectangle, then apply it (Enter)",
                    )
                    .into_any_element(),
                );
                v.push(
                    chip_action(
                        "crop-cancel",
                        "Cancel",
                        false,
                        self.tools.crop.is_some() || self.tools.straighten != 0.,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.tool_cancel(cx);
                        }),
                    )
                    .test_support()
                    .into_any_element(),
                );
                v.push(self.crop_options(p, cx));
                match self.tools.crop {
                    Some((_, _, w, h)) => v.push(
                        div()
                            .flex_none()
                            .text_color(p.ink)
                            .child(format!("{:.0} × {:.0}", w, h))
                            .into_any_element(),
                    ),
                    None => v.push(
                        div()
                            .flex_none()
                            .child("drag a crop; it may extend past the canvas")
                            .into_any_element(),
                    ),
                }
                let s = self.tools.straighten;
                v.push(self.opt_slider(
                    SliderKey::Straighten,
                    "straighten",
                    format!("{s:+.1}°"),
                    (s + 45.0) / 90.0,
                    (-45.0, 45.0, 0.1),
                    p,
                    cx,
                ));
                let centered = self.tools.crop_centered;
                v.push(
                    chip("crop-centre", "from centre", centered, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.tools.crop_centered = !centered;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
                let delete = self.tools.crop_delete;
                v.push(
                    crate::widgets::tip(
                        chip("crop-delete", "delete cropped pixels", delete, p).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.tools.crop_delete = !delete;
                                cx.notify();
                            }),
                        ),
                        "On: pixel layers are cut to the crop, as in Photoshop. Off: layers stay whole past the edge and can be moved back into view.",
                    )
                    .into_any_element(),
                );
                let fill = self.tools.fill_edges;
                v.push(
                    chip("crop-fill", "fill new edges", fill, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.tools.fill_edges = !fill;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
                v.push(
                    chip("size-panel", "size…", self.size_panel.is_some(), p)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_size_panel(window, cx)),
                        )
                        .into_any_element(),
                );
                let (w, h) = (self.editor.doc.width, self.editor.doc.height);
                v.push(
                    chip("img-half", "image 50%", false, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.execute(
                                Command::ImageSize {
                                    width: (w / 2).max(1),
                                    height: (h / 2).max(1),
                                },
                                cx,
                            );
                            this.fit_pending = true;
                        }))
                        .into_any_element(),
                );
                v.push(
                    chip("img-double", "image 200%", false, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.execute(
                                Command::ImageSize {
                                    width: w * 2,
                                    height: h * 2,
                                },
                                cx,
                            );
                            this.fit_pending = true;
                        }))
                        .into_any_element(),
                );
            }
            Tool::Type => self.type_options(&mut v, p, cx),
            Tool::Pen => {
                v.push(self.pen_operation_control(cx));
                let pen_w = self.tools.pen.width;
                v.push(self.opt_slider(
                    SliderKey::PenWidth,
                    "width",
                    format!("{pen_w:.1}px"),
                    (pen_w / 60.0).sqrt(),
                    (0.0, 60.0, 0.5),
                    p,
                    cx,
                ));
                let (so, fo) = (self.tools.pen.stroke_on, self.tools.pen.fill_on);
                v.push(
                    chip("pen-stroke", "stroke", so, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.tools.pen.stroke_on = !so;
                            this.pen_restyle(cx);
                        }))
                        .into_any_element(),
                );
                v.push(
                    chip("pen-fill", "fill", fo, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.tools.pen.fill_on = !fo;
                            this.pen_restyle(cx);
                        }))
                        .into_any_element(),
                );
                if self.pen_target().is_some() {
                    v.push(
                        chip("pen-colours", "use colours", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.pen_restyle(cx)))
                            .into_any_element(),
                    );
                }
                let building = self.tools.pen.building.is_some();
                let anchors = self
                    .tools
                    .pen
                    .building
                    .as_ref()
                    .map_or(0, |path| path.anchors.len());
                let target = self.pen_target().is_some();
                v.push(
                    chip_action(
                        "pen-finish",
                        if building { "finish ⏎" } else { "new path" },
                        building,
                        !building || anchors >= 2,
                        p,
                        cx.listener(|this, _, _, cx| {
                            if this.tools.pen.building.is_some() {
                                this.pen_finish(cx);
                            } else {
                                this.set_layer_selection(Vec::new(), None);
                                this.tools.pen.selected = None;
                                cx.notify();
                            }
                        }),
                    )
                    .into_any_element(),
                );
                v.push(
                    tip(
                        chip_action(
                            "pen-close",
                            "close & finish",
                            false,
                            anchors >= 2,
                            p,
                            cx.listener(|this, _, _, cx| {
                                if let Some(sp) = &mut this.tools.pen.building {
                                    sp.closed = true;
                                }
                                this.pen_finish(cx);
                            }),
                        ),
                        "Add at least two anchors before closing the path",
                    )
                    .into_any_element(),
                );
                v.push(
                    tip(
                        chip_action(
                            "pen-sel",
                            "to selection",
                            false,
                            anchors >= 3 || (!building && target),
                            p,
                            cx.listener(|this, _, _, cx| this.pen_to_selection(cx)),
                        ),
                        "Select an existing path or draw at least three anchors",
                    )
                    .into_any_element(),
                );
                v.push(
                    tip(
                        chip_action(
                            "pen-paint",
                            "paint along path",
                            false,
                            anchors >= 2 || (!building && target),
                            p,
                            cx.listener(|this, _, _, cx| this.pen_paint_along(cx)),
                        ),
                        "Select an existing path or draw at least two anchors",
                    )
                    .into_any_element(),
                );
                if self.tools.pen.selected.is_some() || building {
                    v.push(
                        chip("pen-del", "delete anchor ⌫", false, p)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pen_delete(cx);
                            }))
                            .into_any_element(),
                    );
                }
                let hint = if building {
                    "click to add corners · drag for curves · click the first anchor or ⏎ to finish"
                } else if self.pen_target().is_some() {
                    "drag anchors and handles · alt-click an anchor to toggle corner/curve · click the outline to add one"
                } else {
                    "click to start a path · stroke uses the foreground colour, fill the background"
                };
                v.push(div().flex_none().child(hint).into_any_element());
            }
            Tool::Shape => {
                let cur = self.tools.shape;
                v.push(self.mode_chip(
                    "sh-rect",
                    "rectangle",
                    ShapeKind::Rect,
                    cur,
                    p,
                    cx,
                    |e, k, cx| {
                        e.tools.shape = k;
                        cx.notify();
                    },
                ));
                v.push(self.mode_chip(
                    "sh-ell",
                    "ellipse",
                    ShapeKind::Ellipse,
                    cur,
                    p,
                    cx,
                    |e, k, cx| {
                        e.tools.shape = k;
                        cx.notify();
                    },
                ));
                v.extend(self.shape_toolbar(cx));
            }
            Tool::Move => {
                v.push(self.alignment_controls(p, cx));
                let fields = self.transform_field_views(p);
                if fields.is_empty() {
                    v.push(
                        div()
                            .flex_none()
                            .child("drag a layer or group · arrows: 1 px · Shift+arrows: 10 px · Esc: cancel move")
                            .into_any_element(),
                    );
                } else if self.warp.is_some() {
                    v.push(self.group("warp", p));
                    v.push(
                        chip("warp-apply", "Apply", true, p)
                            .on_click(cx.listener(|this, _, _, cx| this.finish_warp(cx)))
                            .into_any_element(),
                    );
                    v.push(
                        chip("warp-cancel", "Cancel", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_warp(cx)))
                            .into_any_element(),
                    );
                    v.push(
                        div()
                            .flex_none()
                            .child("drag the grid points to bend the layer, then apply")
                            .into_any_element(),
                    );
                } else {
                    v.extend(fields);
                    v.push(
                        tip(
                            chip("warp-start", "warp", false, p)
                                .on_click(cx.listener(|this, _, _, cx| this.start_warp(cx))),
                            "Bend the layer with a 3×3 grid of points",
                        )
                        .into_any_element(),
                    );
                    v.push(
                        div()
                            .flex_none()
                            .child("drag handles to scale · outside a corner to rotate · ctrl+corner to distort")
                            .into_any_element(),
                    );
                }
            }
            Tool::Hand => {
                v.push(
                    div()
                        .flex_none()
                        .child(if self.tools.rotate_view {
                            "drag to rotate the view · Shift snaps to 15° · H to pan"
                        } else {
                            "drag to pan · ctrl+scroll to zoom · R to rotate the view"
                        })
                        .into_any_element(),
                );
                v.push(
                    chip("reset-view-rotation", "Reset view", false, p)
                        .test_support()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.rotate(0.0, cx);
                            window.focus(&this.canvas_focus, cx);
                        }))
                        .into_any_element(),
                );
            }
            Tool::Eyedropper => {
                let [r, g, b, _] = self.tools.fg;
                v.push(
                    div()
                        .flex_none()
                        .child(format!(
                            "click picks the foreground colour (#{r:02X}{g:02X}{b:02X}) · alt-click picks the background"
                        ))
                        .into_any_element(),
                );
            }
            Tool::Zoom => {
                v.push(
                    chip("zoom-in", "zoom in", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.zoom_step(true, cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("zoom-out", "zoom out", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.zoom_step(false, cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("zoom-fit", "fit", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.zoom_fit(cx)))
                        .into_any_element(),
                );
                v.push(
                    chip("zoom-100", "100%", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.zoom_100(cx)))
                        .into_any_element(),
                );
                v.push(
                    div()
                        .flex_none()
                        .child("click to zoom in · Shift/Alt-click to zoom out · double-click for 100%")
                        .into_any_element(),
                );
            }
        }
        v
    }

    /// Is a brush being used (as opposed to bucket, gradient or liquify)?
    pub(crate) fn brushy(&self) -> bool {
        match self.tool {
            Tool::Brush => matches!(
                self.tools.paint,
                PaintKind::Brush | PaintKind::Eraser | PaintKind::Smudge
            ),
            Tool::Heal | Tool::Clone | Tool::Mask => true,
            _ => false,
        }
    }

    pub(crate) fn brush_settings_panel(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.brush_settings_section;
        let navigation = div().flex().flex_wrap().gap_1().children(
            [
                (
                    BrushSettingsSection::Presets,
                    "brush-settings-presets",
                    "Presets",
                ),
                (BrushSettingsSection::Tip, "brush-settings-tip", "Tip"),
                (
                    BrushSettingsSection::Texture,
                    "brush-settings-texture",
                    "Texture",
                ),
                (
                    BrushSettingsSection::Dynamics,
                    "brush-settings-dynamics",
                    "Dynamics",
                ),
                (
                    BrushSettingsSection::Drawing,
                    "brush-settings-drawing",
                    "Drawing",
                ),
            ]
            .into_iter()
            .map(|(section, id, label)| {
                chip(id, label, current == section, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.brush_settings_section = section;
                        if section == BrushSettingsSection::Presets {
                            this.prepare_presets(cx);
                        }
                        cx.notify();
                    }))
                    .test_support()
            }),
        );
        let controls = if current == BrushSettingsSection::Presets {
            self.presets_view(p, cx)
                .map(IntoElement::into_any_element)
                .into_iter()
                .collect()
        } else {
            self.brush_settings_controls(p, cx)
        };
        div()
            .id("brush-settings-panel")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .w_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(mono("Brush settings", 12., p.ink))
                    .child(
                        chip("brush-settings-close", "Close", false, p)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_sidebar(SidebarTab::History, cx);
                                window.focus(&this.canvas_focus, cx);
                            }))
                            .test_support(),
                    ),
            )
            .child(navigation)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_3()
                    .children(controls),
            )
            .into_any_element()
    }

    /// Settings for the selected brush category.
    fn brush_settings_controls(&mut self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        if !self.brushy() {
            return v;
        }
        let b = self.tools.brush;
        if self.brush_settings_section == BrushSettingsSection::Tip && self.tool != Tool::Heal {
            v.push(self.opt_slider(
                SliderKey::ToolFlow,
                "flow",
                format!("{:.0}%", b.flow * 100.0),
                b.flow,
                (1.0, 100.0, 1.0),
                p,
                cx,
            ));
        }
        v.extend(self.brush_more_options(p, cx));
        if self.brush_settings_section != BrushSettingsSection::Drawing || self.tool == Tool::Mask {
            return v;
        }
        v.push(self.group("symmetry", p));
        let (mx, my) = (self.tools.mirror_x, self.tools.mirror_y);
        v.push(
            tip(
                chip("mirror-x", "mirror ↔", mx, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.tools.mirror_x = !mx;
                    cx.notify();
                })),
                "Also paint each stroke mirrored left to right",
            )
            .into_any_element(),
        );
        v.push(
            tip(
                chip("mirror-y", "mirror ↕", my, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.tools.mirror_y = !my;
                    cx.notify();
                })),
                "Also paint each stroke mirrored top to bottom",
            )
            .into_any_element(),
        );
        let sym = self.tools.symmetry;
        let sym_label = if sym >= 2 {
            format!("radial ×{sym}")
        } else {
            "radial".to_string()
        };
        v.push(
            tip(
                chip("radial-sym", sym_label, sym >= 2, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        // Off → 4 → 6 → 8 → 12 → off.
                        this.tools.symmetry = match sym {
                            0 | 1 => 4,
                            4 => 6,
                            6 => 8,
                            8 => 12,
                            _ => 0,
                        };
                        cx.notify();
                    },
                )),
                "Repeat each stroke around the centre (mandalas); click to cycle 4, 6, 8, 12, off",
            )
            .into_any_element(),
        );
        v.push(self.group("guide", p));
        let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let gk = self.tools.guide.kind.clone();
        let g_on = gk != super::guides::GuideKind::Off;
        v.push(
            tip(
                chip("draw-guide", gk.label(), g_on, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        // Off → grid → isometric → 1/2/3-point → off.
                        this.tools.guide.kind = gk.cycle(w, h);
                        cx.notify();
                    },
                )),
                "Drawing guide over the canvas; click to cycle grid, isometric, 1-, 2-, 3-point perspective, off",
            )
            .into_any_element(),
        );
        if g_on {
            let assist = self.tools.guide.assist;
            v.push(
                tip(
                    chip("draw-assist", "assist", assist, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.tools.guide.assist = !assist;
                            this.set_status(
                                if assist {
                                    "Drawing assist off"
                                } else {
                                    "Drawing assist: strokes follow the guide"
                                },
                                false,
                                cx,
                            );
                            cx.notify();
                        },
                    )),
                    "Strokes snap to the guide's lines",
                )
                .into_any_element(),
            );
        }
        v.push(self.group("stroke", p));
        let al = self.tools.alpha_lock;
        v.push(
            tip(
                chip("alpha-lock", "alpha lock", al, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.tools.alpha_lock = !al;
                        cx.notify();
                    },
                )),
                "Paint only where the layer already has pixels",
            )
            .into_any_element(),
        );
        let qs = self.tools.quick_shape;
        v.push(
            tip(
                chip("quick-shape", "QuickShape", qs, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.tools.quick_shape = !qs;
                        cx.notify();
                    },
                )),
                "Hold still at the end of a stroke to snap it to a line, circle, ellipse or polygon",
            )
            .into_any_element(),
        );
        v
    }

    /// Every remaining brush setting, as sliders and chips.
    fn brush_more_options(&mut self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let b = self.tools.brush;
        let mut v = Vec::new();
        macro_rules! sl {
            ($key:expr, $name:expr, $display:expr, $norm:expr, $spec:expr) => {
                v.push(self.opt_slider($key, $name, $display, $norm, $spec, p, cx));
            };
        }
        if self.brush_settings_section == BrushSettingsSection::Tip {
            sl!(
                SliderKey::ToolSpacing,
                "spacing",
                format!("{:.0}%", b.spacing * 100.0),
                ((b.spacing - 0.02) / 1.98).sqrt(),
                (2.0, 200.0, 1.0)
            );
            sl!(
                SliderKey::ToolRoundness,
                "round",
                format!("{:.0}%", b.roundness * 100.0),
                (b.roundness - 0.05) / 0.95,
                (5.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolAngle,
                "angle",
                format!("{:.0}°", b.angle),
                b.angle / 360.0,
                (0.0, 360.0, 1.0)
            );
            let fp = b.follow_path;
            v.push(
                chip("follow-path", "follow path", fp, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tools.brush.follow_path = !fp;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        if self.brush_settings_section == BrushSettingsSection::Texture {
            for (id, t, k) in [
                ("gr-none", "no grain", GrainKind::None),
                ("gr-paper", "paper", GrainKind::Paper),
                ("gr-canvas", "canvas", GrainKind::Canvas),
                ("gr-chalk", "chalk", GrainKind::Chalk),
                ("gr-speck", "speckle", GrainKind::Speckle),
                ("gr-bristle", "bristle", GrainKind::Bristle),
                ("gr-tone", "screentone", GrainKind::Halftone),
                ("gr-hatch", "hatch", GrainKind::Hatch),
                ("gr-cross", "cross hatch", GrainKind::CrossHatch),
            ] {
                v.push(self.mode_chip(id, t, k, b.grain, p, cx, |e, k, cx| {
                    e.tools.brush.grain = k;
                    if k != GrainKind::None && e.tools.brush.grain_strength == 0.0 {
                        e.tools.brush.grain_strength = 0.7;
                    }
                    cx.notify();
                }));
            }
            if b.grain != GrainKind::None {
                sl!(
                    SliderKey::ToolGrainScale,
                    "grain size",
                    format!("{:.0}px", b.grain_scale),
                    ((b.grain_scale - 1.0) / 63.0).sqrt(),
                    (1.0, 64.0, 1.0)
                );
                sl!(
                    SliderKey::ToolGrainStrength,
                    "grain",
                    format!("{:.0}%", b.grain_strength * 100.0),
                    b.grain_strength,
                    (0.0, 100.0, 1.0)
                );
            }
            sl!(
                SliderKey::ToolWetness,
                "wet",
                format!("{:.0}%", b.wetness * 100.0),
                b.wetness,
                (0.0, 100.0, 1.0)
            );
        }
        if self.brush_settings_section == BrushSettingsSection::Dynamics {
            sl!(
                SliderKey::ToolStabilizer,
                "steady",
                format!("{:.0}%", b.stabilizer * 100.0),
                b.stabilizer,
                (0.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolTaper,
                "taper",
                format!("{:.0}px", b.taper_end),
                (b.taper_end / 300.0).sqrt(),
                (0.0, 300.0, 1.0)
            );
            sl!(
                SliderKey::ToolPressureSize,
                "pressure→size",
                format!("{:.0}%", b.size_pressure * 100.0),
                b.size_pressure,
                (0.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolPressureFlow,
                "pressure→flow",
                format!("{:.0}%", b.flow_pressure * 100.0),
                b.flow_pressure,
                (0.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolSpeed,
                "speed thins",
                format!("{:.0}%", b.speed_thins * 100.0),
                b.speed_thins,
                (0.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolScatter,
                "scatter",
                format!("{:.0}%", b.scatter * 100.0),
                b.scatter,
                (0.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolSizeJitter,
                "size jitter",
                format!("{:.0}%", b.size_jitter * 100.0),
                b.size_jitter,
                (0.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolColorJitter,
                "colour jitter",
                format!("{:.0}%", b.color_jitter * 100.0),
                b.color_jitter,
                (0.0, 100.0, 1.0)
            );
            sl!(
                SliderKey::ToolTilt,
                "tilt",
                format!("{:.0}%", b.tilt * 100.0),
                b.tilt,
                (0.0, 100.0, 1.0)
            );
            // Pressure curve on a log scale: soft (0.25) … linear (1) … firm (4).
            sl!(
                SliderKey::ToolPressureCurve,
                "pressure curve",
                if (b.pressure_curve - 1.0).abs() < 0.05 {
                    "linear".to_string()
                } else if b.pressure_curve < 1.0 {
                    format!("soft {:.2}", b.pressure_curve)
                } else {
                    format!("firm {:.2}", b.pressure_curve)
                },
                (b.pressure_curve.log2() + 2.0) / 4.0,
                (0.0, 100.0, 1.0)
            );
            v.push(mono(crate::tablet::status(), 10., p.muted).into_any_element());
        }
        if self.brush_settings_section == BrushSettingsSection::Drawing {
            v.push(
                mono(
                    format!("blend: {} · choose it in the options bar", b.blend.label()),
                    10.,
                    p.muted,
                )
                .into_any_element(),
            );
        }
        v
    }

    /// Foreground / background swatches at the foot of the tool rail.
    pub(crate) fn swatches(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let [fr, fgc, fb, _] = self.tools.fg;
        let [br, bgc, bb, _] = self.tools.bg;
        div()
            .relative()
            .size(px(40.))
            .child(
                div()
                    .id("bg-swatch")
                    .tab_index(0)
                    .role(Role::Button)
                    .aria_label("Swap foreground and background colours")
                    .focus_visible(|s| s.border_2().border_color(p.accent))
                    .tooltip(|w, cx| gpui_kit::component::tooltip::Tooltip::new("Swap foreground and background colours (X)").build(w, cx))
                    .absolute()
                    .left(px(14.))
                    .top(px(14.))
                    .size(px(24.))
                    .border_1()
                    .border_color(p.ink)
                    .bg(rgb(((br as u32) << 16) | ((bgc as u32) << 8) | bb as u32))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.swap_colors(cx))),
            )
            .child(
                div()
                    .id("fg-swatch")
                    .test_support()
                    .occlude()
                    .tab_index(0)
                    .role(Role::Button)
                    .aria_label("Choose foreground colour")
                    .focus_visible(|s| s.border_2().border_color(p.accent))
                    .absolute()
                    .left(px(2.))
                    .top(px(2.))
                    .size(px(24.))
                    .border_1()
                    .border_color(p.ink)
                    .bg(rgb(((fr as u32) << 16) | ((fgc as u32) << 8) | fb as u32))
                    .cursor_pointer()
                    // ColorDrop: drag the swatch onto the canvas to fill.
                    .on_drag(super::DraggedColor(self.tools.fg), |d, _, _, cx| {
                        cx.new(|_| d.clone())
                    })
                    .tooltip(|w, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "Foreground colour: click to pick, drag onto the canvas to fill an area",
                        )
                        .build(w, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        if !this.tools.picker && this.foreground_edits_text() {
                            this.open_text_colour(window, cx);
                            return;
                        }
                        this.tools.picker = !this.tools.picker;
                        this.tools.hue = rgb_to_hsv(this.tools.fg).0;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn picker(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.tools.picker {
            return None;
        }
        let sv_track = self.tracks.entry(SliderKey::PickerSv).or_default().clone();
        let hue_track = self.tracks.entry(SliderKey::PickerHue).or_default().clone();
        let (h, s, v) = rgb_to_hsv(self.tools.fg);
        let hue = if s < 0.01 { self.tools.hue } else { h };
        let [hr, hg, hb] = hsv_to_rgb(hue, 1.0, 1.0);
        let pure = rgb(((hr as u32) << 16) | ((hg as u32) << 8) | hb as u32);
        let (t1, t2) = (sv_track.clone(), hue_track.clone());
        let hex = format!(
            "#{:02X}{:02X}{:02X}",
            self.tools.fg[0], self.tools.fg[1], self.tools.fg[2]
        );
        const SWATCHES: [u32; 10] = [
            0x0A0A0B, 0xFFFFFF, 0x6E6D68, 0xD93A1E, 0xE8A33B, 0xF2E4C9, 0x4E8A4B, 0x3B6EA8,
            0x7A4EA8, 0xC07A4A,
        ];
        let hue_segments: Vec<AnyElement> = (0..6)
            .map(|i| {
                let a = hsv_to_rgb(i as f32 / 6.0, 1.0, 1.0);
                let b = hsv_to_rgb((i + 1) as f32 / 6.0, 1.0, 1.0);
                let c = |x: [u8; 3]| -> Hsla {
                    rgb(((x[0] as u32) << 16) | ((x[1] as u32) << 8) | x[2] as u32).into()
                };
                div()
                    .flex_1()
                    .h_full()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(c(a), 0.),
                        linear_color_stop(c(b), 1.),
                    ))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                anchored()
                    .position(point(px(62.), px(420.)))
                    .snap_to_window()
                    .child(
                        div()
                            .id("picker")
                            .occlude()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .p(px(10.))
                            .w(px(212.))
                            .bg(p.panel)
                            .border_1()
                            .border_color(p.ink)
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.tools.picker = false;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .id("picker-sv")
                                    .test_support()
                                    .relative()
                                    .w(px(190.))
                                    .h(px(150.))
                                    .bg(pure)
                                    .cursor(CursorStyle::Crosshair)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                            this.begin_colour_drag(window, cx);
                                            this.pick_sv(&t1, e.position, cx);
                                            this.drag = Some(Drag::Tool(ToolDrag::PickSv {
                                                track: t1.clone(),
                                            }));
                                        }),
                                    )
                                    .child(
                                        canvas(
                                            {
                                                let t = sv_track.clone();
                                                move |b, _, _| t.set(Some(b))
                                            },
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .size_full(),
                                    )
                                    .child(div().absolute().size_full().bg(linear_gradient(
                                        90.,
                                        linear_color_stop(gpui_kit::white(), 0.),
                                        linear_color_stop(gpui_kit::white().opacity(0.), 1.),
                                    )))
                                    .child(div().absolute().size_full().bg(linear_gradient(
                                        180.,
                                        linear_color_stop(gpui_kit::black().opacity(0.), 0.),
                                        linear_color_stop(gpui_kit::black(), 1.),
                                    )))
                                    .child(
                                        div()
                                            .absolute()
                                            .left(relative(s))
                                            .top(relative(1.0 - v))
                                            .ml(px(-5.))
                                            .mt(px(-5.))
                                            .size(px(10.))
                                            .border_1()
                                            .border_color(gpui_kit::white()),
                                    ),
                            )
                            .child(
                                div()
                                    .id("picker-hue")
                                    .test_support()
                                    .relative()
                                    .flex()
                                    .w(px(190.))
                                    .h(px(12.))
                                    .cursor(CursorStyle::PointingHand)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                            this.begin_colour_drag(window, cx);
                                            this.pick_hue(&t2, e.position, cx);
                                            this.drag = Some(Drag::Tool(ToolDrag::PickHue {
                                                track: t2.clone(),
                                            }));
                                        }),
                                    )
                                    .child(
                                        canvas(
                                            {
                                                let t = hue_track.clone();
                                                move |b, _, _| t.set(Some(b))
                                            },
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .size_full(),
                                    )
                                    .children(hue_segments)
                                    .child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left(relative(hue))
                                            .ml(px(-2.))
                                            .w(px(4.))
                                            .h_full()
                                            .border_1()
                                            .border_color(p.ink),
                                    ),
                            )
                            .child(div().flex().gap(px(4.)).children(
                                SWATCHES.iter().enumerate().map(|(i, c)| {
                                    let c = *c;
                                    div()
                                        .id(("sw", i))
                                        .test_support()
                                        .size(px(15.))
                                        .border_1()
                                        .border_color(p.line)
                                        .bg(rgb(c))
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.close_text_field(cx);
                                            window.focus(&this.canvas_focus, cx);
                                            this.set_fg(
                                                [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255],
                                                cx,
                                            );
                                        }))
                                }),
                            ))
                            .child(mono(format!("{hex} · x swaps · d resets"), 10., p.muted)),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "tool_lifecycle_tests.rs"]
mod tool_lifecycle_tests;
