//! Authored headless GUI/async coverage, executed serially by the integration owner.
use super::smart_filter_mask_tests::{document, mask, setup};
use super::*;
use ::core::prelude::v1::test;
use emulsion_filters::{Filter, FilterStyle};
use gpui_kit::component::WindowExt;
use gpui_kit::{TestAppContext, VisualTestContext, test::TestWindowExt};

fn exact_cache(actual: &Document, expected: &Document) {
    assert_eq!(actual, expected);
    let (
        NodeKind::Smart {
            source: a,
            cache: ca,
            offset: oa,
            ..
        },
        NodeKind::Smart {
            source: b,
            cache: cb,
            offset: ob,
            ..
        },
    ) = (&actual.nodes[0].kind, &expected.nodes[0].kind)
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(a, b));
    assert!(Arc::ptr_eq(ca, cb));
    assert_eq!(oa, ob);
}

fn click_in_properties(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
    let id = id.into();
    let panel_id = ("sidebar-content", SidebarTab::Properties as usize);
    for _ in 0..20 {
        let (position, scroll) = cx.update(|window, _| {
            let panel = window.find(panel_id).bounds();
            let target = window.find(id.clone());
            let bounds = target.bounds();
            let fully_visible = target.visible()
                && bounds.top() >= panel.top()
                && bounds.bottom() <= panel.bottom()
                && bounds.left() >= panel.left()
                && bounds.right() <= panel.right();
            let scroll = if bounds.top() < panel.top() {
                160.
            } else {
                -160.
            };
            (fully_visible.then_some(bounds.center()), scroll)
        });
        if let Some(position) = position {
            cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
            cx.run_until_parked();
            return;
        }
        // Use the real scroll container so pointer events hit the requested
        // control, rather than another pane underneath its clipped bounds.
        cx.update(|window, cx| {
            window.scroll(panel_id, ScrollDelta::Pixels(point(px(0.), px(scroll))), cx);
        });
        cx.run_until_parked();
    }
    panic!("properties control {id:?} did not scroll fully into view");
}

fn catalogue_properties(
    cx: &mut TestAppContext,
    design: bool,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let original = document();
    let (workspace, cx) = crate::tests::open(cx, original.clone());
    cx.simulate_resize(size(px(1188.), px(900.)));
    let editor = cx.update(|_, cx| {
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| {
            if design {
                e.editor = emulsion_core::project::ProjectEditor::new_project(
                    emulsion_core::project::ProjectKind::Design,
                    original,
                )
                .unwrap();
            }
            e.sidebar_tab = SidebarTab::Properties;
            e.sidebar_layout.flyout_open = false;
            e.set_layer_selection(vec![1], Some(1));
            e.set_tool(Tool::Hand, cx);
            cx.notify();
        });
        editor
    });
    cx.run_until_parked();
    if design {
        let at = cx.update(|window, _| {
            let toggle = window.find("design-inspector-toggle");
            assert!(toggle.visible());
            toggle.bounds().center()
        });
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
    } else {
        click_in_properties("photo-layer-details", cx);
    }
    (editor, cx)
}

fn click_catalogue_invert(cx: &mut VisualTestContext) {
    let index = Filter::catalogue()
        .iter()
        .position(|filter| matches!(filter, Filter::Invert))
        .unwrap();
    click_in_properties(("filter-add", index), cx);
}

fn press_catalogue_shortcut(keys: &str, cx: &mut VisualTestContext) {
    // Action dispatch bypasses the focus route that failed in the native app.
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn catalogue_selection_keeps_save_as_and_undo_shortcuts_in_photo_and_design(
    cx: &mut TestAppContext,
) {
    for design in [false, true] {
        let (editor, cx) = catalogue_properties(cx, design);
        for additions in 1..=2 {
            let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
            click_in_properties("smart-add", cx);
            click_catalogue_invert(cx);
            let completed = cx.update(|window, cx| {
                let e = editor.read(cx);
                assert!(e.smart.menu_for.is_none());
                assert!(!e.smart.has_pending());
                assert!(
                    e.panel_focus.is_focused(window),
                    "the disappearing catalogue must leave a live focus scope"
                );
                assert_eq!(e.editor.history.len(), additions);
                let (filters, styles, enabled) = e.requested_stack(1).unwrap();
                assert_eq!(filters.len(), additions + 1);
                assert_eq!(filters.last(), Some(&Filter::Invert));
                assert!(enabled && styles.iter().all(|style| style.enabled));
                assert_eq!(mask(&e.editor.doc), mask(&before));
                e.editor.doc.clone()
            });
            // No corrective canvas click or explicit test focus before Save As.
            press_catalogue_shortcut("ctrl-shift-s", cx);
            assert!(cx.did_prompt_for_new_path(), "Save As must reach Workspace");
            cx.simulate_new_path_selection(|_| None);
            cx.run_until_parked();
            press_catalogue_shortcut("ctrl-z", cx);
            cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, before));
            press_catalogue_shortcut("ctrl-shift-z", cx);
            cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, completed));
        }
    }
}

