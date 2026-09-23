//! The Editor screen: document bar, tool rail, context bar, canvas, status
//! strip, and node panel.

use crate::theme::{self, MONO_FONT, Palette, dim};
use crate::viewport::{self, CanvasBounds, Scene, TileCache, View, Which};
use crate::widgets::{TrackBounds, button, chip, label, mono, slider, track_fraction};

mod adjust_ui;
mod ai_tools;
mod alignment;
mod animation;
mod brush_library_ui;
mod brush_memory;
mod brush_quick;
mod brush_studio;
mod canvas_size;
pub(crate) mod channels;
mod clipboard;
mod compact;
pub(crate) mod crop;
mod draw_workspace;
pub(crate) mod export_ui;
mod filters;
pub(crate) mod generate_ui;
pub(crate) mod guides;
mod history;
mod layer_effect_rows;
mod layer_links_ui;
mod layer_menu;
mod layer_selection;
mod layers_panel;
mod lens;
mod menu_bar;
mod movement;
mod panels;
mod pen;
mod toolbox;
mod workspace_layout;
pub(crate) use pen::PenMode;
mod presets;
#[cfg(test)]
pub(crate) use presets::shared_library;
mod rail;
mod raw_panel;
mod raw_settings_ui;
mod recipes;
mod rotation;
mod shape_path_ops;
pub(crate) mod shapes;
mod sidebar;
pub(crate) use sidebar::{DockTab, SidebarTab};
mod smart;
mod snap;
mod style_pattern;
mod styles_ui;
mod text_properties;
mod tools;
mod transform;
mod type_tool;
pub use canvas_size::SizeMode;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::adjust::ParamSpec;
use emulsion_raster::composite::{CompositeTree, level_size, render_tile, tile_to_bgra8};
use emulsion_raster::{Adjustment, BlendMode, Placement, Raster, TileCoord, color};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
pub(crate) use history::SaveTarget;
pub(crate) use history::doc_thumb;
pub use history::recovery_dir;
use rayon::prelude::*;
pub use recipes::recipes_dir;
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
    /// Click to pick the foreground colour from the picture.
    Eyedropper,
    /// Click to zoom in, alt-click to zoom out.
    Zoom,
}

