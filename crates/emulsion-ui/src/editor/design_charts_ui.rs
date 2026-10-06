//! Data-backed chart and table controls in the Design element library.
use super::design_chart_data::ChartDataEditor;
use super::*;
use emulsion_core::design_charts::{self, Chart, Kind};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

/// Display name for a chart kind; `Kind::label` stays the English source text.
pub(super) fn chart_kind_label(kind: Kind) -> String {
    match kind {
        Kind::Bar => t!("editor.design_charts_ui.kind_bar"),
        Kind::Line => t!("editor.design_charts_ui.kind_line"),
        Kind::Pie => t!("editor.design_charts_ui.kind_pie"),
        Kind::Table => t!("editor.design_charts_ui.kind_table"),
        Kind::Area => t!("editor.design_charts_ui.kind_area"),
        Kind::Scatter => t!("editor.design_charts_ui.kind_scatter"),
        Kind::StackedBar => t!("editor.design_charts_ui.kind_stacked_bar"),
        Kind::Donut => t!("editor.design_charts_ui.kind_donut"),
    }
    .into_owned()
}

impl EditorView {
    pub(super) fn design_chart_controls(&self, cx: &Context<Self>) -> AnyElement {
        let editable = self
            .selected
            .is_some_and(|id| self.editor.doc.design.charts.contains_key(&id));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(t!("editor.design_charts_ui.heading"))
            .child(div().grid().grid_cols(2).gap_1().children(
                Kind::ALL.into_iter().enumerate().map(|(i, kind)| {
                    Button::new(("design-chart-add", i))
                        .label(chart_kind_label(kind))
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.design_chart_dialog(kind, false, window, cx)
                        }))
                }),
            ))
            .when(editable, |d| {
                d.child(
                    Button::new("design-chart-edit")
                        .label(t!("editor.design_charts_ui.edit_data"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_chart_dialog(Kind::Bar, true, window, cx)
                        })),
                )
                .child(
                    Button::new("design-chart-detach")
                        .label(t!("editor.design_charts_ui.detach"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.prepare_page_action(cx) {
                                return;
                            }
                            if let Some(id) = this.selected {
                                match design_charts::detach(&mut this.editor, id) {
                                    Ok(()) => this.after_change(cx),
                                    Err(error) => this.set_status(error, true, cx),
                                }
                            }
                        })),
                )
            })
            .into_any_element()
    }
    pub(super) fn design_chart_dialog(
        &mut self,
        kind: Kind,
        editing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let existing = if editing {
            self.selected
                .filter(|id| self.editor.doc.design.charts.contains_key(id))
        } else {
            None
        };
        if editing && existing.is_none() {
            return;
        }
        let mut chart = existing
            .and_then(|id| self.editor.doc.design.charts.get(&id))
            .cloned()
            .unwrap_or_else(|| Chart::example(kind));
        let origin = if let Some(bounds) = existing.and_then(|id| {
            emulsion_core::geometry::node_bounds(&self.editor.doc, id)
                .ok()
                .flatten()
        }) {
            chart.size = (f64::from(bounds.w).max(160.), f64::from(bounds.h).max(160.));
            (f64::from(bounds.x), f64::from(bounds.y))
        } else {
            chart.size = (
                f64::from(self.editor.doc.width).clamp(160., 600.),
                f64::from(self.editor.doc.height).clamp(160., 400.),
            );
            (
                (f64::from(self.editor.doc.width) - chart.size.0) / 2.,
                (f64::from(self.editor.doc.height) - chart.size.1) / 2.,
            )
        };
        let fields = [
            chart.title.clone(),
            format!("{:.0}", chart.size.0),
            format!("{:.0}", chart.size.1),
            chart
                .colors
                .iter()
                .map(|c| {
                    if c[3] == 255 {
                        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
                    } else {
                        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
        ]
        .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let data = cx.new(|cx| ChartDataEditor::new(&chart, window, cx));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, window, _| {
            let inputs = fields.clone();
            let data = data.clone();
            let owner = owner.clone();
            let chart = chart.clone();
            dialog
                .title(t!("editor.design_charts_ui.dialog_title"))
                .width(px(720.))
                .child(
                    div()
                        .id("design-chart-dialog-body")
                        .max_h(px(
                            (f32::from(window.viewport_size().height) - 180.).max(100.)
                        ))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div().grid().grid_cols(2).gap_2().children(
                                [
                                    t!("editor.design_charts_ui.field_title"),
                                    t!("editor.design_charts_ui.field_width"),
                                    t!("editor.design_charts_ui.field_height"),
                                    t!("editor.design_charts_ui.field_colors"),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(i, label)| {
                                    div()
                                        .child(label)
                                        .child(Input::new(&fields[i]).id(("design-chart-input", i)))
                                }),
                            ),
                        )
                        .child(data.clone())
                        .child(t!("editor.design_charts_ui.dialog_note")),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_charts_ui.apply"
                )))
                .on_ok(move |_, _, cx| {
                    let mut chart = chart.clone();
                    chart.title = inputs[0].read(cx).value().to_string();
                    chart.size = (
                        inputs[1]
                            .read(cx)
                            .value()
                            .trim()
                            .parse()
                            .unwrap_or(f64::NAN),
                        inputs[2]
                            .read(cx)
                            .value()
                            .trim()
                            .parse()
                            .unwrap_or(f64::NAN),
                    );
                    let colors = inputs[3].read(cx).value().to_string();
                    chart.kind = data.read(cx).kind;
                    let rows = data.read(cx).rows(cx);
                    owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                this.set_status(
                                    t!("editor.design_charts_ui.page_changed"),
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            let result = (|| {
                                chart.colors = colors
                                    .split(',')
                                    .map(|color| {
                                        let color =
                                            color.trim().strip_prefix('#').unwrap_or(color.trim());
                                        if !matches!(color.len(), 6 | 8) || !color.is_ascii() {
                                            return Err(t!("editor.design_charts_ui.bad_colors")
                                                .into_owned());
                                        }
                                        let value =
                                            u32::from_str_radix(color, 16).map_err(|_| {
                                                t!("editor.design_charts_ui.bad_hex").into_owned()
                                            })?;
                                        let rgba = if color.len() == 6 {
                                            (value << 8) | 255
                                        } else {
                                            value
                                        };
                                        Ok(rgba.to_be_bytes())
                                    })
                                    .collect::<Result<_, String>>()?;
                                chart.rows = rows?;
                                data.read(cx).apply_options(&mut chart, cx)?;
                                design_charts::apply(&mut this.editor, existing, chart, origin)
                            })();
                            match result {
                                Ok(id) => {
                                    this.set_layer_selection(vec![id], Some(id));
                                    this.after_change(cx);
                                    this.set_tool(Tool::Move, cx);
                                    true
                                }
                                Err(error) => {
                                    data.update(cx, |data, cx| {
                                        data.error = Some(error.clone());
                                        cx.notify();
                                    });
                                    this.set_status(error, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
}
