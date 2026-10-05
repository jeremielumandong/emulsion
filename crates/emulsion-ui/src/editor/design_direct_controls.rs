//! Stable, selection-first controls for Design's everyday object edits.
//! The same commands and dialogs back these controls, Properties and shortcuts.
use super::design_appearance_ui::Edit;
use super::*;
use gpui_kit::component::{
    Disableable, Sizable,
    button::Button,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    popover::Popover,
};

pub(super) fn control(id: &'static str, label: impl Into<SharedString>) -> Button {
    let label = label.into();
    Button::new(id)
        .label(label.clone())
        .accessibility_label(label)
        .xsmall()
        .outline()
        .h(px(25.))
        .px(px(7.))
        .flex_none()
}

fn selection_label(doc: &Document, ids: &[NodeId]) -> String {
    if ids.len() != 1 {
        return t!("design.direct.objects", count = ids.len()).to_string();
    }
    let Some(node) = doc.node(ids[0]) else {
        return String::new();
    };
    let kind = match &node.kind {
        NodeKind::Text { .. } => t!("design.direct.text"),
        NodeKind::Path { .. } => t!("design.direct.shape"),
        NodeKind::Raster { .. } | NodeKind::Smart { .. } => t!("design.direct.image"),
        NodeKind::Group { .. } => t!("design.direct.group"),
        _ => t!("design.direct.object"),
    };
    let name = match &node.kind {
        NodeKind::Text { spec, .. } if !spec.text.trim().is_empty() => {
            spec.text.split_whitespace().collect::<Vec<_>>().join(" ")
        }
        _ => node.name.clone(),
    };
    format!("{kind}: {name}")
}

/// Canvas actions commit an active text edit and retain canvas keyboard focus.
fn object_item(
    editor: &Entity<EditorView>,
    label: impl Into<SharedString>,
    enabled: bool,
    action: impl Fn(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) -> PopupMenuItem {
    let owner = editor.downgrade();
    PopupMenuItem::new(label)
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            owner
                .update(cx, |view, cx| {
                    if view.prepare_page_action(cx) {
                        action(view, window, cx);
                        window.focus(&view.canvas_focus, cx);
                    }
                })
                .ok();
        })
}

fn compact_controls(window: &Window) -> bool {
    window.viewport_size().width < px(900.)
}

impl EditorView {
    /// Shared with the narrow library overlay, which must start below both rows.
    pub(super) fn design_direct_controls_height(&self, window: &Window) -> Pixels {
        if !self.is_design() || self.previewing() {
            return px(0.);
        }
        window.rem_size() * if compact_controls(window) { 4.5 } else { 2.25 }
    }

    pub(super) fn design_direct_controls(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_design() || self.previewing() {
            return None;
        }
        let content = if self.design_full_tools() {
            self.design_appearance_controls(p, cx)
        } else {
            self.design_selection_toolbar_content(p, window, cx, false)
        };
        // Keep enough room for the target label and the first object controls.
        // A narrow single row leaves the font/crop trigger behind fixed actions.
        // Both rows stay present across selections, so picking an object never
        // shifts the canvas or moves the page/layout controls.
        let compact = compact_controls(window);
        let selection_controls = div()
            .id("design-direct-controls-scroll")
            .test_support()
            .flex()
            .items_center()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .when(compact, |row| row.w_full().h_9().px_2().flex_none())
            .children(content)
            // Keep font/crop actions at their established visible positions.
            // The existing horizontal scroll also exposes transform modes.
            .children(self.transform_mode_controls(p, cx))
            .into_any_element();
        let (inline, below) = if compact {
            (None, Some(selection_controls))
        } else {
            (Some(selection_controls), None)
        };
        let editor = cx.entity();
        Some(
            div()
                .id("design-direct-controls")
                .test_support()
                .h(self.design_direct_controls_height(window))
                .flex()
                .flex_col()
                .flex_none()
                .min_w_0()
                .bg(p.panel)
                .border_b_1()
                .border_color(p.line)
                .child(
                    div()
                        .id("design-direct-actions-row")
                        .test_support()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        // Page and layout actions never scroll out of reach,
                        // even while another object is selected.
                        .child(self.design_page_background_controls(p, cx))
                        .children(inline)
                        .when(compact, |row| row.child(div().flex_1().min_w_0()))
                        .child(self.design_align_space_controls(cx))
                        .child(
                            control(
                                "design-direct-arrange",
                                t!("design.direct.arrange").to_string(),
                            )
                            .dropdown_menu(move |menu, _, cx| {
                                Self::design_arrange_items(menu, &editor, cx)
                            }),
                        ),
                )
                .children(below)
                .into_any_element(),
        )
    }

