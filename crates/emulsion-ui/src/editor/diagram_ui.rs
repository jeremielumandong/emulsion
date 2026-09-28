//! Native diagram stencils, connection gestures and graph properties.
use super::*;
use emulsion_core::{
    diagram::{self, Endpoint, Layout, Port, Routing, ShapeKind},
    project::ProjectKind,
};
use gpui_kit::component::{
    Disableable, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    input::{InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};

#[path = "diagram_hit_test.rs"]
mod hit_test;
#[path = "diagram_object_menu.rs"]
mod object_menu;
#[path = "diagram_reconnect.rs"]
mod reconnect;
#[path = "diagram_used_stencils.rs"]
mod used_stencils;
pub(crate) use used_stencils::DraggedDocumentStencil;

type DiagramPalette = ([u8; 4], [u8; 4], [u8; 4]);
const DIAGRAM_STYLES: [(&str, DiagramPalette); 4] = [
    (
        "White",
        (
            diagram::DEFAULT_FILL,
            diagram::DEFAULT_LINE,
            diagram::DEFAULT_TEXT,
        ),
    ),
    (
        "Soft teal",
        (
            [178, 242, 235, 255],
            diagram::DEFAULT_LINE,
            diagram::DEFAULT_TEXT,
        ),
    ),
    (
        "Soft blue",
        (
            [236, 244, 255, 255],
            diagram::DEFAULT_LINE,
            diagram::DEFAULT_TEXT,
        ),
    ),
    (
        "Charcoal",
        ([75, 81, 89, 255], diagram::DEFAULT_LINE, [255; 4]),
    ),
];

#[derive(Clone)]
pub(super) struct DraggedStencil(pub diagram::stencils::Stencil);
impl Render for DraggedStencil {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .px_3()
            .py_2()
            .rounded(px(6.))
            .bg(p.panel)
            .border_1()
            .border_color(p.accent)
            .text_color(p.ink)
            .child(self.0.label)
    }
}

pub(super) struct DiagramUi {
    hit_cache: RefCell<hit_test::HitCache>,
    used: used_stencils::UsedStencils,
    copied_style: Option<object_menu::ObjectStyle>,
    open: bool,
    pub(super) search: Option<Entity<InputState>>,
    subscription: Option<Subscription>,
    pub(super) connecting: bool,
    source: Option<Endpoint>,
    pointer: Option<(f64, f64)>,
    hover_shape: Option<NodeId>,
    press: Option<(f64, f64)>,
    dragged: bool,
    marquee: Option<DiagramMarquee>,
    reconnect: Option<(NodeId, bool)>,
    endpoint_drag: Option<reconnect::EndpointDrag>,
    pub(super) grid: bool,
    property_tab: usize,
    pub(super) library_tab: usize,
    pub(super) theme_selection: bool,
    pub(super) stencil_page: usize,
    pub(super) library_installing: bool,
    pub(super) expanded_stencil_packs: std::collections::HashSet<u64>,
    import_notes: Vec<String>,
    pub(super) collapsed_categories: std::collections::HashSet<&'static str>,
}
struct DiagramMarquee {
    start: (f64, f64),
    end: (f64, f64),
    base: Vec<NodeId>,
}

