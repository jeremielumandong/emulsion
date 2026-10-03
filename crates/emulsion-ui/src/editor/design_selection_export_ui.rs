//! Native selection export form; render and file IO run outside the UI thread.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_io::selection_export::{self, Bounds as ExportBounds, Format, Options};
use gpui_kit::component::{Selectable, Sizable, WindowExt, button::Button};
struct Form {
    format: Format,
    bounds: ExportBounds,
    transparent: bool,
    strict: bool,
    padding: Entity<InputState>,
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(t!("editor.design_selection_export_ui.intro"))
            .child(
                div().flex().gap_2().children(
                    [Format::Svg, Format::Pdf, Format::Png]
                        .into_iter()
                        .enumerate()
                        .map(|(i, format)| {
                            Button::new(("selection-export-format", i))
                                .label(format.extension().to_uppercase())
                                .small()
                                .outline()
                                .selected(self.format == format)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.format = format;
                                    cx.notify();
                                }))
                        }),
                ),
            )
            .child(
                div().flex().flex_wrap().gap_2().children(
                    [
                        (
                            ExportBounds::Content,
                            t!("editor.design_selection_export_ui.content_bounds"),
                        ),
                        (
                            ExportBounds::Frame,
                            t!("editor.design_selection_export_ui.frame_bounds"),
                        ),
                        (
                            ExportBounds::Canvas,
                            t!("editor.design_selection_export_ui.canvas_bounds"),
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (bounds, label))| {
                        Button::new(("selection-export-bounds", i))
                            .label(label)
                            .small()
                            .outline()
                            .selected(self.bounds == bounds)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.bounds = bounds;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(
                div()
                    .child(t!("editor.design_selection_export_ui.padding"))
                    .child(Input::new(&self.padding).id("selection-export-padding")),
            )
            .child(
                Button::new("selection-export-transparent")
                    .label(if self.transparent {
                        t!("editor.design_selection_export_ui.transparent")
                    } else {
                        t!("editor.design_selection_export_ui.white")
                    })
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.transparent = !this.transparent;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("selection-export-strict")
                    .label(if self.strict {
                        t!("editor.design_selection_export_ui.strict")
                    } else {
                        t!("editor.design_selection_export_ui.allow_rendered")
                    })
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.strict = !this.strict;
                        cx.notify();
                    })),
            )
            .child(t!("editor.design_selection_export_ui.note"))
    }
}
impl EditorView {
    pub(crate) fn show_selection_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            self.set_status(t!("editor.design_selection_export_ui.select"), true, cx);
            return;
        }
        let source = self.editor.doc.clone();
        let owner = cx.weak_entity();
        let form = cx.new(|cx| Form {
            format: Format::Svg,
            bounds: ExportBounds::Content,
            transparent: true,
            strict: false,
            padding: cx.new(|cx| InputState::new(window, cx).default_value("0")),
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            let source = source.clone();
            let ids = ids.clone();
            let form = form.clone();
            dialog
                .title(t!("editor.design_selection_export_ui.title"))
                .width(px(520.))
                .child(form.clone())
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_selection_export_ui.choose_file"
                )))
                .on_ok(move |_, _, cx| {
                    let state = form.read(cx);
                    let Ok(padding) = state.padding.read(cx).value().trim().parse::<u32>() else {
                        owner
                            .update(cx, |this, cx| {
                                this.set_status(
                                    t!("editor.design_selection_export_ui.padding_error"),
                                    true,
                                    cx,
                                )
                            })
                            .ok();
                        return false;
                    };
                    if padding > 1000 {
                        owner
                            .update(cx, |this, cx| {
                                this.set_status(
                                    t!("editor.design_selection_export_ui.padding_error"),
                                    true,
                                    cx,
                                )
                            })
                            .ok();
                        return false;
                    }
                    let format = state.format;
                    let options = Options {
                        bounds: state.bounds,
                        padding,
                        transparent: state.transparent,
                        strict_vectors: state.strict,
                        overwrite: true,
                    };
                    let source = source.clone();
                    let ids = ids.clone();
                    owner
                        .update(cx, move |this, cx| {
                            this.choose_selection_export_file(source, ids, format, options, cx)
                        })
                        .is_ok()
                })
        });
    }
    fn choose_selection_export_file(
        &mut self,
        source: Document,
        ids: Vec<NodeId>,
        format: Format,
        options: Options,
        cx: &mut Context<Self>,
    ) {
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
        let rx = cx.prompt_save_path(
            &dir,
            Some(&format!("{}-selection.{}", self.name, format.extension())),
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension(format.extension());
            this.update(cx, |this, cx| {
                this.set_status(t!("editor.design_selection_export_ui.exporting"), false, cx)
            })
            .ok();
            let output = path.clone();
            let result = cx
                .background_spawn(async move {
                    selection_export::write(&source, &ids, format, &options, &output)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(report) => this.set_status(
                    t!(
                        "editor.design_selection_export_ui.exported",
                        width = report.width,
                        height = report.height,
                        path = path.display(),
                        diagnostics = report.diagnostics.join(" ")
                    ),
                    false,
                    cx,
                ),
                Err(error) => this.set_status(
                    t!("editor.design_selection_export_ui.failed", error = error),
                    true,
                    cx,
                ),
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui::TestAppContext;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn design_selection_export_form_cancel_and_invalid_padding_never_edit_source(
        cx: &mut TestAppContext,
    ) {
        let mut editor = emulsion_core::Editor::new(Document::new(640, 480), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    emulsion_core::text::TextSpec {
                        text: "Export me".into(),
                        x: 20.,
                        y: 20.,
                        ..Default::default()
                    },
                    640,
                    480,
                )),
                slot: emulsion_core::command::Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let original = editor.doc.clone();
        let (ws, cx) = crate::tests::open(cx, editor.doc);
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.set_layer_selection(vec![id], Some(id));
                this.show_selection_export(window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.click(("selection-export-format", 2usize), cx);
            window.click("selection-export-transparent", cx);
            window.click("selection-export-padding", cx);
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("-1");
        cx.run_until_parked();
        cx.update(|w, cx| w.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("selection-export-padding").visible());
            assert_eq!(view.read(cx).editor.doc, original);
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    }
}