#[gpui_kit::test]
fn catalogue_pending_render_blocks_save_and_preserves_newer_text_focus(cx: &mut TestAppContext) {
    for design in [false, true] {
        let (editor, cx) = catalogue_properties(cx, design);
        click_in_properties("smart-add", cx);
        let (ready, release) =
            cx.update(|_, cx| editor.update(cx, |e, _| e.smart.pause_next_render()));
        click_catalogue_invert(cx);
        ready
            .try_recv()
            .expect("catalogue render held before publication");
        cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(e.smart.menu_for.is_none());
            assert!(e.panel_focus.is_focused(window));
            assert!(e.smart.has_pending());
            assert!(e.editor.history.is_empty());
        });
        press_catalogue_shortcut("ctrl-shift-s", cx);
        assert!(
            !cx.did_prompt_for_new_path(),
            "pending render still rejects Save As"
        );
        // Start real text entry after the catalogue closes but before its
        // worker completes. Completion must not redirect subsequent typing.
        press_catalogue_shortcut("f2 ctrl-a", cx);
        let input = cx.update(|window, cx| window.focused_input(cx).unwrap());
        drop(release);
        cx.run_until_parked();
        cx.update(|window, cx| {
            let focused = window.focused_input(cx);
            let e = editor.read(cx);
            assert!(!e.smart.has_pending());
            assert_eq!(e.editor.history.len(), 1);
            assert_eq!(focused.as_ref(), Some(&input));
            assert_eq!(
                e.renaming.as_ref().unwrap().1.read(cx).selected_range(),
                0..5
            );
        });
        cx.simulate_input("vgb");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(input.value(cx).as_str(), "vgb");
            assert_eq!(window.focused_input(cx).as_ref(), Some(&input));
            assert_eq!(editor.read(cx).tool, Tool::Hand);
        });
    }
}

#[gpui_kit::test]
fn catalogue_selection_leaves_later_filter_slider_keyboard_focus_alone(cx: &mut TestAppContext) {
    for design in [false, true] {
        let (editor, cx) = catalogue_properties(cx, design);
        click_in_properties("smart-add", cx);
        click_catalogue_invert(cx);
        let key = SliderKey::Filter(1, 0, "radius");
        click_in_properties(SharedString::from(format!("{key:?}")), cx);
        let (focus, radius, step) = cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(!e.panel_focus.is_focused(window));
            assert!(!e.smart.has_pending());
            let spec = e.requested_stack(1).unwrap().0[0].params().remove(0);
            (window.focused(cx), spec.value, spec.step)
        });
        press_catalogue_shortcut("right", cx);
        cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(!e.smart.has_pending());
            assert_eq!(window.focused(cx), focus);
            let (filters, styles, enabled) = e.requested_stack(1).unwrap();
            let Filter::GaussianBlur { radius: actual } = filters[0] else {
                panic!("the keyboard step must keep the original filter");
            };
            assert!((actual - radius - step).abs() < 0.0001);
            assert_eq!(filters[1], Filter::Invert);
            assert!(enabled && styles.iter().all(|style| style.enabled));
        });
    }
}

