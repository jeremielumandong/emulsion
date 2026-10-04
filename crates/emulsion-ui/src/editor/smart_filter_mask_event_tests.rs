//! Real event coverage for interrupted Smart filter-mask gestures and native
//! Save. These tests are authored source until the serial integration gate runs.
use super::smart_filter_mask_tests::{
    document, drag, mask, setup, unchanged_source_and_layer_masks,
};
use super::*;
use crate::workspace::Workspace;
use ::core::prelude::v1::test;
use emulsion_core::{MaskProperties, SmartFilterMask};
use emulsion_raster::{Mask, paint::Brush};
use gpui_kit::{InputEvent, TestAppContext, VisualTestContext, test::TestWindowExt};

fn open_workspace(
    cx: &mut TestAppContext,
    doc: Document,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (workspace, cx) = crate::tests::open(cx, doc);
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|window, cx| {
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| {
            e.select_smart_filter_mask(1, cx);
            e.tools.quick_shape = false;
            e.tools.brush = Brush {
                size: 8.,
                hardness: 1.,
                stabilizer: 0.,
                taper_end: 0.,
                ..Default::default()
            };
            e.set_fg([0, 0, 0, 255], cx);
            window.focus(&e.canvas_focus, cx);
        });
        editor
    });
    cx.run_until_parked();
    (workspace, editor, cx)
}
fn equal_payload(a: &SmartFilterMask, b: &SmartFilterMask) {
    assert_eq!(
        (a.enabled, a.linked, a.transform, a.properties),
        (b.enabled, b.linked, b.transform, b.properties)
    );
    assert_eq!(
        (a.pixels.width(), a.pixels.height(), a.pixels.fill()),
        (b.pixels.width(), b.pixels.height(), b.pixels.fill())
    );
    assert_eq!(
        a.pixels.read_rect(a.pixels.bounds()),
        b.pixels.read_rect(b.pixels.bounds())
    );
}
fn stroke_events(window: &mut Window, cx: &mut App, a: Point<Pixels>, b: Point<Pixels>) {
    window.dispatch_event(
        MouseDownEvent {
            position: a,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    for i in 1..=8 {
        window.dispatch_event(
            MouseMoveEvent {
                position: a + (b - a) * (i as f32 / 8.),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::none(),
            }
            .to_platform_input(),
            cx,
        );
    }
}

#[gpui_kit::test]
fn quick_mask_smudge_after_filter_target_changes_selection_only(cx: &mut TestAppContext) {
    for only_filter_mask in [false, true] {
        let mut original = document();
        if only_filter_mask {
            original.nodes[0].mask = None;
            original.nodes[0].vector_mask = None;
        }
        original.selection = Some(Arc::new(Mask::from_fn(160, 120, 0, |x, _| {
            if x < 75 { 255 } else { 0 }
        })));
        let (editor, cx) = setup(cx, original.clone());
        cx.simulate_keystrokes("q");
        cx.dispatch_action(crate::actions::ToolSmudge);
        cx.update(|_, cx| {
            editor.update(cx, |e, _| {
                assert!(e.tools.quick_mask);
                assert_eq!(e.tools.mask_edit_target, MaskEditTarget::SmartFilterMask);
                assert_eq!(e.tools.paint, PaintKind::Smudge);
                e.tools.brush = Brush {
                    size: 24.,
                    hardness: 1.,
                    opacity: 1.,
                    flow: 1.,
                    stabilizer: 0.,
                    taper_end: 0.,
                    ..Default::default()
                };
            })
        });
        drag(&editor, cx, (60., 55.), (100., 55.));
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_ne!(
                e.editor.doc.selection.as_ref().unwrap().to_gray8(),
                original.selection.as_ref().unwrap().to_gray8()
            );
            assert_eq!(
                e.editor.doc.nodes, original.nodes,
                "Quick Mask never changes any node component"
            );
            assert_eq!(e.editor.history.len(), 1);
            assert!(!e.editor.in_transaction());
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
        cx.simulate_keystrokes("ctrl-shift-z");
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.doc.nodes, original.nodes);
            assert_eq!(e.editor.history.len(), 1);
        });
    }
}

