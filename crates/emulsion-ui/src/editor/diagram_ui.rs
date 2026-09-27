//! Native diagram stencils, connection gestures and graph properties.
use super::*;
use emulsion_core::{
    diagram::{self, Endpoint, Layout, Port, Routing, ShapeKind},
    project::ProjectKind,
};
use gpui_kit::component::{
    Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    input::InputEvent,
    menu::{DropdownMenu, PopupMenuItem},
};

pub(super) struct DiagramUi {
    open: bool,
    search: Option<Entity<InputState>>,
    subscription: Option<Subscription>,
    connecting: bool,
    source: Option<Endpoint>,
    reconnect: Option<(NodeId, bool)>,
    pub(super) grid: bool,
}
impl Default for DiagramUi {
    fn default() -> Self {
        Self {
            open: true,
            search: None,
            subscription: None,
            connecting: false,
            source: None,
            reconnect: None,
            grid: true,
        }
    }
}
impl EditorView {
    fn import_diagram_file(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import draw.io, Visio (.vsdx/.vdx), or Lucid (.lucid) pages".into()),
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
                        this.after_change(cx);
                        this.set_status(
                            format!(
                                "Imported {} editable page(s). {}",
                                ids.len(),
                                warnings.join(" ")
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

    pub(super) fn is_diagram(&self) -> bool {
        self.editor.kind() == Some(ProjectKind::Diagram)
    }
    pub(super) fn diagram_cancel_connection(&mut self) -> bool {
        let active = self.diagram_ui.connecting || self.diagram_ui.reconnect.is_some();
        self.diagram_ui.connecting = false;
        self.diagram_ui.source = None;
        self.diagram_ui.reconnect = None;
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
        let model = self.editor.doc.diagram.as_ref()?;
        let mut nodes = self.editor.doc.nodes.iter().rev().collect::<Vec<_>>();
        nodes.sort_by_key(|n| {
            model
                .shapes
                .get(&n.id)
                .is_some_and(|s| s.kind.is_container())
        });
        for node in nodes {
            let Some(shape) = model.shapes.get(&node.id) else {
                continue;
            };
            let mut visible = node.visible;
            let mut parent = node.parent;
            while let Some(id) = parent {
                let Some(n) = self.editor.doc.node(id) else {
                    break;
                };
                visible &= n.visible;
                parent = n.parent;
            }
            if !visible {
                continue;
            }
            let [x, y, w, h] = diagram::shape_bounds(&self.editor.doc, shape)?;
            let tolerance = 10. / self.view.zoom;
            if point.0 < x - tolerance
                || point.0 > x + w + tolerance
                || point.1 < y - tolerance
                || point.1 > y + h + tolerance
            {
                continue;
            }
            if shape.kind.is_container()
                && point.0 > x + tolerance
                && point.0 < x + w - tolerance
                && point.1 > y + 32.
                && point.1 < y + h - tolerance
            {
                continue;
            }
            let port = [Port::North, Port::East, Port::South, Port::West]
                .into_iter()
                .find(|port| {
                    let (p, _) = port.anchor([x, y, w, h], point);
                    (p.0 - point.0).hypot(p.1 - point.1) <= tolerance
                })
                .unwrap_or(Port::Auto);
            return Some(Endpoint {
                shape: node.id,
                port,
            });
        }
        None
    }
    fn diagram_edge_hit(&self, point: (f64, f64)) -> Option<NodeId> {
        let model = self.editor.doc.diagram.as_ref()?;
        for (id, edge) in model.edges.iter().rev() {
            if !self.editor.doc.node(*id).is_some_and(|n| n.visible) {
                continue;
            }
            let Some(NodeKind::Path { path, .. }) =
                self.editor.doc.node(edge.path).map(|n| &n.kind)
            else {
                continue;
            };
            for line in &path.subpaths {
                for pair in line.anchors.windows(2) {
                    let (a, b) = (pair[0].p, pair[1].p);
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
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_diagram() {
            return false;
        }
        let hit = self.diagram_hit(point);
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
            let object = hit
                .map(|e| e.shape)
                .or_else(|| self.diagram_edge_hit(point));
            let Some(object) = object else {
                if !shift {
                    self.set_layer_selection(Vec::new(), None);
                    cx.notify();
                }
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
        }
        false
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
    pub(crate) fn insert_diagram_shape(&mut self, kind: ShapeKind, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let count = self
            .editor
            .doc
            .diagram
            .as_ref()
            .map_or(0, |d| d.shapes.len());
        let (w, h) = if kind.is_container() {
            (420., 280.)
        } else {
            (140., 80.)
        };
        let x = (self.editor.doc.width as f64 / 2. - w / 2. + (count % 5) as f64 * 24.).max(20.);
        let y = (self.editor.doc.height as f64 / 2. - h / 2. + (count % 5) as f64 * 24.).max(20.);
        match diagram::add_shape(&mut self.editor, kind, [x, y, w, h], kind.label()) {
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
        let label = cx.new(|cx| InputState::new(window, cx).default_value(text));
        let details = cx.new(|cx| InputState::new(window, cx).default_value(data));
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
                .width(px(480.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Label")
                        .child(Input::new(&label))
                        .child(if shape {
                            "Data · JSON object, e.g. {\"owner\":\"Design\"}"
                        } else {
                            "Waypoints · JSON coordinates, e.g. [[200,100],[200,300]]"
                        })
                        .child(Input::new(&details)),
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
                                if text.chars().count() > 2000 {
                                    return Err("Labels can contain up to 2,000 characters.".into());
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
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search shapes"));
            self.diagram_ui.subscription = Some(cx.subscribe(&input, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
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
            .child("Diagram")
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
            .child(Input::new(&search).small());
        content=content.child(Button::new("diagram-connect").label(if self.diagram_ui.connecting{"Cancel connection"}else{"Connect shapes"}).selected(self.diagram_ui.connecting).outline().on_click(cx.listener(|this,_,_,cx|{let active=this.diagram_ui.connecting;this.set_tool(Tool::Move,cx);this.diagram_cancel_connection();this.diagram_ui.connecting= !active;this.set_status(if active{"Connection cancelled."}else{"Click a source shape, then a destination. Click near an edge midpoint for a fixed port."},false,cx);})));
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
        if let Some(id) = self.diagram_object() {
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
                            .label(if routing == Routing::Orthogonal {
                                "Routing: orthogonal"
                            } else {
                                "Routing: straight"
                            })
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_diagram_edge(
                                    id,
                                    |e| {
                                        e.routing = if routing == Routing::Orthogonal {
                                            Routing::Straight
                                        } else {
                                            Routing::Orthogonal
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
        content = content.child(
            Button::new("diagram-conditional-fill")
                .label("Color shapes by data…")
                .small()
                .ghost()
                .on_click(
                    cx.listener(|this, _, window, cx| this.diagram_conditional_fill(window, cx)),
                ),
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
        content = content
            .child(self.creative_pack_controls(cx))
            .child(self.stencil_pack_list(&query, p, cx));
        for (i, kind) in ShapeKind::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, kind)| kind.label().to_lowercase().contains(&query))
        {
            content = content.child(
                Button::new(("diagram-shape", i))
                    .label(kind.label())
                    .w_full()
                    .h(px(42.))
                    .outline()
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.insert_diagram_shape(kind, cx)),
                    ),
            );
        }
        content=content.child(self.alignment_controls(p,cx)).child(div().text_size(px(11.)).text_color(p.muted).child("Shift-click to select several shapes. Double-click text to edit it. Connectors follow moved shapes."));
        let narrow = window.viewport_size().width < px(1100.);
        let drawer = div()
            .id("diagram-drawer")
            .test_support()
            .w(px(250.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
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
                .child(drawer)
                .into_any_element()
        } else {
            drawer.into_any_element()
        })
    }
}
