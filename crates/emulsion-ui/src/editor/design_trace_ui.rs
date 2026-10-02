//! Bounded asynchronous bitmap trace preview; authored pixels remain intact.
use super::*;
use emulsion_core::design_vectors::trace::{self, Options};
use gpui_kit::component::{Sizable, WindowExt, button::Button};
struct Form {
    source: Document,
    id: NodeId,
    resolution: Entity<InputState>,
    threshold: Entity<InputState>,
    alpha_only: bool,
    invert: bool,
    preview: Option<Arc<RenderImage>>,
    previewed: Option<Options>,
    outline: Option<emulsion_raster::vector::Path>,
    busy: bool,
    message: String,
}
impl Form {
    fn options(&self, cx: &App) -> Result<Options, String> {
        Ok(Options {
            resolution: self
                .resolution
                .read(cx)
                .value()
                .trim()
                .parse()
                .map_err(|_| t!("editor.design_trace_ui.resolution_error").into_owned())?,
            threshold: self
                .threshold
                .read(cx)
                .value()
                .trim()
                .parse()
                .map_err(|_| t!("editor.design_trace_ui.threshold_error").into_owned())?,
            alpha_only: self.alpha_only,
            invert: self.invert,
            ..Default::default()
        })
    }
    fn preview(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let options = match self.options(cx) {
            Ok(o) => o,
            Err(e) => {
                self.message = e;
                cx.notify();
                return;
            }
        };
        let doc = self.source.clone();
        let id = self.id;
        self.busy = true;
        self.message = t!("editor.design_trace_ui.generating").into_owned();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let original_path = trace::preview(&doc, id, options)?;
                    let mut path = original_path.clone();
                    let (x, y, w, h) = emulsion_raster::vector_geometry::bounds(&path)
                        .ok_or_else(|| t!("editor.design_trace_ui.empty").into_owned())?;
                    let scale = 240. / w.max(h).max(1.);
                    path.transform(
                        glam::DAffine2::from_scale(glam::dvec2(scale, scale))
                            * glam::DAffine2::from_translation(glam::dvec2(-x, -y)),
                    );
                    let count = path.anchor_count();
                    let raster = path.rasterize(
                        &emulsion_raster::vector::PathStyle {
                            fill: Some([0, 0, 0, 255]),
                            stroke: None,
                            ..Default::default()
                        },
                        (w * scale).ceil().max(1.) as u32,
                        (h * scale).ceil().max(1.) as u32,
                    );
                    let mut rgba = raster.to_srgba8();
                    for p in rgba.as_chunks_mut::<4>().0 {
                        p.swap(0, 2);
                    }
                    Ok::<_, String>((
                        Arc::new(crate::viewport::bgra_image(
                            raster.width(),
                            raster.height(),
                            rgba,
                        )),
                        count,
                        original_path,
                    ))
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok((image, count, path)) => {
                        this.outline = Some(path);
                        this.preview = Some(image);
                        this.previewed = Some(options);
                        this.message =
                            t!("editor.design_trace_ui.points", count = count).into_owned();
                    }
                    Err(e) => {
                        this.previewed = None;
                        this.message = e;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(t!("editor.design_trace_ui.intro"))
            .child(
                div()
                    .child(t!("editor.design_trace_ui.resolution"))
                    .child(Input::new(&self.resolution).id("trace-resolution")),
            )
            .child(
                div()
                    .child(t!("editor.design_trace_ui.threshold"))
                    .child(Input::new(&self.threshold).id("trace-threshold")),
            )
            .child(
                Button::new("trace-alpha")
                    .small()
                    .label(if self.alpha_only {
                        t!("editor.design_trace_ui.alpha")
                    } else {
                        t!("editor.design_trace_ui.dark")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.alpha_only = !this.alpha_only;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("trace-invert")
                    .small()
                    .label(if self.invert {
                        t!("editor.design_trace_ui.inverted")
                    } else {
                        t!("editor.design_trace_ui.invert")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.invert = !this.invert;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("trace-preview")
                    .small()
                    .label(if self.busy {
                        t!("editor.design_trace_ui.working")
                    } else {
                        t!("editor.design_trace_ui.update")
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.preview(cx))),
            )
            .when_some(self.preview.clone(), |d, image| {
                d.child(
                    div().bg(gpui_kit::gpui::white()).h(px(240.)).child(
                        img(ImageSource::Render(image))
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    ),
                )
            })
            .child(self.message.clone())
    }
}
impl EditorView {
    pub(crate) fn show_bitmap_trace(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let source = self.editor.doc.clone();
        let owner = cx.weak_entity();
        let form = cx.new(|cx| {
            let mut form = Form {
                source,
                id,
                resolution: cx.new(|cx| InputState::new(window, cx).default_value("256")),
                threshold: cx.new(|cx| InputState::new(window, cx).default_value("0.5")),
                alpha_only: false,
                invert: false,
                preview: None,
                previewed: None,
                outline: None,
                busy: false,
                message: String::new(),
            };
            form.preview(cx);
            form
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            let form = form.clone();
            dialog
                .title(t!("editor.design_trace_ui.title"))
                .width(px(520.))
                .child(form.clone())
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_trace_ui.create"
                )))
                .on_ok(move |_, _, cx| {
                    let state = form.read(cx);
                    let options = state.options(cx).and_then(|o| {
                        if state.previewed == Some(o) {
                            Ok(o)
                        } else {
                            Err(t!("editor.design_trace_ui.stale").into_owned())
                        }
                    });
                    let source = state.source.node(id).cloned();
                    let outline = state.outline.clone();
                    owner
                        .update(cx, |this, cx| {
                            match options.and_then(|options| {
                                trace::apply_preview(
                                    &mut this.editor,
                                    source.as_ref().ok_or_else(|| {
                                        t!("editor.design_trace_ui.missing_source").into_owned()
                                    })?,
                                    outline.clone().ok_or_else(|| {
                                        t!("editor.design_trace_ui.no_preview").into_owned()
                                    })?,
                                    options.color,
                                )
                            }) {
                                Ok(id) => {
                                    this.set_layer_selection(vec![id], Some(id));
                                    this.after_change(cx);
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
}