#[gpui_kit::test]
fn save_key_flushes_pending_cpu_mask_samples_finalizes_one_undo_and_reopens(
    cx: &mut TestAppContext,
) {
    let mut original = document();
    if let NodeKind::Smart {
        filter_mask: Some(m),
        ..
    } = &mut original.nodes[0].kind
    {
        m.enabled = false;
        m.linked = false;
        m.properties = MaskProperties {
            density: 0.37,
            feather: 1.5,
        };
    }
    let (workspace, editor, cx) = open_workspace(cx, original.clone());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("mask.ora");
    emulsion_io::save(&original, &path).unwrap();
    cx.update(|_, cx| {
        workspace.update(cx, |w, _| {
            w.home_state.projects.catalog_root = Some(folder.path().join("catalog"))
        });
        editor.update(cx, |e, _| {
            e.editor.path = Some(path.clone());
        });
    });
    let (a, b) = cx.update(|_, cx| {
        let e = editor.read(cx);
        (
            e.doc_to_window((46., 48.)).unwrap(),
            e.doc_to_window((80., 48.)).unwrap(),
        )
    });
    cx.update(|window, cx| {
        stroke_events(window, cx, a, b);
        let e = editor.read(cx);
        assert!(
            e.tools.stroke_preview_pending,
            "test must enter Save before the frame flush"
        );
        assert!(e.editor.in_transaction());
        window.dispatch_keystroke(Keystroke::parse("ctrl-s").unwrap(), cx);
    });
    cx.run_until_parked();
    let saved = cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.drag.is_none());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 1);
        assert!(!e.editor.is_modified());
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
        assert_eq!(mask(&e.editor.doc).properties, mask(&original).properties);
        assert!(!mask(&e.editor.doc).enabled && !mask(&e.editor.doc).linked);
        e.editor.doc.clone()
    });
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).editor.doc, saved);
        assert_eq!(editor.read(cx).editor.history.len(), 1);
    });
    let reopened = emulsion_io::open_full(&path).unwrap();
    assert!(reopened.history_error.is_none());
    assert!(reopened.graph.is_some());
    equal_payload(mask(&reopened.doc), mask(&saved));
    // Ordinary Undo is live-session history. Native graph persistence does not
    // currently serialize the ordinary Undo stack across a fresh Editor.
    cx.simulate_keystrokes("ctrl-z");
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
    cx.simulate_keystrokes("ctrl-shift-z");
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, saved));
}

#[gpui_kit::test]
fn save_rejects_live_property_slider_without_committing_or_replacing_file(cx: &mut TestAppContext) {
    let original = document();
    let (_workspace, editor, cx) = open_workspace(cx, original.clone());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("existing.ora");
    std::fs::write(&path, b"existing file remains unchanged").unwrap();
    cx.update(|_, cx| editor.update(cx, |e, _| e.editor.path = Some(path.clone())));
    let key = SliderKey::MaskProperty(
        1,
        MaskEditTarget::SmartFilterMask,
        photo_masks::MaskProperty::Density,
        photo_masks::MaskControlSurface::Taskbar,
    );
    let bounds =
        cx.update(|window, _| window.find(SharedString::from(format!("{key:?}"))).bounds());
    let a = point(bounds.left() + bounds.size.width * 0.3, bounds.center().y);
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.simulate_keystrokes("ctrl-s");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.drag.is_some());
        assert!(e.editor.in_transaction());
        assert!(e.editor.history.is_empty());
        assert!(
            e.status
                .as_ref()
                .is_some_and(|s| s.0.contains("before saving"))
        );
    });
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"existing file remains unchanged"
    );
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(a, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn save_as_callback_rejects_new_held_stroke_and_keeps_existing_destination(
    cx: &mut TestAppContext,
) {
    let original = document();
    let (_workspace, editor, cx) = open_workspace(cx, original.clone());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("chosen.ora");
    std::fs::write(&path, b"do not overwrite").unwrap();
    cx.dispatch_action(crate::actions::SaveAs);
    assert!(cx.did_prompt_for_new_path());
    let (a, b) = cx.update(|_, cx| {
        let e = editor.read(cx);
        (
            e.doc_to_window((46., 48.)).unwrap(),
            e.doc_to_window((78., 48.)).unwrap(),
        )
    });
    cx.update(|window, cx| stroke_events(window, cx, a, b));
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read(&path).unwrap(), b"do not overwrite");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.drag.is_some());
        assert!(e.editor.in_transaction());
        assert!(e.editor.path.is_none());
    });
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.history.len(), 1);
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
    });
}

