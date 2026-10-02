//! Text tab control for placing a shape label inside or around the shape.
use super::*;
use diagram::{LabelColumn, LabelRow};

/// Localized placement words for the position tooltips.
fn row_label(row: LabelRow) -> std::borrow::Cow<'static, str> {
    match row {
        LabelRow::Above => t!("editor.diagram_label_position_ui.above"),
        LabelRow::Top => t!("editor.diagram_label_position_ui.top"),
        LabelRow::Middle => t!("editor.diagram_label_position_ui.middle"),
        LabelRow::Bottom => t!("editor.diagram_label_position_ui.bottom"),
        LabelRow::Below => t!("editor.diagram_label_position_ui.below"),
    }
}
fn column_label(column: LabelColumn) -> std::borrow::Cow<'static, str> {
    match column {
        LabelColumn::Left => t!("editor.diagram_label_position_ui.left"),
        LabelColumn::Center => t!("editor.diagram_label_position_ui.center"),
        LabelColumn::Right => t!("editor.diagram_label_position_ui.right"),
    }
}

impl EditorView {
    /// Five rows of three: above the shape, a 3×3 grid inside it, below it.
    pub(super) fn diagram_label_position_control(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let shape = self.diagram_object()?;
        let current = diagram::label_position(&self.editor.doc, shape)?;
        let rows = |rows: &[(usize, LabelRow)], cx: &mut Context<Self>| {
            let mut grid = div().grid().grid_cols(3).gap(px(2.));
            for &(index, row) in rows {
                for (col, column) in LabelColumn::ALL.into_iter().enumerate() {
                    let selected = current == (row, column);
                    let (accent, muted) = (p.accent, p.muted);
                    grid = grid.child(
                        div()
                            .id(("diagram-label-position", index * 3 + col))
                            .test_support()
                            .h(px(if row.outside() { 20. } else { 24. }))
                            .px(px(7.))
                            .py(px(4.))
                            .flex()
                            .justify_start()
                            .when(column == LabelColumn::Center, |d| d.justify_center())
                            .when(column == LabelColumn::Right, |d| d.justify_end())
                            .when(row == LabelRow::Top, |d| d.items_start())
                            .when(matches!(row, LabelRow::Middle) || row.outside(), |d| {
                                d.items_center()
                            })
                            .when(row == LabelRow::Bottom, |d| d.items_end())
                            .rounded(px(5.))
                            .cursor_pointer()
                            .when(selected, |d| d.bg(accent.opacity(0.14)))
                            .hover(move |d| d.bg(accent.opacity(0.08)))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(SharedString::from(t!(
                                    "editor.diagram_label_position_ui.tooltip",
                                    row = row_label(row),
                                    column = column_label(column)
                                )))
                                .build(window, cx)
                            })
                            .child(div().w(px(14.)).h(px(3.)).rounded(px(1.5)).bg(if selected {
                                accent
                            } else {
                                muted
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_diagram_label_position(row, column, cx)
                            })),
                    );
                }
            }
            grid
        };
        let above = rows(&[(0, LabelRow::Above)], cx);
        let inside = rows(
            &[
                (1, LabelRow::Top),
                (2, LabelRow::Middle),
                (3, LabelRow::Bottom),
            ],
            cx,
        );
        let below = rows(&[(4, LabelRow::Below)], cx);
        Some(
            div()
                .id("diagram-label-position")
                .test_support()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px_3()
                .pt_2()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(t!("editor.diagram_label_position_ui.position")),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .p(px(3.))
                        .w(px(150.))
                        .rounded(px(8.))
                        .bg(p.soft_bg)
                        .child(above)
                        .child(
                            div()
                                .p(px(1.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(p.muted.opacity(0.6))
                                .child(inside),
                        )
                        .child(below),
                )
                .into_any_element(),
        )
    }

    fn set_diagram_label_position(
        &mut self,
        row: LabelRow,
        column: LabelColumn,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(shape) = self.diagram_object() else {
            return;
        };
        if diagram::label_position(&self.editor.doc, shape) == Some((row, column)) {
            return;
        }
        match diagram::label_position_command(&self.editor.doc, shape, row, column)
            .and_then(|command| self.editor.execute(command).map_err(|e| e.to_string()))
        {
            Ok(_) => {
                self.after_change(cx);
                cx.notify();
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
}
