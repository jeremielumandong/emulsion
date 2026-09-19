//! The Editor screen: document bar, tool rail, context bar, canvas, status
//! strip, and node panel.

use crate::theme::{self, MONO_FONT, Palette, dim};
use crate::viewport::{self, CanvasBounds, Scene, TileCache, View, Which};
use crate::widgets::{TrackBounds, button, chip, label, mono, slider, track_fraction};

mod canvas_size;
mod history;
mod pen;
mod presets;
mod snap;
mod tools;
mod transform;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::adjust::ParamSpec;
use emulsion_raster::composite::{CompositeTree, level_size, render_tile, tile_to_bgra8};
use emulsion_raster::{Adjustment, BlendMode, Placement, Raster, TileCoord, color};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
pub use history::recovery_dir;
use rayon::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
pub use tools::{PaintKind, SelectShape, ShapeKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Drag to pan the view. The default, so dragging a photo never moves its pixels by surprise.
    Hand,
    Move,
    Select,
    Mask,
    Brush,
    Heal,
    Clone,
    Grade,
    Type,
    Crop,
    Shape,
    /// Vector paths.
    Pen,
}

/// Rail order, glyphs, and whether the tool works yet.
const TOOLS: [(Tool, &str, &str, bool); 12] = [
    (Tool::Hand, "Hand", "✋", true),
    (Tool::Move, "Move", "✥", true),
    (Tool::Select, "Select", "▢", true),
    (Tool::Mask, "Mask", "◐", false),
    (Tool::Brush, "Brush", "✎", true),
    (Tool::Heal, "Heal", "✚", true),
    (Tool::Clone, "Clone", "◎", true),
    (Tool::Grade, "Grade", "◑", false),
    (Tool::Type, "Type", "T", false),
    (Tool::Crop, "Crop", "⌗", true),
    (Tool::Shape, "Shape", "◇", true),
    (Tool::Pen, "Pen", "✒", true),
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum SliderKey {
    Opacity(NodeId),
    Param(NodeId, &'static str),
    Scale(NodeId),
    Rotation(NodeId),
    Compare,
    ToolSize,
    ToolHardness,
    ToolOpacity,
    ToolFlow,
    ToolSpacing,
    ToolRoundness,
    ToolAngle,
    ToolGrainScale,
    ToolGrainStrength,
    ToolWetness,
    ToolStabilizer,
    ToolTaper,
    ToolPressureSize,
    ToolPressureFlow,
    ToolSpeed,
    ToolScatter,
    ToolSizeJitter,
    ToolColorJitter,
    PenWidth,
    Tolerance,
    Feather,
    Straighten,
    PickerSv,
    PickerHue,
}

impl SliderKey {
    /// Keys that edit the document (their drags are one history step).
    fn edits_document(self) -> bool {
        matches!(
            self,
            SliderKey::Opacity(_)
                | SliderKey::Param(..)
                | SliderKey::Scale(_)
                | SliderKey::Rotation(_)
                | SliderKey::PenWidth
        )
    }
}

enum Drag {
    Tool(tools::ToolDrag),
    Pan {
        last: Point<Pixels>,
    },
    Move {
        id: NodeId,
        start_doc: (f64, f64),
        start: Placement,
    },
    Slider {
        key: SliderKey,
        track: TrackBounds,
        min: f32,
        max: f32,
        step: f32,
    },
    /// Dragging a Path node with the Move tool.
    MovePath {
        id: NodeId,
        start_doc: (f64, f64),
        path: Arc<emulsion_raster::vector::Path>,
        style: emulsion_raster::vector::PathStyle,
    },
    /// Free Transform: a handle of the selected pixel node.
    Transform(transform::Grab),
    /// Distort: one corner moves freely; pixels re-project on release.
    Distort {
        id: NodeId,
        corner: usize,
        quad: [(f64, f64); 4],
    },
    /// A guide dragged from a ruler (new) or grabbed on the canvas.
    Guide {
        vertical: bool,
        /// None while over a ruler or off the canvas: dropping removes it.
        pos: Option<f64>,
        existing: Option<usize>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Blend,
    Add,
}

/// What a panel row drag carries.
#[derive(Clone)]
pub struct DraggedNode {
    id: NodeId,
    name: SharedString,
}

impl Render for DraggedNode {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .px(px(10.))
            .py(px(5.))
            .bg(p.ink)
            .text_color(p.paper)
            .text_size(px(12.5))
            .child(self.name.clone())
    }
}

pub struct EditorView {
    pub editor: Editor,
    /// Display name (file stem of what was opened).
    pub name: String,
    /// Where it came from, for Save As suggestions.
    pub source: Option<PathBuf>,
    pub(crate) view: View,
    pub(crate) fit_pending: bool,
    pub(crate) canvas_bounds: CanvasBounds,
    pub(crate) cache: Rc<RefCell<TileCache>>,
    pub(crate) seen_rev: u64,
    pub(crate) gen_counter: u64,
    pub(crate) render_gen: u64,
    pub(crate) tree: Arc<CompositeTree>,
    pub(crate) seen_commit: u64,
    pub(crate) before_gen: u64,
    pub(crate) before_tree: Option<Arc<CompositeTree>>,
    pub selected: Option<NodeId>,
    pub(crate) tool: Tool,
    drag: Option<Drag>,
    pub(crate) compare: f32,
    pub(crate) rulers: bool,
    pub(crate) space_held: bool,
    pub(crate) renaming: Option<(NodeId, Entity<InputState>, Subscription)>,
    menu: Option<Menu>,
    tracks: HashMap<SliderKey, TrackBounds>,
    pub(crate) thumbs: HashMap<usize, Arc<RenderImage>>,
    pub(crate) checker: (u8, u8),
    pub focus: FocusHandle,
    pub(crate) canvas_focus: FocusHandle,
    pub(crate) panel_focus: FocusHandle,
    pub status: Option<(SharedString, bool)>,
    pub(crate) assistant: crate::assistant::Assistant,
    pub(crate) ask: Option<crate::assistant::AskBar>,
    pub(crate) suggestions: Vec<emulsion_ai::suggest::Suggestion>,
    pub(crate) suggest_rev: u64,
    pub(crate) suggest_busy: bool,
    tools: tools::ToolState,
    pub(crate) history: history::HistoryState,
    /// Snap moves to guides, edges and centres.
    pub(crate) snap: bool,
    /// Ctrl held during a drag: move freely.
    pub(crate) snap_bypass: bool,
    pub(crate) snap_lines: Vec<(bool, f64)>,
    pub(crate) size_panel: Option<canvas_size::SizePanel>,
    pub(crate) transform_fields: Option<transform::TransformFields>,
    pub(crate) presets: presets::PresetState,
    /// Shift held during a drag: free aspect, or 15° rotation steps.
    pub(crate) drag_shift: bool,
}

impl EditorView {
    pub fn new(
        doc: Document,
        graph: Option<emulsion_core::graph::Graph>,
        path: Option<PathBuf>,
        source: Option<PathBuf>,
        name: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected = doc.nodes.last().map(|n| n.id);
        Self::start_ants(cx);
        Self::start_autosave(cx);
        let editor = match graph {
            Some(g) => Editor::with_graph(doc, path, g),
            None => Editor::new(doc, path),
        };
        let tree = Arc::new(editor.doc.composite_tree());
        let rev = editor.revision;
        let commit = editor.committed_revision;
        Self {
            editor,
            name,
            source,
            view: View::default(),
            fit_pending: true,
            canvas_bounds: Default::default(),
            cache: Default::default(),
            seen_rev: rev,
            gen_counter: 1,
            render_gen: 1,
            tree,
            seen_commit: commit,
            before_gen: 0,
            before_tree: None,
            selected,
            tool: Tool::Hand,
            drag: None,
            compare: 0.0,
            rulers: true,
            space_held: false,
            renaming: None,
            menu: None,
            tracks: HashMap::new(),
            thumbs: HashMap::new(),
            checker: theme::palette(cx).checker,
            focus: cx.focus_handle(),
            canvas_focus: cx.focus_handle(),
            panel_focus: cx.focus_handle(),
            status: None,
            assistant: Default::default(),
            ask: None,
            suggestions: Vec::new(),
            suggest_rev: 0,
            suggest_busy: false,
            tools: tools::ToolState::default(),
            history: Default::default(),
            snap: true,
            snap_bypass: false,
            snap_lines: Vec::new(),
            size_panel: None,
            transform_fields: None,
            presets: Default::default(),
            drag_shift: false,
        }
    }

    /// Animate the marching ants while there is a selection.
    fn start_ants(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(400))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    if this.editor.doc.selection.is_some() {
                        this.tools.ants_phase = !this.tools.ants_phase;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn set_status(
        &mut self,
        msg: impl Into<SharedString>,
        error: bool,
        cx: &mut Context<Self>,
    ) {
        self.status = Some((msg.into(), error));
        cx.notify();
    }

    // ── Document changes ────────────────────────────────────────────────

    pub fn execute(&mut self, cmd: Command, cx: &mut Context<Self>) -> Option<NodeId> {
        match self.editor.execute(cmd) {
            Ok(created) => {
                self.after_change(cx);
                created
            }
            Err(e) => {
                self.set_status(e.to_string(), true, cx);
                None
            }
        }
    }

    pub(crate) fn after_change(&mut self, cx: &mut Context<Self>) {
        if let Some(sel) = self.selected
            && self.editor.doc.node(sel).is_none()
        {
            self.selected = self.editor.doc.nodes.last().map(|n| n.id);
        }
        cx.notify();
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if self.editor.undo() {
            self.after_change(cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.editor.redo() {
            self.after_change(cx);
        }
    }

    fn undo_to(&mut self, steps: usize, cx: &mut Context<Self>) {
        for _ in 0..steps {
            self.editor.undo();
        }
        self.after_change(cx);
    }

    /// Refresh render trees when the document or its commit point moved.
    fn sync_trees(&mut self, cx: &mut Context<Self>) {
        let checker = theme::palette(cx).checker;
        if checker != self.checker {
            self.checker = checker;
            self.cache.borrow_mut().clear();
            self.gen_counter += 1;
            self.render_gen = self.gen_counter;
            self.seen_commit = u64::MAX; // force the before tree to rebuild too
        }
        if self.editor.revision != self.seen_rev {
            self.seen_rev = self.editor.revision;
            let old = self.render_gen;
            self.gen_counter += 1;
            self.render_gen = self.gen_counter;
            self.tree = Arc::new(self.editor.doc.composite_tree());
            // Keep every tile the change did not touch.
            match self.editor.take_dirty() {
                emulsion_core::Dirty::Nothing => {
                    self.cache.borrow_mut().retag(old, self.render_gen, None)
                }
                emulsion_core::Dirty::Rect(r) => {
                    self.cache.borrow_mut().retag(old, self.render_gen, Some(r))
                }
                emulsion_core::Dirty::All => {}
            }
        }
        if self.editor.committed_revision != self.seen_commit {
            self.seen_commit = self.editor.committed_revision;
            self.gen_counter += 1;
            self.before_gen = self.gen_counter;
            self.before_tree = None;
            self.cache.borrow_mut().clear_which(Which::Before);
        }
        let differs = self.editor.differs_from_base();
        if self.compare > 0.0 && differs && self.before_tree.is_none() {
            self.before_tree = Some(Arc::new(self.editor.committed.composite_tree()));
        }
    }

    fn before_active(&self) -> bool {
        self.compare > 0.0 && self.editor.differs_from_base() && self.before_tree.is_some()
    }

    fn dispatch_render(&mut self, cx: &mut Context<Self>) {
        let batch = {
            let mut c = self.cache.borrow_mut();
            if c.in_flight {
                return;
            }
            let before = self.before_active().then_some(self.before_gen);
            let b = c.take_batch(self.render_gen, before);
            if b.is_empty() {
                return;
            }
            c.in_flight = true;
            b
        };
        let cur = self.tree.clone();
        let before = self.before_tree.clone();
        let (light, dark) = self.checker;
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let n = batch.len();
            let out: Vec<(viewport::Request, Vec<u8>)> = cx
                .background_spawn(async move {
                    batch
                        .into_par_iter()
                        .filter_map(|r| {
                            let tree = match r.key.which {
                                Which::Current => &cur,
                                Which::Before => before.as_ref()?,
                            };
                            let t =
                                render_tile(tree, r.key.level, TileCoord::new(r.key.x, r.key.y));
                            let lsz = level_size(tree.width, tree.height, r.key.level);
                            let origin = (r.key.x as i64 * 256, r.key.y as i64 * 256);
                            Some((r, tile_to_bgra8(&t, origin, lsz, 8, light, dark)))
                        })
                        .collect()
                })
                .await;
            let elapsed = started.elapsed();
            this.update(cx, |this, cx| {
                {
                    let mut c = this.cache.borrow_mut();
                    for (r, bytes) in out {
                        c.insert(
                            r.key,
                            r.rev,
                            Arc::new(viewport::bgra_image(256, 256, bytes)),
                        );
                    }
                    c.in_flight = false;
                    c.last_batch = Some((n, elapsed));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ── View ────────────────────────────────────────────────────────────

    fn canvas_bounds(&self) -> Option<Bounds<Pixels>> {
        self.canvas_bounds.get()
    }

    pub fn zoom_step(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
        if let Some(b) = self.canvas_bounds() {
            let c = b.center();
            self.view
                .step(zoom_in, (f32::from(c.x) as f64, f32::from(c.y) as f64), &b);
            cx.notify();
        }
    }

    pub fn zoom_fit(&mut self, cx: &mut Context<Self>) {
        if let Some(b) = self.canvas_bounds() {
            self.view
                .fit(self.editor.doc.width, self.editor.doc.height, &b);
            cx.notify();
        }
    }

    pub fn zoom_100(&mut self, cx: &mut Context<Self>) {
        if let Some(b) = self.canvas_bounds() {
            let c = b.center();
            let f = 1.0 / self.view.zoom;
            self.view
                .zoom_at(f, (f32::from(c.x) as f64, f32::from(c.y) as f64), &b);
            cx.notify();
        }
    }

    pub fn rotate(&mut self, degrees: f64, cx: &mut Context<Self>) {
        self.view.rotation = if degrees == 0.0 {
            0.0
        } else {
            (self.view.rotation + degrees).rem_euclid(360.0)
        };
        cx.notify();
    }

    pub fn toggle_rulers(&mut self, cx: &mut Context<Self>) {
        self.rulers = !self.rulers;
        cx.notify();
    }

    // ── Node operations ─────────────────────────────────────────────────

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.execute(Command::RemoveNode { id }, cx);
        }
    }

    pub fn duplicate_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected
            && let Some(new) = self.execute(Command::DuplicateNode { id }, cx)
        {
            self.selected = Some(new);
        }
    }

    pub fn group_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            let n = self
                .editor
                .doc
                .nodes
                .iter()
                .filter(|n| n.is_group())
                .count()
                + 1;
            if let Some(g) = self.execute(
                Command::Group {
                    ids: vec![id],
                    name: format!("Group {n}"),
                },
                cx,
            ) {
                self.selected = Some(g);
            }
        }
    }

    pub fn ungroup_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected
            && self.editor.doc.node(id).is_some_and(|n| n.is_group())
        {
            let first = self.editor.doc.children(Some(id)).last().copied();
            self.execute(Command::Ungroup { id }, cx);
            self.selected = first;
        }
    }

    /// Move the selection one step up (`up`) or down among its siblings.
    pub fn shift_selected(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let sib = self.editor.doc.children(n.parent);
        let i = sib.iter().position(|s| *s == id).unwrap_or(0);
        let target = if up { i + 1 } else { i.saturating_sub(1) };
        if target == i || target >= sib.len() {
            return;
        }
        // Removing first shifts indices above us down by one.
        self.execute(
            Command::MoveNode {
                id,
                slot: Slot {
                    parent: n.parent,
                    index: target,
                },
            },
            cx,
        );
    }

    pub fn toggle_selected_visible(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected
            && let Some(n) = self.editor.doc.node(id)
        {
            let visible = !n.visible;
            self.execute(Command::SetVisible { id, visible }, cx);
        }
    }

    /// Where new nodes go: just above the selection, in its parent.
    fn insertion_slot(&self) -> Slot {
        match self.selected.and_then(|id| self.editor.doc.node(id)) {
            Some(n) => {
                let sib = self.editor.doc.children(n.parent);
                let i = sib.iter().position(|s| *s == n.id).unwrap_or(sib.len());
                Slot {
                    parent: n.parent,
                    index: i + 1,
                }
            }
            None => Slot::TOP,
        }
    }

    fn add_node(&mut self, node: Node, cx: &mut Context<Self>) {
        let slot = self.insertion_slot();
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        ) {
            self.selected = Some(id);
        }
        self.menu = None;
    }

    /// Drop `dragged` onto the row for `target`: into a group, otherwise
    /// just above the target in its parent.
    fn drop_on(&mut self, dragged: NodeId, target: Option<NodeId>, cx: &mut Context<Self>) {
        if Some(dragged) == target {
            return;
        }
        let doc = &self.editor.doc;
        let slot = match target.and_then(|t| doc.node(t)) {
            None => Slot::TOP,
            Some(t) if t.is_group() => Slot::top_of(Some(t.id)),
            Some(t) => {
                let sib: Vec<NodeId> = doc
                    .children(t.parent)
                    .into_iter()
                    .filter(|s| *s != dragged)
                    .collect();
                let i = sib.iter().position(|s| *s == t.id).unwrap_or(sib.len());
                Slot {
                    parent: t.parent,
                    index: i + 1,
                }
            }
        };
        self.execute(Command::MoveNode { id: dragged, slot }, cx);
        self.selected = Some(dragged);
    }

    fn start_rename(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let name = n.name.clone();
        let state = cx.new(|cx| InputState::new(window, cx).default_value(name));
        state.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.commit_rename(cx);
            }
        });
        self.renaming = Some((id, state, sub));
        cx.notify();
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        if let Some((id, state, _)) = self.renaming.take() {
            let name = state.read(cx).value().to_string();
            self.execute(Command::Rename { id, name }, cx);
        }
    }

    // ── Pointer ─────────────────────────────────────────────────────────

    fn canvas_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.canvas_focus, cx);
        self.menu = None;
        if e.button == MouseButton::Left && !self.space_held {
            if let Some(vertical) = self.ruler_hit(e.position) {
                self.drag = Some(Drag::Guide {
                    vertical,
                    pos: None,
                    existing: None,
                });
                cx.notify();
                return;
            }
            if matches!(self.tool, Tool::Move | Tool::Hand)
                && let Some(i) = self.guide_hit(e.position)
            {
                let g = self.editor.doc.guides[i];
                self.drag = Some(Drag::Guide {
                    vertical: g.vertical,
                    pos: Some(g.pos),
                    existing: Some(i),
                });
                cx.notify();
                return;
            }
        }
        let pan = e.button == MouseButton::Middle
            || (e.button == MouseButton::Left && (self.space_held || self.tool == Tool::Hand));
        if pan {
            self.drag = Some(Drag::Pan { last: e.position });
            cx.notify();
            return;
        }
        if e.button != MouseButton::Left {
            return;
        }
        if self.tool != Tool::Move {
            self.tool_down(e, cx);
            return;
        }
        if self.transform_down(e) {
            cx.notify();
            return;
        }
        let Some(b) = self.canvas_bounds() else {
            return;
        };
        let Some(id) = self.selected else { return };
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        if n.locked {
            self.set_status("That node is locked.", false, cx);
            return;
        }
        if let NodeKind::Raster { placement, .. } = &n.kind {
            let start = *placement;
            let d = self.view.screen_to_doc(
                (
                    f32::from(e.position.x) as f64,
                    f32::from(e.position.y) as f64,
                ),
                &b,
            );
            self.editor.begin("Move");
            self.drag = Some(Drag::Move {
                id,
                start_doc: d,
                start,
            });
        } else if let NodeKind::Path { path, style, .. } = &n.kind {
            let d = self.view.screen_to_doc(
                (
                    f32::from(e.position.x) as f64,
                    f32::from(e.position.y) as f64,
                ),
                &b,
            );
            let (path, style) = (path.clone(), *style);
            self.editor.begin("Move path");
            self.drag = Some(Drag::MovePath {
                id,
                start_doc: d,
                path,
                style,
            });
        } else {
            self.set_status("Select a pixel node or a path to move it.", false, cx);
        }
    }

    fn drag_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let inside = self.canvas_bounds().is_some_and(|b| b.contains(&pos));
        let wants_pointer = matches!(self.tool, Tool::Brush | Tool::Heal | Tool::Clone)
            || !self.tools.polygon.is_empty()
            || (self.tool == Tool::Pen && self.tools.pen.building.is_some());
        let pointer = inside.then_some(pos);
        if wants_pointer && pointer != self.tools.pointer {
            self.tools.pointer = pointer;
            if self.drag.is_none()
                && !self.tools.polygon.is_empty()
                && let Some(d) = pointer.and_then(|p| self.doc_point(p))
            {
                self.magnetic_track(d, cx);
            }
            cx.notify();
        }
        let Some(drag) = &self.drag else { return };
        match drag {
            Drag::Tool(_) => self.tool_move(pos, cx),
            Drag::Pan { last } => {
                let d = pos - *last;
                self.view.pan(f32::from(d.x) as f64, f32::from(d.y) as f64);
                self.drag = Some(Drag::Pan { last: pos });
                cx.notify();
            }
            Drag::Move {
                id,
                start_doc,
                start,
            } => {
                let Some(b) = self.canvas_bounds() else {
                    return;
                };
                let d = self
                    .view
                    .screen_to_doc((f32::from(pos.x) as f64, f32::from(pos.y) as f64), &b);
                let (id, start) = (*id, *start);
                let (dx, dy) = self.snap_move(id, &start, d.0 - start_doc.0, d.1 - start_doc.1);
                let mut p = start;
                p.x = (start.x + dx).round();
                p.y = (start.y + dy).round();
                self.execute(Command::SetPlacement { id, placement: p }, cx);
            }
            Drag::MovePath {
                id,
                start_doc,
                path,
                style,
            } => {
                let Some(b) = self.canvas_bounds() else {
                    return;
                };
                let d = self
                    .view
                    .screen_to_doc((f32::from(pos.x) as f64, f32::from(pos.y) as f64), &b);
                let (dx, dy) = ((d.0 - start_doc.0).round(), (d.1 - start_doc.1).round());
                let (id, style, path) = (*id, *style, path.clone());
                let (dx, dy) = self.snap_path_move(&path, &style, dx, dy);
                let mut p = (*path).clone();
                p.translate(dx, dy);
                self.execute(
                    Command::SetPath {
                        id,
                        path: Arc::new(p),
                        style,
                    },
                    cx,
                );
            }
            Drag::Transform(g) => {
                let g = *g;
                if let Some(d) = self.doc_point(pos) {
                    self.transform_move(g, d, cx);
                }
            }
            Drag::Distort { corner, .. } => {
                let corner = *corner;
                if let Some(d) = self.doc_point(pos) {
                    self.distort_move(corner, d, cx);
                }
            }
            Drag::Guide {
                vertical, existing, ..
            } => {
                let (vertical, existing) = (*vertical, *existing);
                let pos = self.guide_position(vertical, pos);
                self.drag = Some(Drag::Guide {
                    vertical,
                    pos,
                    existing,
                });
                cx.notify();
            }
            Drag::Slider {
                key,
                track,
                min,
                max,
                step,
            } => {
                if let Some(f) = track_fraction(track, pos.x) {
                    let v = snap(min + f * (max - min), *step);
                    let key = *key;
                    self.apply_slider(key, v, cx);
                }
            }
        }
    }

    fn drag_end(&mut self, cx: &mut Context<Self>) {
        self.snap_lines.clear();
        match self.drag.take() {
            None => return,
            Some(Drag::Distort { id, quad, .. }) => self.finish_distort(id, quad, cx),
            Some(Drag::Move { .. })
            | Some(Drag::MovePath { .. })
            | Some(Drag::Slider { .. })
            | Some(Drag::Transform(_)) => {
                if self.editor.in_transaction() {
                    self.editor.end();
                }
            }
            Some(Drag::Pan { .. }) => {}
            Some(Drag::Tool(t)) => self.tool_up(t, cx),
            Some(Drag::Guide {
                vertical,
                pos,
                existing,
            }) => self.drop_guide(vertical, pos, existing, cx),
        }
        cx.notify();
    }

    fn scroll(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(b) = self.canvas_bounds() else {
            return;
        };
        let d = e.delta.pixel_delta(px(20.));
        let (dx, dy) = (f32::from(d.x) as f64, f32::from(d.y) as f64);
        if e.modifiers.control || e.modifiers.alt || e.modifiers.platform {
            let f = (dy * 0.004).exp();
            self.view.zoom_at(
                f,
                (
                    f32::from(e.position.x) as f64,
                    f32::from(e.position.y) as f64,
                ),
                &b,
            );
        } else if e.modifiers.shift {
            self.view.pan(dy, dx);
        } else {
            self.view.pan(dx, dy);
        }
        cx.notify();
    }

    fn slider_down(
        &mut self,
        key: SliderKey,
        spec: (f32, f32, f32),
        e: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let track = self.tracks.entry(key).or_default().clone();
        let (min, max, step) = spec;
        if key.edits_document() {
            let name = match key {
                SliderKey::Opacity(_) => "Opacity".to_string(),
                SliderKey::Param(_, k) => k.replace('_', " "),
                SliderKey::Scale(_) => "Scale".into(),
                SliderKey::PenWidth => "Stroke width".into(),
                _ => "Rotate".into(),
            };
            self.editor.begin(name);
        }
        if let Some(f) = track_fraction(&track, e.position.x) {
            self.apply_slider(key, snap(min + f * (max - min), step), cx);
        }
        self.drag = Some(Drag::Slider {
            key,
            track,
            min,
            max,
            step,
        });
    }

    fn apply_slider(&mut self, key: SliderKey, v: f32, cx: &mut Context<Self>) {
        match key {
            SliderKey::ToolSize => {
                // The track is square-root scaled so small sizes get room.
                let f = ((v - 1.0) / 499.0).clamp(0.0, 1.0);
                self.tools.brush.size = (1.0 + f * f * 499.0).round().max(1.0);
                cx.notify();
            }
            SliderKey::ToolHardness => {
                self.tools.brush.hardness = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolOpacity => {
                self.tools.brush.opacity = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolFlow => {
                self.tools.brush.flow = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolSpacing => {
                let f = ((v - 2.0) / 198.0).clamp(0.0, 1.0);
                self.tools.brush.spacing = 0.02 + f * f * 1.98;
                cx.notify();
            }
            SliderKey::ToolRoundness => {
                self.tools.brush.roundness = (v / 100.0).clamp(0.05, 1.0);
                cx.notify();
            }
            SliderKey::ToolAngle => {
                self.tools.brush.angle = v.rem_euclid(360.0);
                cx.notify();
            }
            SliderKey::ToolGrainScale => {
                let f = ((v - 1.0) / 63.0).clamp(0.0, 1.0);
                self.tools.brush.grain_scale = 1.0 + f * f * 63.0;
                cx.notify();
            }
            SliderKey::ToolGrainStrength => {
                self.tools.brush.grain_strength = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolWetness => {
                self.tools.brush.wetness = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolStabilizer => {
                self.tools.brush.stabilizer = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolTaper => {
                let f = (v / 300.0).clamp(0.0, 1.0);
                let t = (f * f * 300.0).round();
                self.tools.brush.taper_end = t;
                self.tools.brush.taper_start = (t * 0.7).round();
                cx.notify();
            }
            SliderKey::ToolPressureSize => {
                self.tools.brush.size_pressure = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolPressureFlow => {
                self.tools.brush.flow_pressure = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolSpeed => {
                self.tools.brush.speed_thins = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolScatter => {
                self.tools.brush.scatter = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolSizeJitter => {
                self.tools.brush.size_jitter = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolColorJitter => {
                self.tools.brush.color_jitter = v / 100.0;
                cx.notify();
            }
            SliderKey::PenWidth => {
                self.tools.pen.width = v;
                self.pen_restyle(cx);
                cx.notify();
            }
            SliderKey::Tolerance => {
                self.tools.tolerance = v as u8;
                cx.notify();
            }
            SliderKey::Feather => {
                self.tools.feather = v;
                cx.notify();
            }
            SliderKey::Straighten => {
                self.tools.straighten = v;
                cx.notify();
            }
            SliderKey::PickerSv | SliderKey::PickerHue => {}
            SliderKey::Compare => {
                self.compare = v / 100.0;
                cx.notify();
            }
            SliderKey::Opacity(id) => {
                self.execute(
                    Command::SetOpacity {
                        id,
                        opacity: v / 100.0,
                    },
                    cx,
                );
            }
            SliderKey::Param(id, k) => {
                self.execute(
                    Command::SetParam {
                        id,
                        key: k.to_string(),
                        value: v,
                    },
                    cx,
                );
            }
            SliderKey::Scale(id) | SliderKey::Rotation(id) => {
                let Some(NodeKind::Raster { raster, placement }) =
                    self.editor.doc.node(id).map(|n| &n.kind)
                else {
                    return;
                };
                let mut p = *placement;
                if let SliderKey::Scale(_) = key {
                    // Scale about the placed centre.
                    let (w, h) = (raster.width() as f64, raster.height() as f64);
                    let (cx0, cy0) = (p.x + w * p.scale_x / 2.0, p.y + h * p.scale_y / 2.0);
                    let s = (v as f64 / 100.0).max(0.01);
                    p.scale_x = s * p.scale_x.signum();
                    p.scale_y = s * p.scale_y.signum();
                    p.x = cx0 - w * p.scale_x / 2.0;
                    p.y = cy0 - h * p.scale_y / 2.0;
                } else {
                    p.rotation = v as f64;
                }
                self.execute(Command::SetPlacement { id, placement: p }, cx);
            }
        }
    }

    // ── Rendering helpers ───────────────────────────────────────────────

    fn thumb(&mut self, raster: &Arc<Raster>) -> Arc<RenderImage> {
        let key = Arc::as_ptr(raster) as usize;
        if let Some(t) = self.thumbs.get(&key) {
            return t.clone();
        }
        if self.thumbs.len() > 256 {
            let mut c = self.cache.borrow_mut();
            for (_, img) in self.thumbs.drain() {
                c.to_drop.push(img);
            }
        }
        const N: u32 = 40;
        let mut level = 0;
        while raster.level_size(level).0.max(raster.level_size(level).1) > 256
            && level < raster.max_level()
        {
            level += 1;
        }
        let tile = raster.tile(level, TileCoord::new(0, 0));
        let (lw, lh) = raster.level_size(level);
        let s = lw.max(lh) as f64 / N as f64;
        let (ox, oy) = (
            (N as f64 - lw as f64 / s) / 2.0,
            (N as f64 - lh as f64 / s) / 2.0,
        );
        let (light, dark) = self.checker;
        let mut out = vec![0u8; (N * N * 4) as usize];
        for y in 0..N {
            for x in 0..N {
                let bg = if ((x / 5) + (y / 5)) % 2 == 0 {
                    light
                } else {
                    dark
                };
                let bgl = color::SRGB8_TO_LINEAR[bg as usize];
                let sx = ((x as f64 + 0.5 - ox) * s).floor();
                let sy = ((y as f64 + 0.5 - oy) * s).floor();
                let p = if sx >= 0.0 && sy >= 0.0 && (sx as u32) < lw && (sy as u32) < lh {
                    tile.as_ref()
                        .map(|t| color::px_to_f(t[(sy as u32 * 256 + sx as u32) as usize]))
                        .unwrap_or([0.0; 4])
                } else {
                    [0.0; 4]
                };
                let k = 1.0 - p[3];
                let i = ((y * N + x) * 4) as usize;
                out[i] = color::linear_to_srgb8(p[2] + bgl * k);
                out[i + 1] = color::linear_to_srgb8(p[1] + bgl * k);
                out[i + 2] = color::linear_to_srgb8(p[0] + bgl * k);
                out[i + 3] = 255;
            }
        }
        let img = Arc::new(viewport::bgra_image(N, N, out));
        self.thumbs.insert(key, img.clone());
        img
    }
}

fn snap(v: f32, step: f32) -> f32 {
    if step <= 0.0 {
        v
    } else {
        (v / step).round() * step
    }
}

// ── Render ──────────────────────────────────────────────────────────────

impl EditorView {
    fn doc_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let d = &self.editor.doc;
        let depth = if d.source_depth == 16 {
            "16 bit"
        } else {
            "8 bit"
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(13.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(9.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.name.clone()),
                    )
                    .child(mono(
                        format!("{}×{} · {depth}", d.width, d.height),
                        10.5,
                        p.muted,
                    )),
            )
            .child(self.branch_badge(p, cx))
            .when(self.editor.is_modified(), |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .px(px(9.))
                        .py(px(4.))
                        .border_1()
                        .border_color(p.ink)
                        .bg(p.panel)
                        .child(div().size(px(6.)).rounded_full().bg(p.accent))
                        .child(mono("unsaved changes", 10., p.ink)),
                )
            })
            .child(div().flex_1())
            .child(
                button("save", "Save", false, p).on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::Save), cx);
                })),
            )
            .child(
                button("export", "Export", true, p).on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::Export), cx);
                })),
            )
    }

    fn tool_rail(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .w(dim::TOOL_RAIL_W)
            .py(px(8.))
            .gap(px(2.))
            .border_r_1()
            .border_color(p.line)
            .children(TOOLS.iter().map(|(tool, name, glyph, enabled)| {
                let on = *tool == self.tool;
                let (tool, name, enabled) = (*tool, *name, *enabled);
                let ink = p.ink;
                div()
                    .id(SharedString::from(name))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(dim::TOOL_BTN_W)
                    .h(dim::TOOL_BTN_H)
                    .border_1()
                    .border_color(if on { p.ink } else { transparent_black() })
                    .bg(if on { p.ink } else { transparent_black() })
                    .text_color(if on {
                        p.paper
                    } else if enabled {
                        p.ink
                    } else {
                        p.muted.opacity(0.45)
                    })
                    .font_family(MONO_FONT)
                    .text_size(px(14.))
                    .when(enabled && !on, |d| d.hover(move |s| s.border_color(ink)))
                    .cursor(if enabled {
                        CursorStyle::PointingHand
                    } else {
                        CursorStyle::Arrow
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if enabled {
                            this.set_tool(tool, cx);
                        } else {
                            this.set_status(format!("{name} is not available yet."), false, cx);
                        }
                    }))
                    .child(*glyph)
            }))
            .child(div().flex_1())
            .child(self.swatches(p, cx))
    }

    fn context_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tool = TOOLS
            .iter()
            .find(|t| t.0 == self.tool)
            .map(|t| t.1)
            .unwrap_or("Move");
        let options = self.tool_options(p, cx);
        let zoom = format!("{:.0}%", self.view.zoom * 100.0);
        let rot = format!("{:.0}°", self.view.rotation);
        let can_compare = self.editor.differs_from_base();
        let track = self.tracks.entry(SliderKey::Compare).or_default().clone();
        let compare = self.compare;
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .overflow_hidden()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .py(px(8.))
            .border_b_1()
            .border_color(p.line)
            .font_family(MONO_FONT)
            .text_size(px(10.5))
            .text_color(p.muted)
            .child(div().text_color(p.ink).child(tool.to_uppercase()))
            .children(options)
            .child(div().flex_1().min_w(px(8.)))
            .child(
                chip("zoom", zoom, false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_100(cx))),
            )
            .child(
                chip("fit", "fit", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_fit(cx))),
            )
            .child(
                chip("rot", rot, self.view.rotation != 0.0, p)
                    .on_click(cx.listener(|this, _, _, cx| this.rotate(0.0, cx))),
            )
            .child(
                chip("rulers", "rulers", self.rulers, p)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_rulers(cx))),
            )
            .child(
                chip("snap", "snap", self.snap, p).on_click(cx.listener(|this, _, _, cx| {
                    this.snap = !this.snap;
                    cx.notify();
                })),
            )
            .when(!self.editor.doc.guides.is_empty(), |d| {
                d.child(
                    chip("clear-guides", "clear guides", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.clear_guides(cx))),
                )
            })
            .child(
                div()
                    .whitespace_nowrap()
                    .text_color(if can_compare {
                        p.muted
                    } else {
                        p.muted.opacity(0.5)
                    })
                    .child("before / after"),
            )
            .child(div().w(dim::COMPARE_SLIDER_W).flex_none().child(slider(
                "compare",
                compare,
                track,
                p,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.slider_down(SliderKey::Compare, (0.0, 100.0, 1.0), e, cx)
                }),
            )))
            .child(div().w(px(34.)).child(format!("{:.0}%", compare * 100.0)))
    }

    fn canvas_area(
        &mut self,
        p: &Palette,
        scale_factor: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let overlay = self.overlay(scale_factor);
        let accent = p.accent;
        let view_for_overlay = self.view;
        // Fit once the canvas has been laid out.
        if self.fit_pending
            && let Some(b) = self.canvas_bounds()
        {
            self.view
                .fit(self.editor.doc.width, self.editor.doc.height, &b);
            self.fit_pending = false;
        }
        let max_level = {
            let m = self.editor.doc.width.max(self.editor.doc.height).max(1);
            31 - m.leading_zeros()
        };
        let before = self
            .before_active()
            .then_some((self.before_gen, self.compare));
        let scene = Scene {
            view: self.view,
            doc_size: (self.editor.doc.width, self.editor.doc.height),
            max_level,
            rev: self.render_gen,
            before,
            stage: p.stage,
            ink: p.ink,
            accent: p.accent,
            rulers: self.rulers,
        };
        let scene2 = scene.clone();
        let cache = self.cache.clone();
        let cache2 = self.cache.clone();
        let bounds_cell = self.canvas_bounds.clone();
        let fit_pending = self.fit_pending;
        let weak = cx.entity().downgrade();
        let (w1, w2, w3) = (weak.clone(), weak.clone(), weak.clone());
        let cursor = match (&self.drag, self.space_held) {
            (Some(Drag::Pan { .. }), _) => CursorStyle::ClosedHand,
            (Some(Drag::Guide { vertical: true, .. }), _) => CursorStyle::ResizeLeftRight,
            (
                Some(Drag::Guide {
                    vertical: false, ..
                }),
                _,
            ) => CursorStyle::ResizeUpDown,
            (_, true) => CursorStyle::OpenHand,
            _ if self.tool == Tool::Hand => CursorStyle::OpenHand,
            _ if self.tool == Tool::Move => CursorStyle::Arrow,
            _ => CursorStyle::Crosshair,
        };
        div()
            .id("canvas")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .track_focus(&self.canvas_focus)
            .key_context("Canvas")
            .cursor(cursor)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e, window, cx| this.canvas_down(e, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, e, window, cx| this.canvas_down(e, window, cx)),
            )
            .on_scroll_wheel(cx.listener(|this, e, _, cx| this.scroll(e, cx)))
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                if let Some(b) = this.canvas_bounds() {
                    this.view.zoom_at(
                        1.0 + e.delta as f64,
                        (
                            f32::from(e.position.x) as f64,
                            f32::from(e.position.y) as f64,
                        ),
                        &b,
                    );
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "space" && !this.space_held {
                    this.space_held = true;
                    cx.notify();
                }
            }))
            .on_key_up(cx.listener(|this, e: &KeyUpEvent, _, cx| {
                if e.keystroke.key == "space" {
                    this.space_held = false;
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    move |b, window, cx| {
                        bounds_cell.set(Some(b));
                        if fit_pending {
                            cx.defer(move |cx| {
                                w1.update(cx, |_, cx| cx.notify()).ok();
                            });
                            return None;
                        }
                        let plan = viewport::prepaint(
                            &scene,
                            &mut cache.borrow_mut(),
                            b,
                            window.scale_factor(),
                        );
                        if !cache.borrow().queue.is_empty() {
                            cx.defer(move |cx| {
                                w1.update(cx, |this, cx| this.dispatch_render(cx)).ok();
                            });
                        }
                        Some(plan)
                    },
                    move |bounds, plan, window, cx| {
                        if let Some(plan) = plan {
                            viewport::paint(plan, &scene2, &cache2, window, cx);
                        }
                        tools::paint_overlay(&overlay, &view_for_overlay, bounds, accent, window);
                        // Drags continue outside the canvas, so listen window-wide.
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                w2.update(cx, |this, cx| {
                                    this.snap_bypass = e.modifiers.control;
                                    this.drag_shift = e.modifiers.shift;
                                    this.drag_move(e.position, cx)
                                })
                                .ok();
                            }
                        });
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                w3.update(cx, |this, cx| this.drag_end(cx)).ok();
                            }
                        });
                    },
                )
                .size_full(),
            )
    }

    fn status_strip(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let n = self.editor.doc.nodes.len();
        let saved = if self.editor.is_modified() {
            "unsaved"
        } else {
            "saved"
        };
        let render = self
            .cache
            .borrow()
            .last_batch
            .map(|(k, d)| format!(" · {k} tiles in {} ms", d.as_millis()))
            .unwrap_or_default();
        let autosaved = self
            .autosave_note()
            .map(|a| format!(" · {a}"))
            .unwrap_or_default();
        let right = format!(
            "non-destructive · {n} node{} · {saved}{autosaved}{render}",
            if n == 1 { "" } else { "s" }
        );
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(9.))
            .px(px(16.))
            .py(px(10.))
            .border_t_1()
            .border_color(p.line)
            .overflow_hidden()
            .when(!self.suggestions.is_empty(), |d| {
                d.child(mono("IT NOTICED", 9.5, p.muted).whitespace_nowrap())
            })
            .children(self.suggestion_chips(p, cx))
            .children(self.status.as_ref().map(|(msg, err)| {
                mono(msg.clone(), 10.5, if *err { p.accent } else { p.ink }).whitespace_nowrap()
            }))
            .child(div().flex_1())
            .child(mono(right, 10., p.muted).whitespace_nowrap())
    }

    fn node_panel(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id("node-panel")
            .flex()
            .flex_none()
            .flex_col()
            .w(dim::NODE_PANEL_W)
            .min_h_0()
            .border_l_1()
            .border_color(p.line)
            .overflow_y_scroll()
            .track_focus(&self.panel_focus)
            .key_context("NodePanel")
            .child(self.scene_graph(p, cx))
            .child(self.inspector(p, window, cx))
            .child(self.history_list(p, cx))
    }

    fn scene_graph(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows = self.editor.doc.panel_rows();
        let accent = p.accent;
        let header = div()
            .id("graph-header")
            .flex()
            .items_center()
            .gap(px(6.))
            .pb(px(6.))
            .drag_over::<DraggedNode>(move |s, _, _, _| s.bg(accent.opacity(0.12)))
            .on_drop(cx.listener(|this, d: &DraggedNode, _, cx| this.drop_on(d.id, None, cx)))
            .child(label("Scene graph", p))
            .child(div().flex_1())
            .child(
                chip("add", "+ node", self.menu == Some(Menu::Add), p).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.menu = if this.menu == Some(Menu::Add) {
                            None
                        } else {
                            Some(Menu::Add)
                        };
                        cx.notify();
                    },
                )),
            )
            .child(
                chip("grp", "grp", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.group_selected(cx))),
            )
            .child(
                chip("dup", "dup", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate_selected(cx))),
            )
            .child(
                chip("del", "del", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.delete_selected(cx))),
            );

        let add_menu = (self.menu == Some(Menu::Add)).then(|| {
            let mut items: Vec<(SharedString, Node)> = Adjustment::catalogue()
                .into_iter()
                .map(|a| (SharedString::from(a.label()), Node::adjust(0, a)))
                .collect();
            items.push((
                "Solid fill".into(),
                Node::new(
                    0,
                    "Fill",
                    NodeKind::Fill {
                        rgba: [255, 255, 255, 255],
                    },
                ),
            ));
            items.push(("Empty group".into(), Node::group(0, "Group")));
            self.menu_list(
                "add-menu",
                items
                    .into_iter()
                    .map(|(l, n)| (l, MenuAction::Add(Box::new(n))))
                    .collect(),
                p,
                cx,
            )
        });

        let row_els: Vec<AnyElement> = rows
            .iter()
            .map(|r| self.node_row(r.id, r.depth, p, cx).into_any_element())
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(15.))
            .pt(px(13.))
            .pb(px(11.))
            .border_b_1()
            .border_color(p.line)
            .child(header)
            .children(add_menu)
            .when(rows.is_empty(), |d| {
                d.child(mono("empty document", 10., p.muted))
            })
            .children(row_els)
    }

    fn node_row(
        &mut self,
        id: NodeId,
        depth: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let n = self.editor.doc.node(id).expect("row node").clone();
        let on = self.selected == Some(id);
        let (fg, bg, border) = if on {
            (p.paper, p.ink, p.ink)
        } else {
            (p.ink, transparent_black(), p.line)
        };
        let meta_fg = if on { p.paper } else { p.muted };
        let chip_el: AnyElement = match &n.kind {
            NodeKind::Raster { raster, .. } => {
                let t = self.thumb(raster);
                img(ImageSource::Render(t))
                    .size(px(20.))
                    .flex_none()
                    .into_any_element()
            }
            NodeKind::Group { collapsed } => {
                let collapsed = *collapsed;
                div()
                    .id(("collapse", id))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(20.))
                    .flex_none()
                    .border_1()
                    .border_color(meta_fg)
                    .font_family(MONO_FONT)
                    .text_size(px(10.))
                    .child(if collapsed { "▸" } else { "▾" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.execute(
                            Command::SetCollapsed {
                                id,
                                collapsed: !collapsed,
                            },
                            cx,
                        );
                    }))
                    .into_any_element()
            }
            NodeKind::Adjust(_) => div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(20.))
                .flex_none()
                .bg(p.line)
                .text_color(p.ink)
                .font_family(MONO_FONT)
                .text_size(px(12.))
                .child("◑")
                .into_any_element(),
            NodeKind::Path { .. } => div()
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(p.line)
                .text_size(px(12.))
                .child("✒")
                .into_any_element(),
            NodeKind::Fill { rgba } => div()
                .size(px(20.))
                .flex_none()
                .border_1()
                .border_color(p.line)
                .bg(rgb(((rgba[0] as u32) << 16)
                    | ((rgba[1] as u32) << 8)
                    | rgba[2] as u32))
                .into_any_element(),
        };
        let mut meta = n.kind.tag().to_string();
        if n.blend != BlendMode::Normal && n.blend != BlendMode::PassThrough {
            meta = format!("{} · {meta}", n.blend.label());
        }
        if n.clip_to.is_some() {
            meta = format!("↓ {meta}");
        }
        if n.mask.is_some() {
            meta = format!("{meta} · m");
        }
        let name_el: AnyElement = match &self.renaming {
            Some((rid, state, _)) if *rid == id => Input::new(state)
                .appearance(false)
                .bordered(false)
                .into_any_element(),
            _ => div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(12.5))
                .when(!n.visible, |d| d.opacity(0.45))
                .child(n.name.clone())
                .into_any_element(),
        };
        let accent = p.accent;
        let dragged = DraggedNode {
            id,
            name: n.name.clone().into(),
        };
        div()
            .id(("row", id))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(6. + depth as f32 * 14.))
            .pr(px(8.))
            .py(px(6.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .cursor_pointer()
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                window.focus(&this.panel_focus, cx);
                this.menu = None;
                if e.click_count() >= 2 {
                    this.start_rename(id, window, cx);
                } else {
                    this.selected = Some(id);
                    cx.notify();
                }
            }))
            .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
            .drag_over::<DraggedNode>(move |s, _, _, _| {
                s.border_color(accent).bg(accent.opacity(0.12))
            })
            .on_drop(
                cx.listener(move |this, d: &DraggedNode, _, cx| this.drop_on(d.id, Some(id), cx)),
            )
            .child(
                div()
                    .id(("eye", id))
                    .w(px(12.))
                    .flex_none()
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .text_color(meta_fg)
                    .child(if n.visible { "●" } else { "○" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        let visible = this.editor.doc.node(id).is_some_and(|n| !n.visible);
                        this.execute(Command::SetVisible { id, visible }, cx);
                    })),
            )
            .child(chip_el)
            .child(name_el)
            .child(mono(meta, 9.5, meta_fg).flex_none())
    }

    fn inspector(
        &mut self,
        p: &Palette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let Some(n) = self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .cloned()
        else {
            return div()
                .px(px(15.))
                .py(px(13.))
                .border_b_1()
                .border_color(p.line)
                .child(mono("select a node", 10., p.muted));
        };
        let id = n.id;
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(15.))
            .py(px(13.))
            .border_b_1()
            .border_color(p.line);
        body = body.child(label(n.name.clone(), p));

        // Opacity + blend.
        body = body.child(self.param_slider(
            SliderKey::Opacity(id),
            "opacity",
            format!("{:.0}%", n.opacity * 100.0),
            n.opacity,
            (0.0, 100.0, 1.0),
            p,
            cx,
        ));
        let blend_open = self.menu == Some(Menu::Blend);
        let modes: Vec<BlendMode> = if n.is_group() {
            std::iter::once(BlendMode::PassThrough)
                .chain(BlendMode::MENU.iter().flatten().copied())
                .collect()
        } else {
            BlendMode::MENU.iter().flatten().copied().collect()
        };
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .font_family(MONO_FONT)
                .text_size(px(10.5))
                .child("blend")
                .child(
                    chip("blend", format!("{} ▾", n.blend.label()), blend_open, p).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.menu = if this.menu == Some(Menu::Blend) {
                                None
                            } else {
                                Some(Menu::Blend)
                            };
                            cx.notify();
                        }),
                    ),
                ),
        );
        if blend_open {
            let items = modes
                .into_iter()
                .map(|m| (SharedString::from(m.label()), MenuAction::Blend(id, m)))
                .collect();
            body = body.child(self.menu_list("blend-menu", items, p, cx));
        }

        // Clipping and mask toggles.
        let sib = self.editor.doc.children(n.parent);
        let below = sib
            .iter()
            .position(|s| *s == id)
            .and_then(|i| i.checked_sub(1))
            .map(|i| sib[i]);
        let mut toggles = div().flex().gap(px(6.)).flex_wrap();
        if let Some(b) = below {
            let clipped = n.clip_to.is_some();
            toggles = toggles.child(chip("clip", "clip to below", clipped, p).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.execute(
                        Command::SetClip {
                            id,
                            clip_to: if clipped { None } else { Some(b) },
                        },
                        cx,
                    );
                }),
            ));
        }
        if n.mask.is_some() {
            let en = n.mask_enabled;
            toggles = toggles.child(chip("mask", "mask", en, p).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.execute(Command::SetMaskEnabled { id, enabled: !en }, cx);
                },
            )));
        }
        let locked = n.locked;
        toggles = toggles.child(chip("lock", "locked", locked, p).on_click(cx.listener(
            move |this, _, _, cx| {
                this.execute(
                    Command::SetLocked {
                        id,
                        locked: !locked,
                    },
                    cx,
                );
            },
        )));
        body = body.child(toggles);

        match &n.kind {
            NodeKind::Adjust(a) => {
                for spec in a.params() {
                    let ParamSpec {
                        key,
                        label: l,
                        min,
                        max,
                        step,
                        value,
                        ..
                    } = spec.clone();
                    let norm = (value - min) / (max - min);
                    body = body.child(self.param_slider(
                        SliderKey::Param(id, key),
                        l,
                        spec.display(),
                        norm,
                        (min, max, step),
                        p,
                        cx,
                    ));
                }
                if a.params().is_empty() {
                    body = body.child(mono("no parameters", 10., p.muted));
                }
            }
            NodeKind::Raster { raster, placement } => {
                body = body.child(mono(
                    format!(
                        "{}×{} px at {:.0}, {:.0}",
                        raster.width(),
                        raster.height(),
                        placement.x,
                        placement.y
                    ),
                    10.5,
                    p.muted,
                ));
                let s = (placement.scale_x.abs() * 100.0) as f32;
                body = body.child(self.param_slider(
                    SliderKey::Scale(id),
                    "scale",
                    format!("{s:.0}%"),
                    (s - 1.0) / 399.0,
                    (1.0, 400.0, 1.0),
                    p,
                    cx,
                ));
                let r = placement.rotation as f32;
                body = body.child(self.param_slider(
                    SliderKey::Rotation(id),
                    "rotation",
                    format!("{r:+.0}°"),
                    (r + 180.0) / 360.0,
                    (-180.0, 180.0, 1.0),
                    p,
                    cx,
                ));
                if !placement.is_identity() {
                    body = body.child(chip("reset-xf", "reset transform", false, p).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.execute(
                                Command::SetPlacement {
                                    id,
                                    placement: Placement::default(),
                                },
                                cx,
                            );
                        }),
                    ));
                }
            }
            NodeKind::Path { path, style, .. } => {
                let stroke = match style.stroke {
                    Some(c) => format!(
                        "stroke #{:02X}{:02X}{:02X} {:.0}px",
                        c[0], c[1], c[2], style.width
                    ),
                    None => "no stroke".into(),
                };
                let fill = match style.fill {
                    Some(c) => format!("fill #{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
                    None => "no fill".into(),
                };
                body = body.child(mono(
                    format!(
                        "{} anchors · {stroke} · {fill} · edit with the Pen (P)",
                        path.anchor_count()
                    ),
                    10.5,
                    p.muted,
                ));
            }
            NodeKind::Fill { rgba } => {
                body = body.child(mono(
                    format!(
                        "#{:02X}{:02X}{:02X} · alpha {}",
                        rgba[0], rgba[1], rgba[2], rgba[3]
                    ),
                    10.5,
                    p.muted,
                ));
            }
            NodeKind::Group { .. } => {
                let k = self.editor.doc.subtree(id).len() - 1;
                body = body.child(mono(
                    format!("{k} node{} inside", if k == 1 { "" } else { "s" }),
                    10.5,
                    p.muted,
                ));
            }
        }
        body
    }

    #[allow(clippy::too_many_arguments)]
    fn param_slider(
        &mut self,
        key: SliderKey,
        name: &str,
        display: String,
        norm: f32,
        spec: (f32, f32, f32),
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let track = self.tracks.entry(key).or_default().clone();
        let id = SharedString::from(format!("{key:?}"));
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .font_family(MONO_FONT)
                    .text_size(px(10.5))
                    .text_color(p.ink)
                    .child(name.to_string())
                    .child(div().text_color(p.muted).child(display)),
            )
            .child(slider(
                id,
                norm,
                track,
                p,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.slider_down(key, spec, e, cx);
                }),
            ))
    }

    fn history_list(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let steps: Vec<String> = self
            .editor
            .history
            .steps()
            .map(|s| s.name.clone())
            .take(14)
            .collect();
        let total = self.editor.history.len();
        let can_redo = self.editor.history.can_redo();
        let mut list = div().flex().flex_col();
        for (i, name) in steps.iter().enumerate() {
            let dot = if i == 0 { p.accent } else { p.muted };
            list = list.child(
                div()
                    .id(("hist", i))
                    .flex()
                    .gap(px(10.))
                    .pb(px(8.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.undo_to(i, cx)))
                    .child(
                        div()
                            .size(px(7.))
                            .mt(px(5.))
                            .rounded_full()
                            .flex_none()
                            .bg(dot),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(p.ink)
                                    .child(name.clone()),
                            )
                            .child(mono(format!("step {}", total - i), 9.5, p.muted)),
                    ),
            );
        }
        list = list.child(
            div()
                .id("hist-open")
                .flex()
                .gap(px(10.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.undo_to(total, cx)))
                .child(
                    div()
                        .size(px(7.))
                        .mt(px(5.))
                        .rounded_full()
                        .flex_none()
                        .bg(if total == 0 { p.accent } else { p.muted }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(p.ink)
                                .child(format!("Open {}", self.name)),
                        )
                        .child(mono("start", 9.5, p.muted)),
                ),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(15.))
            .py(px(13.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("History", p))
                    .child(div().flex_1())
                    .child(
                        chip("graph", "graph →", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.open_history(cx))),
                    )
                    .child(
                        chip("undo", "undo", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                    )
                    .child(
                        chip("redo", if can_redo { "redo" } else { "redo –" }, false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                    ),
            )
            .child(list)
    }

    fn menu_list(
        &self,
        id: &'static str,
        items: Vec<(SharedString, MenuAction)>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let accent = p.accent;
        div()
            .id(id)
            .flex()
            .flex_col()
            .border_1()
            .border_color(p.ink)
            .bg(p.panel)
            .max_h(px(320.))
            .overflow_y_scroll()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menu = None;
                cx.notify();
            }))
            .children(items.into_iter().enumerate().map(|(i, (text, action))| {
                let action = Rc::new(action);
                div()
                    .id((id, i))
                    .px(px(10.))
                    .py(px(5.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(accent).text_color(gpui_kit::white()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match &*action {
                            MenuAction::Blend(id, m) => {
                                let (id, m) = (*id, *m);
                                this.execute(Command::SetBlend { id, blend: m }, cx);
                            }
                            MenuAction::Add(node) => this.add_node((**node).clone(), cx),
                        }
                        this.menu = None;
                        cx.notify();
                    }))
                    .child(text)
            }))
    }
}

