//! Per-paragraph lists and spacing using the shared native shaping pipeline.
use super::*;
use emulsion_core::text::{Align, ParagraphFormat, ParagraphList};
use gpui_kit::component::{Selectable, Sizable, WindowExt, button::Button};
/// Display name for a field; the English name stays the lookup key.
fn field_label(key: &str) -> SharedString {
    match key {
        "Level" => t!("editor.design_paragraph_ui.level"),
        "Left indent" => t!("editor.design_paragraph_ui.left_indent"),
        "Hanging marker" => t!("editor.design_paragraph_ui.hanging"),
        "Space before" => t!("editor.design_paragraph_ui.space_before"),
        "Space after" => t!("editor.design_paragraph_ui.space_after"),
        "Restart numbering" => t!("editor.design_paragraph_ui.restart"),
        other => return SharedString::from(other.to_string()),
    }
    .into()
}
struct Form {
    list: ParagraphList,
    align: Option<Align>,
    fields: Vec<(&'static str, Entity<InputState>)>,
}
impl Form {
    fn values(&self, cx: &App) -> Result<ParagraphFormat, String> {
        let value = |key| {
            self.fields
                .iter()
                .find(|(k, _)| *k == key)
                .unwrap()
                .1
                .read(cx)
                .value()
                .to_string()
        };
        let number = |key| {
            value(key).trim().parse::<f32>().map_err(|_| {
                t!(
                    "editor.design_paragraph_ui.number",
                    field = field_label(key)
                )
                .into_owned()
            })
        };
        let level = value("Level")
            .trim()
            .parse::<u8>()
            .map_err(|_| t!("editor.design_paragraph_ui.level_error").into_owned())?;
        let restart = value("Restart numbering");
        let restart = if restart.trim().is_empty() {
            None
        } else {
            Some(
                restart
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| t!("editor.design_paragraph_ui.restart_error").into_owned())?,
            )
        };
        let format = ParagraphFormat {
            list: self.list,
            align: self.align,
            level,
            indent: number("Left indent")?,
            hanging: number("Hanging marker")?,
            space_before: number("Space before")?,
            space_after: number("Space after")?,
            restart,
        };
        format.validate()?;
        Ok(format)
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(t!("editor.design_paragraph_ui.intro"))
            .child(
                div().flex().gap_1().children(
                    [
                        (
                            ParagraphList::None,
                            t!("editor.design_paragraph_ui.list_none"),
                        ),
                        (
                            ParagraphList::Bullet,
                            t!("editor.design_paragraph_ui.list_bullets"),
                        ),
                        (
                            ParagraphList::Numbered,
                            t!("editor.design_paragraph_ui.list_numbered"),
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (list, label))| {
                        Button::new(("paragraph-list", i))
                            .small()
                            .outline()
                            .label(label)
                            .selected(self.list == list)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.list = list;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(
                div().flex().gap_1().children(
                    [
                        (None, t!("editor.design_paragraph_ui.inherit")),
                        (Some(Align::Left), t!("editor.design_editor.align_left")),
                        (Some(Align::Center), t!("editor.design_editor.align_center")),
                        (Some(Align::Right), t!("editor.design_editor.align_right")),
                        (
                            Some(Align::Justify),
                            t!("editor.design_editor.align_justify"),
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (align, label))| {
                        Button::new(("paragraph-align", i))
                            .small()
                            .outline()
                            .label(label)
                            .selected(self.align == align)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.align = align;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .children(self.fields.iter().enumerate().map(|(i, (label, input))| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.)).child(field_label(label)))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(input).id(("paragraph-value", i))),
                    )
            }))
            .child(t!("editor.design_paragraph_ui.note"))
    }
}
impl EditorView {
    pub(super) fn show_paragraph_format(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.paragraph_edit_range();
        self.close_text_field(cx);
        let Some((id, spec)) = self.text_target() else {
            return;
        };
        let range = range.unwrap_or(0..spec.text.len());
        let start = spec.text[..range.start].rfind('\n').map_or(0, |i| i + 1);
        let format = spec
            .paragraphs
            .iter()
            .find(|p| p.start == start)
            .map(|p| p.format)
            .unwrap_or(ParagraphFormat {
                indent: spec.size * 1.5,
                hanging: spec.size * 1.2,
                ..Default::default()
            });
        let values = [
            ("Level", format.level.to_string()),
            ("Left indent", format.indent.to_string()),
            ("Hanging marker", format.hanging.to_string()),
            ("Space before", format.space_before.to_string()),
            ("Space after", format.space_after.to_string()),
            (
                "Restart numbering",
                format.restart.map(|n| n.to_string()).unwrap_or_default(),
            ),
        ];
        let form = cx.new(|cx| Form {
            list: format.list,
            align: format.align,
            fields: values
                .into_iter()
                .map(|(k, v)| (k, cx.new(|cx| InputState::new(window, cx).default_value(v))))
                .collect(),
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let form = form.clone();
            let owner = owner.clone();
            let original = spec.clone();
            let range = range.clone();
            dialog
                .title(t!("editor.design_paragraph_ui.title"))
                .width(px(580.))
                .child(form.clone())
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_paragraph_ui.apply"
                )))
                .on_ok(move |_, _, cx| {
                    let format = form.read(cx).values(cx);
                    owner
                        .update(cx, |this, cx| {
                            let result =
                                (|| {
                                    let format = format?;
                                    let Some(NodeKind::Text { spec, .. }) =
                                        this.editor.doc.node(id).map(|n| &n.kind)
                                    else {
                                        return Err(t!("editor.design_paragraph_ui.text_missing")
                                            .into_owned());
                                    };
                                    if **spec != *original {
                                        return Err(t!("editor.design_paragraph_ui.text_changed")
                                            .into_owned());
                                    }
                                    let next = emulsion_core::text::apply_paragraphs(
                                        spec,
                                        range.clone(),
                                        format,
                                    )?;
                                    this.editor
                                        .execute(Command::SetText {
                                            id,
                                            spec: Box::new(next),
                                        })
                                        .map_err(|e| e.to_string())?;
                                    Ok::<_, String>(())
                                })();
                            match result {
                                Ok(()) => {
                                    this.type_tool.selection = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui::TestAppContext;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn design_paragraph_dialog_applies_native_list_and_undo(cx: &mut TestAppContext) {
        let mut e = emulsion_core::Editor::new(Document::new(600, 500), None);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Paragraphs",
                    emulsion_core::text::TextSpec {
                        text: "First\nSecond".into(),
                        size: 20.,
                        width: Some(180.),
                        ..Default::default()
                    },
                    600,
                    500,
                )),
                slot: emulsion_core::command::Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let original = e.doc.clone();
        let (ws, cx) = crate::tests::open(cx, e.doc);
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.set_layer_selection(vec![id], Some(id));
                this.show_paragraph_format(window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.click(("paragraph-list", 2usize), cx);
            window.click("ok", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, _| {
                let NodeKind::Text { spec, .. } = &this.editor.doc.node(id).unwrap().kind else {
                    panic!()
                };
                assert_eq!(spec.text, "1. First\n2. Second");
                assert_eq!(spec.paragraphs.len(), 2);
                this.editor.undo();
                assert_eq!(this.editor.doc, original);
            })
        });
    }
}
