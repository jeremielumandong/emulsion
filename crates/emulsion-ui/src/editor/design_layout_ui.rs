//! Responsive sizing stays native, validates atomically and uses normal Undo.
use super::*;
use emulsion_core::design_layout::{self as layout, Align, Flow, Frame};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

#[path = "design_breakpoints_ui.rs"]
mod breakpoints;

fn optional_dimension(value: &str) -> Result<Option<f64>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let number: f64 = value
        .parse()
        .map_err(|_| t!("editor.design_layout_ui.limit_error"))?;
    if !number.is_finite() {
        return Err(t!("editor.design_layout_ui.finite_error").into());
    }
    Ok(Some(number))
}

impl EditorView {
    fn layout_targets_editable(&mut self, ids: &[NodeId], cx: &mut Context<Self>) -> bool {
        if ids.iter().any(|id| {
            let locks = self.editor.doc.layer_locks(*id);
            self.editor.doc.node(*id).is_none()
                || self.editor.doc.locked_ancestor(*id).is_some()
                || locks.position
                || locks.pixels
                || locks.transparency
        }) {
            self.set_status(t!("editor.design_layout_ui.unlock_first"), true, cx);
            false
        } else {
            true
        }
    }
    pub(super) fn design_layout_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let frame = self
            .selected
            .and_then(|id| self.editor.doc.design.frames.get(&id));
        let has_selection = !self.selected_layer_roots().is_empty();
        let child = self.selected.and_then(|id| {
            let parent = self.editor.doc.node(id)?.parent?;
            let frame = self.editor.doc.design.frames.get(&parent)?;
            (frame.boundary != id).then_some((
                parent,
                id,
                frame.children.get(&id).copied().unwrap_or_default(),
            ))
        });
        div()
            .id("design-layout-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("editor.design_layout_ui.title")),
            )
            .child(self.responsive_preview_controls(cx))
            .child(
                div().grid().grid_cols(3).gap(px(4.)).children(
                    [
                        (Flow::Row, t!("editor.design_layout_ui.flow_row")),
                        (Flow::Column, t!("editor.design_layout_ui.flow_column")),
                        (Flow::Grid, t!("editor.design_layout_ui.flow_grid")),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (flow, label))| {
                        Button::new(("design-layout-flow", i))
                            .label(label)
                            .small()
                            .outline()
                            .disabled(!has_selection)
                            .when(frame.is_some_and(|f| f.flow == flow), |b| {
                                b.bg(p.soft_bg).text_color(p.accent)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.design_layout_dialog(flow, window, cx)
                            }))
                    }),
                ),
            )
            .when(frame.is_some(), |d| {
                d.child(
                    Button::new("design-layout-breakpoints")
                        .label(t!("editor.design_layout_ui.breakpoints"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_breakpoints_dialog(window, cx)
                        })),
                )
                .child(
                    Button::new("design-layout-remove")
                        .label(t!("editor.design_layout_ui.remove"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.prepare_page_action(cx) {
                                return;
                            }
                            let Some(id) = this.selected else {
                                return;
                            };
                            if !this.layout_targets_editable(&[id], cx) {
                                return;
                            }
                            let mut design = this.editor.doc.design.clone();
                            design.frames.remove(&id);
                            this.execute(
                                Command::SetDesign {
                                    design: Box::new(design),
                                },
                                cx,
                            );
                        })),
                )
            })
            .when_some(child, |d, (parent, id, settings)| {
                d.child(div().text_size(px(11.)).text_color(p.muted).child(t!(
                    "editor.design_layout_ui.child_summary",
                    position = if settings.absolute {
                        t!("editor.design_layout_ui.absolute")
                    } else {
                        t!("editor.design_layout_ui.in_layout")
                    },
                    width = if settings.fill_width {
                        t!("editor.design_layout_ui.fill")
                    } else {
                        t!("editor.design_layout_ui.fixed")
                    },
                    height = if settings.fill_height {
                        t!("editor.design_layout_ui.fill")
                    } else {
                        t!("editor.design_layout_ui.fixed")
                    }
                )))
                .child(
                    Button::new("design-layout-child-sizing")
                        .label(t!("editor.design_layout_ui.object_sizing"))
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.design_layout_child_dialog(parent, id, window, cx)
                        })),
                )
                .children(
                    self.editor
                        .doc
                        .design
                        .frames
                        .get(&parent)
                        .into_iter()
                        .flat_map(|f| f.breakpoints.iter())
                        .enumerate()
                        .map(|(index, entry)| {
                            Button::new(("design-layout-child-breakpoint", index))
                                .label(t!(
                                    "editor.design_layout_ui.object_sizing_at",
                                    width = entry.min_width
                                ))
                                .small()
                                .outline()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.design_layout_child_at_dialog(
                                        parent,
                                        id,
                                        Some(index),
                                        window,
                                        cx,
                                    )
                                }))
                        }),
                )
            })
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(t!("editor.design_layout_ui.hint")),
            )
            .into_any_element()
    }

    pub(crate) fn design_layout_dialog(
        &mut self,
        flow: Flow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() || !self.layout_targets_editable(&ids, cx) {
            return;
        }
        let group = (ids.len() == 1 && self.editor.doc.node(ids[0]).is_some_and(|n| n.is_group()))
            .then_some(ids[0]);
        let settings = group
            .and_then(|id| self.editor.doc.design.frames.get(&id))
            .cloned()
            .unwrap_or_default();
        let bounds = group.and_then(|id| layout::bounds(&self.editor.doc, id));
        let fields = [
            Some(bounds.map_or(self.editor.doc.width as f64 * 0.8, |b| b.2)),
            Some(bounds.map_or(self.editor.doc.height as f64 * 0.8, |b| b.3)),
            Some(settings.gap),
            Some(settings.padding[0]),
            Some(settings.padding[1]),
            Some(settings.padding[2]),
            Some(settings.padding[3]),
            Some(settings.columns as f64),
            settings.min_width,
            settings.max_width,
            settings.min_height,
            settings.max_height,
        ]
        .map(|v| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(v.map(|v| v.to_string()).unwrap_or_default())
            })
        });
        let original_fill =
            !settings.children.is_empty() && settings.children.values().all(|c| c.fill_width);
        let fill = cx.new(|_| original_fill);
        let wrap = cx.new(|_| settings.wrap);
        let clip = cx.new(|_| settings.clip_content);
        let hug_width = cx.new(|_| settings.hug_width);
        let hug_height = cx.new(|_| settings.hug_height);
        let alignment = cx.new(|_| settings.align);
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx, move |dialog, window, cx| {
            let fields = fields.clone();
            let inputs = fields.clone();
            let owner = owner.clone();
            let ids = ids.clone();
            let settings = settings.clone();
            let fill_state = fill.clone();
            let wrap_state = wrap.clone();
            let align_state = alignment.clone();
            let fill_apply = fill.clone();
            let wrap_apply = wrap.clone();
            let align_apply = alignment.clone();
            let width_state = hug_width.clone();
            let width_apply = hug_width.clone();
            let height_state = hug_height.clone();
            let height_apply = hug_height.clone();
            let error_apply = error.clone();
            let clip_state = clip.clone();
            let clip_apply = clip.clone();
            dialog
                .title(t!("editor.design_layout_ui.title"))
                .width(px(480.))
                .child(
                    div()
                        .id("design-layout-dialog-body")
                        .test_support()
                        .max_h(px(
                            (f32::from(window.viewport_size().height) - 220.).clamp(100., 680.)
                        ))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div().grid().grid_cols(2).gap_2().children(
                                [
                                    t!("editor.design_layout_ui.frame_width"),
                                    t!("editor.design_layout_ui.frame_height"),
                                    t!("editor.design_layout_ui.gap"),
                                    t!("editor.design_layout_ui.padding_top"),
                                    t!("editor.design_layout_ui.padding_right"),
                                    t!("editor.design_layout_ui.padding_bottom"),
                                    t!("editor.design_layout_ui.padding_left"),
                                    t!("editor.design_layout_ui.grid_columns"),
                                    t!("editor.design_layout_ui.min_width"),
                                    t!("editor.design_layout_ui.max_width"),
                                    t!("editor.design_layout_ui.min_height"),
                                    t!("editor.design_layout_ui.max_height"),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(i, label)| {
                                    div().child(label).child(
                                        Input::new(&fields[i]).id(("design-layout-input", i)),
                                    )
                                }),
                            ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .child(t!("editor.design_layout_ui.limit_help")),
                        )
                        .child(
                            Button::new("design-layout-fill")
                                .label(if *fill.read(cx) {
                                    t!("editor.design_layout_ui.fill_children")
                                } else {
                                    t!("editor.design_layout_ui.keep_child_widths")
                                })
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    fill_state.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(
                            Button::new("design-layout-clip")
                                .label(if *clip.read(cx) {
                                    t!("editor.design_layout_ui.clip")
                                } else {
                                    t!("editor.design_layout_ui.no_clip")
                                })
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    clip_state.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(
                            Button::new("design-layout-wrap")
                                .label(if *wrap.read(cx) {
                                    t!("editor.design_layout_ui.wrap")
                                } else {
                                    t!("editor.design_layout_ui.no_wrap")
                                })
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    wrap_state.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(
                            Button::new("design-layout-hug-width")
                                .label(if *hug_width.read(cx) {
                                    t!("editor.design_layout_ui.width_fit")
                                } else {
                                    t!("editor.design_layout_ui.width_fixed")
                                })
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    width_state.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(
                            Button::new("design-layout-hug-height")
                                .label(if *hug_height.read(cx) {
                                    t!("editor.design_layout_ui.height_fit")
                                } else {
                                    t!("editor.design_layout_ui.height_fixed")
                                })
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    height_state.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(
                            Button::new("design-layout-align")
                                .label(t!(
                                    "editor.design_layout_ui.alignment",
                                    align = match *alignment.read(cx) {
                                        Align::Start => t!("editor.design_layout_ui.align_start"),
                                        Align::Center => t!("editor.design_layout_ui.align_center"),
                                        Align::End => t!("editor.design_layout_ui.align_end"),
                                    }
                                ))
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    align_state.update(cx, |v, cx| {
                                        *v = match *v {
                                            Align::Start => Align::Center,
                                            Align::Center => Align::End,
                                            Align::End => Align::Start,
                                        };
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .child(t!("editor.design_layout_ui.axis_help")),
                        ),
                )
                .footer(
                    div()
                        .id("design-layout-footer")
                        .test_support()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(!error.read(cx).is_empty(), |d| {
                            d.child(
                                div()
                                    .id("design-layout-error")
                                    .test_support()
                                    .text_size(px(12.))
                                    .child(error.read(cx).clone()),
                            )
                        })
                        .child(crate::widgets::form_dialog_footer(t!(
                            "editor.design_layout_ui.apply_layout"
                        ))),
                )
                .on_ok(move |_, window, cx| {
                    let parsed = inputs
                        .each_ref()
                        .map(|i| optional_dimension(i.read(cx).value().as_ref()));
                    let fill = *fill_apply.read(cx);
                    let wrap = *wrap_apply.read(cx);
                    let align = *align_apply.read(cx);
                    let clip_content = *clip_apply.read(cx);
                    let hug_width = *width_apply.read(cx);
                    let hug_height = *height_apply.read(cx);
                    let accepted = owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                this.set_status(
                                    t!("editor.design_layout_ui.page_changed_layout"),
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            if !this.layout_targets_editable(&ids, cx) {
                                return false;
                            }
                            let values = match parsed.into_iter().collect::<Result<Vec<_>, _>>() {
                                Ok(values) => values,
                                Err(error) => {
                                    this.set_status(error, true, cx);
                                    return false;
                                }
                            };
                            if values[..8].iter().any(Option::is_none)
                                || values[7].unwrap().fract() != 0.
                                || !(1. ..=64.).contains(&values[7].unwrap())
                            {
                                this.set_status(t!("editor.design_layout_ui.dims_error"), true, cx);
                                return false;
                            }
                            this.editor.begin("Responsive layout");
                            let result = (|| {
                                let group = match group {
                                    Some(id) => id,
                                    None => this
                                        .editor
                                        .execute(Command::Group {
                                            ids: ids.clone(),
                                            name: "Responsive frame".into(),
                                        })
                                        .map_err(|e| e.to_string())?
                                        .ok_or(t!("editor.design_layout_ui.no_group"))?,
                                };
                                let mut frame = Frame {
                                    flow,
                                    clip_content,
                                    gap: values[2].unwrap(),
                                    padding: [
                                        values[3].unwrap(),
                                        values[4].unwrap(),
                                        values[5].unwrap(),
                                        values[6].unwrap(),
                                    ],
                                    columns: values[7].unwrap() as u32,
                                    wrap,
                                    align,
                                    hug_width,
                                    hug_height,
                                    min_width: values[8],
                                    max_width: values[9],
                                    min_height: values[10],
                                    max_height: values[11],
                                    ..settings.clone()
                                };
                                if fill != original_fill {
                                    for id in this.editor.doc.children(Some(group)) {
                                        if id != frame.boundary {
                                            frame.children.entry(id).or_default().fill_width = fill;
                                        }
                                    }
                                }
                                layout::enable(
                                    &mut this.editor,
                                    group,
                                    frame,
                                    (values[0].unwrap(), values[1].unwrap()),
                                )?;
                                Ok::<_, String>(group)
                            })();
                            match result {
                                Ok(id) => {
                                    this.editor.end();
                                    this.set_layer_selection(vec![id], Some(id));
                                    this.after_change(cx);
                                    true
                                }
                                Err(error) => {
                                    this.editor.cancel();
                                    this.set_status(error, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false);
                    if !accepted {
                        let message = owner
                            .read_with(cx, |this, _| {
                                this.status.as_ref().map(|(text, _)| text.to_string())
                            })
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| t!("editor.design_layout_ui.doc_gone").into());
                        error_apply.update(cx, |error, cx| {
                            *error = message;
                            cx.notify();
                        });
                        window.refresh();
                    }
                    accepted
                })
        });
    }

    pub(crate) fn design_layout_child_dialog(
        &mut self,
        parent: NodeId,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.design_layout_child_at_dialog(parent, id, None, window, cx);
    }
    pub(crate) fn design_layout_child_at_dialog(
        &mut self,
        parent: NodeId,
        id: NodeId,
        breakpoint: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) || !self.layout_targets_editable(&[parent, id], cx) {
            return;
        }
        let Some(frame) = self.editor.doc.design.frames.get(&parent) else {
            return;
        };
        if frame.boundary == id || self.editor.doc.node(id).and_then(|n| n.parent) != Some(parent) {
            return;
        }
        let inherited = frame.children.get(&id).copied().unwrap_or_default();
        let settings = breakpoint
            .and_then(|i| frame.breakpoints.get(i))
            .and_then(|b| b.overrides.children.get(&id))
            .copied()
            .unwrap_or(inherited);
        let inherit = cx.new(|_| {
            breakpoint.is_some_and(|i| {
                frame
                    .breakpoints
                    .get(i)
                    .is_none_or(|b| !b.overrides.children.contains_key(&id))
            })
        });
        let fields = [
            settings.min_width,
            settings.max_width,
            settings.min_height,
            settings.max_height,
            settings.aspect_ratio,
        ]
        .map(|v| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(v.map(|v| v.to_string()).unwrap_or_default())
            })
        });
        let state = cx.new(|_| settings);
        let ratio = layout::item_dimensions(&self.editor.doc, id)
            .and_then(|(w, h)| (h > 0.).then_some(w / h));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx, move |dialog, window, cx| {
            let inputs = fields.clone();
            let owner = owner.clone();
            let apply = state.clone();
            let settings = *state.read(cx);
            let aspect = fields[4].clone();
            let buttons = [
                (
                    "design-layout-child-position",
                    if settings.absolute {
                        t!("editor.design_layout_ui.position_absolute")
                    } else {
                        t!("editor.design_layout_ui.position_in_layout")
                    },
                ),
                (
                    "design-layout-child-width",
                    if settings.fill_width {
                        t!("editor.design_layout_ui.width_fill")
                    } else {
                        t!("editor.design_layout_ui.width_fixed")
                    },
                ),
                (
                    "design-layout-child-height",
                    if settings.fill_height {
                        t!("editor.design_layout_ui.height_fill")
                    } else {
                        t!("editor.design_layout_ui.height_fixed")
                    },
                ),
            ];
            let error_apply = error.clone();
            let inherit_apply = inherit.clone();
            let change_inherit = inherit.clone();
            dialog
                .title(if breakpoint.is_some() {
                    t!("editor.design_layout_ui.breakpoint_sizing")
                } else {
                    t!("editor.design_layout_ui.object_sizing")
                })
                .width(px(440.))
                .child(
                    div()
                        .id("design-layout-child-dialog-body")
                        .test_support()
                        .max_h(px(
                            (f32::from(window.viewport_size().height) - 220.).clamp(100., 680.)
                        ))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(breakpoint.is_some(), |d| {
                            d.child(
                                Button::new("design-layout-child-inherit")
                                    .label(if *inherit.read(cx) {
                                        t!("editor.design_layout_ui.inherit")
                                    } else {
                                        t!("editor.design_layout_ui.override")
                                    })
                                    .small()
                                    .outline()
                                    .on_click(move |_, window, cx| {
                                        change_inherit.update(cx, |v, cx| {
                                            *v = !*v;
                                            cx.notify();
                                        });
                                        window.refresh();
                                    }),
                            )
                        })
                        .children(buttons.into_iter().enumerate().map(|(i, (key, label))| {
                            let state = state.clone();
                            Button::new(key).label(label).small().outline().on_click(
                                move |_, window, cx| {
                                    state.update(cx, |value, cx| {
                                        match i {
                                            0 => value.absolute = !value.absolute,
                                            1 => value.fill_width = !value.fill_width,
                                            _ => value.fill_height = !value.fill_height,
                                        };
                                        cx.notify();
                                    });
                                    window.refresh();
                                },
                            )
                        }))
                        .child(
                            div().grid().grid_cols(2).gap_2().children(
                                [
                                    t!("editor.design_layout_ui.min_width"),
                                    t!("editor.design_layout_ui.max_width"),
                                    t!("editor.design_layout_ui.min_height"),
                                    t!("editor.design_layout_ui.max_height"),
                                    t!("editor.design_layout_ui.aspect"),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(i, label)| {
                                    div().child(label).child(
                                        Input::new(&fields[i]).id(("design-layout-child-input", i)),
                                    )
                                }),
                            ),
                        )
                        .child(
                            Button::new("design-layout-child-aspect")
                                .label(if fields[4].read(cx).value().trim().is_empty() {
                                    t!("editor.design_layout_ui.keep_aspect")
                                } else {
                                    t!("editor.design_layout_ui.unlock_aspect")
                                })
                                .small()
                                .outline()
                                .disabled(ratio.is_none())
                                .on_click(move |_, window, cx| {
                                    let value = if aspect.read(cx).value().trim().is_empty() {
                                        ratio.map(|v| v.to_string()).unwrap_or_default()
                                    } else {
                                        String::new()
                                    };
                                    aspect
                                        .update(cx, |state, cx| state.set_value(value, window, cx));
                                    window.refresh();
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .child(t!("editor.design_layout_ui.child_help")),
                        ),
                )
                .footer(
                    div()
                        .id("design-layout-child-footer")
                        .test_support()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(!error.read(cx).is_empty(), |d| {
                            d.child(
                                div()
                                    .id("design-layout-child-error")
                                    .test_support()
                                    .text_size(px(12.))
                                    .child(error.read(cx).clone()),
                            )
                        })
                        .child(crate::widgets::form_dialog_footer(t!(
                            "editor.design_layout_ui.apply_sizing"
                        ))),
                )
                .on_ok(move |_, window, cx| {
                    let parsed = inputs
                        .each_ref()
                        .map(|i| optional_dimension(i.read(cx).value().as_ref()));
                    let mut settings = *apply.read(cx);
                    let accepted = owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                this.set_status(
                                    t!("editor.design_layout_ui.page_changed_sizing"),
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            if !this.layout_targets_editable(&[parent, id], cx) {
                                return false;
                            }
                            let values = match parsed.into_iter().collect::<Result<Vec<_>, _>>() {
                                Ok(v) => v,
                                Err(error) => {
                                    this.set_status(error, true, cx);
                                    return false;
                                }
                            };
                            settings.min_width = values[0];
                            settings.max_width = values[1];
                            settings.min_height = values[2];
                            settings.max_height = values[3];
                            settings.aspect_ratio = values[4];
                            let mut design = this.editor.doc.design.clone();
                            let Some(frame) = design.frames.get_mut(&parent) else {
                                return false;
                            };
                            if this.editor.doc.node(id).and_then(|n| n.parent) != Some(parent) {
                                return false;
                            }
                            if let Some(index) = breakpoint {
                                let Some(entry) = frame.breakpoints.get_mut(index) else {
                                    return false;
                                };
                                if *inherit_apply.read(cx) {
                                    entry.overrides.children.remove(&id);
                                } else {
                                    entry.overrides.children.insert(id, settings);
                                }
                            } else {
                                frame.children.insert(id, settings);
                            }
                            match this.editor.execute(Command::SetDesign {
                                design: Box::new(design),
                            }) {
                                Ok(_) => {
                                    this.after_change(cx);
                                    true
                                }
                                Err(error) => {
                                    this.set_status(error.to_string(), true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false);
                    if !accepted {
                        let message = owner
                            .read_with(cx, |this, _| {
                                this.status.as_ref().map(|(text, _)| text.to_string())
                            })
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| t!("editor.design_layout_ui.doc_gone").into());
                        error_apply.update(cx, |error, cx| {
                            *error = message;
                            cx.notify();
                        });
                        window.refresh();
                    }
                    accepted
                })
        });
    }
}