    pub(super) fn design_selection_target(&self, p: &Palette, _cx: &Context<Self>) -> AnyElement {
        div()
            .id("design-selection-target")
            .test_support()
            .flex_none()
            .max_w(px(120.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(11.))
            .text_color(p.muted)
            .mr_1()
            .child(selection_label(
                &self.editor.doc,
                &self.selected_layer_roots(),
            ))
            .into_any_element()
    }

    /// Small, obvious entry points; validation, text ranges, gradients and undo
    /// continue to live in the existing appearance dialogs.
    pub(super) fn design_direct_appearance_actions(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            return Vec::new();
        }
        let nodes: Vec<_> = ids
            .iter()
            .filter_map(|id| self.editor.doc.node(*id))
            .collect();
        let locked = ids.iter().any(|id| {
            self.editor.doc.locked_ancestor(*id).is_some()
                || self.editor.doc.layer_locks(*id).pixels
        });
        let text = nodes.len() == 1 && matches!(nodes[0].kind, NodeKind::Text { .. });
        let paths = nodes
            .iter()
            .all(|node| matches!(node.kind, NodeKind::Path { .. }));
        let fill = nodes.iter().all(|node| {
            matches!(
                node.kind,
                NodeKind::Text { .. } | NodeKind::Path { .. } | NodeKind::Fill { .. }
            )
        });
        let mut controls = Vec::new();
        if fill {
            let color = match &nodes[0].kind {
                NodeKind::Text { spec, .. } => Some(
                    spec.style_at(self.text_style_range().map_or(0, |r| r.start))
                        .color,
                ),
                NodeKind::Path { style, .. } => style.fill,
                NodeKind::Fill { rgba } => Some(*rgba),
                _ => None,
            };
            controls.push(
                control(
                    "design-direct-color",
                    if text {
                        t!("design.direct.color")
                    } else {
                        t!("design.direct.fill")
                    }
                    .to_string(),
                )
                .disabled(locked)
                .when_some(color, |button, color| {
                    let [r, g, b, a] = color.map(|v| v as f32 / 255.);
                    button.child(
                        div()
                            .size(px(12.))
                            .rounded(px(2.))
                            .border_1()
                            .border_color(rgb(0x888888))
                            .bg(Rgba { r, g, b, a }),
                    )
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.design_appearance_dialog(Edit::Fill, window, cx)
                }))
                .into_any_element(),
            );
        }
        if paths {
            controls.push(
                control(
                    "design-direct-border",
                    t!("design.direct.border").to_string(),
                )
                .disabled(locked)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.design_appearance_dialog(Edit::Stroke, window, cx)
                }))
                .into_any_element(),
            );
        }
        if text {
            controls.push(
                control(
                    "design-direct-spacing",
                    t!("design.direct.spacing").to_string(),
                )
                .disabled(locked)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.design_appearance_dialog(Edit::Typography, window, cx)
                }))
                .into_any_element(),
            );
        } else {
            controls.push(
                control(
                    "design-direct-opacity",
                    t!("design.direct.opacity").to_string(),
                )
                .disabled(locked)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.design_appearance_dialog(Edit::Opacity, window, cx)
                }))
                .into_any_element(),
            );
        }
        controls
    }

    pub(super) fn design_advanced_appearance_button(&self, cx: &Context<Self>) -> AnyElement {
        let editor = cx.weak_entity();
        Popover::new("design-advanced-appearance")
            .trigger(control(
                "design-direct-effects",
                t!("design.direct.effects").to_string(),
            ))
            .content(move |_, window, cx| {
                editor
                    .update(cx, |this, cx| {
                        div()
                            .id("design-advanced-appearance-content")
                            .w(rems(26.))
                            .max_w((window.viewport_size().width - px(24.)).max(px(0.)))
                            .max_h(window.viewport_size().height - px(80.))
                            .overflow_y_scroll()
                            .children(
                                this.design_appearance_popup_controls(&theme::palette(cx), cx),
                            )
                            .into_any_element()
                    })
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    /// Shared by the labeled button and Design's right-click menu.
    pub(super) fn design_arrange_items(
        mut menu: PopupMenu,
        editor: &Entity<Self>,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let view = editor.read(cx);
        let ids = view.selected_layer_roots();
        let ready = !ids.is_empty()
            && !view.assistant.running
            && view.drag.is_none()
            && (!view.editor.in_transaction() || view.type_tool.field.is_some());
        let unlocked = ready
            && ids.iter().all(|id| {
                view.editor.doc.subtree(*id).into_iter().all(|id| {
                    view.editor.doc.locked_ancestor(id).is_none()
                        && !view.editor.doc.layer_locks(id).position
                })
            });
        let group = unlocked && ids.len() > 1;
        let ungroup = unlocked
            && ids
                .iter()
                .any(|id| view.editor.doc.node(*id).is_some_and(Node::is_group));
        let has_lock = ids
            .iter()
            .any(|id| view.editor.doc.node(*id).is_some_and(|node| node.locked));
        let can_lock = ready
            && ids.iter().all(|id| {
                view.editor
                    .doc
                    .locked_ancestor(*id)
                    .is_none_or(|locked| locked == *id)
            });
        let idle = !view.assistant.running
            && view.drag.is_none()
            && (!view.editor.in_transaction() || view.type_tool.field.is_some());
        let ticket = view.edit_ticket();
        // Locked objects are deliberately skipped by canvas picking. Keep a
        // named recovery path here after they have been deselected, without
        // requiring the advanced Layers panel or unlocking unrelated objects.
        let locked_objects: Vec<_> = view
            .editor
            .doc
            .nodes
            .iter()
            .filter(|node| {
                node.locked
                    && !emulsion_core::design_background::is_background_node(
                        &view.editor.doc,
                        node.id,
                    )
                    && node
                        .parent
                        .is_none_or(|parent| view.editor.doc.locked_ancestor(parent).is_none())
            })
            .map(|node| (node.id, selection_label(&view.editor.doc, &[node.id])))
            .collect();
        for (key, up, end) in [
            ("design.direct.forward", true, false),
            ("design.direct.backward", false, false),
            ("design.direct.front", true, true),
            ("design.direct.back", false, true),
        ] {
            menu = menu.item(object_item(
                editor,
                t!(key).to_string(),
                unlocked,
                move |view, _, cx| {
                    if end {
                        view.shift_selected_to_end(up, cx);
                    } else {
                        view.shift_selected(up, cx);
                    }
                },
            ));
        }
        menu = menu
            .separator()
            .item(object_item(
                editor,
                t!("design.direct.group").to_string(),
                group,
                |view, _, cx| view.group_selected(cx),
            ))
            .item(object_item(
                editor,
                t!("design.direct.ungroup").to_string(),
                ungroup,
                |view, _, cx| view.ungroup_selected(cx),
            ))
            .separator()
            .item(object_item(
                editor,
                if has_lock {
                    t!("design.direct.unlock")
                } else {
                    t!("design.direct.lock")
                }
                .to_string(),
                can_lock,
                move |view, _, cx| {
                    let commands = view
                        .selected_layer_roots()
                        .into_iter()
                        .map(|id| Command::SetLocked {
                            id,
                            locked: !has_lock,
                        })
                        .collect();
                    view.execute_layer_commands("Lock objects", commands, cx);
                },
            ))
            .separator()
            .item(object_item(
                editor,
                t!("design.direct.order_index").to_string(),
                unlocked && ids.len() == 1,
                |view, window, cx| view.design_layer_index_dialog(window, cx),
            ));
        if !locked_objects.is_empty() {
            menu = menu.separator();
            for (id, label) in locked_objects {
                menu = menu.item(object_item(
                    editor,
                    format!("{}: {label}", t!("design.direct.unlock")),
                    idle,
                    move |view, _, cx| {
                        if view.edit_ticket() != ticket {
                            return;
                        }
                        view.execute(Command::SetLocked { id, locked: false }, cx);
                        if view.editor.doc.locked_ancestor(id).is_none() {
                            view.set_layer_selection(vec![id], Some(id));
                            cx.notify();
                        }
                    },
                ));
            }
        }
        menu
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::{
        project::{ProjectEditor, ProjectKind},
        text::TextSpec,
    };
    use emulsion_raster::{vector::PathStyle, vector_geometry};
    use gpui_kit::test::TestWindowExt;

    fn fixture() -> (Document, [NodeId; 3]) {
        let mut doc = Document::new(600, 400);
        let nodes = [
            Node::text(
                0,
                "Heading layer",
                TextSpec {
                    text: "My selected\nheading".into(),
                    x: 50.,
                    y: 60.,
                    size: 32.,
                    ..Default::default()
                },
                600,
                400,
            ),
            Node::path(
                0,
                "Blue card",
                Arc::new(vector_geometry::rectangle(60., 160., 150., 100.)),
                PathStyle {
                    fill: Some([30, 80, 240, 255]),
                    ..Default::default()
                },
                600,
                400,
            ),
            Node::raster(
                0,
                "Photo",
                Arc::new(Raster::solid(100, 80, [1., 0., 0., 1.])),
                Placement {
                    x: 320.,
                    y: 170.,
                    ..Default::default()
                },
            ),
        ];
        let ids = nodes.map(|node| {
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap()
        });
        (doc, ids)
    }

    fn setup(
        cx: &mut TestAppContext,
        design: bool,
    ) -> (Entity<EditorView>, [NodeId; 3], &mut VisualTestContext) {
        let (doc, ids) = fixture();
        let (workspace, cx) = crate::tests::open(cx, doc.clone());
        cx.simulate_resize(size(px(1440.), px(1000.)));
        let view = cx.update(|window, cx| {
            if design {
                workspace.update(cx, |workspace, cx| {
                    workspace.install_project(
                        ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                        "Direct controls".into(),
                        window,
                        cx,
                    )
                });
            }
            let view = workspace.read(cx).editor.clone().unwrap();
            view.update(cx, |view, cx| {
                view.set_layer_selection(vec![ids[0]], Some(ids[0]));
                view.set_tool(Tool::Move, cx);
            });
            view
        });
        cx.run_until_parked();
        if design {
            cx.update(|window, cx| {
                if window.try_find("design-drawer-close").is_some() {
                    window.click("design-drawer-close", cx);
                }
            });
            cx.run_until_parked();
        }
        (view, ids, cx)
    }

    fn select(view: &Entity<EditorView>, ids: &[NodeId], cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.set_layer_selection(ids.to_vec(), ids.last().copied());
                cx.notify();
            })
        });
        cx.run_until_parked();
    }

    fn arrange(cx: &mut VisualTestContext, index: usize) {
        cx.update(|window, cx| window.click("design-direct-arrange", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(index, cx));
        cx.run_until_parked();
    }

    #[test]
    fn target_label_uses_text_content_and_identifies_multiple_objects() {
        let (doc, ids) = fixture();
        assert_eq!(
            selection_label(&doc, &ids[..1]),
            "Text: My selected heading"
        );
        assert_eq!(selection_label(&doc, &ids[1..2]), "Shape: Blue card");
        assert_eq!(selection_label(&doc, &ids), "3 objects");
    }

    #[gpui_kit::test]
    fn direct_controls_follow_selection_without_moving_canvas(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx, true);
        let canvas = cx.update(|window, cx| {
            assert!(window.find("design-selection-target").visible());
            for id in [
                "design-text-font",
                "design-text-size",
                "design-direct-color",
                "design-direct-spacing",
            ] {
                assert!(window.find(id).visible(), "{id}");
            }
            assert!(window.try_find("node-panel").is_none());
            view.read(cx).canvas_bounds()
        });
        for (selection, controls) in [
            (
                vec![ids[1]],
                vec![
                    "design-direct-color",
                    "design-direct-border",
                    "design-direct-opacity",
                ],
            ),
            (
                vec![ids[2]],
                vec![
                    "design-image-crop",
                    "design-image-replace",
                    "design-image-set-background",
                    "design-direct-opacity",
                ],
            ),
            (ids[..2].to_vec(), vec!["design-multiple-selection-toolbar"]),
            (Vec::new(), Vec::new()),
        ] {
            select(&view, &selection, cx);
            cx.update(|window, cx| {
                assert_eq!(view.read(cx).canvas_bounds(), canvas);
                assert!(window.find("design-page-background-controls").visible());
                assert!(window.find("design-direct-arrange").visible());
                for id in controls {
                    assert!(window.find(id).visible(), "{id}");
                }
            });
        }
        for width in [480., 600., 1000.] {
            cx.simulate_resize(size(px(width), px(700.)));
            select(&view, &[ids[0]], cx);
            cx.update(|window, _| {
                let background = window.find("design-page-background-controls").bounds();
                let arrange = window.find("design-direct-arrange").bounds();
                assert!(background.left() >= px(0.) && background.right() <= px(width));
                assert!(arrange.left() >= background.right() && arrange.right() <= px(width));
            });
        }
    }

    #[gpui_kit::test]
    fn narrow_object_controls_stay_clickable_above_the_open_library(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx, true);
        for width in [480., 600., 899., 900.] {
            cx.simulate_resize(size(px(width), px(700.)));
            select(&view, &ids[..1], cx);
            cx.update(|_, cx| {
                view.update(cx, |view, cx| {
                    view.show_design_section(super::super::design_ui::Section::Templates, cx);
                });
            });
            cx.run_until_parked();
            let canvas = cx.update(|window, cx| {
                let toolbar = window.find("design-direct-controls").bounds();
                let scroll = window.find("design-direct-controls-scroll").bounds();
                let drawer = window.find("design-drawer").bounds();
                assert!(
                    drawer.top() >= toolbar.bottom(),
                    "{width}: {drawer:?} / {toolbar:?}"
                );
                for id in [
                    "design-selection-target",
                    "design-text-font",
                    "design-text-size",
                ] {
                    let bounds = window.find(id).bounds();
                    assert!(
                        bounds.left() >= scroll.left() && bounds.right() <= scroll.right(),
                        "{width} {id}: {bounds:?} / {scroll:?}"
                    );
                }
                for id in [
                    "design-page-background-controls",
                    "design-direct-align-space",
                    "design-direct-arrange",
                ] {
                    let bounds = window.find(id).bounds();
                    assert!(
                        bounds.left() >= toolbar.left() && bounds.right() <= toolbar.right(),
                        "{width} {id}: {bounds:?} / {toolbar:?}"
                    );
                }
                window.click("design-text-font", cx);
                view.read(cx).canvas_bounds()
            });
            cx.run_until_parked();
            cx.update(|window, _| assert!(window.find("font-picker").visible()));
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            cx.update(|window, cx| window.click("design-direct-align-space", cx));
            cx.run_until_parked();
            cx.update(|window, _| assert!(window.find("design-align-space-content").visible()));
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            for selection in [&ids[1..2], &ids[..0]] {
                select(&view, selection, cx);
                cx.update(|_, cx| assert_eq!(view.read(cx).canvas_bounds(), canvas));
            }
        }
    }

    #[gpui_kit::test]
    fn arrange_changes_order_groups_and_locks_without_layers_and_undoes(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx, true);
        let original = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        arrange(cx, 2); // Bring to front.
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).editor.doc.children(None).last(),
                Some(&ids[0])
            );
            view.update(cx, |view, cx| view.undo(cx));
            assert_eq!(view.read(cx).editor.doc, original);
        });
        select(&view, &ids[..2], cx);
        arrange(cx, 5); // Group, following the separator.
        cx.update(|_, cx| {
            let editor = view.read(cx);
            assert!(
                editor
                    .editor
                    .doc
                    .node(editor.selected.unwrap())
                    .unwrap()
                    .is_group()
            );
        });
        arrange(cx, 6); // Ungroup.
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).selected_layer_roots().len(), 2);
        });
        arrange(cx, 8); // Lock.
        cx.update(|_, cx| {
            assert!(
                ids[..2]
                    .iter()
                    .all(|id| view.read(cx).editor.doc.node(*id).unwrap().locked)
            );
        });
        arrange(cx, 8); // Unlock.
        cx.update(|_, cx| {
            assert!(
                ids[..2]
                    .iter()
                    .all(|id| !view.read(cx).editor.doc.node(*id).unwrap().locked)
            );
            view.update(cx, |view, cx| {
                for _ in 0..4 {
                    view.undo(cx);
                }
            });
            assert_eq!(view.read(cx).editor.doc, original);
        });
    }

    #[gpui_kit::test]
    fn arrange_recovers_a_deselected_locked_object_without_layers(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx, true);
        let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        arrange(cx, 8);
        select(&view, &[], cx);
        cx.update(|window, cx| window.click("design-direct-arrange", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(
                window.within("popup-menu").find(12usize).label(),
                Some("Unlock: Text: My selected heading")
            );
            window.within("popup-menu").click(12usize, cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).selected, Some(ids[0]));
            assert_eq!(view.read(cx).editor.doc, before);
            assert!(window.try_find("node-panel").is_none());
            view.update(cx, |view, cx| view.undo(cx));
            assert!(view.read(cx).editor.doc.node(ids[0]).unwrap().locked);
        });
    }

    #[gpui_kit::test]
    fn arrange_send_to_back_stops_above_color_and_photo_backgrounds(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx, true);
        let background = cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                emulsion_core::design_background::set_color(&mut view.editor, [240, 245, 255, 255])
                    .unwrap();
                emulsion_core::design_background::set_image(&mut view.editor, ids[2]).unwrap();
                let background = emulsion_core::design_background::parts(&view.editor.doc).unwrap();
                view.set_layer_selection(vec![ids[1]], Some(ids[1]));
                view.after_change(cx);
                background
            })
        });
        cx.run_until_parked();
        let original = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        arrange(cx, 3); // Send to back, above both pinned background roots.
        cx.update(|_, cx| {
            let editor = view.read(cx);
            assert_eq!(
                editor.editor.doc.children(None),
                vec![
                    background.fill,
                    background.image.unwrap().group,
                    ids[1],
                    ids[0]
                ]
            );
        });
        let history = cx.update(|_, cx| view.read(cx).editor.history.len());
        arrange(cx, 3); // Repeated no-op must terminate and add no undo entry.
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.shift_selected(false, cx);
                assert_eq!(view.editor.history.len(), history);
                view.undo(cx);
                assert_eq!(view.editor.doc, original);
                for id in [background.fill, background.image.unwrap().group] {
                    view.set_layer_selection(vec![id], Some(id));
                    view.shift_selected_to_end(true, cx);
                    view.shift_selected_to_end(false, cx);
                    view.shift_selected(true, cx);
                    assert_eq!(view.editor.doc, original);
                }
            })
        });
    }

    #[gpui_kit::test]
    fn photo_reorder_does_not_treat_ordinary_fill_as_page_background(cx: &mut TestAppContext) {
        let (view, ids, cx) = setup(cx, false);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                let fill = view
                    .editor
                    .execute(Command::AddNode {
                        node: Box::new(Node::new(
                            0,
                            "Ordinary fill",
                            NodeKind::Fill { rgba: [255; 4] },
                        )),
                        slot: Slot {
                            parent: None,
                            index: 0,
                        },
                    })
                    .unwrap()
                    .unwrap();
                assert!(view.editor.doc.design.page_background.is_none());
                view.set_layer_selection(vec![ids[0]], Some(ids[0]));
                view.shift_selected_to_end(false, cx);
                assert_eq!(view.editor.doc.children(None)[0], ids[0]);
                view.set_layer_selection(vec![fill], Some(fill));
                view.shift_selected(true, cx);
                assert_eq!(view.editor.doc.children(None)[2], fill);
                view.shift_selected_to_end(true, cx);
                assert_eq!(view.editor.doc.children(None).last(), Some(&fill));
            })
        });
    }

    fn context_labels(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
        let position = cx.update(|_, cx| view.read(cx).canvas_bounds().unwrap().center());
        cx.simulate_mouse_down(position, MouseButton::Right, Default::default());
        cx.run_until_parked();
        cx.update(|window, _| {
            (0usize..40)
                .filter_map(|i| {
                    window
                        .within("popup-menu")
                        .try_find(i)
                        .and_then(|item| item.label().map(str::to_owned))
                })
                .collect()
        })
    }

    #[gpui_kit::test]
    fn design_context_menu_uses_object_actions(cx: &mut TestAppContext) {
        let (view, _, cx) = setup(cx, true);
        let labels = context_labels(&view, cx);
        for label in ["Arrange", "Duplicate", "Delete", "Cut", "Copy", "Paste"] {
            assert!(labels.iter().any(|value| value == label), "{labels:?}");
        }
        for label in [
            "Rectangle selection",
            "Delete selected pixels",
            "Free transform",
        ] {
            assert!(!labels.iter().any(|value| value == label), "{labels:?}");
        }
    }

    #[gpui_kit::test]
    fn photo_context_menu_keeps_pixel_and_transform_actions(cx: &mut TestAppContext) {
        let (view, _, cx) = setup(cx, false);
        cx.update(|window, _| assert!(window.try_find("design-direct-controls").is_none()));
        let labels = context_labels(&view, cx);
        for label in [
            "Rectangle selection",
            "Delete selected pixels",
            "Free transform",
        ] {
            assert!(labels.iter().any(|value| value == label), "{labels:?}");
        }
    }
}
