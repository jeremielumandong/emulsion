//! Native typed-variable authoring and selection binding.
use super::*;
use emulsion_core::design_variables::{self as variables, Property, Value};
use gpui_kit::component::{
    Disableable, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

fn display(value: &Value) -> String {
    match value {
        Value::Number(v) => v.to_string(),
        Value::Color(c) => format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3]),
    }
}
fn parse(value: &str, color: bool) -> Result<Value, String> {
    if color {
        let s = value.trim().trim_start_matches('#');
        if !matches!(s.len(), 6 | 8) || !s.is_ascii() {
            return Err(t!("editor.design_variables_ui.enter_color").into_owned());
        }
        let mut c = [255; 4];
        for i in 0..s.len() / 2 {
            c[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| t!("editor.design_variables_ui.enter_hex").into_owned())?;
        }
        Ok(Value::Color(c))
    } else {
        let n = value
            .trim()
            .parse::<f64>()
            .map_err(|_| t!("editor.design_variables_ui.enter_number").into_owned())?;
        if !n.is_finite() || n.abs() > 1e9 {
            return Err(t!("editor.design_variables_ui.enter_finite").into_owned());
        }
        Ok(Value::Number(n))
    }
}
/// Localized display name for a bindable property.
fn property_label(property: Property) -> std::borrow::Cow<'static, str> {
    match property {
        Property::Fill => t!("editor.design_variables_ui.prop_fill"),
        Property::Stroke => t!("editor.design_variables_ui.prop_stroke"),
        Property::TextColor => t!("editor.design_variables_ui.prop_text_color"),
        Property::Opacity => t!("editor.design_variables_ui.prop_opacity"),
        Property::FontSize => t!("editor.design_variables_ui.prop_font_size"),
        Property::StrokeWidth => t!("editor.design_variables_ui.prop_stroke_width"),
        Property::FrameGap => t!("editor.design_variables_ui.prop_frame_gap"),
        Property::FramePadding => t!("editor.design_variables_ui.prop_frame_padding"),
    }
}
impl EditorView {
    pub(super) fn design_variable_controls(
        &self,
        query: &str,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selected = self.selected_layer_roots();
        let mut panel = div()
            .id("design-variable-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .child(t!("editor.design_variables_ui.title"))
            .child(
                Button::new("project-variable-import")
                    .label(t!("editor.design_variables_ui.import_from_page"))
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.project_variable_import_dialog(window, cx)
                    })),
            )
            .child(
                Button::new("design-variable-new")
                    .label(t!("editor.design_variables_ui.new_variable"))
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_variable_dialog(None, window, cx)
                    })),
            );
        for (index, (name, value)) in self
            .editor
            .doc
            .design
            .variables
            .iter()
            .filter(|(name, _)| name.to_lowercase().contains(&query.to_lowercase()))
            .enumerate()
        {
            let edit = name.clone();
            let bind = name.clone();
            let remove = name.clone();
            let count = self
                .editor
                .doc
                .design
                .variable_bindings
                .values()
                .filter(|b| b.values().any(|v| v == name))
                .count();
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(t!(
                        "editor.design_variables_ui.summary",
                        name = name,
                        value = display(value),
                        count = count
                    ))
                    .child(self.design_variable_library_buttons(name, index, cx))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                Button::new(("design-variable-edit", index))
                                    .label(t!("menu.edit"))
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.design_variable_dialog(Some(edit.clone()), window, cx)
                                    })),
                            )
                            .child(
                                Button::new(("design-variable-bind", index))
                                    .label(t!("editor.design_variables_ui.bind"))
                                    .small()
                                    .ghost()
                                    .disabled(selected.is_empty())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.design_variable_bind_dialog(bind.clone(), window, cx)
                                    })),
                            )
                            .child(
                                Button::new(("design-variable-remove", index))
                                    .label(t!("editor.design_variables_ui.remove"))
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.prepare_page_action(cx) {
                                            return;
                                        }
                                        let result = variables::remove(&mut this.editor, &remove);
                                        this.variable_result(
                                            result,
                                            &t!("editor.design_variables_ui.removed"),
                                            cx,
                                        );
                                    })),
                            ),
                    ),
            )
        }
        if let Some(id) = self.selected
            && let Some(bindings) = self.editor.doc.design.variable_bindings.get(&id)
        {
            for (index, (property, name)) in bindings.iter().enumerate() {
                let property = *property;
                panel = panel.child(
                    Button::new(("design-variable-unlink", index))
                        .label(t!(
                            "editor.design_variables_ui.unlink",
                            property = property_label(property),
                            name = name
                        ))
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.prepare_page_action(cx) {
                                return;
                            }
                            let result = variables::bind(&mut this.editor, &[id], property, None);
                            this.variable_result(
                                result,
                                &t!("editor.design_variables_ui.unbound"),
                                cx,
                            );
                        })),
                );
            }
        }
        panel
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(t!("editor.design_variables_ui.bound_note")),
            )
            .into_any_element()
    }
    fn variable_result(
        &mut self,
        result: Result<(), String>,
        success: &str,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                self.after_change(cx);
                self.set_status(success, false, cx);
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub(crate) fn design_variable_dialog(
        &mut self,
        old: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let existing = old
            .as_ref()
            .and_then(|n| self.editor.doc.design.variables.get(n))
            .cloned()
            .unwrap_or(Value::Color([80, 110, 235, 255]));
        let name =
            cx.new(|cx| InputState::new(window, cx).default_value(old.clone().unwrap_or_default()));
        let value = cx.new(|cx| InputState::new(window, cx).default_value(display(&existing)));
        let color = cx.new(|_| matches!(existing, Value::Color(_)));
        let error = cx.new(|_| String::new());
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let name = name.clone();
            let value = value.clone();
            let old = old.clone();
            let owner = owner.clone();
            let error_apply = error.clone();
            let color_apply = color.clone();
            let change = color.clone();
            let change_value = value.clone();
            dialog
                .title(if old.is_some() {
                    t!("editor.design_variables_ui.edit_title")
                } else {
                    t!("editor.design_variables_ui.new_title")
                })
                .width(px(440.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(t!("home.name"))
                        .child(Input::new(&name).id("design-variable-name"))
                        .child(
                            Button::new("design-variable-type")
                                .label(if *color.read(cx) {
                                    t!("editor.design_variables_ui.type_color")
                                } else {
                                    t!("editor.design_variables_ui.type_number")
                                })
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    change.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                    change_value.update(cx, |v, cx| {
                                        v.set_value(
                                            if *change.read(cx) { "#506EEBFF" } else { "16" },
                                            window,
                                            cx,
                                        )
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(if *color.read(cx) {
                            t!("editor.design_variables_ui.color_hint")
                        } else {
                            t!("editor.design_variables_ui.number")
                        })
                        .child(Input::new(&value).id("design-variable-value")),
                )
                .footer(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(!error.read(cx).is_empty(), |d| {
                            d.child(
                                div()
                                    .id("design-variable-error")
                                    .test_support()
                                    .child(error.read(cx).clone()),
                            )
                        })
                        .child(crate::widgets::form_dialog_footer(t!(
                            "editor.design_variables_ui.save"
                        ))),
                )
                .on_ok(move |_, _, cx| {
                    let next = parse(value.read(cx).value().as_ref(), *color_apply.read(cx));
                    let name = name.read(cx).value().trim().to_string();
                    let result = owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                return Err(t!("editor.design_variables_ui.page_changed_dialog")
                                    .into_owned());
                            }
                            variables::put(&mut this.editor, old.as_deref(), &name, next?)?;
                            this.after_change(cx);
                            Ok(())
                        })
                        .unwrap_or_else(|_| {
                            Err(t!("editor.design_variable_library_ui.editor_closed").into_owned())
                        });
                    match result {
                        Ok(()) => true,
                        Err(e) => {
                            error_apply.update(cx, |v, cx| {
                                *v = e;
                                cx.notify();
                            });
                            false
                        }
                    }
                })
        });
    }
    pub(crate) fn design_variable_bind_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        let Some(value) = self.editor.doc.design.variables.get(&name) else {
            return;
        };
        let color = matches!(value, Value::Color(_));
        let properties: Vec<_> = Property::ALL
            .into_iter()
            .filter(|p| {
                p.is_color() == color && ids.iter().all(|id| p.supports(&self.editor.doc, *id))
            })
            .collect();
        if ids.is_empty() || properties.is_empty() {
            self.set_status(t!("editor.design_variables_ui.no_properties"), true, cx);
            return;
        }
        let property = cx.new(|_| properties[0]);
        let error = cx.new(|_| String::new());
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, cx| {
            let owner = owner.clone();
            let ids = ids.clone();
            let name = name.clone();
            let selected = property.clone();
            let error_apply = error.clone();
            dialog
                .title(t!("editor.design_variables_ui.bind_title", name = name))
                .width(px(420.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(properties.iter().enumerate().map(|(i, p)| {
                            let p = *p;
                            let state = property.clone();
                            Button::new(("design-variable-property", i))
                                .label(property_label(p))
                                .small()
                                .outline()
                                .selected(*property.read(cx) == p)
                                .on_click(move |_, window, cx| {
                                    state.update(cx, |v, cx| {
                                        *v = p;
                                        cx.notify();
                                    });
                                    window.refresh();
                                })
                        })),
                )
                .footer(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(!error.read(cx).is_empty(), |d| {
                            d.child(error.read(cx).clone())
                        })
                        .child(crate::widgets::form_dialog_footer(t!(
                            "editor.design_variables_ui.bind_selection"
                        ))),
                )
                .on_ok(move |_, _, cx| {
                    let p = *selected.read(cx);
                    let result = owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                return Err(t!("editor.design_variables_ui.page_changed_bindings")
                                    .into_owned());
                            }
                            variables::bind(&mut this.editor, &ids, p, Some(&name))?;
                            this.after_change(cx);
                            Ok(())
                        })
                        .unwrap_or_else(|_| {
                            Err(t!("editor.design_variable_library_ui.editor_closed").into_owned())
                        });
                    match result {
                        Ok(()) => true,
                        Err(e) => {
                            error_apply.update(cx, |v, cx| {
                                *v = e;
                                cx.notify();
                            });
                            false
                        }
                    }
                })
        });
    }
}