enum MenuAction {
    Blend(NodeId, BlendMode),
    Add(Box<Node>),
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        self.sync_trees(cx);
        self.sync_transform_fields(window, cx);
        let doc_bar = self.doc_bar(&p, cx);
        if self.history.open {
            let page = self.history_page(&p, cx);
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .track_focus(&self.focus)
                .child(doc_bar)
                .child(page)
                .into_any_element();
        }
        let rail = self.tool_rail(&p, cx);
        let context = self.context_bar(&p, cx);
        let canvas = self.canvas_area(&p, window.scale_factor(), cx);
        let picker = self.picker(&p, cx);
        self.refresh_suggestions(cx);
        let strip = self.status_strip(&p, cx);
        let ask = self.ask_bar(&p, cx);
        let size_panel = self.size_panel_view(&p, cx);
        let presets = self.presets_view(&p, cx);
        let dock = self.assistant_dock(&p, cx);
        let panel = self.node_panel(&p, window, cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .track_focus(&self.focus)
            .child(doc_bar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(rail)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(context)
                            .children(size_panel)
                            .children(presets)
                            .children(ask)
                            .child(canvas)
                            .children(dock)
                            .child(strip),
                    )
                    .child(panel)
                    .children(picker),
            )
            .into_any_element()
    }
}
