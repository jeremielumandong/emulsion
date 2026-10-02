//! Beginner alignment and spacing without opening the Layers panel.
//! Uses ordinary layout commands: no frame conversion, resizing or new layout rules.
use super::design_direct_controls::control;
use super::*;
use emulsion_core::{
    command::Alignment,
    layer_links::{self, Arrange, ArrangeTarget, Distribution},
};
use gpui_kit::component::{
    Disableable, Selectable, Sizable,
    button::Button,
    popover::{Popover, PopoverState},
};

#[derive(Clone, Copy)]
enum Action {
    Align(Alignment),
    Gaps(Distribution),
    Tidy,
}

struct SelectionLayout {
    count: usize,
    locked: bool,
    bounds: Option<Vec<emulsion_raster::IRect>>,
}

impl SelectionLayout {
    fn new(doc: &Document, ids: &[NodeId]) -> Self {
        Self {
            count: ids.len(),
            locked: layer_links::check_movable(doc, ids).is_err(),
            bounds: ids
                .iter()
                .map(|id| emulsion_core::geometry::node_bounds(doc, *id))
                .collect(),
        }
    }

    fn allows(&self, action: Action, target: ArrangeTarget) -> bool {
        !self.locked
            && self.bounds.is_some()
            && self.count
                >= match action {
                    Action::Align(_) if target == ArrangeTarget::Canvas => 1,
                    Action::Align(_) => 2,
                    Action::Gaps(_) | Action::Tidy => 3,
                }
    }

    /// Center spread describes the existing row/column independently of object
    /// sizes or click order. A tie consistently chooses horizontal spacing.
    fn tidy_axis(&self) -> Distribution {
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for bounds in self.bounds.iter().flatten() {
            let center = [
                bounds.x as f64 + bounds.w as f64 / 2.,
                bounds.y as f64 + bounds.h as f64 / 2.,
            ];
            for axis in 0..2 {
                min[axis] = min[axis].min(center[axis]);
                max[axis] = max[axis].max(center[axis]);
            }
        }
        if max[1] - min[1] > max[0] - min[0] {
            Distribution::VerticalGap
        } else {
            Distribution::HorizontalGap
        }
    }
}

impl EditorView {
    fn design_arrange_ready(&self) -> bool {
        self.is_design()
            && !self.previewing()
            && !self.assistant.running
            && !self.raw.is_pending()
            && self.styles_ui.dialog_for.is_none()
            && self.drag.is_none()
            && self.warp.is_none()
            && (!self.editor.in_transaction() || self.type_tool.field.is_some())
    }

