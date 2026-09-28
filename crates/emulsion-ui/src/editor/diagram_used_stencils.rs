//! A bounded, asynchronously rendered toolbox of artwork already on the page.
use super::*;
const PAGE_SIZE: usize = 24;
type Key = (u64, u64, String, usize);
#[derive(Default)]
pub(super) struct UsedStencils {
    requested: Option<Key>,
    shown: Option<Key>,
    building: bool,
    pub(super) page: usize,
    total: usize,
    entries: Vec<(diagram::DocumentStencil, Option<Arc<RenderImage>>)>,
    retired: Vec<Arc<RenderImage>>,
}
#[derive(Clone)]
pub(crate) struct DraggedDocumentStencil {
    page: u64,
    source: NodeId,
    name: String,
}
impl Render for DraggedDocumentStencil {
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
            .child(self.name.clone())
    }
}
impl EditorView {
    pub(crate) fn release_document_stencil_previews(&mut self, window: &mut Window) {
        let used = &mut self.diagram_ui.used;
        used.requested = None;
        used.shown = None;
        for image in used
            .retired
            .drain(..)
            .chain(used.entries.drain(..).filter_map(|(_, image)| image))
        {
            window.drop_image(image).ok();
        }
    }

    pub(crate) fn drop_document_stencil(
        &mut self,
        drag: &DraggedDocumentStencil,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.is_diagram() || self.editor.active_page() != drag.page {
            return;
        }
        if let Some(center) = self.doc_point(position) {
            self.place_document_stencil(drag.source, center, cx);
        }
    }
    pub(crate) fn place_document_stencil(
        &mut self,
        source: NodeId,
        center: (f64, f64),
        cx: &mut Context<Self>,
    ) {
        if !self.is_diagram() || !self.prepare_page_action(cx) {
            return;
        }
        match diagram::insert_document_stencil(&mut self.editor, source, center) {
            Ok(ids) => {
                self.after_change(cx);
                self.set_layer_selection(ids.clone(), ids.first().copied());
                self.set_tool(Tool::Move, cx);
                self.set_status("Placed editable object from this diagram.", false, cx);
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub(super) fn document_stencil_toolbox(
        &mut self,
        query: &str,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self
            .diagram_ui
            .used
            .shown
            .as_ref()
            .is_some_and(|s| s.0 != self.editor.active_page())
        {
            self.diagram_ui.used.page = 0;
        }
        let key = (
            self.editor.active_page(),
            self.editor.revision,
            query.to_string(),
            self.diagram_ui.used.page,
        );
        let used = &mut self.diagram_ui.used;
        for image in used.retired.drain(..) {
            window.drop_image(image).ok();
        }
        if self.drag.is_none() && !used.building && used.requested.as_ref() != Some(&key) {
            used.building = true;
            used.requested = Some(key.clone());
            let request = key.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(180))
                    .await;
                let source = this
                    .update(cx, |v, _| {
                        ((v.editor.active_page(), v.editor.revision) == (request.0, request.1))
                            .then(|| v.editor.doc.clone())
                    })
                    .ok()
                    .flatten();
                let result = if let Some(doc) = source {
                    let query = request.2.clone();
                    let page = request.3;
                    Some(
                        cx.background_spawn(async move {
                            let entries = diagram::document_stencils(&doc)
                                .into_iter()
                                .filter(|e| e.name.to_lowercase().contains(&query))
                                .collect::<Vec<_>>();
                            let total = entries.len();
                            let entries = entries
                                .into_iter()
                                .skip(page * PAGE_SIZE)
                                .take(PAGE_SIZE)
                                .map(|entry| {
                                    let preview = (|| {
                                        let fragment =
                                            diagram::document_stencil(&doc, entry.source).ok()?;
                                        let mut source =
                                            emulsion_core::Document::new(doc.width, doc.height);
                                        source.nodes = fragment.nodes;
                                        source.diagram = fragment.diagram.map(Arc::new);
                                        source.design = fragment.design;
                                        source.normalize();
                                        let b = emulsion_core::geometry::node_bounds(
                                            &source,
                                            entry.source,
                                        )?;
                                        let scale = 112. / b.w.max(b.h).max(1) as f64;
                                        let scene =
                                            emulsion_io::svg_viewport::SvgViewport::new(&source)
                                                .ok()?;
                                        let pixels = scene
                                            .render(
                                                (128, 128),
                                                [
                                                    scale,
                                                    0.,
                                                    0.,
                                                    scale,
                                                    64. - (b.x as f64 + b.w as f64 / 2.) * scale,
                                                    64. - (b.y as f64 + b.h as f64 / 2.) * scale,
                                                ],
                                            )
                                            .ok()?;
                                        Some(Arc::new(crate::viewport::bgra_image(
                                            128, 128, pixels,
                                        )))
                                    })();
                                    (entry, preview)
                                })
                                .collect::<Vec<_>>();
                            (total, entries)
                        })
                        .await,
                    )
                } else {
                    None
                };
                this.update(cx, |v, cx| {
                    let current = (v.editor.active_page(), v.editor.revision);
                    let used = &mut v.diagram_ui.used;
                    used.building = false;
                    if v.visible
                        && current == (request.0, request.1)
                        && let Some((total, entries)) = result
                    {
                        used.retired
                            .extend(used.entries.drain(..).filter_map(|(_, image)| image));
                        used.entries = entries;
                        used.total = total;
                        if total > 0 && used.page * PAGE_SIZE >= total {
                            used.page = 0;
                        }
                        used.shown = Some(request);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let used = &self.diagram_ui.used;
        let visible = used
            .shown
            .as_ref()
            .is_some_and(|s| s.0 == key.0 && s.2 == key.2 && s.3 == key.3);
        let mut section = div()
            .id("diagram-used-stencils")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(p.ink)
                    .child(if visible {
                        format!("Shapes in this diagram ({})", used.total)
                    } else {
                        "Shapes in this diagram".into()
                    }),
            );
        if visible {
            let mut grid = div().grid().grid_cols(4).gap(px(4.));
            for (entry, preview) in &used.entries {
                let source = entry.source;
                let page = key.0;
                let name = entry.name.clone();
                let mut cell = div()
                    .id(("diagram-used-shape", source))
                    .test_support()
                    .cursor_pointer()
                    .h(px(52.))
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(5.))
                    .hover(|d| d.border_color(p.accent).bg(p.accent.opacity(0.06)))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(format!(
                            "{name} · Drag to reuse"
                        ))
                        .build(window, cx)
                    });
                if let Some(preview) = preview {
                    cell = cell.child(
                        img(preview.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    );
                } else {
                    cell = cell.child(div().text_size(px(10.)).child(entry.name.clone()));
                }
                grid = grid.child(
                    cell.on_drag(
                        DraggedDocumentStencil {
                            page,
                            source,
                            name: entry.name.clone(),
                        },
                        |drag, _, _, cx| cx.new(|_| drag.clone()),
                    )
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if v.editor.active_page() == page {
                            let center = v.view.center;
                            v.place_document_stencil(source, (center.0, center.1), cx);
                        }
                    })),
                );
            }
            section = section.child(grid).child(
                Button::new("diagram-save-used-stencils").label("Save shapes to library").xsmall().ghost()
                .on_click(cx.listener(|v,_,_,cx|v.save_imported_stencils(&[v.editor.active_page()],cx)))
            );
            if used.total > PAGE_SIZE {
                let page = used.page;
                section = section.child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            Button::new("diagram-used-prev")
                                .label("Previous")
                                .xsmall()
                                .ghost()
                                .disabled(page == 0)
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.diagram_ui.used.page =
                                        v.diagram_ui.used.page.saturating_sub(1);
                                    cx.notify();
                                })),
                        )
                        .child(div().text_size(px(10.)).child(format!(
                            "{} / {}",
                            page + 1,
                            used.total.div_ceil(PAGE_SIZE)
                        )))
                        .child(
                            Button::new("diagram-used-next")
                                .label("Next")
                                .xsmall()
                                .ghost()
                                .disabled((page + 1) * PAGE_SIZE >= used.total)
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.diagram_ui.used.page += 1;
                                    cx.notify();
                                })),
                        ),
                );
            }
            if used.total == 0 {
                section = section.child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child("Imported shapes appear here automatically."),
                );
            }
        } else {
            section = section.child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("Preparing object previews…"),
            );
        }
        section.into_any_element()
    }
}