impl Default for DiagramUi {
    fn default() -> Self {
        Self {
            hit_cache: Default::default(),
            used: Default::default(),
            copied_style: None,
            open: true,
            search: None,
            subscription: None,
            connecting: false,
            source: None,
            pointer: None,
            hover_shape: None,
            press: None,
            dragged: false,
            marquee: None,
            reconnect: None,
            endpoint_drag: None,
            grid: true,
            property_tab: 0,
            library_tab: 0,
            theme_selection: false,
            stencil_page: 0,
            library_installing: false,
            expanded_stencil_packs: Default::default(),
            import_notes: Vec::new(),
            collapsed_categories: diagram::stencils::CATEGORIES
                .iter()
                .copied()
                .filter(|c| !matches!(*c, "General" | "Flowchart"))
                .collect(),
        }
    }
}
impl EditorView {
    pub(super) fn prepare_diagram_svg(&mut self, cx: &mut Context<Self>) {
        if !self.is_diagram() {
            return;
        }
        let key = (self.editor.active_page(), self.editor.revision);
        {
            let mut cache = self.svg_canvas.borrow_mut();
            if cache.building || cache.requested == Some(key) {
                return;
            }
            cache.requested = Some(key);
            cache.building = true;
        }
        let doc = Arc::new(self.editor.doc.clone());
        let previous = self
            .svg_canvas
            .borrow()
            .document
            .clone()
            .filter(|(revision, _)| revision.0 == key.0);
        let source = doc.clone();
        let previous_scene = self
            .svg_canvas
            .borrow()
            .scene
            .as_ref()
            .filter(|(r, _)| r.0 == key.0)
            .map(|(_, s)| s.clone());
        cx.spawn(async move |this, cx| {
            let (scene, damage) = cx
                .background_spawn(async move {
                    let damage = previous.and_then(|(revision, old)| {
                        emulsion_io::svg_viewport::changed_bounds(&old, &source)
                            .map(|bounds| (revision, bounds))
                    });
                    (
                        emulsion_io::svg_viewport::SvgViewport::updated(
                            &source,
                            previous_scene.as_deref(),
                        )
                        .map(Arc::new),
                        damage,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                {
                    let mut cache = this.svg_canvas.borrow_mut();
                    cache.building = false;
                    if this.editor.active_page() == key.0 {
                        // A completed intermediate frame is useful while dragging;
                        // the next render schedules only the latest revision.
                        cache.scene = scene.ok().map(|scene| (key, scene));
                        cache.document = Some((key, doc));
                        cache.damage = damage;
                    }
                }
                this.notify_canvas(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn diagram_canvas_toolbar(
        &self,
        p: &Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_diagram() {
            return None;
        }
        let narrow = window.viewport_size().width < px(1100.);
        let owner = cx.weak_entity();
        let mut bar = div()
            .id("diagram-canvas-toolbar")
            .test_support()
            .h(px(38.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .min_w_0()
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line);
        if !narrow {
            let name = self
                .editor
                .page_list()
                .iter()
                .find(|page| page.id == self.editor.active_page())
                .map(|page| page.name.clone())
                .unwrap_or_else(|| self.name.clone());
            bar = bar.child(
                div()
                    .max_w(px(160.))
                    .text_ellipsis()
                    .text_size(px(12.))
                    .child(name),
            );
        }
        for (index, (label, icon, tool)) in [
            ("Select", "move", Tool::Move),
            ("Text", "type", Tool::Type),
            ("Pan", "hand", Tool::Hand),
        ]
        .into_iter()
        .enumerate()
        {
            bar = bar.child(
                Button::new(("diagram-canvas-tool", index))
                    .accessibility_label(label)
                    .tooltip(label)
                    .xsmall()
                    .ghost()
                    .size(px(26.))
                    .selected(self.tool == tool && !self.diagram_ui.connecting)
                    .child(rail::tool_icon(icon).text_color(p.ink).size(px(13.)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diagram_cancel_connection();
                        this.set_tool(tool, cx);
                    })),
            );
        }
        bar = bar
            .child(
                Button::new("diagram-canvas-connect")
                    .accessibility_label("Connect shapes")
                    .tooltip("Connect shapes")
                    .xsmall()
                    .ghost()
                    .size(px(26.))
                    .selected(self.diagram_ui.connecting)
                    .child(rail::tool_icon("link").text_color(p.ink).size(px(13.)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let active = this.diagram_ui.connecting;
                        this.set_tool(Tool::Move, cx);
                        this.diagram_cancel_connection();
                        this.diagram_ui.connecting = !active;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("diagram-canvas-layout")
                    .label("Auto layout")
                    .xsmall()
                    .outline()
                    .h(px(24.))
                    .dropdown_menu(move |mut menu, _, _| {
                        for layout in Layout::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(layout.label()).on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| this.layout_diagram(layout, cx))
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
            )
            .child(
                Button::new("diagram-canvas-group")
                    .label("Group")
                    .tooltip("Group selection (Ctrl+G)")
                    .xsmall()
                    .ghost()
                    .disabled(self.selected_layer_roots().len() < 2)
                    .on_click(cx.listener(|this, _, _, cx| this.group_selected(cx))),
            )
            .child(
                Button::new("diagram-canvas-ungroup")
                    .label("Ungroup")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.ungroup_selected(cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("diagram-canvas-fit")
                    .label(format!("{:.0}%", self.view.zoom * 100.))
                    .tooltip("Fit diagram")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_fit(cx))),
            );
        Some(bar.into_any_element())
    }

    fn import_diagram_file(&mut self, cx: &mut Context<Self>) {
        self.import_diagram_file_named(
            "Import draw.io, Visio or Lucid pages into this diagram",
            cx,
        );
    }

    pub(super) fn import_diagram_file_named(
        &mut self,
        title: &'static str,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(title.into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let ticket = this
                .update(cx, |this, cx| {
                    this.prepare_page_action(cx).then(|| this.edit_ticket())
                })
                .ok()
                .flatten();
            let Some(ticket) = ticket else {
                return;
            };
            let result = cx
                .background_spawn(async move { emulsion_io::diagram_import::read(&path) })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(
                        "The project changed during import. Import the diagram again.",
                        false,
                        cx,
                    );
                    return;
                }
                match result.map_err(|e| e.to_string()).and_then(|imported| {
                    this.editor
                        .import_pages(imported.project)
                        .map(|ids| (ids, imported.warnings))
                }) {
                    Ok((ids, warnings)) => {
                        this.diagram_import_notes(warnings.clone());
                        this.after_change(cx);
                        this.set_status(
                            format!(
                                "Imported {} editable page(s). {}",
                                ids.len(),
                                if warnings.is_empty() {
                                    String::new()
                                } else {
                                    format!(
                                        "{} import notes are available in the Shapes drawer.",
                                        warnings.len()
                                    )
                                }
                            ),
                            !warnings.is_empty(),
                            cx,
                        );
                    }
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
    fn export_drawio_file(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(project) = self.editor.snapshot() else {
            return;
        };
        let dir = self
            .editor
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ".".into())
            });
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{}.drawio", self.name)));
        cx.spawn(async move|this,cx|{
            let Ok(Ok(Some(mut path)))=rx.await else{return;};path.set_extension("drawio");let output=path.clone();
            let result=cx.background_spawn(async move{emulsion_io::drawio::write(&project,&output)}).await;
            this.update(cx,|this,cx|match result{Ok(())=>this.set_status(format!("Exported editable diagram to {}. Save the .emu project to retain history and all native effects.",path.display()),false,cx),Err(e)=>this.set_status(e.to_string(),true,cx)}).ok();
        }).detach();
    }

    pub(crate) fn save_imported_stencils(&mut self, pages: &[u64], cx: &mut Context<Self>) {
        let Some(mut project) = self.editor.snapshot() else {
            return;
        };
        project.pages.retain(|p| pages.contains(&p.meta.id));
        if project.pages.is_empty() {
            return;
        }
        project.active = project.pages[0].meta.id;
        let name = project.pages[0].meta.name.clone();
        cx.spawn(async move |this,cx| {
            let result=cx.background_spawn(async move {
                emulsion_io::document_stencils::save(&emulsion_io::creative_library::root(),&project,&name)
            }).await;
            this.update(cx,|v,cx|match result {
                Ok(_)=>v.refresh_creative_library(cx),
                Err(e)=>{v.diagram_ui.import_notes.push(format!("Could not save reusable stencils: {e}"));v.set_status("Diagram imported; reusable stencil library could not be saved. See import notes.",true,cx);}
            }).ok();
        }).detach();
    }
    pub(crate) fn diagram_import_notes(&mut self, notes: Vec<String>) {
        self.diagram_ui.import_notes = notes;
    }

    pub(super) fn show_diagram_import_notes(&self, window: &mut Window, cx: &mut Context<Self>) {
        let notes = self.diagram_ui.import_notes.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("Import / export notes").child(
                div()
                    .id("diagram-import-notes-body")
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .text_size(px(12.))
                    .children(notes.iter().map(|note| div().child(note.clone()))),
            )
        });
    }

    pub(super) fn is_diagram(&self) -> bool {
        self.editor.kind() == Some(ProjectKind::Diagram)
    }
    pub(super) fn diagram_cancel_pointer_gesture(&mut self) -> bool {
        (self.diagram_ui.press.is_some()
            || self.diagram_ui.endpoint_drag.is_some()
            || self.diagram_ui.marquee.is_some())
            && self.diagram_cancel_connection()
    }

    pub(super) fn diagram_cancel_connection(&mut self) -> bool {
        let active = self.diagram_ui.connecting
            || self.diagram_ui.reconnect.is_some()
            || self.diagram_ui.endpoint_drag.is_some()
            || self.diagram_ui.marquee.is_some();
        if let Some(marquee) = self.diagram_ui.marquee.take() {
            let active = marquee.base.last().copied();
            self.set_layer_selection(marquee.base, active);
        }
        self.diagram_ui.connecting = false;
        self.diagram_ui.source = None;
        self.diagram_ui.press = None;
        self.diagram_ui.dragged = false;
        self.diagram_ui.reconnect = None;
        self.diagram_ui.endpoint_drag = None;
        active
    }
    fn diagram_object(&self) -> Option<NodeId> {
        let model = self.editor.doc.diagram.as_ref()?;
        let mut id = self.selected?;
        loop {
            if model.shapes.contains_key(&id) || model.edges.contains_key(&id) {
                return Some(id);
            }
            id = self.editor.doc.node(id)?.parent?;
        }
    }
    fn diagram_hit(&self, point: (f64, f64)) -> Option<Endpoint> {
        self.diagram_hit_excluding(point, None)
    }
    fn diagram_hit_excluding(
        &self,
        point: (f64, f64),
        excluded: Option<NodeId>,
    ) -> Option<Endpoint> {
        let edge = self.diagram_edge_hit_excluding(point, excluded);
        let cache = self.diagram_hit_cache();
        for (id, kind, [x, y, w, h]) in cache.shapes.iter().copied() {
            if Some(id) == excluded {
                continue;
            }
            let tolerance = 10. / self.view.zoom;
            if point.0 < x - tolerance
                || point.0 > x + w + tolerance
                || point.1 < y - tolerance
                || point.1 > y + h + tolerance
            {
                continue;
            }
            if kind.is_container()
                && point.0 > x + tolerance
                && point.0 < x + w - tolerance
                && point.1 > y + 32.
                && point.1 < y + h - tolerance
            {
                continue;
            }
            if let Some(edge) = edge
                && cache.order.get(&edge) > cache.order.get(&id)
            {
                return diagram::connector_attachment(&self.editor.doc, edge, point)
                    .map(|(e, _)| e);
            }
            // Store the picked position in object-relative coordinates so the
            // attachment follows movement/resizing without jumping to a midpoint.
            let port = Port::Custom {
                x: ((point.0 - x) / w.max(f64::EPSILON)).clamp(0., 1.),
                y: ((point.1 - y) / h.max(f64::EPSILON)).clamp(0., 1.),
            };
            return Some(Endpoint { shape: id, port });
        }
        drop(cache);
        let id = edge?;
        diagram::connector_attachment(&self.editor.doc, id, point).map(|(endpoint, _)| endpoint)
    }
    fn diagram_edge_hit_excluding(
        &self,
        point: (f64, f64),
        excluded: Option<NodeId>,
    ) -> Option<NodeId> {
        let cache = self.diagram_hit_cache();
        let tolerance = 7. / self.view.zoom;
        for (id, [x, y, w, h], lines) in &cache.edges {
            if Some(*id) == excluded {
                continue;
            }
            if point.0 < x - tolerance
                || point.0 > x + w + tolerance
                || point.1 < y - tolerance
                || point.1 > y + h + tolerance
            {
                continue;
            }
            for line in lines {
                for pair in line.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let delta = (b.0 - a.0, b.1 - a.1);
                    let length = delta.0 * delta.0 + delta.1 * delta.1;
                    let t = if length == 0. {
                        0.
                    } else {
                        (((point.0 - a.0) * delta.0 + (point.1 - a.1) * delta.1) / length)
                            .clamp(0., 1.)
                    };
                    if (point.0 - a.0 - t * delta.0).hypot(point.1 - a.1 - t * delta.1)
                        <= 7. / self.view.zoom
                    {
                        return Some(*id);
                    }
                }
            }
        }
        None
    }
    /// Return true when a connector gesture consumed the canvas click.
    pub(super) fn diagram_pointer_down(
        &mut self,
        point: (f64, f64),
        shift: bool,
        click_count: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_diagram() {
            return false;
        }
        self.diagram_ui.pointer = Some(point);
        if self.tool == Tool::Move
            && !shift
            && click_count < 2
            && !self.diagram_ui.connecting
            && self.diagram_ui.reconnect.is_none()
            && self.begin_diagram_endpoint_drag(point, cx)
        {
            return true;
        }
        let port = self.diagram_port_hit(point);
        let hit = port.clone().or_else(|| self.diagram_hit(point));
        if let Some((id, source)) = self.diagram_ui.reconnect {
            if let Some(endpoint) = hit {
                let mut model = self
                    .editor
                    .doc
                    .diagram
                    .as_deref()
                    .cloned()
                    .unwrap_or_default();
                if let Some(edge) = model.edges.get_mut(&id) {
                    if source {
                        edge.source = endpoint;
                    } else {
                        edge.target = endpoint;
                    }
                    edge.waypoints.clear();
                    self.execute(
                        Command::SetDiagram {
                            diagram: Some(Arc::new(model)),
                        },
                        cx,
                    );
                }
                self.diagram_cancel_connection();
                cx.notify();
            }
            return true;
        }
        if self.diagram_ui.connecting {
            if let Some(endpoint) = hit {
                if let Some(source) = self.diagram_ui.source.take() {
                    self.diagram_connect(source, endpoint, cx);
                    self.diagram_ui.connecting = false;
                } else {
                    self.diagram_ui.source = Some(endpoint);
                    self.diagram_ui.press = Some(point);
                    self.diagram_ui.dragged = false;
                    self.notify_canvas(cx);
                    self.set_status(
                        "Click the destination shape or port. Escape cancels.",
                        false,
                        cx,
                    );
                }
            }
            return true;
        }
        if self.tool == Tool::Move
            && !shift
            && let Some(endpoint) = port
        {
            self.diagram_ui.connecting = true;
            self.diagram_ui.source = Some(endpoint);
            self.diagram_ui.press = Some(point);
            self.diagram_ui.dragged = false;
            self.notify_canvas(cx);
            cx.notify();
            return true;
        }
        if self.tool == Tool::Move
            && !shift
            && let Some(selected) = self.selected
            && self
                .editor
                .doc
                .diagram
                .as_ref()
                .is_some_and(|d| d.edges.values().any(|e| e.label == selected))
            && emulsion_core::geometry::node_bounds(&self.editor.doc, selected).is_some_and(|b| {
                point.0 >= b.x as f64
                    && point.1 >= b.y as f64
                    && point.0 <= (b.x + b.w) as f64
                    && point.1 <= (b.y + b.h) as f64
            })
        {
            return false;
        }
        if self.tool == Tool::Move {
            let object = (!shift)
                .then(|| self.diagram_active_hit(point))
                .flatten()
                .or_else(|| hit.map(|e| self.diagram_selection_root(e.shape)));
            let Some(object) = object else {
                let base = self.selected_layer_ids();
                if !shift {
                    self.set_layer_selection(Vec::new(), None);
                }
                self.diagram_ui.marquee = Some(DiagramMarquee {
                    start: point,
                    end: point,
                    base: if shift { base } else { Vec::new() },
                });
                self.notify_canvas(cx);
                cx.notify();
                return true;
            };
            if shift {
                let mut ids = self.selected_layer_ids();
                if ids.contains(&object) {
                    ids.retain(|id| *id != object);
                } else {
                    ids.push(object);
                }
                let selected = ids.last().copied();
                self.set_layer_selection(ids, selected);
                cx.notify();
                return true;
            }
            if !self.layer_is_selected(object) {
                self.set_layer_selection(vec![object], Some(object));
            }
            if click_count < 2 && self.begin_diagram_endpoint_drag(point, cx) {
                return true;
            }
        }
        false
    }
    fn diagram_selection_root(&self, mut id: NodeId) -> NodeId {
        let Some(model) = &self.editor.doc.diagram else {
            return id;
        };
        while let Some(parent) = self.editor.doc.node(id).and_then(|n| n.parent) {
            if model.shapes.contains_key(&parent) || model.edges.contains_key(&parent) {
                break;
            }
            id = parent;
        }
        id
    }

    pub(super) fn diagram_select_all(&mut self, cx: &mut Context<Self>) {
        self.diagram_cancel_connection();
        let mut ids = Vec::new();
        if let Some(model) = &self.editor.doc.diagram {
            for id in model.shapes.keys().chain(model.edges.keys()) {
                let id = self.diagram_selection_root(*id);
                if !ids.contains(&id)
                    && self.diagram_shape_visible(id)
                    && self.editor.doc.locked_ancestor(id).is_none()
                {
                    ids.push(id);
                }
            }
        }
        let active = ids.last().copied();
        self.set_layer_selection(ids, active);
        self.notify_canvas(cx);
        cx.notify();
    }

    fn update_diagram_marquee(&mut self, point: (f64, f64)) {
        let Some(m) = &mut self.diagram_ui.marquee else {
            return;
        };
        m.end = point;
        let (start, end, mut ids) = (m.start, m.end, m.base.clone());
        if (end.0 - start.0).hypot(end.1 - start.1) * self.view.zoom > 3. {
            let rect = [
                start.0.min(end.0),
                start.1.min(end.1),
                start.0.max(end.0),
                start.1.max(end.1),
            ];
            if let Some(model) = &self.editor.doc.diagram {
                for id in model.shapes.keys().chain(model.edges.keys()) {
                    let id = self.diagram_selection_root(*id);
                    if ids.contains(&id)
                        || !self.diagram_shape_visible(id)
                        || self.editor.doc.locked_ancestor(id).is_some()
                    {
                        continue;
                    }
                    if let Some(b) = emulsion_core::geometry::node_bounds(&self.editor.doc, id)
                        && b.x as f64 <= rect[2]
                        && b.y as f64 <= rect[3]
                        && b.right() as f64 >= rect[0]
                        && b.bottom() as f64 >= rect[1]
                    {
                        ids.push(id);
                    }
                }
            }
        }
        let active = ids.last().copied();
        self.set_layer_selection(ids, active);
    }

    /// Ports sit outside the resize handles, at a constant screen-space distance.
    fn diagram_ports(&self) -> Vec<(Endpoint, (f64, f64))> {
        if !self.is_diagram() || self.tool != Tool::Move || self.space_held {
            return Vec::new();
        }
        let Some(model) = self.editor.doc.diagram.as_ref() else {
            return Vec::new();
        };
        let hover = self.diagram_ui.hover_shape;
        let selected = self.diagram_object();
        let mut ports = Vec::new();
        for (id, shape) in &model.shapes {
            if Some(*id) != hover
                && Some(*id) != selected
                && self
                    .diagram_ui
                    .source
                    .as_ref()
                    .is_none_or(|source| source.shape != *id)
            {
                continue;
            }
            if !self.diagram_shape_visible(*id) || self.editor.doc.locked_ancestor(*id).is_some() {
                continue;
            }
            let Some(bounds) = diagram::shape_bounds(&self.editor.doc, shape) else {
                continue;
            };
            for port in [Port::North, Port::East, Port::South, Port::West] {
                let (point, normal) = port.anchor(bounds, (0., 0.));
                ports.push((
                    Endpoint { shape: *id, port },
                    (
                        point.0 + normal.0 * 12. / self.view.zoom,
                        point.1 + normal.1 * 12. / self.view.zoom,
                    ),
                ));
            }
        }
        ports
    }
    fn diagram_shape_visible(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(id) = current {
            let Some(node) = self.editor.doc.node(id) else {
                return false;
            };
            if !node.visible {
                return false;
            }
            current = node.parent;
        }
        true
    }
    fn diagram_port_hit(&self, point: (f64, f64)) -> Option<Endpoint> {
        self.diagram_ports()
            .into_iter()
            .rev()
            .find(|(_, p)| (p.0 - point.0).hypot(p.1 - point.1) <= 7. / self.view.zoom)
            .map(|(endpoint, _)| endpoint)
    }
    pub(super) fn diagram_pointer_move(
        &mut self,
        point: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_diagram() {
            return false;
        }
        if self.diagram_ui.endpoint_drag.is_some() {
            self.diagram_ui.pointer = point;
            self.notify_canvas(cx);
            return true;
        }
        if self.diagram_ui.marquee.is_some() {
            if let Some(point) = point {
                self.update_diagram_marquee(point);
                self.notify_canvas(cx);
                cx.notify();
            }
            return true;
        }
        let previous = self.diagram_ui.pointer;
        self.diagram_ui.pointer = point;
        self.diagram_ui.hover_shape = point.and_then(|point| {
            self.diagram_hit(point).map(|e| e.shape).or_else(|| {
                let id = self.diagram_ui.hover_shape?;
                let shape = self.editor.doc.diagram.as_ref()?.shapes.get(&id)?;
                let [x, y, w, h] = diagram::shape_bounds(&self.editor.doc, shape)?;
                let margin = 22. / self.view.zoom;
                (point.0 >= x - margin
                    && point.0 <= x + w + margin
                    && point.1 >= y - margin
                    && point.1 <= y + h + margin)
                    .then_some(id)
            })
        });
        if let (Some(start), Some(point)) = (self.diagram_ui.press, point) {
            self.diagram_ui.dragged |=
                (start.0 - point.0).hypot(start.1 - point.1) * self.view.zoom > 3.;
        }
        if previous != point && (self.tool == Tool::Move || self.diagram_ui.connecting) {
            self.notify_canvas(cx);
        }
        self.diagram_ui.press.is_some()
    }
    pub(super) fn diagram_pointer_up(
        &mut self,
        point: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.diagram_ui.endpoint_drag.is_some() {
            self.finish_diagram_endpoint_drag(point, cx);
            return true;
        }
        if self.diagram_ui.marquee.is_some() {
            if let Some(point) = point {
                self.update_diagram_marquee(point);
            }
            self.diagram_ui.marquee = None;
            self.notify_canvas(cx);
            cx.notify();
            return true;
        }
        let Some(start) = self.diagram_ui.press.take() else {
            return false;
        };
        let dragged = std::mem::take(&mut self.diagram_ui.dragged)
            || point.is_none_or(|p| (p.0 - start.0).hypot(p.1 - start.1) * self.view.zoom > 3.);
        if dragged {
            let target =
                point.and_then(|p| self.diagram_port_hit(p).or_else(|| self.diagram_hit(p)));
            let source = self.diagram_ui.source.take();
            self.diagram_cancel_connection();
            if let (Some(source), Some(target)) = (source, target)
                && source.shape != target.shape
            {
                self.diagram_connect(source, target, cx);
            }
        }
        self.notify_canvas(cx);
        cx.notify();
        true
    }
    pub(super) fn diagram_connection_overlay(&self) -> DiagramOverlay {
        let mut overlay = DiagramOverlay {
            ports: self.diagram_ports().into_iter().map(|(_, p)| p).collect(),
            ..Default::default()
        };
        if self.is_diagram() && self.tool == Tool::Move {
            overlay.selected = self
                .selected_layer_roots()
                .into_iter()
                .filter(|id| {
                    !self
                        .editor
                        .doc
                        .diagram
                        .as_ref()
                        .is_some_and(|d| d.edges.contains_key(id))
                })
                .filter_map(|id| emulsion_core::geometry::node_bounds(&self.editor.doc, id))
                .map(|b| [b.x as f64, b.y as f64, b.w as f64, b.h as f64])
                .collect();
            overlay.marquee = self.diagram_ui.marquee.as_ref().map(|m| {
                [
                    m.start.0.min(m.end.0),
                    m.start.1.min(m.end.1),
                    (m.end.0 - m.start.0).abs(),
                    (m.end.1 - m.start.1).abs(),
                ]
            });
        }
        if let (Some(source), Some(pointer)) = (&self.diagram_ui.source, self.diagram_ui.pointer)
            && let Some(start) = diagram::endpoint_position(&self.editor.doc, source, pointer)
        {
            let hit = self
                .diagram_port_hit(pointer)
                .or_else(|| self.diagram_hit(pointer));
            let end = hit
                .as_ref()
                .and_then(|hit| diagram::endpoint_position(&self.editor.doc, hit, start))
                .unwrap_or(pointer);
            let mid = (start.0 + end.0) / 2.;
            overlay.preview = vec![start, (mid, start.1), (mid, end.1), end];
            overlay.target = hit.map(|_| end);
        }
        self.diagram_endpoint_overlay(&mut overlay);
        overlay
    }

    pub(crate) fn diagram_connect(
        &mut self,
        source: Endpoint,
        target: Endpoint,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        match diagram::connect(&mut self.editor, source, target, "", Routing::Orthogonal) {
            Ok(id) => {
                self.set_layer_selection(vec![id], Some(id));
                self.after_change(cx);
                self.set_status(
                    "Connected. Move either shape to reroute the connector.",
                    false,
                    cx,
                );
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub(super) fn drop_diagram_stencil(
        &mut self,
        stencil: diagram::stencils::Stencil,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.is_diagram()
            || !self.canvas_bounds().is_some_and(|b| b.contains(&position))
            || !self.prepare_page_action(cx)
        {
            return;
        }
        let Some((x, y)) = self.doc_point(position) else {
            return;
        };
        let (w, h) = stencil.default_size();
        match stencil.insert(&mut self.editor, [x - w / 2., y - h / 2., w, h]) {
            Ok(id) => {
                self.set_layer_selection(vec![id], Some(id));
                self.after_change(cx);
                self.set_tool(Tool::Move, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(crate) fn insert_diagram_stencil(
        &mut self,
        stencil: diagram::stencils::Stencil,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let count = self
            .editor
            .doc
            .diagram
            .as_ref()
            .map_or(0, |d| d.shapes.len());
        let (w, h) = stencil.default_size();
        let x = (self.editor.doc.width as f64 / 2. - w / 2. + (count % 5) as f64 * 24.).max(20.);
        let y = (self.editor.doc.height as f64 / 2. - h / 2. + (count % 5) as f64 * 24.).max(20.);
        match stencil.insert(&mut self.editor, [x, y, w, h]) {
            Ok(id) => {
                self.set_layer_selection(vec![id], Some(id));
                self.after_change(cx);
                self.set_tool(Tool::Move, cx);
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    fn layout_diagram(&mut self, layout: Layout, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        match diagram::arrange(&mut self.editor, layout) {
            Ok(()) => {
                self.after_change(cx);
                self.fit_pending = true;
                let overflow = self
                    .editor
                    .doc
                    .diagram
                    .as_ref()
                    .into_iter()
                    .flat_map(|d| d.shapes.values())
                    .filter_map(|s| diagram::shape_bounds(&self.editor.doc, s))
                    .filter(|b| {
                        b[0] < 0.
                            || b[1] < 0.
                            || b[0] + b[2] > self.editor.doc.width as f64
                            || b[1] + b[3] > self.editor.doc.height as f64
                    })
                    .count();
                self.set_status(if overflow==0 { "Arranged diagram; manually locked placements were preserved.".into() }else{format!("Arranged diagram. {overflow} shapes extend outside the page; increase the canvas size to include them.")},overflow>0,cx);
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    fn diagram_properties(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self.diagram_object() else {
            return;
        };
        let model = self.editor.doc.diagram.as_ref().unwrap();
        let (label_id, data) = if let Some(shape) = model.shapes.get(&id) {
            (
                shape.label,
                serde_json::to_string_pretty(&shape.data).unwrap_or_default(),
            )
        } else {
            (
                model.edges[&id].label,
                serde_json::to_string(&model.edges[&id].waypoints).unwrap_or_default(),
            )
        };
        let shape = model.shapes.contains_key(&id);
        let text = if let Some(Node {
            kind: NodeKind::Text { spec, .. },
            ..
        }) = self.editor.doc.node(label_id)
        {
            spec.text.clone()
        } else {
            String::new()
        };
        let label = cx.new(|cx| TextareaState::new(window, cx).rows(3).default_value(text));
        let details = cx.new(|cx| TextareaState::new(window, cx).rows(10).default_value(data));
        let owner = cx.weak_entity();
        let page = self.editor.active_page();
        window.open_dialog(cx, move |dialog, _, _| {
            let label = label.clone();
            let details = details.clone();
            let owner = owner.clone();
            dialog
                .title(if shape {
                    "Shape label and data"
                } else {
                    "Connector label and waypoints"
                })
                .width(px(800.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Label")
                        .child(
                            Textarea::new(&label)
                                .h(rems(6.))
                                .flex_shrink_0()
                                .aria_label("Object label"),
                        )
                        .child(if shape {
                            "Data · JSON object, e.g. {\"owner\":\"Design\"}"
                        } else {
                            "Waypoints · JSON coordinates, e.g. [[200,100],[200,300]]"
                        })
                        .child(
                            Textarea::new(&details)
                                .h(rems(16.))
                                .flex_shrink_0()
                                .aria_label("Object data"),
                        ),
                )
                .footer(crate::widgets::form_dialog_footer("Apply changes"))
                .on_ok(move |_, _, cx| {
                    let text = label.read(cx).value().to_string();
                    let data = details.read(cx).value().to_string();
                    owner
                        .update(cx, |this, cx| {
                            if this.editor.active_page() != page {
                                this.set_status(
                                    "The active page changed. Open properties again.",
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            let result: Result<Vec<Command>, String> = (|| {
                                if text.chars().count() > emulsion_core::text::MAX_CHARS {
                                    return Err(format!(
                                        "Labels can contain up to {} characters.",
                                        emulsion_core::text::MAX_CHARS
                                    ));
                                }
                                let mut model = this
                                    .editor
                                    .doc
                                    .diagram
                                    .as_deref()
                                    .cloned()
                                    .ok_or("Diagram no longer exists")?;
                                if shape {
                                    model
                                        .shapes
                                        .get_mut(&id)
                                        .ok_or("Shape no longer exists")?
                                        .data = serde_json::from_str(&data)
                                        .map_err(|e| format!("Invalid shape data: {e}"))?;
                                } else {
                                    model
                                        .edges
                                        .get_mut(&id)
                                        .ok_or("Connector no longer exists")?
                                        .waypoints = serde_json::from_str(&data)
                                        .map_err(|e| format!("Invalid waypoints: {e}"))?;
                                }
                                let Some(Node {
                                    kind: NodeKind::Text { spec, .. },
                                    ..
                                }) = this.editor.doc.node(label_id)
                                else {
                                    return Err("Label no longer exists".into());
                                };
                                let mut spec = (**spec).clone();
                                spec.text = text;
                                let commands = vec![
                                    Command::SetText {
                                        id: label_id,
                                        spec: Box::new(spec),
                                    },
                                    Command::SetDiagram {
                                        diagram: Some(Arc::new(model)),
                                    },
                                ];
                                let mut trial = this.editor.doc.clone();
                                for command in &commands {
                                    command.apply(&mut trial).map_err(|e| e.to_string())?;
                                }
                                Ok(commands)
                            })(
                            );
                            match result {
                                Ok(commands) => {
                                    this.execute_layer_commands("Diagram properties", commands, cx);
                                    true
                                }
                                Err(e) => {
                                    this.set_status(e, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
    pub(super) fn diagram_quick_create(
        &mut self,
        direction: Port,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_diagram() || !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self.diagram_object() else {
            self.set_status("Select a source shape first.", false, cx);
            return;
        };
        let Some(kind) = self
            .editor
            .doc
            .diagram
            .as_ref()
            .and_then(|d| d.shapes.get(&id))
            .map(|s| s.kind)
        else {
            return;
        };
        let kind = if kind.is_container() {
            ShapeKind::Process
        } else {
            kind
        };
        match diagram::quick_create(&mut self.editor, id, direction, kind) {
            Ok(id) => {
                self.after_change(cx);
                self.set_layer_selection(vec![id], Some(id));
                window.focus(&self.canvas_focus, cx);
                let outside = self
                    .editor
                    .doc
                    .diagram
                    .as_ref()
                    .and_then(|d| d.shapes.get(&id))
                    .and_then(|s| diagram::shape_bounds(&self.editor.doc, s))
                    .is_some_and(|[x, y, w, h]| {
                        x < 0.
                            || y < 0.
                            || x + w > self.editor.doc.width as f64
                            || y + h > self.editor.doc.height as f64
                    });
                self.set_status(if outside { "Connected shape added outside the page. Arrange the diagram or enlarge the page." } else { "Connected shape added. Ctrl+Alt+Arrow adds another; Properties edits its label." }, outside, cx);
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    fn diagram_edit_bends(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(mut model) = self.editor.doc.diagram.as_deref().cloned() else {
            return;
        };
        let Some(edge) = model.edges.get_mut(&id) else {
            return;
        };
        let path_id = edge.path;
        let Some(Node {
            kind: NodeKind::Path { path, .. },
            ..
        }) = self.editor.doc.node(path_id)
        else {
            return;
        };
        let Some(line) = path.subpaths.first().filter(|s| s.anchors.len() >= 2) else {
            return;
        };
        edge.waypoints = line.anchors[1..line.anchors.len() - 1]
            .iter()
            .map(|a| a.p)
            .collect();
        if edge.waypoints.is_empty() {
            let a = line.anchors.first().unwrap().p;
            let b = line.anchors.last().unwrap().p;
            edge.waypoints.push(((a.0 + b.0) / 2., (a.1 + b.1) / 2.));
        }
        self.execute(
            Command::SetDiagram {
                diagram: Some(Arc::new(model)),
            },
            cx,
        );
        self.set_layer_selection(vec![path_id], Some(path_id));
        self.set_tool(Tool::Pen, cx);
        window.focus(&self.canvas_focus, cx);
        self.set_status("Drag the interior Pen anchors to position connector bends. Endpoints remain attached to their shapes.",false,cx);
    }
    fn diagram_line_dash(&mut self, path_id: NodeId, dash: bool, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(Node {
            kind: NodeKind::Path { path, style, .. },
            ..
        }) = self.editor.doc.node(path_id)
        else {
            return;
        };
        let mut style = *style;
        style.dash = [8., 5., 0., 0., 0., 0.];
        style.dash_count = if dash { 2 } else { 0 };
        self.execute(
            Command::SetPath {
                id: path_id,
                path: path.clone(),
                style,
            },
            cx,
        );
    }
    fn update_diagram_edge(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut diagram::Edge),
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(mut model) = self.editor.doc.diagram.as_deref().cloned() else {
            return;
        };
        let Some(edge) = model.edges.get_mut(&id) else {
            return;
        };
        f(edge);
        self.execute(
            Command::SetDiagram {
                diagram: Some(Arc::new(model)),
            },
            cx,
        );
    }
    pub(super) fn diagram_inspector(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut tabs = div()
            .h(px(38.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(3.))
            .px(px(6.))
            .border_b_1()
            .border_color(p.line);
        for (index, label) in ["Style", "Text", "Arrange", "Data"].into_iter().enumerate() {
            tabs = tabs.child(
                Button::new(("diagram-property-tab", index))
                    .label(label)
                    .xsmall()
                    .ghost()
                    .selected(self.diagram_ui.property_tab == index)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diagram_ui.property_tab = index;
                        cx.notify();
                    })),
            );
        }
        let mut panel = div()
            .id("diagram-inspector")
            .test_support()
            .flex()
            .flex_col()
            .child(tabs);
        match self.diagram_ui.property_tab {
            1 => {
                panel = panel.child(
                    Button::new("diagram-edit-text")
                        .label("Edit label…")
                        .small()
                        .ghost()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.diagram_properties(window, cx)),
                        ),
                );
                panel = if let Some(properties) = self.text_properties(window, cx) {
                    panel.child(properties)
                } else {
                    panel.child(
                        div()
                            .p_3()
                            .text_size(px(11.))
                            .child("Select a shape or connector to format its label."),
                    )
                };
            }
            2 => {
                let mut layouts = div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.alignment_controls(p, cx));
                for (index, layout) in Layout::ALL.into_iter().enumerate() {
                    layouts = layouts.child(
                        Button::new(("diagram-inspector-layout", index))
                            .label(layout.label())
                            .small()
                            .outline()
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.layout_diagram(layout, cx)),
                            ),
                    );
                }
                panel = panel.child(layouts).child(self.inspector(p, window, cx));
            }
            3 => {
                let mut data = div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(11.5))
                    .child(
                        Button::new("diagram-edit-data")
                            .label("Edit label and data…")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.diagram_properties(window, cx)
                            })),
                    )
                    .child(
                        Button::new("diagram-data-condition")
                            .label("Conditional fill…")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.diagram_conditional_fill(window, cx)
                            })),
                    );
                if let Some(shape) = self
                    .diagram_object()
                    .and_then(|id| self.editor.doc.diagram.as_ref()?.shapes.get(&id))
                {
                    for (key, value) in &shape.data {
                        if !key.starts_with("emulsion_") && key != "drawio_geometry_style" {
                            data = data.child(div().child(format!("{key}: {value}")));
                        }
                    }
                }
                panel = panel.child(data);
            }
            _ => {
                panel = panel
                    .child(self.diagram_selection_panel(p, cx))
                    .children(self.shape_properties(window, cx));
            }
        }
        panel.into_any_element()
    }

    pub(crate) fn diagram_fill(&mut self, color: [u8; 4], cx: &mut Context<Self>) {
        self.diagram_color("fill", color, cx);
    }

    fn diagram_color_nodes(&self) -> Vec<NodeId> {
        let roots = self.selected_layer_roots();
        self.editor
            .doc
            .nodes
            .iter()
            .filter(|node| {
                let mut current = Some(node.id);
                while let Some(id) = current {
                    if roots.contains(&id) {
                        return true;
                    }
                    current = self.editor.doc.node(id).and_then(|n| n.parent);
                }
                false
            })
            .map(|n| n.id)
            .collect()
    }

    pub(crate) fn diagram_color(&mut self, key: &str, color: [u8; 4], cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let commands = self
            .diagram_color_nodes()
            .into_iter()
            .filter_map(|id| {
                match &self.editor.doc.node(id)?.kind {
                    NodeKind::Path { path, style, .. } if key != "text" => {
                        let mut style = *style;
                        if key == "fill" {
                            // Open connector paths and arrow geometry are controlled by stroke.
                            if self
                                .editor
                                .doc
                                .diagram
                                .as_ref()
                                .is_some_and(|d| d.edges.values().any(|e| e.path == id))
                            {
                                return None;
                            }
                            style.fill = Some(color);
                            style.fill_paint = emulsion_raster::vector::PathPaint::Solid;
                        } else {
                            style.stroke = Some(color);
                            style.stroke_paint = emulsion_raster::vector::PathPaint::Solid;
                        }
                        Some(Command::SetPath {
                            id,
                            path: path.clone(),
                            style,
                        })
                    }
                    NodeKind::Text { spec, .. } if key == "text" => {
                        let mut spec = (**spec).clone();
                        spec.color = color;
                        spec.apply_style(0..spec.text.len(), |s| s.color = color);
                        Some(Command::SetText {
                            id,
                            spec: Box::new(spec),
                        })
                    }
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            self.execute_layer_commands("Diagram color", commands, cx);
        }
    }

    fn diagram_style_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        match emulsion_core::diagram_library::theme_commands(
            &self.editor.doc,
            &self.selected_layer_roots(),
            {
                let (name, (fill, line, text)) = DIAGRAM_STYLES[index];
                emulsion_core::diagram_library::Theme {
                    id: "preset",
                    name,
                    fill,
                    line,
                    text,
                }
            },
        ) {
            Ok(commands) => {
                self.execute_layer_commands("Diagram style", commands, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }

    fn diagram_color_dialog(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use super::styles_ui::color_picker::StyleColorPicker;
        use gpui_kit::component::color_picker::ColorPickerState;
        if !self.prepare_page_action(cx) {
            return;
        }
        let color = self
            .diagram_color_nodes()
            .into_iter()
            .find_map(|id| match &self.editor.doc.node(id)?.kind {
                NodeKind::Path { style, .. } if key == "fill" => style.fill,
                NodeKind::Path { style, .. } if key == "stroke" => style.stroke,
                NodeKind::Text { spec, .. } if key == "text" => Some(spec.color),
                _ => None,
            })
            .unwrap_or([0, 0, 0, 255]);
        let [r, g, b, a] = color.map(|v| v as f32 / 255.);
        let state =
            cx.new(|cx| ColorPickerState::new(window, cx).default_value(Rgba { r, g, b, a }));
        let picker = cx.new(|cx| StyleColorPicker::new(state.clone(), window, cx));
        let body = picker.clone();
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let selection = self.selected_layer_ids();
        window.focus(&self.canvas_focus, cx);
        let confirm = Rc::new(move |window: &mut Window, cx: &mut App| {
            if !picker.update(cx, |p, cx| p.commit_pending(window, cx)) {
                return false;
            }
            if let Some(color) = state.read(cx).value() {
                let c = color.to_rgb();
                let color = [c.r, c.g, c.b, c.a].map(|v| (v * 255.).round().clamp(0., 255.) as u8);
                let _ = owner.update(cx, |this, cx| {
                    if this.edit_ticket() == ticket && this.selected_layer_ids() == selection {
                        this.diagram_color(key, color, cx);
                    } else {
                        this.set_status("Selection changed; choose the color again.", false, cx);
                    }
                });
            }
            true
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let ok = confirm.clone();
            let button_ok = confirm.clone();
            dialog
                .title(format!("Diagram {key} color"))
                .width(px(590.))
                .child(body.clone())
                .on_ok(move |_, window, cx| ok(window, cx))
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("diagram-color-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("diagram-color-ok").label("Apply").on_click(
                            move |_, window, cx| {
                                if button_ok(window, cx) {
                                    window.close_dialog(cx);
                                }
                            },
                        )),
                )
        });
        cx.notify();
    }

    pub(super) fn diagram_selection_panel(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content = div()
            .id("diagram-selection-properties")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(12.))
            .text_size(px(11.5))
            .child(div().font_weight(FontWeight::MEDIUM).child("Selection"));
        if !self.selected_layer_ids().is_empty() {
            let count = self.selected_layer_roots().len();
            if count > 1 {
                content = content.child(format!("{count} objects selected"));
            }
            content = content.child(
                div()
                    .flex()
                    .gap_1()
                    .children(DIAGRAM_STYLES.iter().enumerate().map(
                        |(index, (name, (fill, line, _)))| {
                            Button::new(("diagram-style", index))
                                .tooltip(*name)
                                .accessibility_label(*name)
                                .xsmall()
                                .outline()
                                .size(px(28.))
                                .p_0()
                                .child(
                                    div()
                                        .size(px(18.))
                                        .rounded(px(4.))
                                        .bg(gpui::rgba(u32::from_be_bytes(*fill)))
                                        .border_1()
                                        .border_color(gpui::rgba(u32::from_be_bytes(*line))),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.diagram_style_preset(index, cx)
                                }))
                        },
                    )),
            );
            content = content.child(
                div().flex().gap_1().children(
                    [("fill", "Fill…"), ("stroke", "Line…"), ("text", "Text…")]
                        .into_iter()
                        .map(|(key, label)| {
                            Button::new(SharedString::from(format!("diagram-color-{key}")))
                                .label(label)
                                .xsmall()
                                .outline()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.diagram_color_dialog(key, window, cx)
                                }))
                        }),
                ),
            );
        }
        if let Some(id) = self.diagram_object() {
            if self
                .editor
                .doc
                .diagram
                .as_ref()
                .is_some_and(|d| d.shapes.contains_key(&id))
            {
                let mut swatches = div().flex().gap(px(4.));
                for (index, color) in [
                    [255, 255, 255, 255],
                    [233, 239, 251, 255],
                    [218, 232, 252, 255],
                    [213, 232, 212, 255],
                    [255, 242, 204, 255],
                    [248, 206, 204, 255],
                    [225, 213, 231, 255],
                    [0, 0, 0, 0],
                ]
                .into_iter()
                .enumerate()
                {
                    let title = if color[3] == 0 {
                        "Transparent".to_string()
                    } else {
                        format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
                    };
                    swatches = swatches.child(
                        Button::new(("diagram-fill", index))
                            .accessibility_label(title.clone())
                            .tooltip(title)
                            .xsmall()
                            .outline()
                            .p_0()
                            .size(px(23.))
                            .child(
                                div()
                                    .size(px(15.))
                                    .bg(gpui::rgba(u32::from_be_bytes(color)))
                                    .border_1()
                                    .border_color(p.line),
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.diagram_fill(color, cx)),
                            ),
                    );
                }
                content = content.child("Fill").child(swatches);
            }
            content = content.child(
                Button::new("diagram-properties")
                    .label("Edit label and properties…")
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.diagram_properties(window, cx)),
                    ),
            );
            let model = self.editor.doc.diagram.as_ref().unwrap();
            if let Some(edge) = model.edges.get(&id) {
                let routing = edge.routing;
                let arrow = edge.arrow_end;
                let arrow_start = edge.arrow_start;
                let path_id = edge.path;
                let label_id = edge.label;
                let dashed = matches!(self.editor.doc.node(path_id).map(|n|&n.kind),Some(NodeKind::Path {style,..}) if style.dash_count > 0);
                content = content
                    .child(
                        Button::new("diagram-routing")
                            .label(match routing {
                                Routing::Orthogonal => "Routing: orthogonal",
                                Routing::Straight => "Routing: straight",
                                Routing::Curved => "Routing: curved",
                                Routing::Cyclical => "Routing: cyclical",
                            })
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_diagram_edge(
                                    id,
                                    |e| {
                                        e.routing = match routing {
                                            Routing::Orthogonal => Routing::Straight,
                                            Routing::Straight => Routing::Curved,
                                            Routing::Curved => Routing::Cyclical,
                                            Routing::Cyclical => Routing::Orthogonal,
                                        };
                                        e.waypoints.clear();
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("diagram-arrow")
                            .label("End arrow")
                            .selected(arrow)
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_diagram_edge(id, |e| e.arrow_end = !arrow, cx)
                            })),
                    );
                let jump = edge.jump_style;
                content = content.child(
                    Button::new("diagram-line-jumps")
                        .label(format!("Crossings: {}", jump.drawio()))
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_diagram_edge(
                                id,
                                |e| {
                                    e.jump_style = match jump {
                                        diagram::JumpStyle::None => diagram::JumpStyle::Arc,
                                        diagram::JumpStyle::Arc => diagram::JumpStyle::Gap,
                                        diagram::JumpStyle::Gap => diagram::JumpStyle::Sharp,
                                        diagram::JumpStyle::Sharp => diagram::JumpStyle::None,
                                    }
                                },
                                cx,
                            )
                        })),
                );
                for (index, start, marker) in [
                    (0usize, true, edge.start_marker),
                    (1usize, false, edge.end_marker),
                ] {
                    content = content
                        .child(
                            Button::new(("diagram-marker-kind", index))
                                .label(format!(
                                    "{}: {}",
                                    if start { "Start marker" } else { "End marker" },
                                    marker.kind.drawio()
                                ))
                                .small()
                                .outline()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.update_diagram_edge(
                                        id,
                                        |edge| {
                                            let kinds = diagram::MarkerKind::ALL;
                                            let next = kinds[(kinds
                                                .iter()
                                                .position(|k| *k == marker.kind)
                                                .unwrap_or(0)
                                                + 1)
                                                % kinds.len()];
                                            if start {
                                                edge.start_marker.kind = next;
                                                edge.arrow_start =
                                                    next != diagram::MarkerKind::None;
                                            } else {
                                                edge.end_marker.kind = next;
                                                edge.arrow_end = next != diagram::MarkerKind::None;
                                            }
                                        },
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::new(("diagram-marker-fill", index))
                                .label(if marker.filled {
                                    "Filled marker"
                                } else {
                                    "Hollow marker"
                                })
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.update_diagram_edge(
                                        id,
                                        |edge| {
                                            if start {
                                                edge.start_marker.filled = !marker.filled;
                                            } else {
                                                edge.end_marker.filled = !marker.filled;
                                            }
                                        },
                                        cx,
                                    )
                                })),
                        );
                }
                content = content
                    .child(
                        Button::new("diagram-arrow-start")
                            .label("Start arrow")
                            .selected(arrow_start)
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_diagram_edge(id, |e| e.arrow_start = !arrow_start, cx)
                            })),
                    )
                    .child(
                        Button::new("diagram-line-dash")
                            .label("Dashed line")
                            .selected(dashed)
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.diagram_line_dash(path_id, !dashed, cx)
                            })),
                    );
                content = content
                    .child(
                        Button::new("diagram-edit-bends")
                            .label("Edit bends on canvas")
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.diagram_edit_bends(id, window, cx)
                            })),
                    )
                    .child(
                        Button::new("diagram-move-label")
                            .label("Move connector label")
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if !this.prepare_page_action(cx) {
                                    return;
                                }
                                this.set_layer_selection(vec![label_id], Some(label_id));
                                this.set_tool(Tool::Move, cx);
                                window.focus(&this.canvas_focus, cx);
                                this.set_status(
                                    "Drag the label or use arrow keys to adjust its position.",
                                    false,
                                    cx,
                                );
                            })),
                    );
                for source in [true, false] {
                    let port = if source {
                        edge.source.port
                    } else {
                        edge.target.port
                    };
                    let owner = cx.weak_entity();
                    content = content.child(
                        Button::new(if source {
                            "diagram-source-port"
                        } else {
                            "diagram-target-port"
                        })
                        .label(format!(
                            "{} port: {} ▾",
                            if source { "Source" } else { "Target" },
                            port.label()
                        ))
                        .small()
                        .outline()
                        .dropdown_menu(move |mut menu, _, _| {
                            for port in Port::ALL {
                                let owner = owner.clone();
                                menu = menu.item(PopupMenuItem::new(port.label()).on_click(
                                    move |_, _, cx| {
                                        owner
                                            .update(cx, |this, cx| {
                                                this.update_diagram_edge(
                                                    id,
                                                    |e| {
                                                        if source {
                                                            e.source.port = port;
                                                        } else {
                                                            e.target.port = port;
                                                        }
                                                    },
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    },
                                ));
                            }
                            let owner = owner.clone();
                            menu.item(PopupMenuItem::new("Reconnect to another shape…").on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.diagram_cancel_connection();
                                            this.diagram_ui.reconnect = Some((id, source));
                                            this.set_status(
                                                "Click the new endpoint shape. Escape cancels.",
                                                false,
                                                cx,
                                            );
                                        })
                                        .ok();
                                },
                            ))
                        }),
                    );
                }
            } else if let Some(shape) = model.shapes.get(&id) {
                let mut quick = div().flex().gap_1();
                for (direction, label, key) in [
                    (Port::West, "←", "left"),
                    (Port::North, "↑", "up"),
                    (Port::South, "↓", "down"),
                    (Port::East, "→", "right"),
                ] {
                    quick = quick.child(
                        Button::new((ElementId::from("diagram-quick"), key))
                            .label(label)
                            .tooltip(format!("Add connected shape · Ctrl+Alt+{key}"))
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.diagram_quick_create(direction, window, cx)
                            })),
                    );
                }
                content = content.child("Add connected shape").child(quick);
                let locked = shape.layout_locked;
                content = content.child(
                    Button::new("diagram-layout-lock")
                        .label("Keep position during layout")
                        .selected(locked)
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let Some(mut model) = this.editor.doc.diagram.as_deref().cloned()
                            else {
                                return;
                            };
                            if let Some(shape) = model.shapes.get_mut(&id) {
                                shape.layout_locked = !locked;
                            }
                            this.execute(
                                Command::SetDiagram {
                                    diagram: Some(Arc::new(model)),
                                },
                                cx,
                            );
                        })),
                );
                let containers = model
                    .shapes
                    .iter()
                    .filter(|(other, s)| {
                        **other != id
                            && s.kind.is_container()
                            && !self.editor.doc.is_ancestor(id, **other)
                    })
                    .map(|(id, _)| (*id, self.editor.doc.node(*id).unwrap().name.clone()))
                    .collect::<Vec<_>>();
                let owner = cx.weak_entity();
                content = content.child(
                    Button::new("diagram-container")
                        .label("Move into container ▾")
                        .small()
                        .outline()
                        .dropdown_menu(move |mut menu, _, _| {
                            for (container, name) in
                                std::iter::once((None, "Outside containers".to_string())).chain(
                                    containers
                                        .iter()
                                        .map(|(id, name)| (Some(*id), name.clone())),
                                )
                            {
                                let owner = owner.clone();
                                menu = menu.item(PopupMenuItem::new(name).on_click(
                                    move |_, _, cx| {
                                        owner
                                            .update(cx, |this, cx| {
                                                this.execute(
                                                    Command::MoveNode {
                                                        id,
                                                        slot: Slot::top_of(container),
                                                    },
                                                    cx,
                                                );
                                            })
                                            .ok();
                                    },
                                ));
                            }
                            menu
                        }),
                );
            }
        }
        content.into_any_element()
    }

    pub(super) fn diagram_drawer(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_diagram() {
            return None;
        }
        self.load_creative_library(cx);
        if self.diagram_ui.search.is_none() {
            self.diagram_ui.expanded_stencil_packs.extend(
                crate::app_state::settings(cx)
                    .diagram_stencil_packs
                    .iter()
                    .copied(),
            );
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search library"));
            self.diagram_ui.subscription = Some(cx.subscribe(&input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.diagram_ui.stencil_page = 0;
                    this.diagram_ui.used.page = 0;
                    cx.notify();
                }
            }));
            self.diagram_ui.search = Some(input);
        }
        let search = self.diagram_ui.search.as_ref().unwrap().clone();
        let query = search.read(cx).value().to_lowercase();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(38.))
            .flex_none()
            .px(px(12.))
            .text_size(px(12.))
            .border_b_1()
            .border_color(p.line)
            .child(
                [
                    "Shapes",
                    "Templates",
                    "Containers",
                    "Themes",
                    "Stencil packs",
                ][self.diagram_ui.library_tab],
            )
            .when(self.diagram_ui.open, |header| {
                header.child(
                    Button::new("diagram-more-shapes")
                        .label("Add shapes…")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.diagram_library_dialog(window, cx)
                        })),
                )
            })
            .child(
                Button::new("diagram-toggle-drawer")
                    .label(if self.diagram_ui.open { "‹" } else { "›" })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.diagram_ui.open = !this.diagram_ui.open;
                        cx.notify();
                    })),
            );
        if !self.diagram_ui.open {
            return Some(
                div()
                    .w(px(68.))
                    .flex_none()
                    .p_2()
                    .child(header)
                    .into_any_element(),
            );
        }
        let mut content = div()
            .id("diagram-drawer-content")
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .py(px(8.))
            .child(
                div()
                    .id("diagram-stencil-search")
                    .test_support()
                    .child(Styled::h(Input::new(&search).small(), px(26.))),
            );
        content = content.child(
            div().grid().grid_cols(2).gap_1().children(
                [
                    "Shapes",
                    "Templates",
                    "Containers",
                    "Themes",
                    "Stencil packs",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    Button::new(("diagram-library-tab", index))
                        .label(label)
                        .small()
                        .ghost()
                        .selected(self.diagram_ui.library_tab == index)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.diagram_ui.library_tab = index;
                            if let Some(search) = &this.diagram_ui.search {
                                search.update(cx, |s, cx| s.set_value("", window, cx));
                            }
                            cx.notify();
                        }))
                }),
            ),
        );
        if self.diagram_ui.library_tab == 1 {
            content = content.child(self.diagram_template_cards(&query, p, cx));
        }
        if self.diagram_ui.library_tab == 3 {
            content = content.child(self.diagram_theme_cards(&query, p, cx));
        }
        if self.diagram_ui.library_tab == 4 {
            content = content.child(self.diagram_pack_cards(&query, p, cx));
        }
        if !self.diagram_ui.import_notes.is_empty() {
            content = content.child(
                Button::new("diagram-import-notes")
                    .label(format!(
                        "Import notes ({})",
                        self.diagram_ui.import_notes.len()
                    ))
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_diagram_import_notes(window, cx)
                    })),
            );
        }
        if self.diagram_ui.library_tab == 0 {
            content = content.child(self.document_stencil_toolbox(&query, p, window, cx));
        }
        if matches!(self.diagram_ui.library_tab, 0 | 2) {
            let enabled = crate::app_state::settings(cx)
                .diagram_shape_libraries
                .clone();
            let mut categories = diagram::stencils::CATEGORIES
                .iter()
                .enumerate()
                .collect::<Vec<_>>();
            categories.sort_by_key(|(i, label)| match **label {
                "General" => 0,
                "Flowchart" => 1,
                _ => i + 2,
            });
            for (category_index, &label) in categories {
                if self.diagram_ui.library_tab == 0 && !enabled.iter().any(|c| c == label) {
                    continue;
                }
                let display_label = if label == "General" {
                    "Standard"
                } else {
                    label
                };
                let stencils = diagram::stencils::STENCILS
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(_, stencil)| {
                        stencil.category == label
                            && stencil.matches(&query)
                            && (self.diagram_ui.library_tab == 0 || stencil.kind.is_container())
                    })
                    .collect::<Vec<_>>();
                if stencils.is_empty() {
                    continue;
                }
                let collapsed = self.diagram_ui.library_tab == 0
                    && query.is_empty()
                    && self.diagram_ui.collapsed_categories.contains(label);
                content = content.child(
                    Button::new(("diagram-stencil-category", category_index))
                        .label(format!(
                            "{} {display_label} ({})",
                            if collapsed { "›" } else { "⌄" },
                            stencils.len()
                        ))
                        .small()
                        .ghost()
                        .w_full()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.diagram_ui.collapsed_categories.remove(label) {
                                this.diagram_ui.collapsed_categories.insert(label);
                            }
                            cx.notify();
                        })),
                );
                if collapsed {
                    continue;
                }
                let mut grid = div().id(label).grid().grid_cols(5).gap(px(4.));
                for (i, stencil) in stencils {
                    let ink = p.ink;
                    let glyph = canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let x = f32::from(bounds.left()) + 5.;
                            let y = f32::from(bounds.top()) + 8.;
                            let path = stencil.path([
                                x as f64,
                                y as f64,
                                (f32::from(bounds.size.width) - 10.) as f64,
                                (f32::from(bounds.size.height) - 16.) as f64,
                            ]);
                            let mut drawing = PathBuilder::stroke(px(1.));
                            for (points, closed) in path.flatten(0.3) {
                                if let Some(first) = points.first() {
                                    drawing.move_to(point(px(first.0 as f32), px(first.1 as f32)));
                                }
                                for p in points.iter().skip(1) {
                                    drawing.line_to(point(px(p.0 as f32), px(p.1 as f32)));
                                }
                                if closed {
                                    drawing.close();
                                }
                            }
                            if let Ok(path) = drawing.build() {
                                window.paint_path(path, ink);
                            }
                        },
                    )
                    .size_full();
                    grid = grid.child(
                        div()
                            .id(("diagram-shape", i))
                            .test_support()
                            .cursor_pointer()
                            .w_full()
                            .h(px(42.))
                            .border_1()
                            .border_color(p.line)
                            .rounded(px(5.))
                            .hover(|d| d.border_color(p.accent).bg(p.accent.opacity(0.08)))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(format!(
                                    "{} · Drag to canvas",
                                    stencil.label
                                ))
                                .build(window, cx)
                            })
                            .child(glyph)
                            .on_drag(DraggedStencil(stencil), |drag, _, _, cx| {
                                cx.new(|_| drag.clone())
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.insert_diagram_stencil(stencil, cx)
                            })),
                    );
                }
                content = content.child(grid);
            }
        }
        if self.diagram_ui.library_tab == 0 {
            content=content.child(Button::new("diagram-connect").label(if self.diagram_ui.connecting{"Cancel connection"}else{"Connect shapes"}).selected(self.diagram_ui.connecting).outline().on_click(cx.listener(|this,_,_,cx|{let active=this.diagram_ui.connecting;this.set_tool(Tool::Move,cx);this.diagram_cancel_connection();this.diagram_ui.connecting= !active;this.set_status(if active{"Connection cancelled."}else{"Click anywhere on a source object, then anywhere on the destination. Attachments follow the objects."},false,cx);})));
            let owner = cx.weak_entity();
            content = content.child(
                Button::new("diagram-layout")
                    .label("Arrange diagram ▾")
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for layout in Layout::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(layout.label()).on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| this.layout_diagram(layout, cx))
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
            );
            content = content.child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("diagram-grid")
                            .label("Grid")
                            .selected(self.diagram_ui.grid)
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.diagram_ui.grid = !this.diagram_ui.grid;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("diagram-minimap")
                            .label("Minimap")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_navigator(cx))),
                    ),
            );
            content = content
                .child(
                    Button::new("diagram-import-file")
                        .label("Import diagram pages…")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.import_diagram_file(cx))),
                )
                .child(
                    Button::new("diagram-export-file")
                        .label("Export editable .drawio…")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.export_drawio_file(cx))),
                );
            let owner = cx.weak_entity();
            content = content.child(
                Button::new("diagram-generate")
                    .label("Generate from data ▾")
                    .small()
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for format in emulsion_io::diagram_data::Format::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(format.label()).on_click(
                                move |_, window, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.diagram_data_dialog(format, false, window, cx)
                                        })
                                        .ok();
                                },
                            ));
                        }
                        let import = owner.clone();
                        let refresh = owner.clone();
                        menu.separator()
                            .item(PopupMenuItem::new("Import local data file…").on_click(
                                move |_, _, cx| {
                                    import
                                        .update(cx, |this, cx| this.import_diagram_data(cx))
                                        .ok();
                                },
                            ))
                            .item(
                                PopupMenuItem::new("Refresh mapped labels and data from CSV…")
                                    .on_click(move |_, window, cx| {
                                        refresh
                                            .update(cx, |this, cx| {
                                                this.diagram_data_dialog(
                                                    emulsion_io::diagram_data::Format::Csv,
                                                    true,
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    }),
                            )
                    }),
            );
            content =
                content.child(
                    Button::new("diagram-conditional-fill")
                        .label("Color shapes by data…")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.diagram_conditional_fill(window, cx)
                        })),
                );
            if self
                .editor
                .doc
                .diagram
                .as_ref()
                .is_some_and(|d| d.shapes.values().any(|s| !s.conditions.is_empty()))
            {
                content = content.child(
                    Button::new("diagram-clear-conditions")
                        .label("Clear selected color rules")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| this.clear_diagram_conditions(cx))),
                );
            }
        }
        if self.diagram_ui.library_tab == 4 {
            content = content.child(self.creative_pack_controls(cx));
        } else if self.diagram_ui.library_tab == 0 {
            content = content.child(self.stencil_pack_list(&query, p, cx));
        }
        content=content.child(div().text_size(px(11.)).text_color(p.muted).child("Drag on empty canvas to select. Ctrl/Shift-click adds or removes objects. Ctrl+G groups the selection. Double-click text to edit it. Connectors follow moved shapes."));
        let narrow = window.viewport_size().width < px(1100.);
        let drawer = div()
            .id("diagram-drawer")
            .test_support()
            .w(px(250.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(p.panel)
            .border_r_1()
            .border_color(p.line)
            .child(header)
            .child(content)
            .when(narrow, |d| {
                d.absolute().top_0().bottom_0().left_0().occlude()
            });
        Some(if narrow {
            div()
                .relative()
                .w(px(68.))
                .flex_none()
                .child(deferred(drawer).with_priority(1))
                .into_any_element()
        } else {
            drawer.into_any_element()
        })
    }
}

