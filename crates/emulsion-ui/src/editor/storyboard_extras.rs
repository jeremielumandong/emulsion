//! Storyboard workflow extras on the Stage: line mileage (SB4) counted
//! from every committed stroke and shown in the panel inspector, and the
//! overlays and menu entries of Edit in an external editor
//! (`storyboard_external`) and Sketch Sprint (`storyboard_sprint`).
use super::storyboard_external::ExternalUi;
use super::storyboard_sprint::{Sprint, SprintPlan};
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::project::mileage::{Mileage, path_length};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{PopupMenu, PopupMenuItem},
};

/// The extras' state in the editor.
#[derive(Default)]
pub(crate) struct ExtrasUi {
    /// The panel out in an external editor.
    pub(crate) external: Option<ExternalUi>,
    /// The Sketch Sprint setup card, while open.
    pub(crate) sprint_setup: Option<SprintPlan>,
    /// The running or just finished session.
    pub(crate) sprint: Option<Sprint>,
    pub(crate) sprint_ticker: Option<Task<()>>,
}

/// The length of a brush stroke's path from its points.
pub(crate) fn ink_of(points: impl IntoIterator<Item = (f32, f32)>) -> f64 {
    path_length(
        points
            .into_iter()
            .map(|(x, y)| (f64::from(x), f64::from(y))),
    )
}

/// The outline length of a shape drawn in `(x, y, w, h)`: a rectangle's
/// perimeter or an ellipse's (Ramanujan's approximation).
pub(crate) fn shape_ink((_, _, w, h): (f64, f64, f64, f64), ellipse: bool) -> f64 {
    let (w, h) = (w.abs(), h.abs());
    if ellipse {
        let (a, b) = (w / 2., h / 2.);
        std::f64::consts::PI * (3. * (a + b) - ((3. * a + b) * (a + 3. * b)).sqrt())
    } else {
        2. * (w + h)
    }
}

impl EditorView {
    /// Count a stroke of `length` panel pixels just committed on the
    /// active panel (brush, eraser, vector line or shape), and record it
    /// for a running Sketch Sprint. Call it after the stroke's Undo step
    /// lands.
    pub(crate) fn note_ink(&mut self, length: f64, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() || length.is_nan() || length <= 0. {
            return;
        }
        let page = self.editor.active_page();
        self.editor.record_ink(page, length);
        let ink = Mileage::from_px(length, self.editor.doc.resolution);
        self.sprint_record(page, ink, cx);
    }

    /// Start counting mileage again on `panel`, or on the whole project.
    pub(crate) fn reset_mileage(&mut self, panel: Option<PageId>, cx: &mut Context<Self>) {
        self.editor
            .reset_mileage(panel.as_ref().map(std::slice::from_ref));
        self.set_status(
            if panel.is_some() {
                "Reset this panel's line mileage."
            } else {
                "Reset the project's line mileage."
            },
            false,
            cx,
        );
        cx.notify();
    }

    /// The panel inspector's line mileage: this panel, the project, and
    /// Reset for each.
    pub(super) fn mileage_section(
        &self,
        panel: PageId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let here = self.editor.panel_mileage(panel);
        let all = self.editor.project_mileage();
        let project = match all.comparison() {
            Some(fun) => format!("Project: {} · {fun}", all.label()),
            None => format!("Project: {}", all.label()),
        };
        div()
            .id("storyboard-mileage")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(label("Line mileage", p).pt(px(4.)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("storyboard-mileage-panel")
                            .test_support()
                            .flex_1()
                            .child(format!("This panel: {}", here.label())),
                    )
                    .child(
                        Button::new("storyboard-mileage-reset")
                            .label("Reset")
                            .tooltip("Count this panel's line mileage from zero")
                            .xsmall()
                            .ghost()
                            .on_click(
                                cx.listener(move |e, _, _, cx| e.reset_mileage(Some(panel), cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("storyboard-mileage-project")
                            .test_support()
                            .flex_1()
                            .child(mono(project, 10., p.muted)),
                    )
                    .child(
                        Button::new("storyboard-mileage-reset-all")
                            .label("Reset all")
                            .tooltip("Count every panel's line mileage from zero")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|e, _, _, cx| e.reset_mileage(None, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The extras floating over the Stage or Board: the external editor's
    /// chip and Sketch Sprint.
    pub(super) fn storyboard_extras_layers(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = self.external_edit_chip(p, cx).into_iter().collect();
        if !self.board_open() {
            out.extend(self.sprint_overlay(p, cx));
        }
        out
    }

    /// View menu entries: Sketch Sprint and editing the active panel
    /// elsewhere.
    pub(super) fn storyboard_extras_view_items(
        menu: PopupMenu,
        editor: &Entity<EditorView>,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        if editor.read(cx).editor.storyboard().is_none() {
            return menu;
        }
        let sprinting = editor.read(cx).extras.sprint.is_some();
        let sprint = editor.downgrade();
        let external = editor.downgrade();
        menu.item(
            PopupMenuItem::new("Sketch Sprint…")
                .disabled(sprinting)
                .on_click(move |_, window, cx| {
                    sprint
                        .update(cx, |e, cx| e.open_sketch_sprint(window, cx))
                        .ok();
                }),
        )
        .item(
            PopupMenuItem::new("Edit Panel in External Editor").on_click(move |_, _, cx| {
                external
                    .update(cx, |e, cx| {
                        let panel = e.editor.active_page();
                        e.start_external_edit(panel, cx)
                    })
                    .ok();
            }),
        )
        .separator()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn shapes_count_their_outline() {
        assert_eq!(shape_ink((0., 0., 10., 5.), false), 30.);
        let circle = shape_ink((0., 0., 20., 20.), true);
        assert!((circle - std::f64::consts::PI * 20.).abs() < 1e-9);
        assert_eq!(ink_of([(0., 0.), (3., 4.)]), 5.);
    }
}