#[gpui_kit::test]
fn filter_slider_duplicate_keeps_copy_state_and_workspace_shortcuts(cx: &mut TestAppContext) {
    for design in [false, true] {
        for (root_enabled, item_enabled) in [(true, false), (false, true)] {
            let (editor, cx) = catalogue_properties(cx, design);
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    e.set_filters_enabled(1, root_enabled, cx);
                    e.set_filter_enabled(1, 0, item_enabled, cx);
                    e.set_filter_style(1, 0, None, Some(0.), cx);
                });
            });
            cx.run_until_parked();
            let key = SliderKey::Filter(1, 0, "radius");
            click_in_properties(SharedString::from(format!("{key:?}")), cx);
            press_catalogue_shortcut("right", cx);
            let (before, history) = cx.update(|window, cx| {
                let e = editor.read(cx);
                assert!(!e.panel_focus.is_focused(window));
                assert!(!e.smart.has_pending());
                (e.editor.doc.clone(), e.editor.history.len())
            });
            press_catalogue_shortcut("ctrl-j", cx);
            let copied = cx.update(|window, cx| {
                let e = editor.read(cx);
                assert!(e.panel_focus.is_focused(window));
                assert!(!e.smart.has_pending());
                assert_eq!(e.editor.history.len(), history + 1);
                assert_eq!(e.editor.doc.nodes.len(), 2);
                let copy_id = e.selected.unwrap();
                assert_ne!(copy_id, 1);
                assert_eq!(e.editor.doc.node(1), before.node(1));
                let copy = e.editor.doc.node(copy_id).unwrap();
                let mut expected = before.node(1).unwrap().clone();
                expected.id = copy_id;
                expected.name = format!("{} copy", expected.name);
                assert_eq!(copy, &expected, "all independent masks and flags copy");
                let (
                    NodeKind::Smart { source, cache, .. },
                    NodeKind::Smart {
                        source: original_source,
                        cache: original_cache,
                        ..
                    },
                ) = (&copy.kind, &expected.kind)
                else {
                    panic!("duplication must retain the Smart source");
                };
                assert!(Arc::ptr_eq(source, original_source));
                assert!(Arc::ptr_eq(cache, original_cache));
                let (_, styles, enabled) = e.requested_stack(copy_id).unwrap();
                assert_eq!(enabled, root_enabled);
                assert_eq!(styles[0].enabled, item_enabled);
                assert_eq!(styles[0].opacity, 0.);
                e.editor.doc.clone()
            });
            press_catalogue_shortcut("ctrl-shift-s", cx);
            assert!(cx.did_prompt_for_new_path(), "Save As must follow Ctrl+J");
            cx.simulate_new_path_selection(|_| None);
            cx.run_until_parked();
            press_catalogue_shortcut("ctrl-z", cx);
            cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, before));
            press_catalogue_shortcut("ctrl-shift-z", cx);
            cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, copied));
        }
    }
}

#[gpui_kit::test]
fn filter_slider_rejected_duplicate_hands_off_focus_without_document_changes(
    cx: &mut TestAppContext,
) {
    for design in [false, true] {
        let (editor, cx) = catalogue_properties(cx, design);
        let key = SliderKey::Filter(1, 0, "radius");
        let slider_id = SharedString::from(format!("{key:?}"));
        click_in_properties(slider_id.clone(), cx);
        // Lock the target while preserving the currently focused slider so
        // the propagated Workspace action reaches the real rejection guard.
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.execute(
                    Command::SetLocked {
                        id: 1,
                        locked: true,
                    },
                    cx,
                );
                e.status = None;
            });
        });
        cx.run_until_parked();
        let (before, revision, history) = cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(!e.panel_focus.is_focused(window));
            assert!(!e.smart.has_pending());
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.history.len(),
            )
        });
        press_catalogue_shortcut("ctrl-j", cx);
        cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(e.panel_focus.is_focused(window));
            assert_eq!(e.selected, Some(1));
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.history.len(), history);
            assert_eq!(
                e.status,
                Some((
                    emulsion_core::CommandError::Locked(1).to_string().into(),
                    true
                )),
                "the propagated action must still reach the locked-layer guard"
            );
            assert!(!e.smart.has_pending());
        });
        press_catalogue_shortcut("ctrl-shift-s", cx);
        assert!(
            cx.did_prompt_for_new_path(),
            "a rejected duplicate keeps Save As reachable"
        );
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
        // The handoff is synchronous: later interaction with the remaining
        // slider owns focus, even though its locked parameter cannot change.
        click_in_properties(slider_id, cx);
        let focus = cx.update(|window, cx| {
            assert!(!editor.read(cx).panel_focus.is_focused(window));
            window.focused(cx)
        });
        press_catalogue_shortcut("right", cx);
        cx.update(|window, cx| {
            assert_eq!(window.focused(cx), focus);
            assert_eq!(editor.read(cx).editor.doc, before);
            assert!(!editor.read(cx).smart.has_pending());
        });
    }
}

