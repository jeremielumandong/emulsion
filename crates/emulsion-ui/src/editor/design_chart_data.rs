//! A dialog-local data grid. Only Apply commits its draft to the document.
use super::*;
use emulsion_core::design_charts::{Axis, Chart, Kind, Merge};
use gpui_kit::component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
    input::{Textarea, TextareaState},
};

pub(super) struct ChartDataEditor {
    pub kind: Kind,
    cells: Vec<Vec<Entity<TextareaState>>>,
    csv: Entity<TextareaState>,
    csv_mode: bool,
    pub error: Option<String>,
    axes: [Entity<InputState>; 8],
    show_labels: [bool; 2],
    merges: Vec<Merge>,
    merge_fields: [Entity<InputState>; 4],
}

impl ChartDataEditor {
    pub fn new(chart: &Chart, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            kind: chart.kind,
            cells: Self::cells(&chart.rows, window, cx),
            csv: cx.new(|cx| TextareaState::new(window, cx).rows(9)),
            csv_mode: false,
            error: None,
            axes: [
                chart.x_axis.min.map(|v| v.to_string()).unwrap_or_default(),
                chart.x_axis.max.map(|v| v.to_string()).unwrap_or_default(),
                chart.x_axis.ticks.to_string(),
                chart.x_axis.label.clone(),
                chart.y_axis.min.map(|v| v.to_string()).unwrap_or_default(),
                chart.y_axis.max.map(|v| v.to_string()).unwrap_or_default(),
                chart.y_axis.ticks.to_string(),
                chart.y_axis.label.clone(),
            ]
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v))),
            show_labels: [chart.x_axis.show_labels, chart.y_axis.show_labels],
            merges: chart.merges.clone(),
            merge_fields: ["1", "1", "1", "2"]
                .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v))),
        }
    }

    fn cells(
        rows: &[Vec<String>],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Vec<Entity<TextareaState>>> {
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|value| {
                        cx.new(|cx| {
                            TextareaState::new(window, cx)
                                .rows(2)
                                .default_value(value.clone())
                        })
                    })
                    .collect()
            })
            .collect()
    }

    pub fn rows(&self, cx: &App) -> Result<Vec<Vec<String>>, String> {
        let rows = if self.csv_mode {
            emulsion_io::design_charts::rows(&self.csv.read(cx).value())
                .map_err(|error| error.to_string())?
        } else {
            self.cells
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.read(cx).value().to_string())
                        .collect()
                })
                .collect()
        };
        // Validate the grid shape independently of the selected chart type.
        // Switching types must never discard columns or coerce table text.
        let mut table = Chart::example(Kind::Table);
        table.rows = rows;
        table.validate()?;
        Ok(table.rows)
    }

    pub fn apply_options(&self, chart: &mut Chart, cx: &App) -> Result<(), String> {
        let axis = |offset: usize| -> Result<Axis, String> {
            let number = |index: usize| -> Result<Option<f64>, String> {
                let s = self.axes[index].read(cx).value();
                let s = s.trim();
                if s.is_empty() {
                    Ok(None)
                } else {
                    s.parse()
                        .map(Some)
                        .map_err(|_| "Axis bounds must be numbers or blank for automatic.".into())
                }
            };
            Ok(Axis {
                min: number(offset)?,
                max: number(offset + 1)?,
                ticks: self.axes[offset + 2]
                    .read(cx)
                    .value()
                    .trim()
                    .parse()
                    .map_err(|_| "Use 2–20 integer axis ticks.".to_string())?,
                label: self.axes[offset + 3].read(cx).value().to_string(),
                show_labels: self.show_labels[offset / 4],
            })
        };
        chart.x_axis = axis(0)?;
        chart.y_axis = axis(4)?;
        chart.merges = self.merges.clone();
        chart.validate()
    }
    fn merge_selection(&mut self, clear: bool, cx: &mut Context<Self>) {
        let result =
            (|| {
                let values =
                    self.merge_fields
                        .iter()
                        .map(|f| {
                            f.read(cx).value().trim().parse::<usize>().map_err(|_| {
                                "Merge coordinates must be positive integers.".to_string()
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                let row = values[0]
                    .checked_sub(1)
                    .ok_or("Rows start at1 (the header).")?;
                let column = values[1].checked_sub(1).ok_or("Columns start at1 (A).")?;
                let mut chart = Chart::example(Kind::Table);
                chart.rows = self.rows(cx)?;
                chart.merges = self.merges.clone();
                if clear {
                    chart.merges.retain(|m| !m.contains(row, column));
                } else {
                    chart.merges.push(Merge {
                        row,
                        column,
                        rows: values[2],
                        columns: values[3],
                    });
                }
                chart.validate()?;
                self.merges = chart.merges;
                Ok::<_, String>(())
            })();
        self.error = result.err();
        cx.notify();
    }
    fn remove_merge_index(&mut self, index: usize, row: bool) {
        for m in &mut self.merges {
            let (start, count) = if row {
                (&mut m.row, &mut m.rows)
            } else {
                (&mut m.column, &mut m.columns)
            };
            if index < *start {
                *start -= 1;
            } else if index < *start + *count {
                *count -= 1;
            }
        }
        self.merges
            .retain(|m| m.rows > 0 && m.columns > 0 && (m.rows > 1 || m.columns > 1));
    }
    fn switch_mode(&mut self, csv: bool, window: &mut Window, cx: &mut Context<Self>) {
        if csv == self.csv_mode {
            return;
        }
        match self.rows(cx) {
            Ok(rows) => {
                if csv {
                    let csv_text = emulsion_io::design_charts::to_csv(&rows);
                    if csv_text.len() > 512 * 1024 {
                        self.error = Some("This dataset exceeds the 512 KB CSV limit. Continue editing it in Cells.".into());
                        cx.notify();
                        return;
                    }
                    self.csv = cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .rows(9)
                            .default_value(csv_text)
                    });
                } else {
                    self.cells = Self::cells(&rows, window, cx);
                }
                self.csv_mode = csv;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }
}

impl Render for ChartDataEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let columns = self.cells[0].len();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().flex().flex_wrap().gap_1().children(
                Kind::ALL.into_iter().enumerate().map(|(index, kind)| {
                    Button::new(("design-chart-kind", index))
                        .label(kind.label())
                        .small()
                        .outline()
                        .selected(self.kind == kind)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.kind = kind;
                            this.error = None;
                            cx.notify();
                        }))
                }),
            ))
            .child(
                div().flex().flex_wrap().gap_1()
                    .child(Button::new("design-chart-grid-mode").label("Cells").small().outline()
                        .selected(!self.csv_mode)
                        .on_click(cx.listener(|this, _, window, cx| this.switch_mode(false, window, cx))))
                    .child(Button::new("design-chart-csv-mode").label("CSV").small().outline()
                        .selected(self.csv_mode)
                        .on_click(cx.listener(|this, _, window, cx| this.switch_mode(true, window, cx))))
                    .when(!self.csv_mode, |d| {
                        d.child(Button::new("design-chart-add-row").label("Add row").small().outline()
                            .disabled(self.cells.len() >= 51)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.cells.len() >= 51 { return; }
                                let row = (0..this.cells[0].len()).map(|column| {
                                    let value = if column == 0 || this.kind == Kind::Table { "" } else { "0" };
                                    cx.new(|cx| TextareaState::new(window, cx).rows(2).default_value(value))
                                }).collect();
                                this.cells.push(row);
                                cx.notify();
                            })))
                        .child(Button::new("design-chart-add-column").label("Add column").small().outline()
                            .disabled(columns >= 9)
                            .on_click(cx.listener(|this, _, window, cx| {
                                let columns = this.cells[0].len();
                                if columns >= 9 { return; }
                                for (index, row) in this.cells.iter_mut().enumerate() {
                                    let value = if index == 0 {
                                        format!("{} {}", if this.kind == Kind::Table { "Column" } else { "Series" }, columns)
                                    } else if this.kind == Kind::Table { String::new() } else { "0".into() };
                                    row.push(cx.new(|cx| TextareaState::new(window, cx).rows(2).default_value(value)));
                                }
                                cx.notify();
                            })))
                    }),
            )
            .when(!matches!(self.kind,Kind::Table|Kind::Pie|Kind::Donut),|d| {
                let fields=["X minimum · auto if blank","X maximum · auto if blank","X ticks · scatter","X axis label","Y minimum · auto if blank","Y maximum · auto if blank","Y ticks","Y axis label"];
                d.child(div().grid().grid_cols(4).gap_1().children(fields.into_iter().enumerate().filter(|(i,_)| *i >= 3 || self.kind == Kind::Scatter).map(|(i,label)| {
                    div().child(label).child(Input::new(&self.axes[i]).id(("design-chart-axis",i)))
                }))).child(div().flex().gap_1().children((0..2).map(|i| {
                    Button::new(("design-chart-axis-labels",i)).label(if i==0 {"X labels"}else{"Y labels"}).small().outline().selected(self.show_labels[i]).on_click(cx.listener(move|this,_,_,cx| {this.show_labels[i]=!this.show_labels[i];cx.notify();}))
                })))
            })
            .when(self.kind==Kind::Table,|d| {
                d.child(div().grid().grid_cols(4).gap_1().children(["Start row · 1 = header","Start column · 1 = A","Row span","Column span"].into_iter().enumerate().map(|(i,label)| {
                    div().child(label).child(Input::new(&self.merge_fields[i]).id(("design-table-merge-input",i)))
                }))).child(div().flex().flex_wrap().gap_1()
                    .child(Button::new("design-table-merge").label("Merge range").small().outline().on_click(cx.listener(|this,_,_,cx|this.merge_selection(false,cx))))
                    .child(Button::new("design-table-unmerge").label("Unmerge at cell").small().outline().on_click(cx.listener(|this,_,_,cx|this.merge_selection(true,cx))))
                    .child(format!("{} merged regions · covered cell values are retained",self.merges.len())))
            })
            .child("The first row contains headings. Column A contains category labels; charts use numbers in the other columns. Pie/donut need one value column; scatter needs numeric X in column A. Tables accept text; merged cells display the top-left value without deleting covered data.")
            .when(self.csv_mode, |d| {
                d.child(div().id("design-chart-data").test_support().child(Textarea::new(&self.csv)))
            })
            .when(!self.csv_mode, |d| {
                d.child(
                    div().id("design-chart-grid").test_support().max_h(px(280.)).overflow_scroll()
                        .child(div().flex().flex_col().gap_1().w(px((columns * 144 + 156) as f32))
                            .child(div().flex().gap_1().child(div().w(px(148.)).flex_shrink_0().child("Row"))
                                .children((0..columns).map(|column| {
                                    div().w(px(140.)).flex_shrink_0().flex().items_center().justify_between()
                                        .child(char::from(b'A' + column as u8).to_string())
                                        .child(Button::new(("design-chart-remove-column", column))
                                            .label("Remove").small().ghost().disabled(columns <= 2)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.cells[0].len() > 2 {
                                                    for row in &mut this.cells { row.remove(column); }
                                                    this.remove_merge_index(column,false);
                                                    cx.notify();
                                                }
                                            })))
                                })))
                            .children(self.cells.iter().enumerate().map(|(row_index, row)| {
                                div().flex().gap_1().items_center()
                                    .child(div().w(px(148.)).flex_shrink_0().flex().items_center().justify_between()
                                        .child(if row_index == 0 { "Header".into() } else { row_index.to_string() })
                                        .when(row_index > 0, |d| d.child(
                                            Button::new(("design-chart-remove-row", row_index))
                                                .label("Remove").small().ghost().disabled(self.cells.len() <= 2)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if this.cells.len() > 2 { this.cells.remove(row_index); this.remove_merge_index(row_index,true); cx.notify(); }
                                                })),
                                        ))
                                    )
                                    .children(row.iter().enumerate().map(|(column, cell)| {
                                        div().id(("design-chart-cell", row_index * 9 + column)).test_support()
                                            .w(px(140.)).flex_shrink_0().child(
                                                Textarea::new(cell).h(px(48.)).aria_label(format!("{}{}", char::from(b'A' + column as u8), row_index + 1)),
                                            )
                                    }))
                            }))),
                )
            })
            .when_some(self.error.clone(), |d, error| {
                d.child(div().id("design-chart-error").test_support().child(error))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[gpui_kit::test]
    fn oversized_csv_transition_keeps_the_editable_grid(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut chart = Chart::example(Kind::Table);
        chart.rows = vec![vec!["界".repeat(1000); 9]; 51];
        chart.validate().unwrap();
        let (view, cx) = cx.add_window_view(|window, cx| ChartDataEditor::new(&chart, window, cx));
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.switch_mode(true, window, cx);
                assert!(!view.csv_mode);
                assert!(view.error.as_ref().unwrap().contains("512 KB"));
                assert_eq!(view.rows(cx).unwrap(), chart.rows);
                let mut editor = Editor::new(Document::new(600, 400), None);
                let mut applied = chart.clone();
                applied.rows = view.rows(cx).unwrap();
                let id = emulsion_core::design_charts::apply(&mut editor, None, applied, (0., 0.))
                    .unwrap();
                assert_eq!(editor.doc.design.charts[&id], chart);
            });
        });
    }
}
