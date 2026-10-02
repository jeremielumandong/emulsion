//! Native multistop gradient editor shared by selected shape fills and strokes.
use super::*;
use emulsion_raster::vector::{GradientStop, PathPaint};
use gpui_kit::component::{Selectable, Sizable, WindowExt, button::Button};
struct StopInputs {
    offset: Entity<InputState>,
    color: Entity<InputState>,
}
struct GradientForm {
    stops: Vec<StopInputs>,
    radial: bool,
    angle: Entity<InputState>,
}
fn inputs(stop: GradientStop, window: &mut Window, cx: &mut App) -> StopInputs {
    StopInputs {
        offset: cx
            .new(|cx| InputState::new(window, cx).default_value(format!("{}", stop.offset * 100.))),
        color: cx.new(|cx| {
            InputState::new(window, cx).default_value(format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                stop.color[0], stop.color[1], stop.color[2], stop.color[3]
            ))
        }),
    }
}
fn parse_color(value: &str) -> Result<[u8; 4], String> {
    let hex = value
        .trim()
        .strip_prefix('#')
        .ok_or_else(|| t!("editor.design_gradient_ui.color_format"))?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(t!("editor.design_gradient_ui.color_format").into());
    }
    let mut color = [255; 4];
    for (i, c) in color.iter_mut().enumerate().take(hex.len() / 2) {
        *c = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(color)
}
impl GradientForm {
    fn values(&self, cx: &App) -> Result<(Vec<GradientStop>, f32), String> {
        let stops = self
            .stops
            .iter()
            .map(|s| {
                Ok(GradientStop {
                    offset: s
                        .offset
                        .read(cx)
                        .value()
                        .trim()
                        .parse::<f32>()
                        .map_err(|_| t!("editor.design_gradient_ui.stop_positions").into_owned())?
                        / 100.,
                    color: parse_color(&s.color.read(cx).value())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let angle = self
            .angle
            .read(cx)
            .value()
            .trim()
            .parse::<f32>()
            .map_err(|_| t!("editor.design_gradient_ui.numeric_angle").into_owned())?;
        PathPaint::from_stops(&stops, self.radial, angle)?;
        Ok((stops, angle))
    }
}
impl Render for GradientForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div().flex().gap_2().children(
                    [
                        (false, t!("editor.design_gradient_ui.linear")),
                        (true, t!("editor.design_gradient_ui.radial")),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (radial, label))| {
                        Button::new(("gradient-kind", i))
                            .small()
                            .outline()
                            .label(label)
                            .selected(self.radial == radial)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.radial = radial;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(t!("editor.design_gradient_ui.stops_hint"))
            .children(self.stops.iter().enumerate().map(|(i, stop)| {
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(95.))
                            .child(Input::new(&stop.offset).id(("gradient-position", i))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&stop.color).id(("gradient-color", i))),
                    )
                    .child(
                        Button::new(("gradient-remove", i))
                            .small()
                            .label(t!("editor.design_gradient_ui.remove"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.stops.len() > 2 {
                                    this.stops.remove(i);
                                    cx.notify();
                                }
                            })),
                    )
            }))
            .child(
                Button::new("gradient-add")
                    .small()
                    .label(t!("editor.design_gradient_ui.add_stop"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.stops.len() < 16 {
                            let next = inputs(
                                GradientStop {
                                    offset: 1.,
                                    color: [255; 4],
                                },
                                window,
                                cx,
                            );
                            this.stops.push(next);
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .child(t!("editor.design_gradient_ui.angle"))
                    .child(Input::new(&self.angle).id("gradient-angle")),
            )
            .child(t!("editor.design_gradient_ui.note"))
    }
}
impl EditorView {
    pub(super) fn show_gradient_editor(
        &mut self,
        id: NodeId,
        stroke: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(Node {
            kind: NodeKind::Path { style, .. },
            ..
        }) = self.editor.doc.node(id)
        else {
            return;
        };
        let (paint, color) = if stroke {
            (style.stroke_paint, style.stroke)
        } else {
            (style.fill_paint, style.fill)
        };
        let stops = paint
            .gradient_stops(color.unwrap_or([0, 0, 0, 255]))
            .unwrap_or_else(|| {
                vec![
                    GradientStop {
                        offset: 0.,
                        color: color.unwrap_or([0, 0, 0, 255]),
                    },
                    GradientStop {
                        offset: 1.,
                        color: [255; 4],
                    },
                ]
            });
        let form = cx.new(|cx| GradientForm {
            stops: stops.into_iter().map(|s| inputs(s, window, cx)).collect(),
            radial: paint.is_radial(),
            angle: cx.new(|cx| {
                InputState::new(window, cx).default_value(paint.gradient_angle().to_string())
            }),
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let form = form.clone();
            let owner = owner.clone();
            dialog
                .title(if stroke {
                    t!("editor.design_gradient_ui.stroke_title")
                } else {
                    t!("editor.design_gradient_ui.fill_title")
                })
                .width(px(490.))
                .child(
                    div()
                        .id("design-gradient-scroll")
                        .max_h(px(500.))
                        .overflow_y_scroll()
                        .child(form.clone()),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_gradient_ui.apply"
                )))
                .on_ok(move |_, _, cx| {
                    let values = form.read(cx).values(cx);
                    let radial = form.read(cx).radial;
                    owner
                        .update(cx, |this, cx| {
                            let result = values.and_then(|(stops, angle)| {
                                emulsion_core::design_vectors::gradient(
                                    &mut this.editor,
                                    id,
                                    stroke,
                                    &stops,
                                    radial,
                                    angle,
                                )
                            });
                            match result {
                                Ok(()) => {
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
    fn design_multistop_dialog_validates_preserves_native_source_and_undo(cx: &mut TestAppContext) {
        let mut editor = emulsion_core::Editor::new(Document::new(400, 300), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Shape",
                    Arc::new(emulsion_raster::vector_geometry::rectangle(
                        20., 20., 150., 100.,
                    )),
                    Default::default(),
                    400,
                    300,
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
                this.show_gradient_editor(id, false, window, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.click("gradient-add", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click(("gradient-position", 2usize), cx));
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("-1");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            window.click(("gradient-position", 2usize), cx);
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("100");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, _| {
                let NodeKind::Path { style, .. } = &this.editor.doc.node(id).unwrap().kind else {
                    panic!()
                };
                assert_eq!(style.fill_paint.gradient_stops([0; 4]).unwrap().len(), 3);
                this.editor.undo();
                assert_eq!(this.editor.doc, original);
            })
        });
    }
}