#[gpui_kit::test]
fn queued_toggle_style_and_parameter_intents_publish_together_and_stale_older_jobs(
    cx: &mut TestAppContext,
) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let ticket = e.begin_edit_job().unwrap();
            let revision = e.editor.revision;
            e.set_filter_param(1, 0, "radius", 5., true, cx);
            e.set_filters_enabled(1, false, cx);
            e.set_filter_enabled(1, 0, false, cx);
            e.set_filter_style(1, 0, None, Some(0.25), cx);
            e.set_filters_enabled(1, true, cx);
            assert_eq!(e.editor.revision, revision);
            assert_ne!(e.edit_ticket(), ticket);
            assert!(!e.accept_edit_result(ticket, "older source job", cx));
            let (filters, styles, enabled) = e.requested_stack(1).unwrap();
            assert_eq!(filters, vec![Filter::GaussianBlur { radius: 5. }]);
            assert!(enabled && !styles[0].enabled);
            assert_eq!(styles[0].opacity, 0.25);
            assert!(e.smart.has_pending());
            assert!(!e.prepare_native_save(cx));
            assert!(e.smart_source_ready().is_err());
            assert!(!e.prepare_page_action(cx));
            assert!(
                e.smart.has_pending(),
                "refusing another operation preserves queued intent"
            );
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert!(!e.smart.has_pending());
            assert_eq!(e.editor.history.len(), 1);
            let NodeKind::Smart {
                source,
                cache,
                filters_enabled,
                filter_styles,
                filters,
                offset,
                ..
            } = &e.editor.doc.nodes[0].kind
            else {
                panic!()
            };
            assert!(*filters_enabled && !filter_styles[0].enabled);
            assert_eq!(filter_styles[0].opacity, 0.25);
            assert_eq!(filters, &[Filter::GaussianBlur { radius: 5. }]);
            assert!(Arc::ptr_eq(source, cache));
            assert_eq!(*offset, (0, 0));
            assert_eq!(mask(&e.editor.doc), mask(&original));
            e.undo(cx);
            exact_cache(&e.editor.doc, &original);
        })
    });
}

#[gpui_kit::test]
fn throttled_parameter_is_merged_before_remove_reindexes_items(cx: &mut TestAppContext) {
    let mut original = document();
    Command::SetFilters {
        id: 1,
        filters: vec![
            Filter::GaussianBlur { radius: 2. },
            Filter::BoxBlur { radius: 3. },
        ],
    }
    .apply(&mut original)
    .unwrap();
    let (editor, cx) = setup(cx, original);
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_filter_param(1, 1, "radius", 4., true, cx);
            e.set_filter_param(1, 1, "radius", 7., false, cx);
            e.remove_filter(1, 0, cx);
            e.set_filters_enabled(1, false, cx);
            e.flush_filter_param(cx);
            let (filters, _, enabled) = e.requested_stack(1).unwrap();
            assert_eq!(filters, vec![Filter::BoxBlur { radius: 7. }]);
            assert!(!enabled);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| { let e = editor.read(cx); assert!(matches!(&e.editor.doc.nodes[0].kind, NodeKind::Smart { filters, filters_enabled:false, .. } if filters == &[Filter::BoxBlur { radius:7. }])); });
}

#[gpui_kit::test]
fn later_generic_job_stales_filter_but_independent_filter_queues_both_survive(
    cx: &mut TestAppContext,
) {
    let mut original = document();
    let mut second = original.nodes[0].clone();
    second.id = 2;
    second.name = "Other".into();
    original.nodes.push(second);
    original.next_id = 3;
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_filters_enabled(1, false, cx);
            let newer = e.begin_edit_job().unwrap();
            assert_eq!(e.edit_ticket(), newer);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            exact_cache(&e.editor.doc, &original);
            assert!(!e.smart.has_pending());
            e.pending_edit_job = None;
            e.set_filters_enabled(1, false, cx);
            e.set_filters_enabled(2, false, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.history.len(), 2);
        for node in &e.editor.doc.nodes {
            assert!(matches!(
                node.kind,
                NodeKind::Smart {
                    filters_enabled: false,
                    ..
                }
            ));
        }
    });
}