#[gpui_kit::test]
fn save_as_cancel_and_changed_active_document_do_not_write(cx: &mut TestAppContext) {
    let (workspace, editor, cx) = open_workspace(cx, document());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("unchanged.ora");
    std::fs::write(&path, b"original bytes").unwrap();
    cx.dispatch_action(crate::actions::SaveAs);
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert_eq!(std::fs::read(&path).unwrap(), b"original bytes");
    cx.dispatch_action(crate::actions::SaveAs);
    assert!(cx.did_prompt_for_new_path());
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.install(
                Document::new(24, 18),
                None,
                None,
                None,
                "New".into(),
                window,
                cx,
            );
        })
    });
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read(&path).unwrap(), b"original bytes");
    cx.update(|_, cx| {
        assert!(editor.read(cx).editor.path.is_none());
        let current = workspace.read(cx).editor.as_ref().unwrap().read(cx);
        assert!(current.editor.doc.nodes.is_empty());
        assert!(current.editor.path.is_none());
    });
}

#[gpui_kit::test]
fn delete_keys_consume_filter_mask_on_canvas_and_panel_but_explicit_delete_layer_works(
    cx: &mut TestAppContext,
) {
    for panel in [false, true] {
        for filter_only in [false, true] {
            let mut original = document();
            if filter_only {
                original.nodes[0].mask = None;
                original.nodes[0].vector_mask = None;
            }
            let (editor, cx) = setup(cx, original.clone());
            cx.update(|window, cx| {
                let focus = {
                    let e = editor.read(cx);
                    if panel {
                        e.panel_focus.clone()
                    } else {
                        e.canvas_focus.clone()
                    }
                };
                window.focus(&focus, cx);
            });
            cx.simulate_keystrokes("delete backspace delete");
            cx.update(|_, cx| {
                let e = editor.read(cx);
                assert_eq!(e.editor.doc, original);
                assert!(e.editor.history.is_empty());
            });
            cx.dispatch_action(crate::actions::DeleteNode);
            cx.update(|_, cx| assert!(editor.read(cx).editor.doc.nodes.is_empty()));
            cx.simulate_keystrokes("ctrl-z");
            cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
        }
    }
}

