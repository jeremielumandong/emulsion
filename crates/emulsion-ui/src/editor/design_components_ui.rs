//! Local component libraries and explicit linked-instance publishing.
use super::*;
use emulsion_core::design_components as components;
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(super) fn design_component_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let linked = self
            .selected
            .and_then(|id| components::owner(&self.editor.doc, id))
            .and_then(|id| self.editor.doc.design.component_links.get(&id));
        let variants = linked
            .and_then(|link| self.editor.doc.design.components.get(&link.component))
            .map(|d| d.variants.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        div()
            .id("design-component-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("editor.design_components_ui.heading")),
            )
            .child(
                Button::new("design-component-create")
                    .label(t!("editor.design_components_ui.create"))
                    .small()
                    .outline()
                    .disabled(self.selected_layer_roots().is_empty())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_component_name(false, window, cx)
                    })),
            )
            .child(
                Button::new("design-component-library")
                    .label(t!("editor.design_components_ui.browse"))
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.design_component_library(window, cx)
                        }),
                    ),
            )
            .when_some(linked, |d, link| {
                d.child(div().text_size(px(11.)).child(t!(
                    "editor.design_components_ui.family",
                    component = link.component,
                    variant = link.variant
                )))
                .child(
                    Button::new("design-component-overrides")
                        .label(t!("editor.design_components_ui.preserve_menu"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_component_overrides(window, cx)
                        })),
                )
                .child(
                    Button::new("design-component-update")
                        .label(t!("editor.design_components_ui.update"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.design_component_action("update", None, cx)
                        })),
                )
                .child(
                    Button::new("design-component-publish-project")
                        .label(t!("editor.design_components_ui.publish"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.design_component_action("publish_project", None, cx)
                        })),
                )
                .child(
                    Button::new("design-component-reset")
                        .label(t!("editor.design_components_ui.reset"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.design_component_action("reset", None, cx)
                        })),
                )
                .child(
                    Button::new("design-component-variant")
                        .label(t!("editor.design_components_ui.save_variant_menu"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_component_name(true, window, cx)
                        })),
                )
                .children(variants.into_iter().enumerate().map(|(index, variant)| {
                    Button::new(("design-component-switch", index))
                        .label(t!("editor.design_components_ui.switch_to", name = variant))
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.design_component_action("reset", Some(&variant), cx)
                        }))
                }))
                .child(
                    Button::new("design-component-detach")
                        .label(t!("editor.design_components_ui.detach"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.design_component_action("detach", None, cx)
                        })),
                )
            })
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(t!("editor.design_components_ui.note")),
            )
            .into_any_element()
    }
    fn design_component_action(
        &mut self,
        action: &str,
        variant: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self
            .selected
            .and_then(|id| components::owner(&self.editor.doc, id))
        else {
            return;
        };
        let result = match action {
            "publish_project" => components::publish_project(&mut self.editor, id).map(|_| ()),
            "update" => components::update(&mut self.editor, id, None),
            "detach" => components::detach(&mut self.editor, id),
            _ => components::reset(&mut self.editor, id, variant),
        };
        match result {
            Ok(()) => {
                self.after_change(cx);
                self.set_status(t!("editor.design_components_ui.updated"), false, cx);
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub(crate) fn design_component_overrides(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(node) = self.selected else {
            return;
        };
        let Some(instance) = components::owner(&self.editor.doc, node) else {
            return;
        };
        let Some(target) = self.editor.doc.node(node) else {
            return;
        };
        let content = matches!(
            target.kind,
            NodeKind::Text { .. } | NodeKind::Raster { .. } | NodeKind::Smart { .. }
        );
        let geometry = matches!(
            target.kind,
            NodeKind::Text { .. }
                | NodeKind::Path { .. }
                | NodeKind::Raster { .. }
                | NodeKind::Smart { .. }
        );
        let text = matches!(target.kind, NodeKind::Text { .. });
        let path = matches!(target.kind, NodeKind::Path { .. });
        let fill = path || matches!(target.kind, NodeKind::Fill { .. });
        let state = cx.new(|_| components::overrides_for(&self.editor.doc, instance, node));
        let tracking = cx.new(|_| self.editor.doc.design.component_links[&instance].auto_overrides);
        let error = cx.new(|_| String::new());
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let current = *state.read(cx);
            let apply = state.clone();
            let tracking_apply = tracking.clone();
            let tracking_click = tracking.clone();
            let automatic = *tracking.read(cx);
            let owner = owner.clone();
            let error_apply = error.clone();
            dialog
                .title(t!("editor.design_components_ui.preserve_title"))
                .width(px(460.))
                .child(
                    div()
                        .id("design-component-override-body")
                        .test_support()
                        .max_h(px(
                            (f32::from(window.viewport_size().height) - 220.).clamp(100., 500.)
                        ))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(t!("editor.design_components_ui.preserve_body"))
                        .child(
                            Button::new("design-component-auto-overrides")
                                .label(format!(
                                    "{} {}",
                                    if automatic { "✓" } else { "○" },
                                    t!("editor.design_components_ui.auto_preserve")
                                ))
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    tracking_click.update(cx, |value, cx| {
                                        *value = !*value;
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .children(
                            [
                                (
                                    0usize,
                                    t!("editor.design_components_ui.ov_content"),
                                    current.content,
                                    content,
                                ),
                                (
                                    1,
                                    t!("editor.design_components_ui.ov_appearance"),
                                    current.appearance,
                                    true,
                                ),
                                (
                                    2,
                                    t!("editor.design_components_ui.ov_geometry"),
                                    current.geometry,
                                    geometry,
                                ),
                                (
                                    3,
                                    t!("editor.design_components_ui.ov_opacity"),
                                    current.opacity,
                                    true,
                                ),
                                (
                                    4,
                                    t!("editor.design_components_ui.ov_visibility"),
                                    current.visibility,
                                    true,
                                ),
                                (
                                    5,
                                    t!("editor.design_components_ui.ov_fill"),
                                    current.fill,
                                    fill,
                                ),
                                (
                                    6,
                                    t!("editor.design_components_ui.ov_stroke"),
                                    current.stroke,
                                    path,
                                ),
                                (
                                    7,
                                    t!("editor.design_components_ui.ov_stroke_width"),
                                    current.stroke_width,
                                    path,
                                ),
                                (
                                    8,
                                    t!("editor.design_components_ui.ov_font_family"),
                                    current.font_family,
                                    text,
                                ),
                                (
                                    9,
                                    t!("editor.design_components_ui.ov_font_size"),
                                    current.font_size,
                                    text,
                                ),
                                (
                                    10,
                                    t!("editor.design_components_ui.ov_text_color"),
                                    current.text_color,
                                    text,
                                ),
                                (
                                    11,
                                    t!("editor.design_components_ui.ov_position"),
                                    current.position,
                                    geometry,
                                ),
                                (
                                    12,
                                    t!("editor.design_components_ui.ov_size"),
                                    current.size,
                                    geometry,
                                ),
                                (
                                    13,
                                    t!("editor.design_components_ui.ov_effects"),
                                    current.effects,
                                    true,
                                ),
                            ]
                            .into_iter()
                            .map(|(i, label, checked, enabled)| {
                                let state = state.clone();
                                Button::new(("design-component-override", i))
                                    .label(format!("{} {label}", if checked { "✓" } else { "○" }))
                                    .small()
                                    .outline()
                                    .disabled(!enabled)
                                    .on_click(move |_, window, cx| {
                                        state.update(cx, |state, cx| {
                                            let value = match i {
                                                0 => &mut state.content,
                                                1 => &mut state.appearance,
                                                2 => &mut state.geometry,
                                                3 => &mut state.opacity,
                                                4 => &mut state.visibility,
                                                5 => &mut state.fill,
                                                6 => &mut state.stroke,
                                                7 => &mut state.stroke_width,
                                                8 => &mut state.font_family,
                                                9 => &mut state.font_size,
                                                10 => &mut state.text_color,
                                                11 => &mut state.position,
                                                12 => &mut state.size,
                                                _ => &mut state.effects,
                                            };
                                            *value = !*value;
                                            cx.notify();
                                        });
                                        window.refresh();
                                    })
                            }),
                        )
                        .child(t!("editor.design_components_ui.preserve_note")),
                )
                .footer(
                    div()
                        .id("design-component-override-footer")
                        .test_support()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(!error.read(cx).is_empty(), |d| {
                            d.child(error.read(cx).clone())
                        })
                        .child(crate::widgets::form_dialog_footer(t!(
                            "editor.design_components_ui.save_overrides"
                        ))),
                )
                .on_ok(move |_, window, cx| {
                    let flags = *apply.read(cx);
                    let result = owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                return Err(t!(
                                    "editor.design_components_ui.page_changed_overrides"
                                )
                                .into_owned());
                            }
                            components::configure_overrides(
                                &mut this.editor,
                                instance,
                                node,
                                flags,
                                Some(*tracking_apply.read(cx)),
                            )?;
                            this.after_change(cx);
                            Ok::<_, String>(())
                        })
                        .unwrap_or_else(|_| {
                            Err(t!("editor.design_controls.editor_closed").into_owned())
                        });
                    match result {
                        Ok(()) => true,
                        Err(message) => {
                            error_apply.update(cx, |error, cx| {
                                *error = message;
                                cx.notify();
                            });
                            window.refresh();
                            false
                        }
                    }
                })
        });
    }
    pub(crate) fn design_component_name(
        &mut self,
        variant: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        let selected = self
            .selected
            .and_then(|id| components::owner(&self.editor.doc, id));
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        let input = cx.new(|cx| {
            InputState::new(window, cx).default_value(if variant { "Variant" } else { "Component" })
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let owner = owner.clone();
            let ids = ids.clone();
            dialog
                .title(if variant {
                    t!("editor.design_components_ui.save_variant_title")
                } else {
                    t!("editor.design_components_ui.create_title")
                })
                .width(px(400.))
                .child(Input::new(&input).id("design-component-name"))
                .footer(crate::widgets::form_dialog_footer(t!("file.save")))
                .on_ok(move |_, _, cx| {
                    let name = input.read(cx).value().to_string();
                    owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                this.set_status(
                                    t!("editor.design_components_ui.page_changed_dialog"),
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            let result = if variant {
                                selected
                                    .ok_or_else(|| {
                                        t!("editor.design_components_ui.select_linked").into_owned()
                                    })
                                    .and_then(|id| {
                                        components::update(&mut this.editor, id, Some(&name))
                                            .map(|()| id)
                                    })
                            } else {
                                components::create(&mut this.editor, &ids, &name)
                            };
                            match result {
                                Ok(id) => {
                                    this.set_layer_selection(vec![id], Some(id));
                                    this.after_change(cx);
                                    this.set_tool(Tool::Move, cx);
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
    pub(crate) fn design_component_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let active = self.editor.active_page();
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        let pages = self
            .editor
            .snapshot()
            .map(|project| project.pages)
            .unwrap_or_default();
        let entries: Vec<_> = pages
            .iter()
            .flat_map(|page| {
                page.doc
                    .design
                    .components
                    .iter()
                    .flat_map(|(name, definition)| {
                        definition.variants.keys().map(|variant| {
                            (
                                page.meta.id,
                                page.meta.name.clone(),
                                name.clone(),
                                variant.clone(),
                            )
                        })
                    })
            })
            .collect();

        window.open_dialog(cx,move|dialog,_,_|{
            dialog.title(t!("editor.design_components_ui.library_title")).width(px(500.)).child(div().id("design-component-library-list").test_support().max_h(px(460.)).overflow_y_scroll().flex().flex_col().gap_2()
                .child(t!("editor.design_components_ui.library_body"))
                .when(entries.is_empty(),|d|d.child(t!("editor.design_components_ui.library_empty")))
                .children(entries.iter().enumerate().map(|(index,(page,page_name,name,variant))|{
                    let owner=owner.clone();let page=*page;let name=name.clone();let variant=variant.clone();
                    Button::new(("design-component-insert",index)).label(format!("{page_name} / {name} / {variant}")).small().outline().on_click(move|_,window,cx|{
                        let success=owner.update(cx,|this,cx|{
                            if this.edit_ticket()!=ticket{this.set_status(t!("editor.design_components_ui.page_changed_library"),true,cx);return false;}
                            let result=if page==active{components::insert(&mut this.editor,&name,&variant,(24.,24.))}else{components::insert_project(&mut this.editor,page,&name,&variant,(24.,24.))};
                            match result{Ok(id)=>{this.set_layer_selection(vec![id],Some(id));this.after_change(cx);this.set_tool(Tool::Move,cx);true},Err(e)=>{this.after_change(cx);this.set_status(e,true,cx);false}}
                        }).unwrap_or(false);if success{window.close_dialog(cx);}
                    })
                })))
        });
    }
}