#[gpui_kit::test]
fn same_state_toggles_preserve_redo_and_escape_retires_unpublished_state(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.execute(
                Command::SetFiltersEnabled {
                    id: 1,
                    enabled: false,
                },
                cx,
            );
            e.undo(cx);
            let ticket = e.edit_ticket();
            let revision = e.editor.revision;
            e.set_filters_enabled(1, true, cx);
            e.set_filter_enabled(1, 0, true, cx);
            assert_eq!(e.edit_ticket(), ticket);
            assert_eq!(e.editor.revision, revision);
            assert!(e.editor.history.can_redo());
            e.set_filters_enabled(1, false, cx);
            assert!(e.smart.has_pending());
        })
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        exact_cache(&e.editor.doc, &original);
        assert!(!e.smart.has_pending());
        assert!(e.editor.history.can_redo());
    });
}

#[gpui_kit::test]
fn disabled_stack_filter_slider_cancel_and_finish_keep_item_flags(cx: &mut TestAppContext) {
    let mut original = document();
    Command::SetFiltersEnabled {
        id: 1,
        enabled: false,
    }
    .apply(&mut original)
    .unwrap();
    Command::SetFilterStyles {
        id: 1,
        styles: vec![FilterStyle {
            enabled: false,
            opacity: 0.4,
            ..Default::default()
        }],
    }
    .apply(&mut original)
    .unwrap();
    let (editor, cx) = setup(cx, original.clone());
    for cancel in [true, false] {
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                let key = SliderKey::Filter(1, 0, "radius");
                e.tracks.entry(key).or_default().set(Some(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(100.), px(24.)),
                )));
                e.slider_down(
                    key,
                    (0., 10., 1.),
                    &MouseDownEvent {
                        position: point(px(60.), px(12.)),
                        button: MouseButton::Left,
                        modifiers: Modifiers::none(),
                        click_count: 1,
                        first_mouse: false,
                    },
                    cx,
                );
                assert!(e.editor.in_transaction());
                if cancel {
                    assert!(e.cancel_filter_edits(cx));
                } else {
                    e.drag_end(cx);
                }
            })
        });
        cx.run_until_parked();
        cx.update(|_,cx|editor.update(cx,|e,cx| {
            assert!(!e.editor.in_transaction());
            if cancel { exact_cache(&e.editor.doc,&original); assert!(e.editor.history.is_empty()); }
            else {
                assert_eq!(e.editor.history.len(),1);
                assert!(matches!(&e.editor.doc.nodes[0].kind,NodeKind::Smart{filters_enabled:false,filter_styles,filters,..} if !filter_styles[0].enabled && filter_styles[0].opacity==0.4 && filters==&[Filter::GaussianBlur{radius:6.}]));
                e.undo(cx); exact_cache(&e.editor.doc,&original);
            }
        }));
    }
}