#[gpui_kit::test]
fn actual_density_and_feather_tracks_group_one_undo_and_escape_without_wrong_target(
    cx: &mut TestAppContext,
) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    for property in [
        photo_masks::MaskProperty::Density,
        photo_masks::MaskProperty::Feather,
    ] {
        let key = SliderKey::MaskProperty(
            1,
            MaskEditTarget::SmartFilterMask,
            property,
            photo_masks::MaskControlSurface::Taskbar,
        );
        let bounds =
            cx.update(|window, _| window.find(SharedString::from(format!("{key:?}"))).bounds());
        let (from, to) = if property == photo_masks::MaskProperty::Density {
            (0.25, 0.65)
        } else {
            (0.01, 0.03)
        };
        let a = point(bounds.left() + bounds.size.width * from, bounds.center().y);
        let b = point(bounds.left() + bounds.size.width * to, bounds.center().y);
        cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert!(e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
        });
        cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.history.len(), 1);
            assert_ne!(mask(&e.editor.doc).properties, MaskProperties::default());
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
        cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_keystrokes("escape");
        cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.doc, original);
            assert!(!e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
        });
    }
    // A real thumbnail switch after the gesture changes only subsequent input.
    cx.update(|window, cx| window.click(("layer-mask", 1_u64), cx));
    cx.run_until_parked();
    let filter_before = cx.update(|_, cx| mask(&editor.read(cx).editor.doc).clone());
    drag(&editor, cx, (48., 48.), (68., 48.));
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tools.mask_edit_target, MaskEditTarget::RasterMask);
        assert_eq!(mask(&e.editor.doc), &filter_before);
        assert!(
            e.editor
                .doc
                .node(1)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .to_gray8()
                .contains(&0)
        );
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn native_free_transform_keys_cancel_commit_repeat_and_save_rejection_are_mask_only(
    cx: &mut TestAppContext,
) {
    let original = document();
    let (_workspace, editor, cx) = open_workspace(cx, original.clone());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("existing.ora");
    std::fs::write(&path, b"existing file").unwrap();
    cx.update(|_, cx| editor.update(cx, |e, _| e.editor.path = Some(path.clone())));
    cx.simulate_keystrokes("ctrl-t right right");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.photo_transform_active());
        assert_ne!(mask(&e.editor.doc).transform, mask(&original).transform);
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
    });
    cx.simulate_keystrokes("ctrl-s");
    assert_eq!(std::fs::read(&path).unwrap(), b"existing file");
    cx.update(|_, cx| assert!(editor.read(cx).photo_transform_active()));
    cx.simulate_keystrokes("escape");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
    cx.simulate_keystrokes("ctrl-t right right enter");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(!e.photo_transform_active());
        assert_eq!(e.editor.history.len(), 1);
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
    });
    let once = cx.update(|_, cx| mask(&editor.read(cx).editor.doc).transform);
    cx.simulate_keystrokes("ctrl-shift-t");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_ne!(mask(&e.editor.doc).transform, once);
        assert_eq!(e.editor.history.len(), 2);
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
    });
    cx.simulate_keystrokes("ctrl-z ctrl-z");
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
}

fn click_events(window: &mut Window, cx: &mut App, position: Point<Pixels>) {
    window.dispatch_event(
        MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
}

#[gpui_kit::test]
fn pending_filter_then_real_thumbnail_change_and_ancestor_lock_never_publish_stale_result(
    cx: &mut TestAppContext,
) {
    for lock_ancestor in [false, true] {
        let mut original = document();
        original.nodes[0].parent = Some(2);
        // A group's contiguous descendants precede it in native paint order.
        original.nodes.push(Node::group(2, "Folder"));
        original.next_id = 3;
        original.validate().unwrap();
        let (editor, cx) = setup(cx, original.clone());
        // Row lock toggles expose debug selectors rather than test_support IDs.
        // Resolve their real rendered bounds before queueing the filter worker.
        cx.update(|window, cx| window.render_frame(cx));
        let position = if lock_ancestor {
            cx.debug_bounds("layer-lock-2")
                .expect("ancestor lock toggle is rendered")
                .center()
        } else {
            cx.update(|window, _| window.find(("layer-mask", 1_u64)).bounds().center())
        };
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| {
                e.add_filter(1, emulsion_filters::Filter::GaussianBlur { radius: 5. }, cx)
            });
            assert!(editor.read(cx).smart.has_pending());
            // Schedule the worker, then deliver actual hit-tested pointer events
            // before the executor can publish the queued result.
            click_events(window, cx, position);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.doc.node(1), original.node(1));
            assert!(!e.smart.has_pending());
            if lock_ancestor {
                assert!(e.editor.doc.node(2).unwrap().locked);
                assert_eq!(e.editor.doc.locked_ancestor(1), Some(2));
                assert_eq!(e.editor.history.len(), 1);
            } else {
                assert_eq!(e.tools.mask_edit_target, MaskEditTarget::RasterMask);
                assert!(e.editor.history.is_empty());
            }
        });
        if lock_ancestor {
            cx.simulate_keystrokes("ctrl-z ctrl-shift-z");
            cx.update(|_, cx| {
                let e = editor.read(cx);
                assert!(e.editor.doc.node(2).unwrap().locked);
                assert_eq!(e.editor.doc.node(1), original.node(1));
                assert_eq!(e.editor.history.len(), 1);
            });
        }
    }
}

