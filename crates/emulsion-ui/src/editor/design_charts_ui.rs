//! Data-backed chart and table controls in the Design element library.
use super::*;
use emulsion_core::design_charts::{self, Chart, Kind};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    input::{Textarea, TextareaState},
};

impl EditorView {
    pub(super) fn design_chart_controls(&self, cx: &Context<Self>) -> AnyElement {
        let editable = self
            .selected
            .is_some_and(|id| self.editor.doc.design.charts.contains_key(&id));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child("Charts and tables")
            .child(div().grid().grid_cols(2).gap_1().children(
                Kind::ALL.into_iter().enumerate().map(|(i, kind)| {
                    Button::new(("design-chart-add", i))
                        .label(kind.label())
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
                        .label("Edit selected data…")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_chart_dialog(Kind::Bar, true, window, cx)
                        })),
                )
                .child(
                    Button::new("design-chart-detach")
                        .label("Detach from data")
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
    fn design_chart_dialog(
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
        let origin = if let Some(bounds) =
            existing.and_then(|id| emulsion_core::geometry::node_bounds(&self.editor.doc, id))
        {
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
        let data = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(9)
                .default_value(emulsion_io::design_charts::to_csv(&chart.rows))
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, _| {
            let inputs = fields.clone();
            let data = data.clone();
            let owner = owner.clone();
            let chart = chart.clone();
            dialog
                .title(format!("{} data", chart.kind.label()))
                .width(px(640.))
                .child(
                    div().flex().flex_col().gap_2()
                        .child(
                            div().grid().grid_cols(2).gap_2().children(
                                ["Title", "Width · px", "Height · px", "Colors · #RRGGBB or #RRGGBBAA"]
                                    .into_iter().enumerate().map(|(i, label)| {
                                        div().child(label).child(Input::new(&fields[i]).id(("design-chart-input", i)))
                                    }),
                            ),
                        )
                        .child("CSV: first row is the header. Charts use category labels in column one and numbers in the remaining columns. Pie uses one value column. Tables accept text.")
                        .child(div().id("design-chart-data").test_support().child(Textarea::new(&data)))
                        .child("Editing data redraws the chart at the chosen size. Detach from data to keep manual artwork edits."),
                )
                .footer(crate::widgets::form_dialog_footer("Apply data"))
                .on_ok(move |_, _, cx| {
                    let mut chart = chart.clone();
                    chart.title = inputs[0].read(cx).value().to_string();
                    chart.size = (
                        inputs[1].read(cx).value().trim().parse().unwrap_or(f64::NAN),
                        inputs[2].read(cx).value().trim().parse().unwrap_or(f64::NAN),
                    );
                    let colors = inputs[3].read(cx).value().to_string();
                    let csv = data.read(cx).value().to_string();
                    owner.update(cx, |this, cx| {
                        if this.edit_ticket() != ticket {
                            this.set_status("The page changed. Open chart data again.", true, cx);
                            return false;
                        }
                        let result = (|| {
                            chart.colors = colors.split(',').map(|color| {
                                let color = color.trim().strip_prefix('#').unwrap_or(color.trim());
                                if !matches!(color.len(), 6 | 8) || !color.is_ascii() {
                                    return Err("Use comma-separated #RRGGBB or #RRGGBBAA colors.".to_string());
                                }
                                let value = u32::from_str_radix(color, 16)
                                    .map_err(|_| "Use hexadecimal colors.".to_string())?;
                                let rgba = if color.len() == 6 { (value << 8) | 255 } else { value };
                                Ok(rgba.to_be_bytes())
                            }).collect::<Result<_, String>>()?;
                            chart.rows = emulsion_io::design_charts::rows(&csv).map_err(|e| e.to_string())?;
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
                                this.set_status(error, true, cx);
                                false
                            }
                        }
                    }).unwrap_or(false)
                })
        });
    }
}