#[gpui_kit::test]
fn filter_enabled_pointer_controls_work_in_photo_and_design_properties(cx: &mut TestAppContext) {
    for design in [false, true] {
        let original = document();
        let (editor, cx) = setup(cx, original.clone());
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                if design {
                    e.editor = emulsion_core::project::ProjectEditor::new_project(
                        emulsion_core::project::ProjectKind::Design,
                        original.clone(),
                    )
                    .unwrap();
                }
                e.sidebar_tab = SidebarTab::Properties;
                e.sidebar_layout.flyout_open = false;
                e.set_layer_selection(vec![1], Some(1));
                cx.notify();
            })
        });
        cx.run_until_parked();
        if design {
            // Design mounts the shared Properties sidebar only after its
            // Layers & properties toolbar toggle opens the inspector.
            let position = cx.update(|window, _| {
                let toolbar = window.find("design-canvas-toolbar").bounds();
                let toggle = window.find("design-inspector-toggle");
                let bounds = toggle.bounds();
                assert!(toggle.visible());
                assert!(
                    bounds.top() >= toolbar.top()
                        && bounds.bottom() <= toolbar.bottom()
                        && bounds.left() >= toolbar.left()
                        && bounds.right() <= toolbar.right()
                );
                bounds.center()
            });
            cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(editor.read(cx).design_ui.inspector);
                assert!(window.find("sidebar-properties-content").visible());
                assert!(editor.read(cx).editor.history.is_empty());
            });
        } else {
            click_in_properties("photo-layer-details", cx);
        }
        click_in_properties(("smart-filters-enabled", 1_u64), cx);
        cx.update(|_, cx| {
            assert!(matches!(
                editor.read(cx).editor.doc.nodes[0].kind,
                NodeKind::Smart {
                    filters_enabled: false,
                    ..
                }
            ))
        });
        click_in_properties(("filter-enabled", 0_usize), cx);
        cx.update(|_,cx|editor.update(cx,|e,cx| {
            assert!(matches!(&e.editor.doc.nodes[0].kind,NodeKind::Smart{filters_enabled:false,filter_styles,..} if !filter_styles[0].enabled));
            assert_eq!(mask(&e.editor.doc),mask(&original)); e.undo(cx); e.undo(cx); exact_cache(&e.editor.doc,&original);
        }));
    }
}

#[gpui_kit::test]
fn final_same_as_worker_parameter_retires_older_throttled_value(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, document());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_filter_param(1, 0, "radius", 4., true, cx);
            e.set_filter_param(1, 0, "radius", 7., false, cx);
            e.set_filter_param(1, 0, "radius", 4., true, cx);
            e.flush_filter_param(cx);
            assert_eq!(
                e.requested_stack(1).unwrap().0,
                vec![Filter::GaussianBlur { radius: 4. }]
            );
        })
    });
    cx.run_until_parked();
    cx.update(|_,cx|assert!(matches!(&editor.read(cx).editor.doc.nodes[0].kind,NodeKind::Smart{filters,..} if filters==&[Filter::GaussianBlur{radius:4.}])));
}

#[gpui_kit::test]
fn sibling_toggle_survives_another_nodes_filter_slider_rollback_and_final_render(
    cx: &mut TestAppContext,
) {
    let mut original = document();
    let mut sibling = original.nodes[0].clone();
    sibling.id = 2;
    sibling.name = "Sibling".into();
    original.nodes.push(sibling);
    original.next_id = 3;
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_filters_enabled(2, false, cx);
            let key = SliderKey::Filter(1, 0, "radius");
            e.tracks.entry(key).or_default().set(Some(Bounds::new(
                point(px(0.), px(0.)),
                size(px(100.), px(24.)),
            )));
            e.slider_down(
                key,
                (0., 10., 1.),
                &MouseDownEvent {
                    position: point(px(40.), px(12.)),
                    button: MouseButton::Left,
                    modifiers: Modifiers::none(),
                    click_count: 1,
                    first_mouse: false,
                },
                cx,
            );
            e.set_filter_param(1, 0, "radius", 6., true, cx);
            e.drag_end(cx);
            assert!(!e.editor.in_transaction());
            assert!(e.smart.has_pending());
        })
    });
    cx.run_until_parked();
    cx.update(|_,cx|editor.update(cx,|e,cx| {
        assert!(!e.smart.has_pending()); assert_eq!(e.editor.history.len(),2);
        assert!(matches!(&e.editor.doc.node(1).unwrap().kind,NodeKind::Smart{filters,filters_enabled:true,..} if filters==&[Filter::GaussianBlur{radius:6.}]));
        assert!(matches!(&e.editor.doc.node(2).unwrap().kind,NodeKind::Smart{filters_enabled:false,source,cache,offset,..} if Arc::ptr_eq(source,cache)&&*offset==(0,0)));
        e.undo(cx); e.undo(cx); exact_cache(&e.editor.doc,&original);
        let (NodeKind::Smart{cache:a,..},NodeKind::Smart{cache:b,..})=(&e.editor.doc.node(2).unwrap().kind,&original.node(2).unwrap().kind) else {panic!()};
        assert!(Arc::ptr_eq(a,b));
    }));
}