#[derive(Clone, Default)]
pub(super) struct DiagramOverlay {
    selected: Vec<[f64; 4]>,
    connector_lines: Vec<Vec<(f64, f64)>>,
    connector_ends: Vec<(f64, f64)>,
    connector_bends: Vec<((f64, f64), bool)>,
    marquee: Option<[f64; 4]>,
    ports: Vec<(f64, f64)>,
    preview: Vec<(f64, f64)>,
    target: Option<(f64, f64)>,
}
pub(super) fn paint_connections(
    overlay: &DiagramOverlay,
    view: &View,
    bounds: Bounds<Pixels>,
    accent: Hsla,
    window: &mut Window,
) {
    let screen = |p| {
        let p = view.doc_to_screen(p, &bounds);
        point(px(p.0 as f32), px(p.1 as f32))
    };
    for ([x, y, w, h], marquee) in overlay
        .selected
        .iter()
        .copied()
        .map(|b| (b, false))
        .chain(overlay.marquee.map(|b| (b, true)))
    {
        let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        if marquee {
            let mut path = PathBuilder::fill();
            path.move_to(screen(corners[0]));
            for p in &corners[1..] {
                path.line_to(screen(*p));
            }
            path.close();
            if let Ok(path) = path.build() {
                let mut tint = accent;
                tint.a = 0.12;
                window.paint_path(path, tint);
            }
        }
        let mut path = PathBuilder::stroke(px(1.));
        path.move_to(screen(corners[0]));
        for p in &corners[1..] {
            path.line_to(screen(*p));
        }
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, accent);
        }
    }
    for line in &overlay.connector_lines {
        if let Some(first) = line.first() {
            let mut path = PathBuilder::stroke(px(1.5));
            path.move_to(screen(*first));
            for p in &line[1..] {
                path.line_to(screen(*p));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, accent);
            }
        }
    }
    for (p, vertical) in &overlay.connector_bends {
        let c = screen(*p);
        let (w, h) = if *vertical { (10., 24.) } else { (24., 10.) };
        let b = Bounds::new(c - point(px(w / 2.), px(h / 2.)), size(px(w), px(h)));
        window.paint_quad(fill(b, white()).corner_radii(px(5.)));
        window.paint_quad(
            outline(b, accent, BorderStyle::Solid)
                .border_widths(px(1.5))
                .corner_radii(px(5.)),
        );
    }
    for p in &overlay.connector_ends {
        let c = screen(*p);
        let b = Bounds::new(c - point(px(4.), px(4.)), size(px(8.), px(8.)));
        window.paint_quad(fill(b, white()));
        window.paint_quad(outline(b, accent, BorderStyle::Solid).border_widths(px(1.5)));
    }
    if let Some(first) = overlay.preview.first() {
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(screen(*first));
        for p in &overlay.preview[1..] {
            path.line_to(screen(*p));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, accent);
        }
    }
    for p in &overlay.ports {
        let c = screen(*p);
        let b = Bounds::new(c - point(px(4.), px(4.)), size(px(8.), px(8.)));
        window.paint_quad(fill(b, white()).corner_radii(px(4.)));
        window.paint_quad(
            outline(b, accent, BorderStyle::Solid)
                .border_widths(px(1.5))
                .corner_radii(px(4.)),
        );
    }
    if let Some(p) = overlay.target {
        let c = screen(p);
        let b = Bounds::new(c - point(px(6.), px(6.)), size(px(12.), px(12.)));
        window.paint_quad(
            outline(b, accent, BorderStyle::Solid)
                .border_widths(px(2.))
                .corner_radii(px(6.)),
        );
    }
}