#[gpui_kit::test]
fn queued_filter_during_new_document_dialog_close_or_create_stays_with_original_entity(
    cx: &mut TestAppContext,
) {
    for create in [false, true] {
        let original = document();
        let (workspace, editor, cx) = open_workspace(cx, original.clone());
        cx.simulate_keystrokes("ctrl-n");
        cx.update(|window, cx| {
            window.click(
                (
                    "new-canvas-kind",
                    emulsion_core::creation::CanvasKind::Paint as usize,
                ),
                cx,
            )
        });
        cx.run_until_parked();
        for (field, value) in [("Width", "24"), ("Height", "18")] {
            cx.update(|window, cx| {
                window.click(SharedString::from(format!("new-canvas-field-{field}")), cx)
            });
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input(value);
        }
        let position = cx.update(|window, _| {
            window
                .find(if create {
                    "new-canvas-create"
                } else {
                    "new-canvas-cancel"
                })
                .bounds()
                .center()
        });
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| {
                e.add_filter(1, emulsion_filters::Filter::GaussianBlur { radius: 3. }, cx)
            });
            assert!(editor.read(cx).smart.has_pending());
            click_events(window, cx, position);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("new-canvas-form").is_none());
            let old = editor.read(cx);
            assert_eq!(mask(&old.editor.doc), mask(&original));
            assert_eq!(
                old.editor
                    .doc
                    .node(1)
                    .unwrap()
                    .mask
                    .as_ref()
                    .map(Arc::as_ptr),
                original.node(1).unwrap().mask.as_ref().map(Arc::as_ptr)
            );
            assert_eq!(
                old.editor.doc.node(1).unwrap().vector_mask,
                original.node(1).unwrap().vector_mask
            );
            if create {
                let current = workspace.read(cx).editor.as_ref().unwrap();
                assert_ne!(current, &editor);
                let fresh = current.read(cx);
                assert_eq!((fresh.editor.doc.width, fresh.editor.doc.height), (24, 18));
                assert!(
                    fresh
                        .editor
                        .doc
                        .nodes
                        .iter()
                        .all(|n| !matches!(n.kind, NodeKind::Smart { .. }))
                );
                assert!(fresh.editor.history.is_empty());
            } else {
                assert_eq!(workspace.read(cx).editor.as_ref(), Some(&editor));
            }
            assert!(!old.smart.has_pending());
        });
    }
}

#[gpui_kit::test]
fn save_key_refuses_pending_filter_worker_without_overwriting_existing_file(
    cx: &mut TestAppContext,
) {
    let original = document();
    let (_workspace, editor, cx) = open_workspace(cx, original.clone());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("existing.ora");
    std::fs::write(&path, b"pending work must not replace this file").unwrap();
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.editor.path = Some(path.clone());
            e.add_filter(1, emulsion_filters::Filter::GaussianBlur { radius: 3. }, cx);
        });
        assert!(editor.read(cx).smart.has_pending());
        window.dispatch_keystroke(Keystroke::parse("ctrl-s").unwrap(), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"pending work must not replace this file"
    );
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(!e.history.save_busy);
        assert_eq!(mask(&e.editor.doc), mask(&original));
    });
}