/// One line on what a rail tool does, for its tooltip.
fn tool_help(tool: Tool) -> &'static str {
    match tool {
        Tool::Hand => "Hand: drag to move around the picture. Space + drag works from any tool.",
        Tool::Move => "Move: drag a layer or group. Arrows nudge 1 px; Shift+arrows nudge 10 px.",
        Tool::Select => "Select: rectangle, ellipse, lasso, wand and AI quick select.",
        Tool::Mask => "Mask: paint what shows on the selected layer (reveal or hide).",
        Tool::Brush => {
            "Brush: paint, erase, smudge, fill, gradient and liquify. Choose the kind in the bar above."
        }
        Tool::Heal => "Heal: paint over a blemish to blend it away.",
        Tool::Clone => "Clone: alt-click a source, then paint copies of it.",
        Tool::Grade => "Grade: colour and tone adjustments as layers.",
        Tool::Type => "Type: click to add text.",
        Tool::Crop => "Crop: drag a frame, Enter to crop.",
        Tool::Shape => "Shape: drag a rectangle or ellipse.",
        Tool::Pen => "Pen: click to place path points; drag for curves.",
        Tool::Eyedropper => {
            "Eyedropper: click to pick the foreground colour; alt-click for background."
        }
        Tool::Zoom => "Zoom: click to zoom in, Shift/Alt-click to zoom out, double-click for 100%.",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum SliderKey {
    Opacity(NodeId),
    FillOpacity(NodeId),
    LayerOpacity(NodeId),
    LayerFillOpacity(NodeId),
    BlendRange(NodeId, bool, usize),
    Param(NodeId, &'static str),
    Scale(NodeId),
    Rotation(NodeId),
    Compare,
    TextSize,
    RefineLo,
    RefineHi,
    RefineGrow,
    RefineFeather,
    ToolSize,
    ToolHardness,
    ToolOpacity,
    ToolFlow,
    // Popup controls must not share the toolbar's measured slider tracks.
    QuickBrushSize,
    QuickBrushHardness,
    QuickBrushOpacity,
    QuickBrushFlow,
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
    ToolTilt,
    ToolPressureCurve,
    /// Draw mode's side sliders share the brush keys but need their own tracks.
    SideSize,
    SideOpacity,
    ExportQuality,
    /// A RAW develop parameter, by name.
    Raw(&'static str),
    PenWidth,
    /// Bounds slot for a node's curves editor.
    Curve(NodeId),
    /// A smart layer's filter parameter: node, filter index, key.
    Filter(NodeId, usize, &'static str),
    /// A layer style parameter: node, style index, key.
    Style(NodeId, usize, &'static str),
    StyleOption(NodeId, usize, &'static str),
    StyleStop(NodeId, usize, usize, bool),
    StyleContour(NodeId, usize, usize, bool),
    StyleGlobalLight(bool),
    Tolerance,
    Feather,
    Straighten,
    PickerSv,
    PickerHue,
}

impl SliderKey {
    fn is_quick_brush(self) -> bool {
        matches!(
            self,
            Self::QuickBrushSize
                | Self::QuickBrushHardness
                | Self::QuickBrushOpacity
                | Self::QuickBrushFlow
        )
    }

    /// Keys that edit the document (their drags are one history step).
    fn edits_document(self) -> bool {
        matches!(
            self,
            SliderKey::Opacity(_)
                | SliderKey::FillOpacity(_)
                | SliderKey::LayerOpacity(_)
                | SliderKey::LayerFillOpacity(_)
                | SliderKey::BlendRange(..)
                | SliderKey::Param(..)
                | SliderKey::Scale(_)
                | SliderKey::Rotation(_)
                | SliderKey::PenWidth
                | SliderKey::TextSize
                | SliderKey::RefineLo
                | SliderKey::RefineHi
                | SliderKey::RefineGrow
                | SliderKey::RefineFeather
                | SliderKey::Filter(..)
                | SliderKey::Style(..)
                | SliderKey::StyleOption(..)
                | SliderKey::StyleStop(..)
                | SliderKey::StyleContour(..)
                | SliderKey::StyleGlobalLight(..)
        )
    }
}

/// Bounds for the Layers list height.
pub(crate) const LAYERS_MIN_H: f32 = 340.0;
pub(crate) const LAYERS_MAX_H: f32 = 900.0;

enum Drag {
    Compare,
    Toolbar(compact::ToolbarDrag),
    SidebarResize {
        start_x: Pixels,
        start_w: f32,
    },
    Tool(tools::ToolDrag),
    /// Dragging the handle under the Layers list.
    LayersSplit {
        start_y: Pixels,
        start_h: f32,
    },
    /// Dragging a perspective guide's vanishing point.
    Vanishing(usize),
    Pan {
        last: Point<Pixels>,
    },
    RotateView {
        center: Point<Pixels>,
        angle: f64,
        rotation: f64,
    },
    Move(movement::MoveGesture),
    Slider {
        key: SliderKey,
        track: TrackBounds,
        min: f32,
        max: f32,
        step: f32,
        /// Side sliders run bottom to top.
        vertical: bool,
    },
    /// A point of a curves editor.
    Curve(adjust_ui::CurveDrag),
    /// Free Transform: a handle of the selected pixel node.
    Transform(transform::Grab),
    /// Distort: one corner moves freely; pixels re-project on release.
    Distort {
        id: NodeId,
        corner: usize,
        quad: [(f64, f64); 4],
    },
    /// Dragging one point of the Warp lattice.
    Warp(usize),
    /// Dragging in the navigator pans the view.
    Navigator,
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
    /// The Type tool's font list, each family shown in itself.
    Font,
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

/// ColorDrop: the foreground swatch dragged onto the canvas.
#[derive(Clone)]
pub struct DraggedColor(pub [u8; 4]);

impl Render for DraggedColor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let [r, g, b, _] = self.0;
        div()
            .size(px(22.))
            .border_1()
            .border_color(p.ink)
            .bg(rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32))
    }
}

pub struct EditorView {
    pub editor: Editor,
    /// Display name (file stem of what was opened).
    pub name: String,
    /// Where it came from, for Save As suggestions.
    pub source: Option<PathBuf>,
    pub(crate) view: View,
    /// Warp mesh in progress on a node (Move tool).
    pub(crate) warp: Option<transform::WarpState>,
    /// Animation assist and time-lapse.
    pub(crate) anim: animation::AnimState,
    /// RAW develop panel state.
    pub(crate) raw: raw_panel::RawState,
    /// Other open tabs, supplied by the workspace; weak references never keep closed photos alive.
    pub(crate) raw_peers: Vec<WeakEntity<EditorView>>,
    /// Generative fill prompt and state.
    pub(crate) generate: generate_ui::GenState,
    /// Tool rail fly-outs and remembered picks.
    pub(crate) rail: rail::RailState,
    compact: compact::CompactLayout,
    workspace_customizer: Option<Entity<gpui_kit::component::input::InputState>>,
    workspace_customizer_focus: FocusHandle,
    sidebar_layout: sidebar::SidebarState,
    /// Draw mode: painter's rail and a Layers-only sidebar.
    pub(crate) draw_mode: bool,
    /// Export chooser state and the last format picked.
    pub(crate) export_prefs: export_ui::ExportPrefs,
    pub(crate) fit_pending: bool,
    pub(crate) canvas_bounds: CanvasBounds,
    pub(crate) cache: Rc<RefCell<TileCache>>,
    pub(crate) seen_rev: u64,
    /// A composite tree is being built off the UI thread for this revision.
    tree_building: Option<u64>,
    /// Content requests also include animation/preview changes without edits.
    tree_request: u64,
    /// Dirty area accumulated since the tree on screen was built.
    tree_dirty: emulsion_core::Dirty,
    pub(crate) gen_counter: u64,
    pub(crate) render_gen: u64,
    pub(crate) tree: Arc<CompositeTree>,
    pub(crate) seen_commit: u64,
    pub(crate) before_gen: u64,
    pub(crate) before_tree: Option<Arc<CompositeTree>>,
    pub selected: Option<NodeId>,
    pub(crate) layer_selection: layer_selection::LayerSelection,
    pub(crate) tool: Tool,
    drag: Option<Drag>,
    pub(crate) compare: f32,
    pub(crate) rulers: bool,
    pub(crate) space_held: bool,
    focus_watchers: Option<(Subscription, Subscription)>,
    pub(crate) renaming: Option<(NodeId, Entity<InputState>, Subscription)>,
    menu: Option<Menu>,
    pub(crate) sidebar_tab: SidebarTab,
    pub(crate) brush_settings_section: tools::BrushSettingsSection,
    pub(crate) dock_tab: DockTab,
    sidebar_menu: bool,
    tracks: HashMap<SliderKey, TrackBounds>,
    pub(crate) thumbs: HashMap<usize, Arc<RenderImage>>,
    pub(crate) checker: (u8, u8),
    pub(crate) channels: channels::ChannelState,
    pub(crate) layer_panel: layers_panel::LayerPanelState,
    pub focus: FocusHandle,
    pub(crate) canvas_focus: FocusHandle,
    pub(crate) panel_focus: FocusHandle,
    pub status: Option<(SharedString, bool)>,
    pub(crate) assistant: crate::assistant::Assistant,
    pub(crate) ask: Option<crate::assistant::AskBar>,
    pub(crate) suggestions: Vec<emulsion_ai::suggest::Suggestion>,
    /// What kind of picture this is, from the last suggestion pass.
    pub(crate) doc_kind: Option<emulsion_ai::kind::Classification>,
    pub(crate) suggest_rev: u64,
    pub(crate) suggest_busy: bool,
    pub(crate) tools: tools::ToolState,
    pub(crate) history: history::HistoryState,
    /// Snap moves to guides, edges and centres.
    pub(crate) snap: bool,
    /// Ctrl held during a drag: move freely.
    pub(crate) snap_bypass: bool,
    pub(crate) snap_lines: Vec<(bool, f64)>,
    pub(crate) size_panel: Option<canvas_size::SizePanel>,
    /// Height of the Layers list, from settings until the handle is dragged.
    pub(crate) layers_h: f32,
    pub(crate) transform_fields: Option<transform::TransformFields>,
    pub(crate) rotation_fields: Option<rotation::RotationFields>,
    pub(crate) presets: presets::PresetState,
    brush_workspace: Option<Entity<brush_library_ui::BrushWorkspace>>,
    draw_ui: draw_workspace::DrawUi,
    pub(crate) adjust_ui: adjust_ui::AdjustUi,
    pub(crate) recipes: recipes::RecipeState,
    pub(crate) smart: smart::SmartUi,
    pub(crate) panels: panels::PanelState,
    pub(crate) styles_ui: styles_ui::StylesUi,
    pub(crate) shape_ui: shapes::ShapeUi,
    pub(crate) type_tool: type_tool::TypeState,
    pub(crate) ai: ai_tools::AiState,
    /// Shift held during a drag: free aspect, or 15° rotation steps.
    pub(crate) drag_shift: bool,
    /// Monotonic UI intent counter: unlike history revisions, never rewinds on undo.
    pub(crate) operation_epoch: u64,
    pub(crate) history_epoch: u64,
    selection_request: u64,
    pending_edit_job: Option<(u64, u64)>,
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
        crate::tablet::start();
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
        let draw_mode = cx
            .try_global::<crate::app_state::AppSettings>()
            .is_some_and(|s| s.0.draw_mode);
        let mut view = Self {
            editor,
            name,
            source,
            view: View::default(),
            warp: None,
            anim: Default::default(),
            raw: Default::default(),
            raw_peers: Vec::new(),
            generate: Default::default(),
            rail: Default::default(),
            compact: compact::CompactLayout::for_mode(draw_mode, cx),
            workspace_customizer: None,
            workspace_customizer_focus: cx.focus_handle(),
            sidebar_layout: Default::default(),
            export_prefs: Default::default(),
            draw_mode,
            fit_pending: true,
            canvas_bounds: Default::default(),
            cache: Default::default(),
            seen_rev: rev,
            tree_building: None,
            tree_request: 0,
            tree_dirty: emulsion_core::Dirty::Nothing,
            gen_counter: 1,
            render_gen: 1,
            tree,
            seen_commit: commit,
            before_gen: 0,
            before_tree: None,
            selected,
            layer_selection: Default::default(),
            tool: Tool::Hand,
            drag: None,
            compare: 0.0,
            rulers: true,
            space_held: false,
            focus_watchers: None,
            renaming: None,
            menu: None,
            sidebar_tab: if cx
                .try_global::<crate::app_state::AppSettings>()
                .is_some_and(|settings| settings.0.compact_chrome)
            {
                SidebarTab::Info
            } else {
                SidebarTab::History
            },
            brush_settings_section: Default::default(),
            dock_tab: DockTab::Layers,
            sidebar_menu: false,
            tracks: HashMap::new(),
            thumbs: HashMap::new(),
            checker: theme::palette(cx).checker,
            channels: Default::default(),
            layer_panel: Default::default(),
            focus: cx.focus_handle(),
            canvas_focus: cx.focus_handle(),
            panel_focus: cx.focus_handle(),
            status: None,
            assistant: Default::default(),
            ask: None,
            suggestions: Vec::new(),
            doc_kind: None,
            suggest_rev: 0,
            suggest_busy: false,
            tools: tools::ToolState::default(),
            history: Default::default(),
            snap: true,
            snap_bypass: false,
            snap_lines: Vec::new(),
            size_panel: None,
            layers_h: cx
                .try_global::<crate::app_state::AppSettings>()
                .map(|s| {
                    if s.0.layers_height == 260.0 {
                        400.0
                    } else {
                        s.0.layers_height
                    }
                })
                .unwrap_or(400.0)
                .clamp(LAYERS_MIN_H, LAYERS_MAX_H),
            transform_fields: None,
            rotation_fields: None,
            presets: Default::default(),
            brush_workspace: None,
            draw_ui: Default::default(),
            adjust_ui: Default::default(),
            recipes: Default::default(),
            smart: Default::default(),
            panels: Default::default(),
            styles_ui: Default::default(),
            shape_ui: Default::default(),
            type_tool: Default::default(),
            ai: Default::default(),
            drag_shift: false,
            operation_epoch: 0,
            history_epoch: 0,
            selection_request: 0,
            pending_edit_job: None,
        };
        // An explicit default wins; otherwise reopen the current mode the
        // way it was last arranged.
        if let Some(layout) = cx
            .try_global::<crate::app_state::AppSettings>()
            .and_then(|s| {
                s.0.workspace_default.clone().or_else(|| {
                    if draw_mode {
                        s.0.draw_workspace.clone()
                    } else {
                        s.0.photo_workspace.clone()
                    }
                })
            })
        {
            view.apply_workspace_layout(&layout, cx);
        }
        if view.editor.doc.raw.is_some() {
            view.sidebar_tab = SidebarTab::Properties;
            view.sidebar_layout.collapsed = false;
        }
        view
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

    pub(crate) fn has_unsaved_changes(&self) -> bool {
        self.editor.is_modified() || self.raw.is_pending()
    }

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
        self.operation_epoch = self.operation_epoch.wrapping_add(1);
        self.cancel_raw_develop();
        if let Some(sel) = self.selected
            && self.editor.doc.node(sel).is_none()
        {
            let selected = self.editor.doc.nodes.last().map(|n| n.id);
            self.set_layer_selection(selected.into_iter().collect(), selected);
        }
        cx.notify();
    }

    pub(crate) fn brush_workspace_open(&self) -> bool {
        self.brush_workspace.is_some()
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.finish_shape_color_edit(cx);
        self.close_text_field(cx);
        self.tools.transform_lift = None;
        self.invalidate_pending_edits();
        self.drag = None;
        self.warp = None;
        if self.editor.undo() {
            self.after_change(cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.finish_shape_color_edit(cx);
        self.close_text_field(cx);
        self.tools.transform_lift = None;
        self.invalidate_pending_edits();
        self.drag = None;
        self.warp = None;
        if self.editor.redo() {
            self.after_change(cx);
        }
    }

    fn undo_to(&mut self, steps: usize, cx: &mut Context<Self>) {
        self.finish_shape_color_edit(cx);
        self.close_text_field(cx);
        self.tools.transform_lift = None;
        self.invalidate_pending_edits();
        self.drag = None;
        self.warp = None;
        for _ in 0..steps {
            self.editor.undo();
        }
        self.after_change(cx);
    }

    pub(crate) fn invalidate_pending_edits(&mut self) {
        self.cancel_raw_develop();
        self.operation_epoch = self.operation_epoch.wrapping_add(1);
        self.history_epoch = self.history_epoch.wrapping_add(1);
        self.selection_request = self.selection_request.wrapping_add(1);
        self.smart.cancel_pending();
    }

    pub(crate) fn edit_ticket(&self) -> (u64, u64) {
        (self.operation_epoch, self.editor.revision)
    }

    pub(crate) fn begin_edit_job(&mut self) -> (u64, u64) {
        self.operation_epoch = self.operation_epoch.wrapping_add(1);
        let ticket = self.edit_ticket();
        self.pending_edit_job = Some(ticket);
        ticket
    }

    pub(crate) fn edit_is_current(&self, ticket: (u64, u64)) -> bool {
        self.edit_ticket() == ticket && !self.editor.in_transaction()
    }

    pub(crate) fn accept_edit_result(
        &mut self,
        ticket: (u64, u64),
        label: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let current = self.edit_is_current(ticket);
        if self.pending_edit_job == Some(ticket) {
            self.pending_edit_job = None;
            if !current {
                self.set_status(
                    format!(
                        "{label} canceled because the document changed. Run it again to retry."
                    ),
                    false,
                    cx,
                );
            }
        }
        current
    }

    fn selection_ticket(&mut self) -> ((u64, u64), u64) {
        self.selection_request = self.selection_request.wrapping_add(1);
        (self.edit_ticket(), self.selection_request)
    }

    fn selection_is_current(&self, ticket: ((u64, u64), u64)) -> bool {
        self.edit_is_current(ticket.0) && self.selection_request == ticket.1
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
            self.tree_request = self.tree_request.wrapping_add(1);
            self.seen_rev = self.editor.revision;
            let dirty = self.editor.take_dirty();
            self.tree_dirty =
                std::mem::replace(&mut self.tree_dirty, emulsion_core::Dirty::Nothing).union(dirty);
            // Layer styles render blurs while the tree is built; that work
            // leaves the UI thread so slider drags stay smooth. Plain
            // documents build in microseconds and stay synchronous.
            let heavy = self
                .editor
                .doc
                .nodes
                .iter()
                .any(|n| n.effects_enabled && !n.styles.is_empty());
            if heavy {
                self.build_tree_async(cx);
            } else {
                let tree = if self.previewing() {
                    self.render_doc().composite_tree()
                } else {
                    self.editor.doc.composite_tree()
                };
                self.install_tree(tree);
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

    /// Put a freshly built tree on screen, keeping every cached tile the
    /// accumulated changes did not touch.
    fn install_tree(&mut self, tree: CompositeTree) {
        let old = self.render_gen;
        self.gen_counter += 1;
        self.render_gen = self.gen_counter;
        self.tree = Arc::new(tree);
        match std::mem::replace(&mut self.tree_dirty, emulsion_core::Dirty::Nothing) {
            emulsion_core::Dirty::Nothing => {
                self.cache.borrow_mut().retag(old, self.render_gen, None)
            }
            emulsion_core::Dirty::Rect(r) => {
                self.cache.borrow_mut().retag(old, self.render_gen, Some(r))
            }
            emulsion_core::Dirty::All => {}
        }
    }

    /// Build in the background and display completed progress while the next
    /// revision is prepared. Continuous painting must not starve the viewport.
    fn build_tree_async(&mut self, cx: &mut Context<Self>) {
        if self.tree_building.is_some() {
            return;
        }
        let rev = self.editor.revision;
        let displayed_tree = self.tree.clone();
        let request = self.tree_request;
        self.tree_building = Some(rev);
        let doc = self.render_doc();
        cx.spawn(async move |this, cx| {
            let tree = cx
                .background_spawn(async move { doc.composite_tree() })
                .await;
            this.update(cx, |this, cx| {
                this.tree_building = None;
                this.install_completed_tree(tree, rev, request, &displayed_tree);
                if this.editor.revision != rev || this.tree_request != request {
                    this.build_tree_async(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn install_completed_tree(
        &mut self,
        tree: CompositeTree,
        revision: u64,
        request: u64,
        displayed_tree: &Arc<CompositeTree>,
    ) {
        // A synchronous rebuild may already have installed a newer view (for
        // example after removing the last layer style). Never replace it.
        if !Arc::ptr_eq(&self.tree, displayed_tree) {
            return;
        }
        self.install_tree(tree);
        if self.editor.revision != revision || self.tree_request != request {
            // install_tree consumed changes accumulated after this snapshot.
            // Keep those pixels invalid for the next, newer tree as well.
            self.tree_dirty = emulsion_core::Dirty::All;
        }
    }

    fn before_active(&self) -> bool {
        self.raw_split_active()
            || (!self.raw_split_requested()
                && self.compare > 0.0
                && self.editor.differs_from_base()
                && self.before_tree.is_some())
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
        let before = self.raw_split_tree().or_else(|| self.before_tree.clone());
        let (light, dark) = self.checker;
        let channel = self.channels.view;
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
                            let mut bytes = tile_to_bgra8(&t, origin, lsz, 8, light, dark);
                            channel.apply(&mut bytes);
                            Some((r, bytes))
                        })
                        .collect()
                })
                .await;
            let elapsed = started.elapsed();
            tracing::debug!(target: "emulsion_ui::paint_timing", tiles = n, elapsed_us = elapsed.as_micros() as u64, "viewport tile batch completed");
            this.update(cx, |this, cx| {
                {
                    let mut c = this.cache.borrow_mut();
                    for (r, bytes) in out {
                        // Switching channels clears pending tiles; an older
                        // background batch must not put its colors back.
                        if this.channels.view != channel {
                            continue;
                        }
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

    /// Switch between the painter's shell and the full photo shell. Each
    /// mode keeps its own workspace (toolbars, tools and panels): leaving
    /// one remembers it, entering the other restores how it was left.
    pub fn toggle_draw_mode(&mut self, cx: &mut Context<Self>) {
        let leaving = self.workspace_snapshot();
        let on = !self.draw_mode;
        let mut restore = None;
        crate::app_state::update_settings(cx, |s| {
            s.draw_mode = on;
            let (save, load) = if on {
                (&mut s.photo_workspace, &s.draw_workspace)
            } else {
                (&mut s.draw_workspace, &s.photo_workspace)
            };
            *save = Some(leaving);
            restore = load.clone();
        });
        match restore {
            Some(mut layout) => {
                layout.draw_mode = on;
                self.apply_workspace_layout(&layout, cx);
            }
            None => {
                self.draw_mode = on;
                self.compact = compact::CompactLayout::for_mode(on, cx);
            }
        }
        self.rail = Default::default();
        if on {
            self.set_paint(PaintKind::Brush, cx);
            self.set_status(
                "Draw mode: brushes, colours and paint controls up front. Ctrl+Shift+D or Photo returns to the photo tools.",
                false,
                cx,
            );
        } else {
            self.set_status(
                "Photo mode: every photo tool and panel. Ctrl+Shift+D returns to Draw.",
                false,
                cx,
            );
        }
        cx.notify();
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

    pub(crate) fn set_hand_mode(&mut self, rotate: bool, cx: &mut Context<Self>) {
        if matches!(self.drag, Some(Drag::Pan { .. } | Drag::RotateView { .. })) {
            self.drag = None;
        }
        self.set_tool(Tool::Hand, cx);
        self.tools.rotate_view = rotate;
        cx.notify();
    }

    pub fn toggle_rulers(&mut self, cx: &mut Context<Self>) {
        self.rulers = !self.rulers;
        cx.notify();
    }

    // ── Node operations ─────────────────────────────────────────────────

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        let commands = self
            .selected_layer_roots()
            .into_iter()
            .map(|id| Command::RemoveNode { id })
            .collect();
        self.execute_layer_commands("Delete layers", commands, cx);
    }

    pub fn duplicate_selected(&mut self, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        let commands = self
            .selected_layer_roots()
            .into_iter()
            .map(|id| Command::DuplicateNode { id })
            .collect();
        if let Some(ids) = self.execute_layer_commands("Duplicate layers", commands, cx) {
            let active = ids.last().copied();
            self.set_layer_selection(ids, active);
        }
    }

    pub fn group_selected(&mut self, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        let ids = self.selected_layer_roots();
        if !ids.is_empty() {
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
                    ids,
                    name: format!("Group {n}"),
                },
                cx,
            ) {
                self.set_layer_selection(vec![g], Some(g));
            }
        }
    }

    pub fn ungroup_selected(&mut self, cx: &mut Context<Self>) {
        let groups: Vec<_> = self
            .selected_layer_roots()
            .into_iter()
            .filter(|id| self.editor.doc.node(*id).is_some_and(|n| n.is_group()))
            .collect();
        let children: Vec<_> = groups
            .iter()
            .flat_map(|id| self.editor.doc.children(Some(*id)))
            .collect();
        if groups.is_empty() {
            return;
        }
        if self
            .execute_layer_commands(
                "Ungroup layers",
                groups
                    .into_iter()
                    .map(|id| Command::Ungroup { id })
                    .collect(),
                cx,
            )
            .is_some()
        {
            let active = children.last().copied();
            self.set_layer_selection(children, active);
        }
    }

    /// Move the selection one step up (`up`) or down among its siblings.
    pub fn shift_selected(&mut self, up: bool, cx: &mut Context<Self>) {
        let mut ids = self.selected_layer_roots();
        let selected = ids.clone();
        if up {
            ids.reverse();
        }
        let mut trial = self.editor.doc.clone();
        let mut commands = Vec::new();
        for id in ids {
            let Some(node) = trial.node(id) else { continue };
            let siblings = trial.children(node.parent);
            let index = siblings.iter().position(|other| *other == id).unwrap_or(0);
            let target = if up {
                index + 1
            } else {
                index.saturating_sub(1)
            };
            if target == index || target >= siblings.len() || selected.contains(&siblings[target]) {
                continue;
            }
            let command = Command::MoveNode {
                id,
                slot: Slot {
                    parent: node.parent,
                    index: target,
                },
            };
            if let Err(error) = command.clone().apply(&mut trial) {
                self.set_status(error.to_string(), true, cx);
                return;
            }
            commands.push(command);
        }
        self.execute_layer_commands("Reorder layers", commands, cx);
    }

    pub fn toggle_selected_visible(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected
            && let Some(n) = self.editor.doc.node(id)
        {
            let visible = !n.visible;
            let commands = self
                .selected_layer_ids()
                .into_iter()
                .map(|id| Command::SetVisible { id, visible })
                .collect();
            self.execute_layer_commands("Layer visibility", commands, cx);
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

    /// No layer selected: the Properties panel empties and painting will
    /// go to a new layer. Escape in the panel, Ctrl-click on the selected
    /// row, or a click on empty list space all land here.
    pub(crate) fn deselect_layer(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_some() {
            self.selected = None;
            self.layer_selection = Default::default();
            self.menu = None;
            cx.notify();
        }
    }

    fn add_node(&mut self, node: Node, cx: &mut Context<Self>) {
        self.select_sidebar(SidebarTab::Properties, cx);
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
        self.menu = None;
    }

    /// Drop `dragged` onto the row for `target`: into a group, otherwise
    /// just above the target in its parent.
    fn drop_on(&mut self, dragged: NodeId, target: Option<NodeId>, cx: &mut Context<Self>) {
        if Some(dragged) == target {
            return;
        }
        if !self.layer_is_selected(dragged) {
            self.set_layer_selection(vec![dragged], Some(dragged));
        }
        let ids = self.selected_layer_roots();
        if target.is_some_and(|target| {
            ids.iter()
                .any(|id| *id == target || self.editor.doc.is_ancestor(*id, target))
        }) {
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
                    .filter(|s| !ids.contains(s))
                    .collect();
                let i = sib.iter().position(|s| *s == t.id).unwrap_or(sib.len());
                Slot {
                    parent: t.parent,
                    index: i + 1,
                }
            }
        };
        // Move bottom-to-top to a fixed anchor; recompute indices after removal.
        let mut trial = self.editor.doc.clone();
        let mut commands = Vec::new();
        let mut anchor = target.filter(|target| trial.node(*target).is_some_and(|n| !n.is_group()));
        for id in ids {
            let next_slot = if let Some(anchor) = anchor {
                let siblings: Vec<_> = trial
                    .children(slot.parent)
                    .into_iter()
                    .filter(|other| *other != id)
                    .collect();
                Slot {
                    parent: slot.parent,
                    index: siblings
                        .iter()
                        .position(|other| *other == anchor)
                        .map_or(siblings.len(), |i| i + 1),
                }
            } else {
                Slot::top_of(slot.parent)
            };
            let command = Command::MoveNode {
                id,
                slot: next_slot,
            };
            if let Err(error) = command.clone().apply(&mut trial) {
                self.set_status(error.to_string(), true, cx);
                return;
            }
            commands.push(command);
            anchor = Some(id);
        }
        self.execute_layer_commands("Reorder layers", commands, cx);
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
        self.drag_shift = e.modifiers.shift;
        // A second button must not replace the move that owns an undo transaction.
        if matches!(self.drag, Some(Drag::Move(_))) {
            return;
        }
        window.focus(&self.canvas_focus, cx);
        self.menu = None;
        if self.raw.picking_neutral && e.button == MouseButton::Left && !self.space_held {
            if let Some(point) = self.doc_point(e.position) {
                self.raw_neutral_at(point, cx);
            }
            return;
        }
        if e.button == MouseButton::Left
            && !self.space_held
            && self.tool == Tool::Hand
            && self.tools.rotate_view
        {
            if let Some(bounds) = self.canvas_bounds() {
                let center = bounds.center();
                let delta = e.position - center;
                let angle = f64::from(f32::from(delta.y)).atan2(f64::from(f32::from(delta.x)));
                self.drag = Some(Drag::RotateView {
                    center,
                    angle,
                    rotation: self.view.rotation,
                });
                cx.notify();
            }
            return;
        }
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
            if let Some(i) = self.vanishing_hit(e.position) {
                self.drag = Some(Drag::Vanishing(i));
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
        if self.tool == Tool::Move
            && e.click_count >= 2
            && let Some(d) = self.doc_point(e.position)
            && self.try_edit_text_at(d, e.click_count, window, cx)
        {
            return;
        }
        if self.tool == Tool::Type {
            if let Some(d) = self.doc_point(e.position) {
                self.type_pointer_down(d, e.click_count, e.modifiers.shift, window, cx);
            }
            return;
        }
        if self.tool != Tool::Move {
            self.tool_down(e, window, cx);
            return;
        }
        if self.transform_down(e) {
            cx.notify();
            return;
        }
        if let Some(point) = self.doc_point(e.position) {
            self.begin_move(point, cx);
        }
    }

    fn drag_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        if self.text_pointer_move(pos, cx) {
            return;
        }
        let inside = self.canvas_bounds().is_some_and(|b| b.contains(&pos));
        let wants_pointer = matches!(
            self.tool,
            Tool::Brush | Tool::Heal | Tool::Clone | Tool::Mask | Tool::Zoom
        ) || !self.tools.polygon.is_empty()
            || (self.tool == Tool::Pen && self.tools.pen.building.is_some());
        let pointer = inside.then_some(pos);
        if inside {
            self.note_pointer(pos, cx);
        }
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
            Drag::Compare => {
                if let Some(bounds) = self.canvas_bounds()
                    && bounds.size.width > px(0.)
                {
                    self.compare = (f32::from(pos.x - bounds.origin.x)
                        / f32::from(bounds.size.width))
                    .clamp(0., 1.);
                    cx.notify();
                }
            }
            Drag::Toolbar(drag) => {
                let drag = *drag;
                self.move_toolbar(drag, pos, cx);
            }
            Drag::SidebarResize { start_x, start_w } => {
                self.sidebar_layout.width = Some((*start_w - f32::from(pos.x - *start_x)).max(0.));
                cx.notify();
            }
            Drag::Tool(_) => self.tool_move(pos, cx),
            Drag::RotateView {
                center,
                angle,
                rotation,
            } => {
                let delta = pos - *center;
                let next = f64::from(f32::from(delta.y)).atan2(f64::from(f32::from(delta.x)));
                let degrees = rotation + (next - angle).to_degrees();
                self.view.rotation = if self.drag_shift {
                    (degrees / 15.0).round() * 15.0
                } else {
                    degrees
                }
                .rem_euclid(360.0);
                cx.notify();
            }
            Drag::Pan { last } => {
                let d = pos - *last;
                self.view.pan(f32::from(d.x) as f64, f32::from(d.y) as f64);
                self.drag = Some(Drag::Pan { last: pos });
                cx.notify();
            }
            Drag::Move(gesture) => {
                let gesture = *gesture;
                if let Some(point) = self.doc_point(pos) {
                    self.move_drag(gesture, point, cx);
                }
            }
            Drag::Curve(d) => {
                let d = d.clone();
                self.curve_move(&d, pos, cx);
            }
            Drag::Navigator => self.nav_click(pos, cx),
            Drag::LayersSplit { start_y, start_h } => {
                let dy: f32 = (pos.y - *start_y).into();
                if self.layer_panel.compact && !self.layer_panel.controls_open {
                    self.layer_panel.compact_height =
                        Some((*start_h - dy).clamp(120., LAYERS_MAX_H));
                } else {
                    self.layers_h = (*start_h - dy).clamp(LAYERS_MIN_H, LAYERS_MAX_H);
                }
                cx.notify();
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
            Drag::Warp(i) => {
                let i = *i;
                if let Some(d) = self.doc_point(pos)
                    && let Some(w) = &mut self.warp
                    && let Some(g) = w.grid.get_mut(i)
                {
                    *g = d;
                    cx.notify();
                }
            }
            Drag::Vanishing(i) => {
                let i = *i;
                if let Some(d) = self.doc_point(pos) {
                    self.move_vanishing(i, d);
                    cx.notify();
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
                vertical,
            } => {
                let f = if *vertical {
                    crate::widgets::track_fraction_v(track, pos.y)
                } else {
                    track_fraction(track, pos.x)
                };
                if let Some(f) = f {
                    let v = snap(min + f * (max - min), *step);
                    let key = *key;
                    self.apply_slider(key, v, cx);
                }
            }
        }
    }

    fn drag_end(&mut self, cx: &mut Context<Self>) {
        self.end_text_pointer(cx);
        self.snap_lines.clear();
        let remember_brush = matches!(self.drag, Some(Drag::Slider { key, .. })
            if key.is_quick_brush() || matches!(key, SliderKey::ToolSize | SliderKey::ToolOpacity | SliderKey::SideSize | SliderKey::SideOpacity));
        match self.drag.take() {
            Some(Drag::Toolbar(drag)) => self.finish_toolbar(drag, cx),
            None => return,
            Some(Drag::Distort { id, quad, .. }) => self.finish_distort(id, quad, cx),
            Some(Drag::Slider {
                key: SliderKey::Filter(id, _, _),
                ..
            }) => {
                self.finish_filter_gesture(id, cx);
            }
            Some(Drag::Move(_))
            | Some(Drag::Slider { .. })
            | Some(Drag::Transform(_))
            | Some(Drag::Curve(_)) => {
                self.flush_filter_param(cx);
                if self.editor.in_transaction() {
                    self.editor.end();
                }
            }
            Some(Drag::LayersSplit { .. }) => {
                let h = self.layers_h;
                crate::app_state::update_settings(cx, |s| s.layers_height = h);
            }
            Some(Drag::Compare)
            | Some(Drag::Pan { .. })
            | Some(Drag::SidebarResize { .. })
            | Some(Drag::RotateView { .. })
            | Some(Drag::Navigator)
            | Some(Drag::Vanishing(_))
            | Some(Drag::Warp(_)) => {}
            Some(Drag::Tool(t)) => self.tool_up(t, cx),
            Some(Drag::Guide {
                vertical,
                pos,
                existing,
            }) => self.drop_guide(vertical, pos, existing, cx),
        }
        if remember_brush {
            self.remember_active_brush(cx);
        }
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn has_active_gesture(&self) -> bool {
        self.drag.is_some()
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
            self.close_text_field(cx);
            let name = match key {
                SliderKey::Opacity(_) | SliderKey::LayerOpacity(_) => "Opacity".to_string(),
                SliderKey::FillOpacity(_) | SliderKey::LayerFillOpacity(_) => "Fill opacity".into(),
                SliderKey::BlendRange(..) => "Blend If".into(),
                SliderKey::Param(_, k) => k.replace('_', " "),
                SliderKey::Scale(_) => "Scale".into(),
                SliderKey::PenWidth => "Stroke width".into(),
                SliderKey::TextSize => "Text size".into(),
                SliderKey::RefineLo
                | SliderKey::RefineHi
                | SliderKey::RefineGrow
                | SliderKey::RefineFeather => "Refine selection".into(),
                SliderKey::Filter(_, _, k) => k.replace('_', " "),
                SliderKey::Style(_, _, k) => k.replace('_', " "),
                SliderKey::StyleOption(_, _, k) => k.replace('_', " "),
                SliderKey::StyleStop(..) => "Gradient stop".into(),
                SliderKey::StyleContour(..) => "Effect contour".into(),
                SliderKey::StyleGlobalLight(..) => "Global light".into(),
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
            vertical: false,
        });
    }

    /// Keyboard steps use the same complete edit gesture as pointer sliders,
    /// including filter flushing and a single undo entry per step.
    fn slider_key(
        &mut self,
        key: SliderKey,
        norm: f32,
        spec: (f32, f32, f32),
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        let mods = event.keystroke.modifiers;
        if self.drag.is_some()
            || self.editor.in_transaction()
            || mods.control
            || mods.platform
            || mods.alt
        {
            return;
        }
        let (min, max, step) = spec;
        if max <= min {
            return;
        }
        let current = min + norm.clamp(0., 1.) * (max - min);
        let step =
            if step > 0. { step } else { (max - min) / 100. } * if mods.shift { 10. } else { 1. };
        let next = match event.keystroke.key.as_str() {
            "left" | "down" => current - step,
            "right" | "up" => current + step,
            "home" => min,
            "end" => max,
            _ => return,
        }
        .clamp(min, max);
        let Some(bounds) = self.tracks.get(&key).and_then(|track| track.get()) else {
            return;
        };
        cx.stop_propagation();
        if (next - current).abs() < f32::EPSILON {
            return;
        }
        self.slider_down(
            key,
            spec,
            &MouseDownEvent {
                button: MouseButton::Left,
                position: point(
                    bounds.origin.x + bounds.size.width * ((next - min) / (max - min)),
                    bounds.center().y,
                ),
                modifiers: mods,
                click_count: 1,
                first_mouse: false,
            },
            cx,
        );
        self.drag_end(cx);
        cx.notify();
    }

    /// Mouse down on a vertical side slider.
    pub(crate) fn vslider_down(
        &mut self,
        key: SliderKey,
        spec: (f32, f32, f32),
        e: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let track = self.tracks.entry(key).or_default().clone();
        let (min, max, step) = spec;
        if let Some(f) = crate::widgets::track_fraction_v(&track, e.position.y) {
            self.apply_slider(key, snap(min + f * (max - min), step), cx);
        }
        self.drag = Some(Drag::Slider {
            key,
            track,
            min,
            max,
            step,
            vertical: true,
        });
    }

    fn apply_slider(&mut self, key: SliderKey, v: f32, cx: &mut Context<Self>) {
        match key {
            SliderKey::ToolSize | SliderKey::QuickBrushSize => {
                // The track is square-root scaled so small sizes get room.
                let f = ((v - 1.0) / 499.0).clamp(0.0, 1.0);
                self.tools.brush.size = (1.0 + f * f * 499.0).round().max(1.0);
                cx.notify();
            }
            SliderKey::ToolHardness | SliderKey::QuickBrushHardness => {
                self.tools.brush.hardness = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolOpacity | SliderKey::QuickBrushOpacity => {
                self.tools.brush.opacity = v / 100.0;
                cx.notify();
            }
            SliderKey::ToolFlow | SliderKey::QuickBrushFlow => {
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
            SliderKey::Raw(name) => self.raw_slider(name, v, cx),
            SliderKey::ExportQuality => {
                self.export_prefs.quality = v.round().clamp(1.0, 100.0) as u8;
                cx.notify();
            }
            SliderKey::SideSize => self.apply_slider(SliderKey::ToolSize, v, cx),
            SliderKey::SideOpacity => self.apply_slider(SliderKey::ToolOpacity, v, cx),
            SliderKey::ToolPressureCurve => {
                // 0–100 → 2^(-2 … 2).
                self.tools.brush.pressure_curve = 2f32.powf(v / 25.0 - 2.0);
                cx.notify();
            }
            SliderKey::ToolTilt => {
                self.tools.brush.tilt = v / 100.0;
                cx.notify();
            }
            SliderKey::PenWidth => {
                self.tools.pen.width = v;
                self.pen_restyle(cx);
                cx.notify();
            }
            SliderKey::TextSize => {
                // Square-root track, like the brush size.
                let f = ((v - 4.0) / 396.0).clamp(0.0, 1.0);
                let size = (4.0 + f * f * 396.0).round();
                self.restyle_text(move |s| s.size = size, cx);
            }
            SliderKey::Curve(_) => {}
            SliderKey::Filter(id, idx, key) => self.set_filter_param(id, idx, key, v, false, cx),
            SliderKey::Style(id, idx, key) => self.set_style_param(id, idx, key, v, cx),
            SliderKey::StyleOption(id, idx, key) => {
                self.set_style_option_param(id, idx, key, v, cx)
            }
            SliderKey::StyleStop(id, idx, stop, opacity) => {
                self.set_style_stop_param(id, idx, stop, opacity, v, cx)
            }
            SliderKey::StyleContour(id, idx, point, y) => {
                self.set_style_contour_param(id, idx, point, y, v, cx)
            }
            SliderKey::StyleGlobalLight(altitude) => self.set_style_global_light(altitude, v, cx),
            SliderKey::FillOpacity(id) | SliderKey::LayerFillOpacity(id) => {
                let ids = if self.layer_is_selected(id) {
                    self.selected_layer_ids()
                } else {
                    vec![id]
                };
                let commands = ids
                    .into_iter()
                    .filter_map(|id| {
                        self.editor.doc.node(id).map(|node| {
                            let mut options = node.blending;
                            options.fill_opacity = v / 100.;
                            Command::SetBlendingOptions { id, options }
                        })
                    })
                    .collect();
                self.execute_layer_commands("Layer fill", commands, cx);
            }
            SliderKey::BlendRange(id, backdrop, index) => {
                self.set_blend_range(id, backdrop, index, v / 255., cx)
            }
            SliderKey::Tolerance => {
                self.tools.tolerance = v as u8;
                cx.notify();
            }
            SliderKey::Feather => {
                self.tools.feather = v;
                cx.notify();
            }
            SliderKey::RefineLo => self.set_refine(move |r| r.lo = v.min(r.hi - 1.0), cx),
            SliderKey::RefineHi => self.set_refine(move |r| r.hi = v.max(r.lo + 1.0), cx),
            SliderKey::RefineGrow => self.set_refine(move |r| r.grow = v, cx),
            SliderKey::RefineFeather => self.set_refine(move |r| r.feather = v, cx),
            SliderKey::Straighten => {
                self.tools.straighten = v;
                cx.notify();
            }
            SliderKey::PickerSv | SliderKey::PickerHue => {}
            SliderKey::Compare => {
                self.compare = v / 100.0;
                cx.notify();
            }
            SliderKey::Opacity(id) | SliderKey::LayerOpacity(id) => {
                let ids = if self.layer_is_selected(id) {
                    self.selected_layer_ids()
                } else {
                    vec![id]
                };
                self.execute_layer_commands(
                    "Layer opacity",
                    ids.into_iter()
                        .map(|id| Command::SetOpacity {
                            id,
                            opacity: v / 100.0,
                        })
                        .collect(),
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
        let ink = p.ink;
        let compact = crate::app_state::settings(cx).compact_chrome;
        let depth = if d.source_depth == 16 {
            "16 bit"
        } else {
            "8 bit"
        };
        div()
            .id("editor-document-bar")
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(if compact { px(3.) } else { px(5.) })
            .border_b_1()
            .border_color(p.line)
            .child(self.effect_menus(p, cx))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(9.))
                    .child(crate::widgets::tip(
                        // The dimensions are the door to resizing, where
                        // Photoshop's Image menu would be.
                        div()
                            .id("doc-size")
                            .flex()
                            .items_baseline()
                            .gap(px(4.))
                            .px(px(6.))
                            .py(px(2.))
                            .border_1()
                            .border_color(if self.size_panel.is_some() {
                                p.ink
                            } else {
                                p.line
                            })
                            .cursor_pointer()
                            .hover(|s| s.border_color(ink))
                            .font_family(MONO_FONT)
                            .text_size(px(10.5))
                            .text_color(p.muted)
                            .child(format!("{}×{} · {depth}", d.width, d.height))
                            .child(div().text_size(px(8.)).child("▾"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_size_panel(window, cx)
                            }))
                            .test_support(),
                        "Image size (Ctrl-Alt-I) and canvas size (Ctrl-Alt-C): scale the picture, or grow and trim the canvas",
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
                        .border_color(transparent_black())
                        .bg(p.panel)
                        .child(div().size(px(6.)).rounded_full().bg(p.accent))
                        .child(mono("Modified", 10., p.muted)),
                )
            })
            .child(div().flex_1())
            .child(
                crate::widgets::tip(
                    chip("draw-mode", "Draw", self.draw_mode, p)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_draw_mode(cx))),
                    "Draw mode: a compact painting toolbar with History and Layers. Click again for the full photo toolbar.",
                ),
            )
            .child(
                button("save", "Save", false, p).on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::Save), cx);
                })),
            )
            .child(
                button("export", "Export", true, p)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_export_panel(cx))),
            )
    }

    fn context_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tool = self.active_tool_name();
        let options = self.tool_options(p, cx);
        let row = |p: &Palette| {
            div()
                .flex()
                .flex_none()
                .flex_wrap()
                .overflow_hidden()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .font_family(MONO_FONT)
                .text_size(px(10.5))
                .text_color(p.muted)
        };
        let compact = crate::app_state::settings(cx).compact_chrome;
        let first = row(p)
            .py(if compact { px(4.) } else { px(6.) })
            .border_b_1()
            .border_color(p.line)
            .child(div().text_color(p.ink).child(tool))
            .children(options)
            .child(div().flex_1().min_w(px(8.)));
        let font_picker = self.font_picker(p, cx);
        div()
            .id("editor-tool-options")
            .test_support()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .child(first)
            .children(font_picker)
    }

    fn canvas_area(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let overlay = self.overlay(window.scale_factor());
        let zoom_cursor = self.zoom_cursor(p, window);
        let replay = self.replay_overlay(p, cx);
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
            raw_compare: self.raw_split_active(),
            stage: p.stage,
            ink: p.ink,
            accent: p.accent,
            rulers: self.rulers,
        };
        let scene2 = scene.clone();
        // Keep the entire grab target inside the canvas even at 0% and 100%.
        // The hairline itself stays exactly on the renderer's wipe coordinate.
        let compare_handle_left = self.canvas_bounds().map_or(px(0.), |b| {
            (b.size.width * self.compare - window.rem_size() * 0.75)
                .clamp(px(0.), (b.size.width - window.rem_size() * 1.5).max(px(0.)))
        });
        let cache = self.cache.clone();
        let cache2 = self.cache.clone();
        let bounds_cell = self.canvas_bounds.clone();
        let fit_pending = self.fit_pending;
        let weak = cx.entity().downgrade();
        let (w1, w2, w3, w4) = (weak.clone(), weak.clone(), weak.clone(), weak.clone());
        let cursor = match (&self.drag, self.space_held) {
            (Some(Drag::Compare), _) => CursorStyle::ResizeLeftRight,
            (Some(Drag::Pan { .. } | Drag::RotateView { .. }), _) => CursorStyle::ClosedHand,
            (Some(Drag::Guide { vertical: true, .. }), _) => CursorStyle::ResizeLeftRight,
            (
                Some(Drag::Guide {
                    vertical: false, ..
                }),
                _,
            ) => CursorStyle::ResizeUpDown,
            (_, true) => CursorStyle::OpenHand,
            _ if self.tool == Tool::Hand => CursorStyle::OpenHand,
            _ if self.tool == Tool::Type => CursorStyle::IBeam,
            _ if self.tool == Tool::Zoom => CursorStyle::Arrow,
            _ if matches!(self.tool, Tool::Move | Tool::Grade) => CursorStyle::Arrow,
            _ => CursorStyle::Crosshair,
        };
        div()
            .id("canvas")
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .track_focus(&self.canvas_focus)
            .key_context(if self.type_tool.field.is_some() {
                "CanvasText"
            } else {
                "Canvas"
            })
            .when(self.raw_split_active(), |d| {
                // Arrow keys normally dispatch layer-nudge actions before the
                // key-down callback. Comparison owns them while it is open.
                d.on_action(cx.listener(|this, _: &crate::actions::NudgeLeft, _, cx| {
                    this.compare = (this.compare - 0.02).max(0.);
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &crate::actions::NudgeRight, _, cx| {
                    this.compare = (this.compare + 0.02).min(1.);
                    cx.notify();
                }))
                .on_action(
                    cx.listener(|this, _: &crate::actions::NudgeLeftLarge, _, cx| {
                        this.compare = (this.compare - 0.1).max(0.);
                        cx.notify();
                    }),
                )
                .on_action(cx.listener(
                    |this, _: &crate::actions::NudgeRightLarge, _, cx| {
                        this.compare = (this.compare + 0.1).min(1.);
                        cx.notify();
                    },
                ))
            })
            .when(self.type_tool.field.is_some(), |d| {
                d.on_action(cx.listener(|this, _: &crate::actions::Undo, _, cx| {
                    this.close_text_field(cx);
                    this.undo(cx);
                }))
                .on_action(cx.listener(
                    |this, _: &crate::actions::Redo, _, cx| {
                        this.close_text_field(cx);
                        this.redo(cx);
                    },
                ))
            })
            .on_action(
                cx.listener(|this, _: &crate::actions::RepeatFilter, _, cx| {
                    this.repeat_last_filter(cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ToolEllipseMarquee, _, cx| {
                    this.set_select(tools::SelectShape::Ellipse, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ToolPolygonLasso, _, cx| {
                    this.set_select(tools::SelectShape::Polygon, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ToolMagneticLasso, _, cx| {
                    this.set_select(tools::SelectShape::Magnetic, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ToolQuickSelect, _, cx| {
                    this.set_select(tools::SelectShape::Quick, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &crate::actions::ToolSmudge, _, cx| {
                this.set_paint(tools::PaintKind::Smudge, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::ToolLiquify, _, cx| {
                this.set_paint(tools::PaintKind::Liquify, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::ToolEllipse, _, cx| {
                this.set_tool(Tool::Shape, cx);
                this.tools.shape = tools::ShapeKind::Ellipse;
            }))
            .on_action(cx.listener(|this, _: &crate::actions::ToolShape, _, cx| {
                this.set_tool(Tool::Shape, cx);
                this.tools.shape = tools::ShapeKind::Rect;
            }))
            .on_action(cx.listener(|this, _: &crate::actions::ToolMask, _, cx| {
                this.set_tool(Tool::Mask, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::ToolGrade, _, cx| {
                this.set_tool(Tool::Grade, cx)
            }))
            .cursor(cursor)
            .on_hover(cx.listener(|this, _, _, cx| {
                if this.tool == Tool::Zoom {
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e, window, cx| this.canvas_down(e, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, e, window, cx| this.canvas_down(e, window, cx)),
            )
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Right {
                    window.focus(&this.canvas_focus, cx);
                }
            }))
            .on_scroll_wheel(cx.listener(|this, e, _, cx| this.scroll(e, cx)))
            .on_drop(cx.listener(|this, d: &DraggedColor, window, cx| {
                let pos = window.mouse_position();
                this.color_drop(d.0, pos, cx);
            }))
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
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if this.raw_split_active() {
                    let next = match e.keystroke.key.as_str() {
                        "left" => Some(this.compare - 0.02),
                        "right" => Some(this.compare + 0.02),
                        "home" => Some(0.),
                        "end" => Some(1.),
                        _ => None,
                    };
                    if let Some(next) = next {
                        this.compare = next.clamp(0., 1.);
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                }
                if this.text_key_down(e, window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.type_tool.field.is_some() {
                    return;
                }
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
                        if cache.borrow().settle_pending {
                            // The view is moving: redraw once it has rested so
                            // the crisp image replaces the GPU tiles.
                            let w = w1.clone();
                            cx.spawn(async move |cx| {
                                cx.background_executor()
                                    .timer(viewport::SETTLE + std::time::Duration::from_millis(10))
                                    .await;
                                w.update(cx, |_, cx| cx.notify()).ok();
                            })
                            .detach();
                        }
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
                        if let Some(editor) = w4.upgrade() {
                            editor
                                .read(cx)
                                .paint_text_editing(bounds, window, cx, editor.clone());
                        }
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
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                w3.update(cx, |this, cx| {
                                    this.drag_shift = e.modifiers.shift;
                                    this.drag_end(cx);
                                })
                                .ok();
                            }
                        });
                    },
                )
                .size_full(),
            )
            .children(zoom_cursor)
            .children(replay)
            .when(self.raw_split_active(), |d| {
                d.child(
                    div()
                        .absolute()
                        .top_2()
                        .left_2()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .bg(p.panel)
                        .text_color(p.ink)
                        .child("Before · As shot"),
                )
                .child(
                    div()
                        .absolute()
                        .top_2()
                        .right_2()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .bg(p.panel)
                        .text_color(p.ink)
                        .child("After · Edited"),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(self.compare))
                        .w(px(1.))
                        .bg(p.ink),
                )
                .child(
                    div()
                        .id("raw-compare-handle")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(compare_handle_left)
                        .w_6()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::ResizeLeftRight)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                if this.drag.is_none() && !this.editor.in_transaction() {
                                    window.focus(&this.canvas_focus, cx);
                                    this.drag = Some(Drag::Compare);
                                    cx.notify();
                                }
                                cx.stop_propagation();
                            }),
                        )
                        .child(
                            div()
                                .relative()
                                .px_1()
                                .py_2()
                                .bg(p.panel)
                                .text_color(p.ink)
                                .border_1()
                                .border_color(p.ink)
                                .child("↔"),
                        )
                        .test_support(),
                )
            })
            .context_menu({
                let editor = cx.weak_entity();
                let focus = self.canvas_focus.clone();
                move |menu, window, cx| {
                    let Some(editor) = editor.upgrade() else {
                        return menu;
                    };
                    let menu = if editor.read(cx).brushy() {
                        brush_quick::menu(menu, &editor, cx).separator()
                    } else {
                        menu
                    };
                    let menu = editor.update(cx, |editor, cx| {
                        editor.clipboard_menu(menu, focus.clone(), cx)
                    });
                    clipboard::transform_menu(menu, &editor, focus.clone(), window, cx)
                }
            })
    }

    /// GPUI has no native zoom cursor. Keep a platform-independent magnifier
    /// beside the pointer, using the same modifier predicate as zoom clicks.
    fn zoom_cursor(&self, p: &Palette, window: &Window) -> Option<AnyElement> {
        if self.tool != Tool::Zoom || self.space_held || self.drag.is_some() {
            return None;
        }
        let bounds = self.canvas_bounds()?;
        let position = window.mouse_position();
        if !bounds.contains(&position) {
            return None;
        }
        let out = tools::zoom_out(window.modifiers());
        let icon = if out {
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2"><circle cx="10" cy="10" r="7"/><path d="m15 15 6 6M6 10h8"/></svg>"#.as_slice()
        } else {
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2"><circle cx="10" cy="10" r="7"/><path d="m15 15 6 6M6 10h8M10 6v8"/></svg>"#.as_slice()
        };
        Some(
            div()
                .id(if out {
                    "zoom-cursor-out"
                } else {
                    "zoom-cursor-in"
                })
                .absolute()
                .left(
                    (position.x - bounds.origin.x + px(12.))
                        .min((bounds.size.width - px(28.)).max(px(0.))),
                )
                .top(
                    (position.y - bounds.origin.y + px(12.))
                        .min((bounds.size.height - px(28.)).max(px(0.))),
                )
                .size(px(28.))
                .p(px(2.))
                .bg(p.panel)
                .border_1()
                .border_color(p.ink)
                .child(svg().data(icon).size_full().text_color(p.ink))
                .test_support()
                .into_any_element(),
        )
    }

    /// The view controls that used to crowd every tool's options row: zoom,
    /// fit, rotation, rulers, snap, guides and the before/after slider.
    /// They belong to the window, not the tool, so they live in the strip.
    fn view_controls(&mut self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let zoom = format!("{:.0}%", self.view.zoom * 100.0);
        let rot = format!("{:.0}°", self.view.rotation);
        let can_compare = self.editor.differs_from_base() || self.raw_split_active();
        let track = self.tracks.entry(SliderKey::Compare).or_default().clone();
        let compare = self.compare;
        let tip = crate::widgets::tip;
        let mut v: Vec<AnyElement> = vec![
            tip(
                chip("zoom", zoom, false, p)
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_100(cx))),
                "Zoom · click for 100% (Ctrl-1) · Ctrl-scroll on the canvas",
            )
            .into_any_element(),
            tip(
                chip("fit", "fit", false, p)
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_fit(cx))),
                "Fit the picture in the window (Ctrl-0)",
            )
            .into_any_element(),
            tip(
                chip("rot", rot, self.view.rotation != 0.0, p)
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| this.rotate(0.0, cx))),
                "Canvas rotation · click to reset",
            )
            .into_any_element(),
            tip(
                chip("rulers", "rulers", self.rulers, p)
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_rulers(cx))),
                "Rulers (Ctrl-R); drag from a ruler for a guide",
            )
            .into_any_element(),
            tip(
                chip("snap", "snap", self.snap, p)
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.snap = !this.snap;
                        cx.notify();
                    })),
                "Snap moves and shapes to guides, edges and centres",
            )
            .into_any_element(),
        ];
        if !self.editor.doc.guides.is_empty() {
            v.push(
                chip("clear-guides", "clear guides", false, p)
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| this.clear_guides(cx)))
                    .into_any_element(),
            );
        }
        if can_compare || compare > 0.0 {
            v.push(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .font_family(MONO_FONT)
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(div().whitespace_nowrap().child("before / after"))
                    .child(div().w(dim::COMPARE_SLIDER_W).flex_none().child(slider(
                        "compare",
                        compare,
                        track,
                        p,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            this.slider_down(SliderKey::Compare, (0.0, 100.0, 1.0), e, cx)
                        }),
                    )))
                    .child(div().w(px(30.)).child(format!("{:.0}%", compare * 100.0)))
                    .into_any_element(),
            );
        }
        v
    }

    fn status_strip(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let compact = crate::app_state::settings(cx).compact_chrome;
        let controls = if compact {
            Vec::new()
        } else {
            self.view_controls(p, cx)
        };
        let n = self.editor.doc.nodes.len();
        let saved = if self.editor.is_modified() {
            "unsaved"
        } else {
            "saved"
        };
        let autosaved = self
            .autosave_note()
            .map(|a| format!(" · {a}"))
            .unwrap_or_default();
        let right = format!(
            "{n} layer{} · {saved}{autosaved}",
            if n == 1 { "" } else { "s" }
        );
        div()
            .id("editor-status-strip")
            .flex()
            .flex_none()
            .h(if compact { rems(1.5) } else { rems(1.875) })
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(3.))
            .border_t_1()
            .border_color(p.line)
            .overflow_hidden()
            .when(compact, |d| {
                d.bg(p.paper)
                    .child(mono(self.active_tool_name(), 9.5, p.ink))
            })
            .when(!self.suggestions.is_empty(), |d| {
                d.child(mono("Suggestions", 9.5, p.muted).whitespace_nowrap())
            })
            .children(self.suggestion_chips(p, cx))
            .children(self.status.as_ref().map(|(msg, err)| {
                let detail = msg.clone();
                // Provider errors can contain explicit newlines and long URLs.
                // Keep the strip one line while preserving the full diagnostic.
                mono(
                    msg.split_whitespace().collect::<Vec<_>>().join(" "),
                    10.5,
                    if *err { p.accent } else { p.ink },
                )
                .id("editor-status-message")
                .flex_1()
                .min_w_0()
                .h(px(16.))
                .line_height(px(16.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(detail.clone()).build(window, cx)
                })
                .test_support()
            }))
            .when(self.status.is_none(), |d| d.child(div().flex_1()))
            .child(
                mono(right, 10., p.muted)
                    .id("editor-status-meta")
                    .flex_none()
                    .max_w(px(220.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .test_support(),
            )
            .children(controls)
            .test_support()
    }

    fn node_panel(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        self.ensure_layer_search(window, cx);
        self.sidebar(p, window, cx)
    }

    fn scene_graph(&mut self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows = self.filtered_layer_rows();
        let accent = p.accent;
        let header = div()
            .id("graph-header")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .pb(px(6.))
            .drag_over::<DraggedNode>(move |s, _, _, _| s.bg(accent.opacity(0.12)))
            .on_drop(cx.listener(|this, d: &DraggedNode, _, cx| this.drop_on(d.id, None, cx)))
            .child(label("Layers", p))
            .child(div().flex_1())
            .when(self.layer_panel.compact, |header| {
                header.child(
                    chip(
                        "layer-controls-toggle",
                        "Controls",
                        self.layer_panel.controls_open,
                        p,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.layer_panel.controls_open = !this.layer_panel.controls_open;
                        cx.notify();
                    }))
                    .test_support(),
                )
            })
            .child(
                chip("sidebar-panels-toggle", "Panels ▾", self.sidebar_menu, p)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_menu = !this.sidebar_menu;
                        cx.notify();
                    }))
                    .test_support(),
            );
        let actions = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .gap(px(5.))
            .pt(px(6.))
            .child(
                chip("add", "+ Layer", self.menu == Some(Menu::Add), p).on_click(cx.listener(
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
                chip("grp", "Group", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.group_selected(cx))),
            )
            .child(
                chip("dup", "Duplicate", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate_selected(cx))),
            )
            .child(
                chip("del", "Delete", false, p)
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
            items.push(("LUT from .cube file…".into(), Node::group(0, "__lut__")));
            items.push(("Empty group".into(), Node::group(0, "Group")));
            let mut list: Vec<(SharedString, MenuAction)> =
                vec![("Empty layer (transparent)".into(), MenuAction::NewLayer)];
            list.extend(
                items
                    .into_iter()
                    .map(|(l, n)| (l, MenuAction::Add(Box::new(n)))),
            );
            list.push((
                "Remove background (AI)".into(),
                MenuAction::RemoveBackground,
            ));
            list.push(("Depth map (AI)".into(), MenuAction::DepthMap));
            list.push(("Restore faces (AI)".into(), MenuAction::RestoreFaces));
            list.push((
                format!("Upscale ×{} (AI)", emulsion_ai::upscale::factor()).into(),
                MenuAction::Upscale,
            ));
            self.menu_list("add-menu", list, p, cx)
        });

        let controls = (!self.layer_panel.compact || self.layer_panel.controls_open).then(|| {
            div()
                .id("layer-controls")
                .flex()
                .flex_col()
                .flex_none()
                .when(self.layer_panel.compact, |controls| {
                    controls.max_h_40().overflow_y_scroll()
                })
                .child(self.layer_filter_controls(p, cx))
                .children(self.layer_blend_controls(p, cx))
                .children(self.layer_lock_controls(p, cx))
        });
        let mut row_els: Vec<AnyElement> = Vec::new();
        for row in &rows {
            row_els.push(self.node_row(row.id, row.depth, p, cx).into_any_element());
            if let Some(effects) = self.layer_effect_rows(row.id, row.depth, p, cx) {
                row_els.push(effects);
            }
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(2.))
            .px(px(10.))
            .pt(px(8.))
            .pb(px(8.))
            .border_b_1()
            .border_color(p.line)
            .child(header)
            .children(controls)
            .children(self.sidebar_panel_menu(p, cx))
            .when(rows.is_empty(), |d| {
                d.child(mono(
                    if self.editor.doc.nodes.is_empty() {
                        "Empty document"
                    } else {
                        "No matching layers"
                    },
                    10.,
                    p.muted,
                ))
            })
            .child(
                div()
                    .id("sidebar-layers-list")
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    // Rows stop the click; what reaches here is empty space.
                    .on_click(cx.listener(|this, _, _, cx| this.deselect_layer(cx)))
                    .children(row_els)
                    // Always a little empty space to click, even when the
                    // list is full.
                    .child(div().h(px(10.)).flex_none())
                    .test_support(),
            )
            .child(actions)
            .child(
                div()
                    .id("layer-add-menu")
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .children(add_menu),
            )
    }

    fn node_row(
        &mut self,
        id: NodeId,
        depth: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let n = self.editor.doc.node(id).expect("row node").clone();
        let on = self.layer_is_selected(id);
        let (fg, bg, border) = if on {
            (p.paper, p.ink, p.ink)
        } else {
            (p.ink, transparent_black(), p.line)
        };
        let meta_fg = if on { p.paper } else { p.muted };
        let mask_active = self.selected == Some(id) && self.tools.mask_edit;
        let mask_thumb = n.mask.as_ref().map(|mask| self.mask_thumbnail(id, mask));
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
            NodeKind::Smart { .. } => div()
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(p.line)
                .text_size(px(11.))
                .child("fx")
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
            NodeKind::Text { .. } => div()
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(p.line)
                .text_color(p.ink)
                .font_family(MONO_FONT)
                .text_size(px(12.))
                .child("T")
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
        let ai_badge = n.model_id().map(|_| {
            div()
                .flex_none()
                .px(px(3.))
                .border_1()
                .border_color(p.accent)
                .text_color(p.accent)
                .font_family(MONO_FONT)
                .text_size(px(8.5))
                .child("AI")
        });
        let name_el: AnyElement = match &self.renaming {
            Some((rid, state, _)) if *rid == id => Input::new(state)
                .appearance(false)
                .bordered(false)
                .into_any_element(),
            _ => div()
                .id(("layer-name", id))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(12.5))
                .when(!n.visible, |d| d.opacity(0.45))
                .child(n.name.clone())
                .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                    if e.click_count() >= 2 {
                        cx.stop_propagation();
                        this.select_layer_row(id, false, false, cx);
                        this.start_rename(id, window, cx);
                    }
                }))
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
            .flex_none()
            .items_center()
            .gap(px(8.))
            .pl(px(6. + depth as f32 * 14.))
            .pr(px(8.))
            .py(if crate::app_state::settings(cx).compact_chrome {
                px(3.)
            } else {
                px(6.)
            })
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .cursor_pointer()
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&this.panel_focus, cx);
                this.menu = None;
                this.select_layer_row(
                    id,
                    e.modifiers().control || e.modifiers().platform,
                    e.modifiers().shift,
                    cx,
                );
                if e.click_count() >= 2 {
                    this.open_blending_options(id, window, cx);
                }
            }))
            .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
            .capture_any_mouse_down(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.button == MouseButton::Right {
                        window.focus(&this.panel_focus, cx);
                        this.menu = None;
                        this.select_layer_context(id, cx);
                    }
                }),
            )
            .drag_over::<DraggedNode>(move |s, _, _, _| {
                s.border_color(accent).bg(accent.opacity(0.12))
            })
            .on_drop(cx.listener(move |this, d: &DraggedNode, window, cx| {
                if window.modifiers().alt {
                    this.transfer_layer_style(d.id, id, true, cx);
                } else {
                    this.drop_on(d.id, Some(id), cx);
                }
            }))
            .child(
                div()
                    .id(("layer-color", id))
                    .w_1()
                    .h_6()
                    .flex_none()
                    .bg(layers_panel::label_color(n.color_label, p))
                    .aria_label(format!("{} color label", n.color_label.label()))
                    .test_support(),
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
            .child(
                div()
                    .id(("layer-content", id))
                    .flex_none()
                    .p_0p5()
                    .border_1()
                    .border_color(if on && !mask_active { p.accent } else { p.line })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.select_layer_content(id, cx);
                        window.focus(&this.panel_focus, cx);
                    }))
                    .child(chip_el)
                    .test_support(),
            )
            .when(n.mask.is_some(), |row| {
                use gpui_kit::component::button::{Button, ButtonVariants};
                use gpui_kit::component::{Disableable, Sizable};
                let linked = n.mask_linked;
                row.child(
                    Button::new(("mask-link", id))
                        .label(if linked { "↔" } else { "·" })
                        .xsmall()
                        .ghost()
                        .disabled(self.editor.doc.locked_ancestor(id).is_some())
                        .tooltip(if linked {
                            "Unlink mask from layer"
                        } else {
                            "Link mask to layer"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.execute(
                                Command::SetMaskLinked {
                                    id,
                                    linked: !linked,
                                },
                                cx,
                            );
                        })),
                )
            })
            .children(mask_thumb.map(|thumb| {
                div()
                    .id(("layer-mask", id))
                    .flex_none()
                    .p_0p5()
                    .border_1()
                    .border_color(if mask_active { p.accent } else { p.line })
                    .opacity(if n.mask_enabled { 1. } else { 0.45 })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.select_layer_mask(id, cx);
                        window.focus(&this.panel_focus, cx);
                    }))
                    .child(
                        img(ImageSource::Render(thumb))
                            .size_5()
                            .object_fit(ObjectFit::Contain),
                    )
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Edit layer mask")
                            .build(window, cx)
                    })
                    .test_support()
            }))
            .child(name_el)
            .when(self.layer_is_linked(id), |row| {
                row.child(
                    div()
                        .id(("layer-link-state", id))
                        .text_xs()
                        .child("↔")
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "Linked layers move and transform together",
                            )
                            .build(window, cx)
                        }),
                )
            })
            .children(ai_badge)
            .child(
                mono(meta, 9.5, meta_fg)
                    .flex_none()
                    .max_w_20()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap(),
            )
            .when(
                n.locked || n.locks.pixels || n.locks.position || n.locks.transparency,
                |row| {
                    row.child(
                        div()
                            .id(("layer-lock-state", id))
                            .text_xs()
                            .text_color(meta_fg)
                            .child("🔒")
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(
                                    "Layer has locks enabled",
                                )
                                .build(window, cx)
                            }),
                    )
                },
            )
            .test_support()
            .context_menu({
                let editor = cx.weak_entity();
                let focus = self.panel_focus.clone();
                move |menu, window, cx| {
                    let Some(editor) = editor.upgrade() else {
                        return menu;
                    };
                    layer_menu::layer_context_menu(menu, &editor, id, focus.clone(), window, cx)
                }
            })
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
                .child(mono(
                    "No layer selected. Click one to edit it; a new stroke starts its own layer.",
                    10.,
                    p.muted,
                ));
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
        let advanced = crate::app_state::settings(cx).advanced_tools;
        body = body.child(label(n.name.clone(), p));
        if let Some(raw) = self.raw_panel(id, p, cx) {
            body = body.child(raw);
        }

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
            let editing = self.tool == Tool::Mask;
            toggles = toggles
                .child(
                    chip("mask-edit", "edit mask", editing, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.set_tool(if editing { Tool::Brush } else { Tool::Mask }, cx);
                        },
                    )),
                )
                .child(
                    chip("mask-inv", "invert", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.invert_mask(cx))),
                )
                .child(
                    chip("mask-feather", "feather 6", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.feather_mask(6.0, cx))),
                )
                .child(
                    chip("mask-sel", "to selection", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.mask_to_selection(cx))),
                )
                .child(
                    chip("mask-del", "− mask", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.remove_mask(cx))),
                );
        } else if !matches!(n.kind, NodeKind::Fill { .. }) || self.editor.doc.selection.is_some() {
            let from_sel = self.editor.doc.selection.is_some();
            toggles = toggles.child(
                chip(
                    "mask-add",
                    if from_sel {
                        "+ mask from selection"
                    } else {
                        "+ mask"
                    },
                    false,
                    p,
                )
                .on_click(cx.listener(|this, _, _, cx| this.add_mask(cx)))
                .test_support(),
            );
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

        // What the layer is made of comes next: an adjustment's sliders, a
        // smart layer's filters, a text or path's description.
        match &n.kind {
            NodeKind::Adjust(a) => {
                let a = a.clone();
                for extra in self.adjust_extras(id, &a, p, cx) {
                    body = body.child(extra);
                }
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
                if a.params().is_empty() && !matches!(a, Adjustment::Curves { .. }) {
                    body = body.child(mono("no parameters", 10., p.muted));
                }
            }
            NodeKind::Raster { raster, placement } if advanced => {
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
            NodeKind::Raster { .. } => {}
            NodeKind::Smart {
                source,
                filters,
                filter_styles,
                placement,
                ..
            } => {
                body = body.child(mono(
                    format!(
                        "smart · {}×{} px at {:.0}, {:.0}",
                        source.width(),
                        source.height(),
                        placement.x,
                        placement.y
                    ),
                    10.5,
                    p.muted,
                ));
                let filters = filters.clone();
                let filter_styles = filter_styles.clone();
                for el in self.smart_panel(id, &filters, &filter_styles, p, cx) {
                    body = body.child(el);
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
            NodeKind::Text { spec, .. } => {
                let font = if spec.font.is_empty() {
                    "default font".to_string()
                } else {
                    spec.font.clone()
                };
                body = body.child(mono(
                    format!(
                        "{:?} · {} · {:.0}px{}{} · edit with Type (T)",
                        spec.label(),
                        font,
                        spec.size,
                        if spec.bold { " bold" } else { "" },
                        if spec.italic { " italic" } else { "" },
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
                    format!("{k} layer{} inside", if k == 1 { "" } else { "s" }),
                    10.5,
                    p.muted,
                ));
            }
        }

        // The rarer layer controls, behind the inspector's "advanced" switch.
        // These include layer styles, Smart Object conversion,
        // rotating the object by an angle, the model that made it.
        let styled = matches!(
            n.kind,
            NodeKind::Raster { .. }
                | NodeKind::Smart { .. }
                | NodeKind::Path { .. }
                | NodeKind::Text { .. }
        );
        let has_more = styled || n.model_id().is_some();
        if has_more {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(mono("MORE", 9.5, p.muted))
                    .child(
                        chip(
                            "props-advanced",
                            if advanced { "hide ▴" } else { "show ▾" },
                            advanced,
                            p,
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            crate::app_state::update_settings(cx, |s| s.advanced_tools = !advanced);
                        }))
                        .test_support(),
                    ),
            );
        }
        if advanced {
            if styled {
                let styles = n.styles.clone();
                for el in self.styles_panel(id, &styles, p, cx) {
                    body = body.child(el);
                }
            }
            if let Some(rotation) = self.rotation_controls(p, cx) {
                body = body.child(rotation);
            }
            if matches!(n.kind, NodeKind::Raster { .. }) {
                body = body.child(
                    chip("smart", "Convert to Smart Object", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.convert_smart(cx))),
                );
            }
            if let Some(model) = n.model_id() {
                let model_name = emulsion_ai::models::spec(model)
                    .map(|m| m.name)
                    .unwrap_or(model);
                body = body.child(mono(
                    format!("made by {model_name}, on this machine"),
                    10.,
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
            .child(
                slider(
                    id,
                    norm,
                    track,
                    p,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        this.slider_down(key, spec, e, cx);
                    }),
                )
                .tab_index(0)
                .key_context("Slider")
                .role(Role::Slider)
                .aria_label(name.to_string())
                .test_support()
                .aria_value(format!("{}", spec.0 + norm * (spec.1 - spec.0)))
                .aria_min_numeric_value(spec.0 as f64)
                .aria_max_numeric_value(spec.1 as f64)
                .aria_description(
                    "Arrow keys adjust; Shift adjusts faster; Home and End go to limits",
                )
                .focus_visible(|s| s.bg(p.accent.opacity(0.2)))
                .on_key_down(
                    cx.listener(move |this, e, _, cx| this.slider_key(key, norm, spec, e, cx)),
                ),
            )
    }

    fn menu_list(
        &self,
        id: &'static str,
        items: Vec<(SharedString, MenuAction)>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let accent = p.accent;
        let accent_fg = p.accent_fg;
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
                    .hover(move |s| s.bg(accent).text_color(accent_fg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match &*action {
                            MenuAction::Blend(id, m) => {
                                let (id, m) = (*id, *m);
                                this.execute(Command::SetBlend { id, blend: m }, cx);
                            }
                            MenuAction::Add(node) if node.name == "__lut__" => {
                                this.import_lut(None, cx)
                            }
                            MenuAction::Add(node) => this.add_node((**node).clone(), cx),
                            MenuAction::NewLayer => {
                                if this.new_empty_layer(cx).is_some() {
                                    this.set_status(
                                        "Empty transparent layer added. Paint on it, or fill a selection.",
                                        false,
                                        cx,
                                    );
                                }
                            }
                            MenuAction::RemoveBackground => this.remove_background(cx),
                            MenuAction::DepthMap => this.depth_layer(cx),
                            MenuAction::RestoreFaces => this.restore_faces(cx),
                            MenuAction::Upscale => this.ai_upscale(cx),
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
    /// An empty, transparent pixel layer.
    NewLayer,
    Add(Box<Node>),
    RemoveBackground,
    DepthMap,
    RestoreFaces,
    Upscale,
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(workspace) = &self.brush_workspace {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(workspace.clone())
                .into_any_element();
        }
        self.flush_live_stroke(cx);
        if self.focus_watchers.is_none() {
            let blur = cx.on_focus_out(&self.canvas_focus, window, |this, _, _, cx| {
                if this.space_held {
                    this.space_held = false;
                    cx.notify();
                }
            });
            let activation = cx.observe_window_activation(window, |this, _, cx| {
                if this.space_held {
                    this.space_held = false;
                    cx.notify();
                }
            });
            self.focus_watchers = Some((blur, activation));
        }
        let p = theme::palette(cx);
        self.sync_trees(cx);
        self.sync_transform_fields(window, cx);
        self.sync_rotation_fields(window, cx);
        self.sync_style_color_pickers(window, cx);
        self.ensure_gen_prompt(window, cx);
        if crate::app_state::settings(cx).compact_chrome {
            return self.compact_editor(&p, window, cx);
        }
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
        let canvas = self.canvas_area(&p, window, cx);
        let picker = self.picker(&p, cx);
        self.refresh_suggestions(cx);
        let strip = self.status_strip(&p, cx);
        let ask = self.ask_bar(&p, cx);
        let size_panel = self.size_panel_view(&p, cx);
        let export_panel = self.export_panel_view(&p, cx);
        let dock = self.assistant_dock(&p, cx);
        let panel = self.node_panel(&p, window, cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .track_focus(&self.focus)
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.drag_shift = event.modifiers.shift;
                if matches!(this.tool, Tool::Zoom | Tool::Shape)
                    || matches!(this.drag, Some(Drag::Transform(_)))
                {
                    cx.notify();
                }
            }))
            .child(doc_bar)
            .child(context)
            .child(
                div()
                    .id("editor-work-area")
                    .test_support()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(rail)
                    .children(self.draw_side_sliders(&p, cx))
                    .child(
                        div()
                            .id("editor-canvas-column")
                            .test_support()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .children(size_panel)
                            .children(export_panel)
                            .children(ask)
                            .child(canvas)
                            .children(dock),
                    )
                    .child(panel)
                    .children(picker),
            )
            .child(strip)
            .into_any_element()
    }
}

#[cfg(test)]
mod rendering_tests {
    use super::*;
    use core::prelude::v1::test;

    fn view(cx: &mut TestAppContext) -> Entity<EditorView> {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_global(crate::app_state::AppSettings(Default::default()));
            let mut doc = Document::new(64, 64);
            Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Shape",
                    NodeKind::Fill {
                        rgba: [255, 0, 0, 255],
                    },
                )),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
            cx.new(|cx| EditorView::new(doc, None, None, None, "test".into(), cx))
        })
    }

    #[gpui_kit::test]
    fn completed_tree_shows_progress_and_keeps_later_changes_dirty(cx: &mut TestAppContext) {
        let view = view(cx);
        view.update(cx, |this, _| {
            let generation = this.render_gen;
            let displayed_tree = this.tree.clone();
            let revision = this.editor.revision;
            let snapshot = this.editor.doc.composite_tree();
            this.editor
                .execute(Command::SetOpacity {
                    id: 1,
                    opacity: 0.5,
                })
                .unwrap();
            this.tree_dirty = this.editor.take_dirty();
            this.install_completed_tree(snapshot, revision, this.tree_request, &displayed_tree);
            assert!(
                this.render_gen > generation,
                "progress must appear during continued editing"
            );
            assert_eq!(this.tree.nodes[0].opacity, 1.0);
            assert_eq!(
                this.tree_dirty,
                emulsion_core::Dirty::All,
                "the next tree still invalidates changed pixels"
            );

            let displayed_tree = this.tree.clone();
            this.install_completed_tree(
                this.editor.doc.composite_tree(),
                this.editor.revision,
                this.tree_request,
                &displayed_tree,
            );
            assert_eq!(this.tree.nodes[0].opacity, 0.5);
            assert_eq!(this.tree_dirty, emulsion_core::Dirty::Nothing);

            let displayed_tree = this.tree.clone();
            let request = this.tree_request;
            this.tree_request += 1; // animation preview changed without an edit
            this.install_completed_tree(
                this.editor.doc.composite_tree(),
                this.editor.revision,
                request,
                &displayed_tree,
            );
            assert_eq!(this.tree_dirty, emulsion_core::Dirty::All);
        });
    }

    #[gpui_kit::test]
    fn completed_tree_cannot_overwrite_a_newer_synchronous_view(cx: &mut TestAppContext) {
        let view = view(cx);
        view.update(cx, |this, _| {
            let displayed_tree = this.tree.clone();
            let revision = this.editor.revision;
            let snapshot = this.editor.doc.composite_tree();
            this.editor
                .execute(Command::SetOpacity {
                    id: 1,
                    opacity: 0.25,
                })
                .unwrap();
            this.tree_dirty = this.editor.take_dirty();
            this.install_tree(this.editor.doc.composite_tree());
            let latest = this.render_gen;
            this.install_completed_tree(snapshot, revision, this.tree_request, &displayed_tree);
            assert_eq!(this.render_gen, latest);
            assert_eq!(this.tree.nodes[0].opacity, 0.25);
        });
    }

    #[gpui_kit::test]
    fn checker_cache_change_does_not_drop_a_completed_content_tree(cx: &mut TestAppContext) {
        let view = view(cx);
        view.update(cx, |this, _| {
            this.editor
                .execute(Command::SetOpacity {
                    id: 1,
                    opacity: 0.5,
                })
                .unwrap();
            let revision = this.editor.revision;
            let snapshot = this.editor.doc.composite_tree();
            let displayed_tree = this.tree.clone();
            // Theme/checker changes invalidate tiles but do not install a new
            // content tree. The in-flight content must still reach the screen.
            this.gen_counter += 1;
            this.render_gen = this.gen_counter;
            this.cache.borrow_mut().clear();
            this.install_completed_tree(snapshot, revision, this.tree_request, &displayed_tree);
            assert_eq!(this.tree.nodes[0].opacity, 0.5);
            assert!(!Arc::ptr_eq(&this.tree, &displayed_tree));
        });
    }

    #[gpui_kit::test]
    fn selected_object_rotation_is_undoable_without_switching_tools(cx: &mut TestAppContext) {
        let view = view(cx);
        view.update(cx, |this, cx| {
            let path = Arc::new(
                emulsion_raster::vector::Path::from_svg("M 12 12 L 44 12 L 12 28 Z").unwrap(),
            );
            let id = this
                .editor
                .execute(Command::AddNode {
                    node: Box::new(Node::path(0, "Drawing", path, Default::default(), 64, 64)),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap();
            this.selected = Some(id);
            let before = this.editor.doc.clone();
            let steps = this.editor.history.len();
            assert!(this.tool == Tool::Hand);
            this.rotate_selected_node(90.0, cx);
            assert!(matches!(
                this.editor.doc.node(id).unwrap().kind,
                NodeKind::Path { .. }
            ));
            assert_ne!(this.editor.doc.node(id), before.node(id));
            assert_eq!(
                this.editor.doc.node(1),
                before.node(1),
                "other layers are untouched"
            );
            assert_eq!(this.editor.history.len(), steps + 1);
            this.undo(cx);
            assert_eq!(this.editor.doc, before);
            this.assistant.running = true;
            this.rotate_selected_node(90.0, cx);
            assert_eq!(
                this.editor.doc, before,
                "active assistant drawing keeps a stable target"
            );
        });
    }
}