    pub(super) fn design_align_space_controls(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        Popover::new("design-align-space")
            .trigger(control(
                "design-direct-align-space",
                t!("design.arrange.title").to_string(),
            ))
            .content(move |_, window, cx| {
                // Keyed popover-local state survives target-button redraws and
                // does not add persistent editor/document state.
                let target = window.use_keyed_state("design-arrange-target", cx, |_, _| None);
                let popup = cx.weak_entity();
                owner
                    .update(cx, |view, cx| {
                        view.design_align_space_content(&target, popup, window, cx)
                    })
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    fn design_align_space_content(
        &self,
        target_state: &Entity<Option<ArrangeTarget>>,
        popup: WeakEntity<PopoverState>,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let palette = theme::palette(cx);
        let ids = self.selected_layer_roots();
        let selection = SelectionLayout::new(&self.editor.doc, &ids);
        let target = target_state.read(cx).unwrap_or(if ids.len() > 1 {
            ArrangeTarget::SelectedLayers
        } else {
            ArrangeTarget::Canvas
        });
        let ready = self.design_arrange_ready();
        let ticket = self.edit_ticket();
        let action_button = |id: &'static str, label: String, action: Action, target| {
            let ids = ids.clone();
            let popup = popup.clone();
            Button::new(id)
                .label(label.clone())
                .accessibility_label(label)
                .small()
                .outline()
                .w_full()
                .disabled(!ready || !selection.allows(action, target))
                .on_click(cx.listener(move |view, _, window, cx| {
                    // Dismiss before restoring canvas focus. Validate again so
                    // a stale popover cannot act on a different page/selection.
                    popup.update(cx, |popup, cx| popup.dismiss(window, cx)).ok();
                    view.apply_design_arrange(action, target, &ids, ticket, cx);
                    window.focus(&view.canvas_focus, cx);
                }))
        };
        let hint = if !ready {
            "design.arrange.busy"
        } else if selection.locked {
            "design.arrange.locked"
        } else if ids.is_empty() {
            "design.arrange.select_one"
        } else if selection.bounds.is_none() {
            "design.arrange.no_bounds"
        } else if target == ArrangeTarget::SelectedLayers && ids.len() < 2 {
            "design.arrange.select_two"
        } else if ids.len() < 3 {
            "design.arrange.select_three"
        } else {
            "design.arrange.gaps_hint"
        };
        let tidy_key = if selection.tidy_axis() == Distribution::VerticalGap {
            "design.arrange.tidy_vertical"
        } else {
            "design.arrange.tidy_horizontal"
        };
        div()
            .id("design-align-space-content")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(300.))
            .max_w((window.viewport_size().width - px(48.)).max(px(0.)))
            .max_h((window.viewport_size().height - px(120.)).max(px(0.)))
            .overflow_y_scroll()
            .text_size(px(12.))
            .child(
                div()
                    .text_color(palette.muted)
                    .child(t!("design.arrange.align_to").to_string()),
            )
            .child(
                div().grid().grid_cols(2).gap_1().children(
                    [
                        (
                            "design-align-target-canvas",
                            "design.arrange.canvas",
                            ArrangeTarget::Canvas,
                        ),
                        (
                            "design-align-target-objects",
                            "design.arrange.selected",
                            ArrangeTarget::SelectedLayers,
                        ),
                    ]
                    .into_iter()
                    .map(|(id, key, choice)| {
                        let state = target_state.clone();
                        Button::new(id)
                            .label(t!(key).to_string())
                            .small()
                            .outline()
                            .selected(target == choice)
                            .on_click(move |_, window, cx| {
                                state.update(cx, |target, cx| {
                                    *target = Some(choice);
                                    cx.notify();
                                });
                                window.refresh();
                            })
                    }),
                ),
            )
            .child(
                div().grid().grid_cols(3).gap_1().children(
                    [
                        ("design-align-left", "design.arrange.left", Alignment::Left),
                        (
                            "design-align-center",
                            "design.arrange.center",
                            Alignment::HorizontalCenter,
                        ),
                        (
                            "design-align-right",
                            "design.arrange.right",
                            Alignment::Right,
                        ),
                        ("design-align-top", "design.arrange.top", Alignment::Top),
                        (
                            "design-align-middle",
                            "design.arrange.middle",
                            Alignment::VerticalCenter,
                        ),
                        (
                            "design-align-bottom",
                            "design.arrange.bottom",
                            Alignment::Bottom,
                        ),
                    ]
                    .into_iter()
                    .map(|(id, key, alignment)| {
                        action_button(id, t!(key).to_string(), Action::Align(alignment), target)
                    }),
                ),
            )
            .child(
                div()
                    .text_color(palette.muted)
                    .child(t!("design.arrange.equal_gaps").to_string()),
            )
            .child(
                div().grid().grid_cols(2).gap_1().children(
                    [
                        (
                            "design-space-horizontal",
                            "design.arrange.horizontal",
                            Distribution::HorizontalGap,
                        ),
                        (
                            "design-space-vertical",
                            "design.arrange.vertical",
                            Distribution::VerticalGap,
                        ),
                    ]
                    .into_iter()
                    .map(|(id, key, spacing)| {
                        action_button(
                            id,
                            t!(key).to_string(),
                            Action::Gaps(spacing),
                            ArrangeTarget::SelectedLayers,
                        )
                    }),
                ),
            )
            .child(
                action_button(
                    "design-tidy-up",
                    t!(tidy_key).to_string(),
                    Action::Tidy,
                    ArrangeTarget::SelectedLayers,
                )
                .tooltip(t!("design.arrange.tidy_hint").to_string()),
            )
            .child(
                div()
                    .id("design-arrange-hint")
                    .test_support()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(t!(hint).to_string()),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(t!("design.arrange.tidy_hint").to_string()),
            )
            .into_any_element()
    }

    fn apply_design_arrange(
        &mut self,
        action: Action,
        target: ArrangeTarget,
        ids: &[NodeId],
        ticket: (u64, u64),
        cx: &mut Context<Self>,
    ) {
        if self.edit_ticket() != ticket || self.selected_layer_roots() != ids {
            self.set_status(t!("design.arrange.changed").to_string(), false, cx);
            return;
        }
        if !self.design_arrange_ready()
            || !SelectionLayout::new(&self.editor.doc, ids).allows(action, target)
            || !self.prepare_page_action(cx)
        {
            return;
        }
        // Committing text can change its bounds, so infer Tidy's axis from the
        // committed artwork rather than the popover's earlier preview.
        let operation = match action {
            Action::Align(alignment) => Arrange::Align(alignment),
            Action::Gaps(distribution) => Arrange::Distribute(distribution),
            Action::Tidy => {
                Arrange::Distribute(SelectionLayout::new(&self.editor.doc, ids).tidy_axis())
            }
        };
        self.arrange_selected(operation, target, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_raster::{IRect, vector::PathStyle, vector_geometry};
    use gpui_kit::test::TestWindowExt;

    fn fixture() -> (Document, [NodeId; 3]) {
        let mut doc = Document::new(360, 240);
        let ids = [
            (20., 30., 20., 20.),
            (75., 90., 40., 30.),
            (250., 150., 30., 40.),
        ]
        .map(|(x, y, w, h)| {
            Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Object",
                    Arc::new(vector_geometry::rectangle(x, y, w, h)),
                    PathStyle {
                        fill: Some([0, 80, 240, 255]),
                        // Keep the fixture's artwork bounds at the authored
                        // coordinates; the default has a 3px centered stroke.
                        stroke: None,
                        ..Default::default()
                    },
                    360,
                    240,
                )),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap()
        });
        (doc, ids)
    }

    fn bounds(doc: &Document, ids: &[NodeId]) -> Vec<IRect> {
        ids.iter()
            .map(|id| emulsion_core::geometry::node_bounds(doc, *id).unwrap())
            .collect()
    }

    #[test]
    fn tidy_chooses_the_existing_axis_with_a_stable_horizontal_tie() {
        let (doc, ids) = fixture();
        assert_eq!(
            SelectionLayout::new(&doc, &ids).tidy_axis(),
            Distribution::HorizontalGap
        );
        let column = SelectionLayout {
            count: 3,
            locked: false,
            // Wide objects still form a column; union aspect ratio is wrong here.
            bounds: Some(vec![
                IRect::new(0, 0, 1000, 10),
                IRect::new(0, 70, 1000, 10),
                IRect::new(0, 200, 1000, 10),
            ]),
        };
        assert_eq!(column.tidy_axis(), Distribution::VerticalGap);
        let tie = SelectionLayout {
            bounds: Some(vec![
                IRect::new(0, 0, 10, 10),
                IRect::new(40, 40, 10, 10),
                IRect::new(100, 100, 10, 10),
            ]),
            ..column
        };
        assert_eq!(tie.tidy_axis(), Distribution::HorizontalGap);
    }

    #[test]
    fn arrange_requires_enough_spatial_and_unlocked_objects() {
        let (mut doc, ids) = fixture();
        let align = Action::Align(Alignment::Left);
        for count in 0..=3 {
            let state = SelectionLayout::new(&doc, &ids[..count]);
            assert_eq!(state.allows(align, ArrangeTarget::Canvas), count >= 1);
            assert_eq!(
                state.allows(align, ArrangeTarget::SelectedLayers),
                count >= 2
            );
            assert_eq!(
                state.allows(Action::Tidy, ArrangeTarget::SelectedLayers),
                count >= 3
            );
        }
        doc.node_mut(ids[1]).unwrap().locks.position = true;
        let state = SelectionLayout::new(&doc, &ids);
        assert!(!state.allows(align, ArrangeTarget::Canvas));
        assert!(!state.allows(Action::Tidy, ArrangeTarget::SelectedLayers));
        doc.node_mut(ids[1]).unwrap().locks.position = false;
        doc.node_mut(ids[1]).unwrap().locked = true;
        assert!(!SelectionLayout::new(&doc, &ids).allows(align, ArrangeTarget::Canvas));
        doc.node_mut(ids[1]).unwrap().locked = false;
        let group = Command::Group {
            ids: ids[..2].to_vec(),
            name: "Group".into(),
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        doc.node_mut(ids[1]).unwrap().locks.position = true;
        assert!(!SelectionLayout::new(&doc, &[group, ids[2]]).allows(align, ArrangeTarget::Canvas));
    }

    #[test]
    fn tidy_is_deterministic_for_reversed_selection_and_tied_positions() {
        let (mut doc, ids) = fixture();
        Command::ArrangeLayers {
            ids: ids[..2].to_vec(),
            operation: Arrange::Align(Alignment::Left),
            target: ArrangeTarget::SelectedLayers,
        }
        .apply(&mut doc)
        .unwrap();
        let original = doc.clone();
        let mut reverse = doc.clone();
        let operation = Arrange::Distribute(SelectionLayout::new(&doc, &ids).tidy_axis());
        Command::ArrangeLayers {
            ids: ids.to_vec(),
            operation,
            target: ArrangeTarget::SelectedLayers,
        }
        .apply(&mut doc)
        .unwrap();
        Command::ArrangeLayers {
            ids: ids.into_iter().rev().collect(),
            operation,
            target: ArrangeTarget::SelectedLayers,
        }
        .apply(&mut reverse)
        .unwrap();
        assert_eq!(doc, reverse);
        assert_eq!(doc.children(None), original.children(None));
        assert_eq!(doc.design, original.design);
        for (before, after) in bounds(&original, &ids).iter().zip(bounds(&doc, &ids)) {
            assert_eq!((before.w, before.h, before.y), (after.w, after.h, after.y));
        }
    }

    fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, [NodeId; 3], &mut VisualTestContext) {
        let (doc, ids) = fixture();
        let (workspace, cx) = crate::tests::open(cx, doc.clone());
        cx.simulate_resize(size(px(1000.), px(800.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                    "Align and space".into(),
                    window,
                    cx,
                )
            });
            let view = workspace.read(cx).editor.clone().unwrap();
            view.update(cx, |view, cx| {
                view.set_layer_selection(ids.to_vec(), Some(ids[0]));
                view.set_tool(Tool::Move, cx);
            });
            view
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            if window.try_find("design-drawer-close").is_some() {
                window.click("design-drawer-close", cx);
            }
        });
        cx.run_until_parked();
        (view, ids, cx)
    }

    fn click(cx: &mut VisualTestContext, id: &'static str) {
        cx.update(|window, cx| window.click(id, cx));
        cx.run_until_parked();
    }

    fn select(view: &Entity<EditorView>, ids: &[NodeId], cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.set_layer_selection(ids.to_vec(), ids.first().copied());
                cx.notify();
            })
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn align_popover_distinguishes_canvas_and_selected_objects_and_undoes(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx);
        select(&view, &ids[..2], cx);
        let original = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        for canvas in [false, true] {
            let history = cx.update(|_, cx| view.read(cx).editor.history.len());
            click(cx, "design-direct-align-space");
            click(
                cx,
                if canvas {
                    "design-align-target-canvas"
                } else {
                    "design-align-target-objects"
                },
            );
            click(cx, "design-align-right");
            cx.update(|window, cx| {
                let editor = view.read(cx);
                let result = bounds(&editor.editor.doc, &ids);
                assert_eq!(result[0].right(), if canvas { 360 } else { 115 });
                assert_eq!(result[1].right(), result[0].right());
                assert_eq!(result[2], bounds(&original, &ids)[2]);
                assert_eq!(editor.editor.history.len(), history + 1);
                assert!(window.try_find("design-align-space-content").is_none());
                assert!(window.try_find("node-panel").is_none());
                view.update(cx, |view, cx| view.undo(cx));
                assert_eq!(view.read(cx).editor.doc, original);
            });
            cx.run_until_parked();
        }
        select(&view, &ids[..1], cx);
        click(cx, "design-direct-align-space");
        click(cx, "design-align-target-canvas");
        click(cx, "design-align-center");
        cx.update(|_, cx| assert_eq!(bounds(&view.read(cx).editor.doc, &ids)[0].x, 170));
    }

    #[gpui_kit::test]
    fn spacing_and_tidy_preserve_sizes_order_and_one_step_undo(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx);
        let original = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        for (button, vertical) in [
            ("design-space-horizontal", false),
            ("design-space-vertical", true),
            ("design-tidy-up", false),
        ] {
            let history = cx.update(|_, cx| view.read(cx).editor.history.len());
            click(cx, "design-direct-align-space");
            // Spacing is always selection-relative, even with Canvas chosen.
            click(cx, "design-align-target-canvas");
            click(cx, button);
            cx.update(|_, cx| {
                let editor = view.read(cx);
                let after = bounds(&editor.editor.doc, &ids);
                let before = bounds(&original, &ids);
                for (a, b) in after.iter().zip(&before) {
                    assert_eq!((a.w, a.h), (b.w, b.h));
                    assert_eq!(
                        if vertical { a.x } else { a.y },
                        if vertical { b.x } else { b.y }
                    );
                }
                if vertical {
                    assert_eq!(
                        after.iter().map(|b| b.y).collect::<Vec<_>>(),
                        vec![30, 85, 150]
                    );
                } else {
                    assert_eq!(
                        after.iter().map(|b| b.x).collect::<Vec<_>>(),
                        vec![20, 125, 250]
                    );
                }
                assert_eq!(editor.editor.doc.children(None), original.children(None));
                assert_eq!(editor.editor.doc.design, original.design);
                assert_eq!(editor.editor.history.len(), history + 1);
                view.update(cx, |view, cx| view.undo(cx));
                assert_eq!(view.read(cx).editor.doc, original);
            });
            cx.run_until_parked();
        }
    }

    #[gpui_kit::test]
    fn insufficient_and_locked_selection_controls_cannot_mutate(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx);
        for count in 0..3 {
            select(&view, &ids[..count], cx);
            let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
            let history = cx.update(|_, cx| view.read(cx).editor.history.len());
            click(cx, "design-direct-align-space");
            click(cx, "design-space-horizontal");
            click(cx, "design-space-vertical");
            click(cx, "design-tidy-up");
            if count < 2 {
                click(cx, "design-align-target-objects");
                click(cx, "design-align-left");
            }
            cx.update(|_, cx| {
                assert_eq!(view.read(cx).editor.doc, before);
                assert_eq!(view.read(cx).editor.history.len(), history);
            });
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
        }
        select(&view, &ids, cx);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.editor.doc.node_mut(ids[1]).unwrap().locks.position = true;
                cx.notify();
            })
        });
        cx.run_until_parked();
        let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        click(cx, "design-direct-align-space");
        click(cx, "design-align-left");
        click(cx, "design-tidy-up");
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, before));
    }

    #[gpui_kit::test]
    fn stale_selection_revision_and_unfinished_edits_cannot_arrange(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                let original = view.editor.doc.clone();
                let history = view.editor.history.len();
                let ticket = view.edit_ticket();
                view.set_layer_selection(vec![ids[0]], Some(ids[0]));
                view.apply_design_arrange(
                    Action::Tidy,
                    ArrangeTarget::SelectedLayers,
                    &ids,
                    ticket,
                    cx,
                );
                view.set_layer_selection(ids.to_vec(), Some(ids[0]));
                view.invalidate_pending_edits();
                view.apply_design_arrange(
                    Action::Tidy,
                    ArrangeTarget::SelectedLayers,
                    &ids,
                    ticket,
                    cx,
                );
                let ticket = view.edit_ticket();
                view.assistant.running = true;
                view.apply_design_arrange(
                    Action::Tidy,
                    ArrangeTarget::SelectedLayers,
                    &ids,
                    ticket,
                    cx,
                );
                view.assistant.running = false;
                view.editor.begin("Unfinished edit");
                view.apply_design_arrange(
                    Action::Tidy,
                    ArrangeTarget::SelectedLayers,
                    &ids,
                    ticket,
                    cx,
                );
                assert!(view.editor.in_transaction());
                view.editor.cancel();
                assert_eq!(view.editor.doc, original);
                assert_eq!(view.editor.history.len(), history);
            })
        });
    }

    #[gpui_kit::test]
    fn align_popover_and_toolbar_stay_bounded_on_narrow_windows_and_cancel(
        cx: &mut TestAppContext,
    ) {
        let (_, _, cx) = setup(cx);
        for (width, height) in [(480., 500.), (600., 700.), (1000., 800.)] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.run_until_parked();
            click(cx, "design-direct-align-space");
            cx.update(|window, _| {
                for id in [
                    "design-direct-align-space",
                    "design-direct-arrange",
                    "design-align-space-content",
                    "design-tidy-up",
                ] {
                    let bounds = window.find(id).bounds();
                    assert!(
                        bounds.left() >= px(0.) && bounds.right() <= px(width),
                        "{id}: {bounds:?}"
                    );
                    assert!(
                        bounds.top() >= px(0.) && bounds.bottom() <= px(height),
                        "{id}: {bounds:?}"
                    );
                }
            });
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            cx.update(|window, _| assert!(window.try_find("design-align-space-content").is_none()));
        }
    }
}