#[gpui_kit::test]
fn queued_filter_survives_mode_switch_only_on_its_original_smart_node(cx: &mut TestAppContext) {
    let original = document();
    let (_workspace, editor, cx) = open_workspace(cx, original.clone());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.add_filter(1, emulsion_filters::Filter::GaussianBlur { radius: 3. }, cx)
        });
        assert!(editor.read(cx).smart.has_pending());
        window.dispatch_keystroke(Keystroke::parse("ctrl-alt-shift-d").unwrap(), cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.draw_mode);
        assert_eq!(e.editor.doc.nodes.len(), 1);
        assert_eq!(mask(&e.editor.doc), mask(&original));
        let actual = e.editor.doc.node(1).unwrap();
        let before = original.node(1).unwrap();
        assert_eq!(
            actual.mask.as_ref().map(Arc::as_ptr),
            before.mask.as_ref().map(Arc::as_ptr)
        );
        assert_eq!(actual.vector_mask, before.vector_mask);
        let (
            NodeKind::Smart {
                source, filters, ..
            },
            NodeKind::Smart { source: old, .. },
        ) = (&actual.kind, &before.kind)
        else {
            panic!("Smart content")
        };
        assert!(Arc::ptr_eq(source, old));
        assert_eq!(filters.len(), 2);
        assert!(!e.smart.has_pending());
    });
}

#[gpui_kit::test]
fn stale_heal_ticket_cannot_publish_after_save_accepts_a_newer_selection_revision(
    cx: &mut TestAppContext,
) {
    let mut original = Document::new(160, 120);
    let raster = Arc::new(Raster::from_fn(48, 32, [0; 4], |x, y| {
        if (9..19).contains(&x) && (8..20).contains(&y) {
            [65535, 0, 0, 65535]
        } else {
            [0, 0, 65535, 65535]
        }
    }));
    original.nodes.push(Node::raster(
        1,
        "Photo",
        raster.clone(),
        Placement::at(40., 36.),
    ));
    original.next_id = 2;
    let (workspace, editor, cx) = open_workspace(cx, original.clone());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("healed.ora");
    cx.update(|_, cx| {
        workspace.update(cx, |w, _| {
            w.home_state.projects.catalog_root = Some(folder.path().join("catalog"))
        });
        editor.update(cx, |e, cx| {
            e.editor.path = Some(path.clone());
            e.set_tool(Tool::Heal, cx);
            e.tools.remove.enabled = false;
            e.tools.brush = Brush {
                size: 8.,
                hardness: 1.,
                stabilizer: 0.,
                taper_end: 0.,
                ..Default::default()
            };
        });
    });
    cx.run_until_parked();
    let (a, b) = cx.update(|_, cx| {
        let e = editor.read(cx);
        (
            e.doc_to_window((52., 48.)).unwrap(),
            e.doc_to_window((56., 48.)).unwrap(),
        )
    });
    cx.update(|window, cx| {
        stroke_events(window, cx, a, b);
        window.dispatch_event(
            MouseUpEvent {
                position: b,
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        editor.update(cx, |e, _| {
            let ticket = e.pending_edit_job.expect("healing worker was scheduled");
            assert!(e.edit_is_current(ticket));
            let epoch = e.operation_epoch;
            // Model-only revision change deliberately does not bump the UI
            // epoch. Save and publication must still agree about job currency.
            e.editor
                .execute(Command::SetSelection {
                    selection: Some(Arc::new(Mask::white(160, 120))),
                })
                .unwrap();
            assert_eq!(e.operation_epoch, epoch);
            assert!(!e.edit_is_current(ticket));
        });
        window.dispatch_keystroke(Keystroke::parse("ctrl-s").unwrap(), cx);
    });
    cx.run_until_parked();
    let reopened = emulsion_io::open(&path).unwrap();
    let NodeKind::Raster { raster: saved, .. } = &reopened.node(1).unwrap().kind else {
        panic!("raster source")
    };
    assert_eq!(
        saved.read_rect(saved.bounds()),
        raster.read_rect(raster.bounds())
    );
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes, original.nodes);
        assert_eq!(e.editor.history.len(), 1);
        assert!(e.pending_edit_job.is_none());
        assert!(!e.editor.in_transaction());
    });
}
