//! Modal Photo transforms own one preview baseline, including every copied ID.
//! These tests use the real editor and, for the interaction cases, scoped GPUI
//! key bindings and pointer events rather than calling action handlers directly.
use super::*;
use crate::editor::{EditorView, PaintKind, SidebarTab, Tool};
use emulsion_core::{NodeId, NodeKind};
use glam::{DAffine2, dvec2};
use gpui_kit::component::WindowExt;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton, px, size};

fn add(doc: &mut Document, node: Node, parent: Option<NodeId>) -> NodeId {
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::top_of(parent),
    }
    .apply(doc)
    .unwrap()
    .unwrap()
}

fn artwork() -> Document {
    let mut doc = Document::new(384, 256);
    add(
        &mut doc,
        Node::raster(
            0,
            "Photo",
            Arc::new(Raster::from_fn(48, 32, [0; 4], |x, y| {
                [x as u16 * 1000, y as u16 * 1500, 24000, 65535]
            })),
            Placement::at(40., 40.),
        ),
        None,
    );
    doc
}

fn setup(cx: &mut TestAppContext, doc: Document) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = open(cx, doc);
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let view = cx.update(|window, cx| {
        let view = workspace.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            e.set_tool(Tool::Move, cx);
            e.snap = false;
            window.focus(&e.canvas_focus, cx);
        });
        view
    });
    cx.run_until_parked();
    (view, cx)
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn drag(view: &Entity<EditorView>, from: (f64, f64), to: (f64, f64), cx: &mut VisualTestContext) {
    let (from, to) = cx.update(|_, cx| {
        let e = view.read(cx);
        (e.doc_to_window(from).unwrap(), e.doc_to_window(to).unwrap())
    });
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn assert_matrix(actual: DAffine2, expected: DAffine2) {
    for (actual, expected) in actual
        .to_cols_array()
        .into_iter()
        .zip(expected.to_cols_array())
    {
        assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
    }
}

fn raster_matrix(doc: &Document, id: NodeId) -> DAffine2 {
    let NodeKind::Raster { raster, placement } = &doc.node(id).unwrap().kind else {
        panic!("expected native raster placement")
    };
    placement.to_doc(raster.width(), raster.height())
}

#[gpui_kit::test]
fn photo_copy_escape_restores_baseline_after_multiple_drags_and_numeric_delta(
    cx: &mut TestAppContext,
) {
    let original = artwork();
    let source = original.nodes[0].id;
    let (view, cx) = setup(cx, original.clone());
    cx.update(|_, cx| view.update(cx, |e, cx| e.set_tool(Tool::Select, cx)));
    press("ctrl-alt-t", cx);
    let copy = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(e.photo_transform_active());
        assert_eq!(e.editor.doc.nodes.len(), 2);
        assert_eq!(e.editor.transaction_depth(), 1);
        assert!(e.editor.history.is_empty());
        assert_ne!(e.selected, Some(source));
        e.selected.unwrap()
    });
    drag(&view, (64., 56.), (77., 65.), cx);
    drag(&view, (77., 65.), (85., 70.), cx);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(
                e.photo_transform_active(),
                "mouse-up must keep the session open"
            );
            assert!(e.editor.history.is_empty());
            assert_matrix(
                raster_matrix(&e.editor.doc, copy),
                DAffine2::from_translation(dvec2(61., 54.)),
            );
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(7., -3.)), cx));
            assert_eq!(e.editor.transaction_depth(), 1);
        })
    });
    press("escape", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(
            e.editor.doc, original,
            "cancel must restore pixels, selection and the ID allocator"
        );
        assert_eq!(e.editor.doc.next_id, original.next_id);
        assert_eq!(e.selected_layer_ids(), vec![source]);
        assert_eq!(e.tool, Tool::Select);
        assert!(!e.photo_transform_active());
        assert!(!e.editor.in_transaction());
        assert!(!e.editor.is_modified());
        assert!(e.editor.history.is_empty());
        assert!(!e.editor.history.can_redo());
    });
}

#[gpui_kit::test]
fn photo_copy_enter_commits_all_gestures_as_one_undo_and_redo(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    press("ctrl-alt-t", cx);
    drag(&view, (64., 56.), (75., 61.), cx);
    drag(&view, (75., 61.), (82., 69.), cx);
    press("enter", cx);
    let committed = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.photo_transform_active());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 1);
        assert_eq!(e.editor.doc.nodes.len(), 2);
        assert_eq!(
            e.editor.doc.node(original.nodes[0].id),
            Some(&original.nodes[0])
        );
        assert_matrix(
            raster_matrix(&e.editor.doc, e.selected.unwrap()),
            DAffine2::from_translation(dvec2(58., 53.)),
        );
        e.editor.doc.clone()
    });
    press("ctrl-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    press("ctrl-shift-z", cx);
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.doc, committed);
        assert_eq!(view.read(cx).editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn photo_identity_sessions_do_not_erase_the_last_meaningful_recipe(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.commit_photo_transform(cx));
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.doc.next_id, original.next_id);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.is_modified());
            e.repeat_photo_transform(true, cx);
            assert_eq!(
                e.editor.doc, original,
                "identity must not create an Again recipe"
            );
            assert_eq!(e.editor.doc.next_id, original.next_id);
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(12., 4.)), cx));
            e.commit_photo_transform(cx);
            let transformed = e.editor.doc.clone();
            e.begin_photo_transform(false, cx);
            e.commit_photo_transform(cx);
            assert_eq!(e.editor.history.len(), 1);
            e.begin_photo_transform(true, cx);
            e.commit_photo_transform(cx);
            assert_eq!(
                e.editor.history.len(),
                2,
                "deliberate identity copy is one real edit"
            );
            assert_eq!(e.editor.doc.nodes.len(), 2);
            let copy = e.selected.unwrap();
            assert_matrix(
                raster_matrix(&e.editor.doc, copy),
                DAffine2::from_translation(dvec2(52., 44.)),
            );
            e.repeat_photo_transform(false, cx);
            assert_matrix(
                raster_matrix(&e.editor.doc, copy),
                DAffine2::from_translation(dvec2(64., 48.)),
            );
            e.undo(cx);
            e.undo(cx);
            assert_eq!(e.editor.doc, transformed);
        })
    });
}

#[gpui_kit::test]
fn photo_repeat_and_repeat_copy_use_d_to_the_n_without_resampling(cx: &mut TestAppContext) {
    let original = artwork();
    let source_id = original.nodes[0].id;
    let NodeKind::Raster { raster: source, .. } = &original.nodes[0].kind else {
        unreachable!()
    };
    let source = source.clone();
    let (view, cx) = setup(cx, original.clone());
    for delta in [
        DAffine2::from_translation(dvec2(11., -4.)),
        DAffine2::from_translation(dvec2(96., 72.))
            * DAffine2::from_angle(0.3)
            * DAffine2::from_translation(dvec2(-96., -72.)),
        DAffine2::from_translation(dvec2(5., 3.)) * DAffine2::from_scale(dvec2(1.2, 1.2)),
        DAffine2::from_translation(dvec2(160., 0.)) * DAffine2::from_scale(dvec2(-1., 1.)),
    ] {
        cx.update(|_, cx| {
            view.update(cx, |e, cx| {
                // Undo history is exercised below before the next independent recipe.
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.doc.next_id, original.next_id);
                e.set_layer_selection(vec![source_id], Some(source_id));
                e.begin_photo_transform(false, cx);
                assert!(e.photo_transform_delta(delta, cx));
                e.commit_photo_transform(cx);
                let mut expected = delta * raster_matrix(&original, source_id);
                for n in 2..=6 {
                    let before = e.editor.doc.clone();
                    let previous = e.selected.unwrap();
                    let copy = n != 2;
                    e.repeat_photo_transform(copy, cx);
                    expected = delta * expected;
                    let selected = e.selected.unwrap();
                    assert_matrix(raster_matrix(&e.editor.doc, selected), expected);
                    assert_eq!(e.editor.history.len(), n);
                    assert_eq!(
                        e.editor.doc.nodes.len(),
                        if copy {
                            before.nodes.len() + 1
                        } else {
                            before.nodes.len()
                        }
                    );
                    if copy {
                        assert_ne!(selected, previous);
                        for node in &before.nodes {
                            assert_eq!(
                                e.editor.doc.node(node.id),
                                Some(node),
                                "earlier copies are immutable"
                            );
                        }
                    }
                    for node in &e.editor.doc.nodes {
                        let NodeKind::Raster { raster, .. } = &node.kind else {
                            panic!("native raster expected")
                        };
                        assert!(
                            Arc::ptr_eq(raster, &source),
                            "repeat must never resample pixels"
                        );
                    }
                }
                for _ in 0..6 {
                    e.undo(cx);
                }
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.doc.next_id, original.next_id);
            })
        });
    }
}

#[gpui_kit::test]
fn photo_group_copy_preserves_editable_text_path_smart_source_and_copy_internal_links(
    cx: &mut TestAppContext,
) {
    let mut original = artwork();
    let group = add(&mut original, Node::group(0, "Editable group"), None);
    let text = add(
        &mut original,
        Node::text(
            0,
            "Type",
            emulsion_core::text::TextSpec {
                text: "Editable".into(),
                x: 80.,
                y: 90.,
                size: 16.,
                ..Default::default()
            },
            384,
            256,
        ),
        Some(group),
    );
    let path = add(
        &mut original,
        Node::path(
            0,
            "Path",
            Arc::new(emulsion_raster::vector::Path::from_svg("M 20 20 L 60 20 L 40 50 Z").unwrap()),
            Default::default(),
            384,
            256,
        ),
        Some(group),
    );
    let smart_source = Arc::new(Raster::solid(12, 10, [0.2, 0.7, 0.4, 1.]));
    let smart = add(
        &mut original,
        Node::smart(
            0,
            "Smart",
            smart_source.clone(),
            Vec::new(),
            Placement::at(110., 60.),
        ),
        Some(group),
    );
    Command::SetLayerLinks {
        ids: vec![text, path],
        linked: true,
    }
    .apply(&mut original)
    .unwrap();
    original.node_mut(smart).unwrap().clip_to = Some(path);
    let (view, cx) = setup(cx, original.clone());
    let delta = DAffine2::from_translation(dvec2(8., 5.))
        * DAffine2::from_angle(0.2)
        * DAffine2::from_scale(dvec2(1.1, 1.1));
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![group], Some(group));
            e.begin_photo_transform(true, cx);
            assert!(e.photo_transform_delta(delta, cx));
            e.commit_photo_transform(cx);
            let mut power = DAffine2::IDENTITY;
            for n in 1..=3 {
                if n > 1 {
                    e.repeat_photo_transform(true, cx);
                }
                power = delta * power;
                let copied_group = e.selected.unwrap();
                assert_ne!(copied_group, group);
                let copied = e.editor.doc.subtree(copied_group);
                assert_eq!(copied.len(), 4);
                let children: Vec<_> = copied
                    .iter()
                    .filter_map(|id| e.editor.doc.node(*id))
                    .filter(|node| node.id != copied_group)
                    .collect();
                let copied_text = children
                    .iter()
                    .find(|node| matches!(node.kind, NodeKind::Text { .. }))
                    .unwrap();
                let copied_path = children
                    .iter()
                    .find(|node| matches!(node.kind, NodeKind::Path { .. }))
                    .unwrap();
                let copied_smart = children
                    .iter()
                    .find(|node| matches!(node.kind, NodeKind::Smart { .. }))
                    .unwrap();
                let NodeKind::Text { spec, .. } = &copied_text.kind else {
                    unreachable!()
                };
                let NodeKind::Text {
                    spec: original_spec,
                    ..
                } = &original.node(text).unwrap().kind
                else {
                    unreachable!()
                };
                assert_eq!(spec.text, original_spec.text);
                assert_eq!(spec.size, original_spec.size);
                assert_matrix(spec.transform(), power * original_spec.transform());
                let NodeKind::Path { path: actual, .. } = &copied_path.kind else {
                    unreachable!()
                };
                let NodeKind::Path {
                    path: original_path,
                    ..
                } = &original.node(path).unwrap().kind
                else {
                    unreachable!()
                };
                for (actual, source) in actual.subpaths[0]
                    .anchors
                    .iter()
                    .zip(&original_path.subpaths[0].anchors)
                {
                    let expected = power.transform_point2(dvec2(source.p.0, source.p.1));
                    assert!((actual.p.0 - expected.x).abs() < 0.001);
                    assert!((actual.p.1 - expected.y).abs() < 0.001);
                }
                let NodeKind::Smart {
                    source,
                    placement,
                    filters,
                    ..
                } = &copied_smart.kind
                else {
                    unreachable!()
                };
                assert!(Arc::ptr_eq(source, &smart_source));
                assert!(filters.is_empty());
                assert_eq!(
                    copied_smart.clip_to,
                    Some(copied_path.id),
                    "internal clipping must point to the copied sibling"
                );
                assert_matrix(
                    placement.to_doc(12, 10),
                    power
                        * emulsion_core::transform::local_to_document(
                            original.node(smart).unwrap(),
                        ),
                );
                assert!(copied_text.link_group.is_some());
                assert_eq!(copied_text.link_group, copied_path.link_group);
                assert_ne!(
                    copied_text.link_group,
                    original.node(text).unwrap().link_group
                );
                assert_eq!(e.editor.history.len(), n);
                for node in &original.nodes {
                    assert_eq!(e.editor.doc.node(node.id), Some(node));
                }
            }
            for _ in 0..3 {
                e.undo(cx);
            }
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.doc.next_id, original.next_id);
        })
    });
}

#[gpui_kit::test]
fn photo_selected_pixels_cut_or_copy_cancel_and_commit_as_one_step(cx: &mut TestAppContext) {
    let mut original = artwork();
    original.selection = Some(Arc::new(emulsion_raster::select::rect(
        384, 256, 45., 44., 18., 12.,
    )));
    let source_id = original.nodes[0].id;
    let (view, cx) = setup(cx, original.clone());
    for copy in [false, true] {
        cx.update(|_, cx| {
            view.update(cx, |e, cx| {
                e.set_layer_selection(vec![source_id], Some(source_id));
                e.begin_photo_transform(copy, cx);
                assert!(e.photo_transform_active());
                assert_eq!(e.editor.doc.nodes.len(), 2);
                assert!(e.editor.doc.selection.is_none());
                assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(10., 3.)), cx));
                e.cancel_photo_transform(cx);
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.doc.next_id, original.next_id);
                assert_eq!(e.selected, Some(source_id));
                assert!(e.editor.history.is_empty());
                assert!(!e.editor.is_modified());
                e.begin_photo_transform(copy, cx);
                assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(10., 3.)), cx));
                e.commit_photo_transform(cx);
                assert_eq!(e.editor.history.len(), 1);
                if copy {
                    assert_eq!(e.editor.doc.node(source_id), original.node(source_id));
                } else {
                    assert_ne!(
                        e.editor.doc.node(source_id),
                        original.node(source_id),
                        "cut must clear the selected source pixels"
                    );
                }
                e.undo(cx);
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.doc.next_id, original.next_id);
            })
        });
    }
}

#[gpui_kit::test]
fn photo_rejected_targets_leave_selection_allocator_and_history_unchanged(cx: &mut TestAppContext) {
    let mut original = artwork();
    let group = add(&mut original, Node::group(0, "Group"), None);
    let child = add(
        &mut original,
        Node::raster(
            0,
            "Child",
            Arc::new(Raster::solid(8, 8, [1.; 4])),
            Placement::at(80., 50.),
        ),
        Some(group),
    );
    let (view, cx) = setup(cx, original);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![group], Some(group));
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(3., 2.)), cx));
            e.commit_photo_transform(cx);
            for case in 0..5 {
                e.editor.doc.node_mut(group).unwrap().locked = case == 1 || case == 3;
                e.editor.doc.node_mut(child).unwrap().locked = case == 2;
                e.editor.doc.node_mut(child).unwrap().locks.position = case == 4;
                let selected = if case == 0 {
                    None
                } else if case == 3 {
                    Some(child)
                } else {
                    Some(group)
                };
                e.set_layer_selection(selected.into_iter().collect(), selected);
                let before = e.editor.doc.clone();
                let revision = e.editor.revision;
                let history = e.editor.history.len();
                let selection = e.selected_layer_ids();
                for copy in [false, true] {
                    e.begin_photo_transform(copy, cx);
                    assert!(!e.photo_transform_active(), "invalid target case {case}");
                    e.repeat_photo_transform(copy, cx);
                    assert_eq!(
                        e.editor.doc, before,
                        "invalid target case {case}, copy {copy}"
                    );
                    assert_eq!(e.editor.doc.next_id, before.next_id);
                    assert_eq!(e.editor.revision, revision);
                    assert_eq!(e.editor.history.len(), history);
                    assert_eq!(e.selected_layer_ids(), selection);
                    assert!(!e.editor.in_transaction());
                }
            }
        })
    });
}

#[gpui_kit::test]
fn photo_invalid_preview_cannot_commit_stale_valid_placement(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(true, cx);
            let delta = DAffine2::from_translation(dvec2(4., 6.));
            assert!(e.photo_transform_delta(delta, cx));
            for invalid in [
                DAffine2::from_scale(dvec2(0., 1.)),
                DAffine2::from_cols_array(&[1., 0., 0.5, 1., 0., 0.]),
                DAffine2::from_translation(dvec2(f64::NAN, 0.)),
            ] {
                let valid = e.editor.doc.clone();
                let revision = e.editor.revision;
                assert!(!e.photo_transform_delta(invalid, cx));
                assert_eq!(e.editor.doc, valid);
                assert_eq!(e.editor.doc.next_id, valid.next_id);
                assert_eq!(e.editor.revision, revision);
                e.commit_photo_transform(cx);
                assert!(
                    e.photo_transform_active(),
                    "invalid preview must remain unresolved"
                );
                assert!(e.editor.history.is_empty());
                assert!(
                    e.photo_transform_delta(DAffine2::IDENTITY, cx),
                    "valid correction should clear the invalid flag"
                );
            }
            e.cancel_photo_transform(cx);
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.doc.next_id, original.next_id);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.is_modified());
        })
    });
}

#[gpui_kit::test]
fn photo_repeat_shear_rejection_is_atomic_and_retains_the_recipe(cx: &mut TestAppContext) {
    let mut original = artwork();
    let second = add(
        &mut original,
        Node::raster(
            0,
            "Other",
            Arc::new(Raster::solid(24, 18, [1.; 4])),
            Placement::at(120., 50.),
        ),
        None,
    );
    let first = original.nodes[0].id;
    let (view, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let delta = DAffine2::from_angle(0.5) * DAffine2::from_scale(dvec2(1.5, 1.));
            e.set_layer_selection(vec![first], Some(first));
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(delta, cx));
            e.commit_photo_transform(cx);
            let before = e.editor.doc.clone();
            let revision = e.editor.revision;
            for copy in [false, true, true] {
                e.repeat_photo_transform(copy, cx);
                assert_eq!(
                    e.editor.doc, before,
                    "D squared would introduce unrepresentable shear"
                );
                assert_eq!(e.editor.doc.next_id, before.next_id);
                assert_eq!(e.editor.revision, revision);
                assert_eq!(e.editor.history.len(), 1);
                assert_eq!(e.selected, Some(first));
                assert!(!e.editor.in_transaction());
            }
            e.set_layer_selection(vec![second], Some(second));
            e.repeat_photo_transform(true, cx);
            assert_ne!(e.selected, Some(second));
            assert_matrix(
                raster_matrix(&e.editor.doc, e.selected.unwrap()),
                delta * raster_matrix(&original, second),
            );
            assert_eq!(
                e.editor.history.len(),
                2,
                "failed repeats must retain the valid recipe"
            );
        })
    });
}

#[gpui_kit::test]
fn photo_mask_target_refuses_copy_and_again_without_falling_back_to_artwork(
    cx: &mut TestAppContext,
) {
    let mut original = artwork();
    original.nodes[0].mask = Some(Arc::new(emulsion_raster::select::rect(
        48, 32, 2., 2., 20., 15.,
    )));
    let id = original.nodes[0].id;
    let (view, cx) = setup(cx, original);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(7., 2.)), cx));
            e.commit_photo_transform(cx);
            e.tools.mask_edit_target = crate::editor::MaskEditTarget::RasterMask;
            for locked in [false, true] {
                e.editor.doc.node_mut(id).unwrap().locked = locked;
                let before = e.editor.doc.clone();
                let revision = e.editor.revision;
                for copy in [false, true] {
                    e.begin_photo_transform(copy, cx);
                    e.repeat_photo_transform(copy, cx);
                    assert_eq!(e.editor.doc, before);
                    assert_eq!(e.editor.doc.next_id, before.next_id);
                    assert_eq!(e.editor.revision, revision);
                    assert_eq!(e.editor.history.len(), 1);
                    assert_eq!(e.selected, Some(id));
                    assert!(e.tools.mask_edit_target.is_mask());
                    assert!(!e.photo_transform_active());
                }
            }
        })
    });
}

#[gpui_kit::test]
fn photo_modal_preview_blocks_other_edits_exports_pages_and_tool_switches(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(true, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(10., 0.)), cx));
            let preview = e.editor.doc.clone();
            let revision = e.editor.revision;
            let selected = e.selected;
            let page = e.editor.active_page();
            let pages = e.editor.page_list().to_vec();
            e.execute(
                Command::RemoveNode {
                    id: selected.unwrap(),
                },
                cx,
            );
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.doc.next_id, preview.next_id);
            assert!(
                !e.begin_file_export(cx),
                "an uncommitted copy must never reach file export"
            );
            e.add_project_page(false, cx);
            assert_eq!(e.editor.page_list(), pages);
            assert_eq!(e.editor.active_page(), page);
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.doc.next_id, preview.next_id);
            e.set_tool(Tool::Brush, cx);
            assert_eq!(e.tool, Tool::Move);
            e.set_paint(PaintKind::Brush, cx);
            assert_eq!(e.tool, Tool::Move);
            e.set_mask_edit(true, cx);
            assert!(!e.tools.mask_edit_target.is_mask());
            let selected_ids = e.selected_layer_ids();
            e.set_layer_selection(Vec::new(), None);
            assert_eq!(e.selected_layer_ids(), selected_ids);
            assert_eq!(e.selected, selected);

            assert!(
                e.editor
                    .execute(Command::RemoveNode {
                        id: selected.unwrap()
                    })
                    .is_err(),
                "host/core mutation routes must also refuse the preview"
            );
            assert!(e.editor.set_active_page(page).is_err());
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.selected, selected);
            assert_eq!(e.editor.transaction_depth(), 1);
            assert!(e.editor.history.is_empty());
            e.undo(cx);
            assert_eq!(
                e.editor.doc, original,
                "Undo during preview cancels instead of committing it"
            );
            assert_eq!(e.editor.doc.next_id, original.next_id);
            assert!(!e.photo_transform_active());
            assert!(!e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
            assert!(
                e.begin_file_export(cx),
                "blocked export must not leave its busy flag set"
            );
            e.finish_file_export(cx);
        })
    });
}

#[gpui_kit::test]
fn photo_shortcuts_dispatch_again_and_copy_from_canvas_and_layer_panel(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    press("ctrl-t", cx);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.photo_transform_active());
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(9., 4.)), cx));
        })
    });
    press("enter ctrl-shift-t", cx);
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.history.len(), 2);
        assert_matrix(
            raster_matrix(&e.editor.doc, e.selected.unwrap()),
            DAffine2::from_translation(dvec2(58., 48.)),
        );
        let focus = e.panel_focus.clone();
        window.focus(&focus, cx);
    });
    press("ctrl-alt-shift-t", cx);
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 2);
        assert_eq!(e.editor.history.len(), 3);
        assert_matrix(
            raster_matrix(&e.editor.doc, e.selected.unwrap()),
            DAffine2::from_translation(dvec2(67., 52.)),
        );
        let focus = e.panel_focus.clone();
        window.focus(&focus, cx);
    });
    press("ctrl-alt-t escape", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 2);
        assert_eq!(e.editor.history.len(), 3);
        assert!(!e.photo_transform_active());
    });
}

#[gpui_kit::test]
fn photo_transform_shortcuts_do_not_escape_a_focused_layer_name_input(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, artwork());
    let before = cx.update(|window, cx| {
        view.update(cx, |e, cx| e.rename_layer(window, cx));
        view.read(cx).editor.doc.clone()
    });
    cx.run_until_parked();
    press("ctrl-t ctrl-alt-t ctrl-shift-t ctrl-alt-shift-t", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc, before);
        assert_eq!(e.editor.doc.next_id, before.next_id);
        assert!(!e.photo_transform_active());
        assert!(e.editor.history.is_empty());
    });
    press("escape", cx);
}

#[gpui_kit::test]
fn photo_cancel_preserves_an_earlier_recipe_and_newer_unrelated_edits(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, artwork());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(5., 0.)), cx));
            e.commit_photo_transform(cx);
            e.begin_photo_transform(true, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(90., 0.)), cx));
            e.cancel_photo_transform(cx);
            e.repeat_photo_transform(false, cx);
            let source = e.selected.unwrap();
            assert_matrix(
                raster_matrix(&e.editor.doc, source),
                DAffine2::from_translation(dvec2(50., 40.)),
            );
            e.begin_photo_transform(true, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(9., 1.)), cx));
            // A host cancellation can precede a later edit before UI state catches up.
            assert!(
                !e.editor.undo(),
                "cancel does not consume a committed Undo step"
            );
            e.editor
                .execute(Command::SetOpacity {
                    id: source,
                    opacity: 0.5,
                })
                .unwrap();
            let newer = e.editor.doc.clone();
            let history = e.editor.history.len();
            e.cancel_photo_transform(cx);
            assert_eq!(
                e.editor.doc, newer,
                "stale baseline must never replace newer committed work"
            );
            assert_eq!(e.editor.history.len(), history);
            assert!(!e.photo_transform_active());
        })
    });
}

#[gpui_kit::test]
fn photo_successful_warp_content_invalidates_an_older_affine_recipe(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, artwork());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(8., 4.)), cx));
            e.commit_photo_transform(cx);
            let id = e.selected.unwrap();
            e.execute(
                Command::ReplaceContent {
                    id,
                    raster: Arc::new(Raster::solid(48, 32, [0.9, 0.1, 0.2, 1.])),
                    mask: None,
                    placement: Placement::at(48., 44.),
                    label: "Warp".into(),
                },
                cx,
            );
            let warped = e.editor.doc.clone();
            let revision = e.editor.revision;
            e.repeat_photo_transform(false, cx);
            e.repeat_photo_transform(true, cx);
            assert_eq!(e.editor.doc, warped);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.history.len(), 2);
        })
    });
}

#[gpui_kit::test]
fn photo_multi_selection_copy_uses_stable_ids_and_restores_original_active_row(
    cx: &mut TestAppContext,
) {
    let mut original = artwork();
    let first = original.nodes[0].id;
    let second = add(
        &mut original,
        Node::raster(
            0,
            "Second",
            Arc::new(Raster::solid(24, 18, [1.; 4])),
            Placement::at(100., 50.),
        ),
        None,
    );
    let (view, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![first, second], Some(first));
            for cancel in [true, false] {
                e.begin_photo_transform(true, cx);
                assert!(e.photo_transform_active());
                let copies = e.selected_layer_ids();
                let next_id = e.editor.doc.next_id;
                assert_eq!(copies.len(), 2);
                assert!(copies.iter().all(|id| *id != first && *id != second));
                for _ in 0..3 {
                    assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(4., 2.)), cx));
                    assert_eq!(e.selected_layer_ids(), copies);
                    assert_eq!(
                        e.editor.doc.next_id, next_id,
                        "preview must replay the original allocator, not issue more IDs"
                    );
                    assert_eq!(e.editor.doc.nodes.len(), 4);
                    assert!(e.editor.history.is_empty());
                }
                if cancel {
                    e.cancel_photo_transform(cx);
                    assert_eq!(e.editor.doc, original);
                    assert_eq!(e.editor.doc.next_id, original.next_id);
                    assert_eq!(e.selected_layer_ids(), vec![first, second]);
                    assert_eq!(e.selected, Some(first));
                } else {
                    e.commit_photo_transform(cx);
                    assert_eq!(e.editor.history.len(), 1);
                    e.repeat_photo_transform(true, cx);
                    assert_eq!(e.editor.doc.nodes.len(), 6);
                    assert_eq!(e.selected_layer_ids().len(), 2);
                    assert_eq!(e.editor.history.len(), 2);
                    e.undo(cx);
                    e.undo(cx);
                    assert_eq!(e.editor.doc, original);
                    assert_eq!(e.editor.doc.next_id, original.next_id);
                }
            }
        })
    });
}

/// Native reload recreates pixel Arcs, so compare their contents explicitly
/// before normalizing identity-only payload fields for full metadata equality.
fn assert_native_document(actual: &Document, expected: &Document) {
    assert_eq!(
        actual.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
        expected.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
        "native save must preserve layer IDs and stacking order"
    );
    assert_eq!(
        actual.next_id, expected.next_id,
        "native save must preserve the next node ID"
    );
    let mut normalized = actual.clone();
    for (actual, expected) in actual.nodes.iter().zip(&expected.nodes) {
        match (&actual.kind, &expected.kind) {
            (
                NodeKind::Raster { raster: actual, .. },
                NodeKind::Raster {
                    raster: expected, ..
                },
            ) => {
                assert_eq!(
                    (actual.width(), actual.height()),
                    (expected.width(), expected.height())
                );
                assert_eq!(actual.to_pixels(), expected.to_pixels());
            }
            (
                NodeKind::Smart {
                    source: actual,
                    editable: actual_editable,
                    filters: actual_filters,
                    filter_styles: actual_styles,
                    placement: actual_placement,
                    ..
                },
                NodeKind::Smart {
                    source: expected,
                    editable: expected_editable,
                    filters: expected_filters,
                    filter_styles: expected_styles,
                    placement: expected_placement,
                    ..
                },
            ) => {
                assert_eq!(
                    (actual.width(), actual.height()),
                    (expected.width(), expected.height())
                );
                assert_eq!(actual.to_pixels(), expected.to_pixels());
                assert_eq!(actual_editable, expected_editable);
                assert_eq!(actual_filters, expected_filters);
                assert_eq!(actual_styles, expected_styles);
                assert_eq!(actual_placement, expected_placement);
            }
            (actual, expected) => {
                assert_eq!(actual, expected, "editable content kind/data must survive")
            }
        }
        if let (
            NodeKind::Raster {
                placement: actual, ..
            },
            NodeKind::Raster {
                placement: expected,
                ..
            },
        ) = (&actual.kind, &expected.kind)
        {
            assert_eq!(actual, expected);
        }
        match (&actual.mask, &expected.mask) {
            (Some(actual), Some(expected)) => {
                assert_eq!(
                    (actual.width(), actual.height(), actual.fill()),
                    (expected.width(), expected.height(), expected.fill())
                );
                assert_eq!(actual.to_pixels(), expected.to_pixels());
            }
            (None, None) => {}
            _ => panic!("native save lost or invented a mask"),
        }
        let node = normalized.node_mut(actual.id).unwrap();
        node.kind = expected.kind.clone();
        node.mask = expected.mask.clone();
    }
    // Checks placement-independent metadata too: masks' transforms/link flags,
    // clipping, movement links, styles/effect settings, names and allocator.
    assert_eq!(&normalized, expected);
}

#[gpui_kit::test]
fn photo_committed_repeat_copies_round_trip_native_ora_and_emu_and_remain_independent(
    cx: &mut TestAppContext,
) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    let mut original = Document::new(192, 128);
    let group = add(
        &mut original,
        Node::group(0, "Original editable group"),
        None,
    );
    // Binary linear channels round-trip exactly through both native codecs.
    let pixels = Arc::new(Raster::from_fn(16, 12, [0; 4], |x, y| {
        [
            if x % 2 == 0 { 65535 } else { 0 },
            if y % 2 == 0 { 65535 } else { 0 },
            0,
            65535,
        ]
    }));
    let mut raster = Node::raster(0, "Pixels", pixels.clone(), Placement::at(20., 20.));
    raster.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
        16,
        12,
        0,
        |x, _| if x < 12 { 255 } else { 0 },
    )));
    raster.mask_linked = false;
    raster.mask_transform = DAffine2::from_translation(dvec2(1., 2.)).to_cols_array();
    raster
        .styles
        .push(emulsion_core::styles::LayerStyle::ColorOverlay {
            color: [255, 0, 0],
            opacity: 25.,
        });
    let raster = add(&mut original, raster, Some(group));
    let text = add(
        &mut original,
        Node::text(
            0,
            "Native text",
            emulsion_core::text::TextSpec {
                text: "Editable copy".into(),
                x: 40.,
                y: 30.,
                size: 12.,
                ..Default::default()
            },
            192,
            128,
        ),
        Some(group),
    );
    let path = add(
        &mut original,
        Node::path(
            0,
            "Native path",
            Arc::new(emulsion_raster::vector::Path::from_svg("M 18 52 L 42 52 L 30 68 Z").unwrap()),
            Default::default(),
            192,
            128,
        ),
        Some(group),
    );
    let mut smart = Node::smart(
        0,
        "Native Smart",
        pixels,
        vec![emulsion_filters::Filter::BoxBlur { radius: 1. }],
        Placement::at(48., 50.),
    );
    if let NodeKind::Smart { editable, .. } = &mut smart.kind {
        *editable = Some(emulsion_core::node::SmartEditable::Svg {
            xml: Arc::from(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"12\"><rect width=\"16\" height=\"12\" fill=\"red\"/></svg>",
            ),
        });
    }
    let smart = add(&mut original, smart, Some(group));
    original.node_mut(smart).unwrap().clip_to = Some(path);
    Command::SetLayerLinks {
        ids: vec![text, path],
        linked: true,
    }
    .apply(&mut original)
    .unwrap();
    original
        .validate()
        .expect("native roundtrip fixture is valid");
    let (view, cx) = setup(cx, original.clone());
    let (committed, copied_group) = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![group], Some(group));
            e.begin_photo_transform(true, cx);
            let delta = DAffine2::from_translation(dvec2(10., 5.))
                * DAffine2::from_angle(0.125)
                * DAffine2::from_scale(dvec2(1.125, 1.125));
            assert!(e.photo_transform_delta(delta, cx));
            e.commit_photo_transform(cx);
            e.repeat_photo_transform(true, cx);
            assert_eq!(e.editor.history.len(), 2);
            assert!(!e.photo_transform_active());
            assert!(!e.editor.in_transaction());
            assert_eq!(e.editor.doc.nodes.len(), 15);
            (e.editor.doc.clone(), e.selected.unwrap())
        })
    });
    let directory = tempfile::tempdir().unwrap();
    for extension in ["ora", "emu"] {
        let filename = directory.path().join(format!("repeat-copies.{extension}"));
        let reopened = if extension == "ora" {
            emulsion_io::ora::write(&committed, &filename).unwrap();
            emulsion_io::ora::read(&filename).unwrap()
        } else {
            let project = ProjectEditor::new_project(ProjectKind::Design, committed.clone())
                .unwrap()
                .snapshot()
                .unwrap();
            emulsion_io::project::write(&project, &filename).unwrap();
            let mut reopened = emulsion_io::project::read(&filename).unwrap();
            assert_eq!(reopened.kind, ProjectKind::Design);
            assert_eq!(reopened.active, project.active);
            assert_eq!(reopened.pages.len(), 1);
            assert_eq!(reopened.pages[0].meta, project.pages[0].meta);
            reopened.pages.remove(0).doc
        };
        assert_native_document(&reopened, &committed);
        let before_edit = reopened.clone();
        let copied = reopened.subtree(copied_group);
        let copied_raster = copied
            .iter()
            .copied()
            .find(|id| matches!(reopened.node(*id).unwrap().kind, NodeKind::Raster { .. }))
            .unwrap();
        let copied_text = copied
            .iter()
            .copied()
            .find(|id| matches!(reopened.node(*id).unwrap().kind, NodeKind::Text { .. }))
            .unwrap();
        let NodeKind::Text { spec, .. } = &reopened.node(copied_text).unwrap().kind else {
            unreachable!()
        };
        let mut edited_text = (**spec).clone();
        edited_text.text = "Only this reopened copy".into();
        let NodeKind::Raster { placement, .. } = &reopened.node(copied_raster).unwrap().kind else {
            unreachable!()
        };
        let placement = *placement;
        let mask = reopened.node(copied_raster).unwrap().mask.clone();
        let mut editor = emulsion_core::Editor::new(reopened, Some(filename));
        editor
            .execute(Command::SetText {
                id: copied_text,
                spec: Box::new(edited_text),
            })
            .unwrap();
        editor
            .execute(Command::ReplaceContent {
                id: copied_raster,
                raster: Arc::new(Raster::solid(16, 12, [0., 0., 1., 1.])),
                mask,
                placement,
                label: "Edit reopened copy".into(),
            })
            .unwrap();
        editor
            .execute(Command::TransformNodes {
                ids: vec![copied_group],
                transform: DAffine2::from_translation(dvec2(3., 2.)).to_cols_array(),
            })
            .unwrap();
        assert_ne!(
            editor.doc.node(copied_raster),
            before_edit.node(copied_raster)
        );
        assert_ne!(editor.doc.node(copied_text), before_edit.node(copied_text));
        for node in &before_edit.nodes {
            if !copied.contains(&node.id) {
                assert_eq!(
                    editor.doc.node(node.id),
                    Some(node),
                    "editing a reopened copy must leave originals and earlier copies unchanged ({extension})"
                );
            }
        }
        // Check original pixel bytes too, beyond unchanged Arc identity.
        let NodeKind::Raster {
            raster: unchanged, ..
        } = &editor.doc.node(raster).unwrap().kind
        else {
            unreachable!()
        };
        let NodeKind::Raster { raster: source, .. } = &original.node(raster).unwrap().kind else {
            unreachable!()
        };
        assert_eq!(unchanged.to_pixels(), source.to_pixels());
        for _ in 0..3 {
            assert!(editor.undo());
        }
        assert_eq!(editor.doc, before_edit);
        assert_eq!(editor.doc.next_id, before_edit.next_id);
    }
}

#[gpui_kit::test]
fn photo_numeric_field_enter_updates_preview_without_committing_or_leaking_shortcuts(
    cx: &mut TestAppContext,
) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        view.update(cx, |e, cx| e.show_sidebar_tab(SidebarTab::Properties, cx));
        window.refresh();
    });
    cx.run_until_parked();
    for cancel in [true, false] {
        cx.update(|window, cx| {
            let focus = view.read(cx).canvas_focus.clone();
            window.focus(&focus, cx);
        });
        press("ctrl-alt-t", cx);
        for (field, value, expected) in [
            ("photo-transform-X", "62", dvec2(62., 40.)),
            ("photo-transform-Y", "51", dvec2(62., 51.)),
        ] {
            cx.update(|window, cx| {
                window.within("sidebar-properties-content").click(field, cx);
            });
            cx.run_until_parked();
            let input_focus = cx.update(|window, cx| {
                assert!(!view.read(cx).canvas_focus.is_focused(window));
                window.focused(cx).expect("numeric transform field focused")
            });
            press("ctrl-a", cx);
            cx.simulate_input(value);
            press("enter", cx);
            let preview = cx.update(|window, cx| {
                let e = view.read(cx);
                assert!(input_focus.is_focused(window));
                assert!(
                    e.photo_transform_active(),
                    "Enter in a number field applies the number, not the whole session"
                );
                assert_eq!(e.editor.transaction_depth(), 1);
                assert_eq!(e.editor.doc.nodes.len(), 2);
                assert!(e.editor.history.is_empty());
                assert_matrix(
                    raster_matrix(&e.editor.doc, e.selected.unwrap()),
                    DAffine2::from_translation(expected),
                );
                e.editor.doc.clone()
            });
            press("ctrl-t ctrl-alt-t ctrl-shift-t ctrl-alt-shift-t", cx);
            cx.update(|window, cx| {
                let e = view.read(cx);
                assert!(input_focus.is_focused(window));
                assert!(e.photo_transform_active());
                assert_eq!(
                    e.editor.doc, preview,
                    "transform shortcuts cannot escape an input"
                );
                assert_eq!(e.editor.doc.next_id, preview.next_id);
                assert!(e.editor.history.is_empty());
            });
        }
        cx.update(|window, cx| {
            let focus = view.read(cx).canvas_focus.clone();
            window.focus(&focus, cx);
        });
        if cancel {
            press("escape", cx);
        } else {
            press("enter", cx);
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert!(!e.photo_transform_active());
                assert_eq!(e.editor.history.len(), 1);
                assert_eq!(e.editor.doc.nodes.len(), 2);
            });
            press("ctrl-z", cx);
        }
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.doc.next_id, original.next_id);
            assert!(!e.photo_transform_active());
            assert!(!e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn photo_untouched_warp_preserves_affine_again_and_does_not_resample(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, artwork());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(7., 3.)), cx));
            e.commit_photo_transform(cx);
            let transformed = e.editor.doc.clone();
            let revision = e.editor.revision;
            e.start_warp(cx);
            assert!(e.warp.is_some());
            e.finish_warp(cx);
            assert!(e.warp.is_none());
            assert_eq!(e.editor.doc, transformed);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.history.len(), 1);
            e.repeat_photo_transform(false, cx);
            assert_matrix(
                raster_matrix(&e.editor.doc, e.selected.unwrap()),
                DAffine2::from_translation(dvec2(54., 46.)),
            );
            assert_eq!(e.editor.history.len(), 2);
        })
    });
}

#[gpui_kit::test]
fn photo_refused_actions_during_and_after_drag_leave_preview_commit_eligible(
    cx: &mut TestAppContext,
) {
    let mut original = artwork();
    let first = original.nodes[0].id;
    add(
        &mut original,
        Node::raster(
            0,
            "Unselected",
            Arc::new(Raster::solid(16, 16, [1.; 4])),
            Placement::at(150., 80.),
        ),
        None,
    );
    let (view, cx) = setup(cx, original.clone());
    cx.update(|_, cx| view.update(cx, |e, _| e.set_layer_selection(vec![first], Some(first))));
    press("ctrl-alt-t", cx);
    let (start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        (
            e.doc_to_window((64., 56.)).unwrap(),
            e.doc_to_window((77., 65.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    let (preview, ticket, selected) = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(e.has_active_gesture());
        (
            e.editor.doc.clone(),
            e.edit_ticket(),
            e.selected_layer_ids(),
        )
    });
    for released in [false, true] {
        if released {
            cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
            cx.run_until_parked();
        }
        for key in [
            "delete",
            "ctrl-j",
            "ctrl-alt-a",
            "alt-backspace",
            "ctrl-backspace",
            "ctrl-s",
            "ctrl-shift-s",
            "ctrl-alt-shift-w",
        ] {
            press(key, cx);
            assert!(
                !cx.did_prompt_for_new_path(),
                "{key} must not open a save/export chooser"
            );
            assert!(!cx.did_prompt_for_paths(), "{key} must not ask for a file");
            cx.update(|window, cx| {
                let has_dialog = window.has_active_dialog(cx);
                let e = view.read(cx);
                assert!(
                    !window.has_active_prompt(),
                    "{key} must not open a confirmation/path prompt"
                );
                assert!(!has_dialog, "{key} must not open an export dialog");
                assert!(!e.export_prefs.open);
                assert!(e.editor.path.is_none(), "no save destination was accepted");
                assert!(
                    e.photo_transform_active(),
                    "{key} must not resolve modal ownership"
                );
                assert_eq!(
                    e.has_active_gesture(),
                    !released,
                    "{key} changed pointer ownership"
                );
                assert_eq!(e.editor.doc, preview, "{key}, mouse released={released}");
                assert_eq!(e.editor.doc.next_id, preview.next_id);
                assert_eq!(
                    e.edit_ticket(),
                    ticket,
                    "a refused {key} must not stale the transform"
                );
                assert_eq!(e.selected_layer_ids(), selected);
                assert_eq!(e.editor.transaction_depth(), 1);
                assert!(e.editor.history.is_empty());
            });
        }
    }
    // A later valid edit and Enter must still succeed after every refusal.
    press("right enter", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.photo_transform_active());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 1);
        assert_eq!(e.editor.doc.nodes.len(), 3);
        assert_matrix(
            raster_matrix(&e.editor.doc, e.selected.unwrap()),
            DAffine2::from_translation(dvec2(54., 49.)),
        );
    });
    press("ctrl-z", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc, original);
        assert_eq!(e.editor.doc.next_id, original.next_id);
    });
}

#[gpui_kit::test]
fn photo_modal_preview_rejects_async_jobs_and_all_replay_snapshot_routes(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, artwork());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(4., 2.)), cx));
            e.commit_photo_transform(cx);
            assert_eq!(
                e.replay_docs().len(),
                2,
                "fixture has real history worth replaying"
            );
            e.begin_photo_transform(true, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(8., 3.)), cx));
            let preview = e.editor.doc.clone();
            let ticket = e.edit_ticket();
            let epoch = e.operation_epoch;
            assert!(e.begin_edit_job().is_none());
            assert_eq!(e.edit_ticket(), ticket);
            assert_eq!(e.operation_epoch, epoch);
            e.invalidate_pending_edits();
            e.after_change(cx);
            assert_eq!(
                e.edit_ticket(),
                ticket,
                "harmless refused-action cleanup cannot stale an unchanged preview"
            );
            assert!(
                e.replay_docs().is_empty(),
                "provisional copies must not enter snapshots"
            );
            e.replay_start(cx);
            assert!(
                e.anim.replay.is_none(),
                "blocked replay must not cache preview documents"
            );
            e.export_replay_gif(cx);
            e.export_animation_gif(cx);
            assert!(e.anim.replay.is_none());
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.doc.next_id, preview.next_id);
            assert_eq!(e.edit_ticket(), ticket);
            assert_eq!(e.editor.history.len(), 1);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(1., 0.)), cx));
            e.commit_photo_transform(cx);
            assert!(!e.photo_transform_active());
            assert_eq!(e.editor.history.len(), 2);
            assert_eq!(e.replay_docs().len(), 3);
            assert_eq!(e.replay_docs().last(), Some(&e.editor.doc));
        })
    });
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_new_path());
    cx.update(|window, cx| {
        assert!(!window.has_active_prompt());
        assert!(!window.has_active_dialog(cx));
    });
}

#[gpui_kit::test]
fn photo_pixel_selection_identity_and_out_back_enter_restore_the_entire_lift_baseline(
    cx: &mut TestAppContext,
) {
    let mut original = artwork();
    original.selection = Some(Arc::new(emulsion_raster::select::rect(
        384, 256, 45., 44., 18., 12.,
    )));
    let source = original.nodes[0].id;
    let (view, cx) = setup(cx, original.clone());
    for out_and_back in [false, true] {
        press("ctrl-t", cx);
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(e.photo_transform_active());
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert!(e.editor.doc.selection.is_none());
        });
        if out_and_back {
            press("shift-right shift-left down up", cx);
        }
        press("enter", cx);
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(
                e.editor.doc, original,
                "an ordinary net-identity operation must not leave a lifted layer"
            );
            assert_eq!(e.editor.doc.next_id, original.next_id);
            assert_eq!(e.selected_layer_ids(), vec![source]);
            assert_eq!(e.selected, Some(source));
            assert!(!e.photo_transform_active());
            assert!(!e.editor.in_transaction());
            assert!(!e.editor.is_modified());
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.history.can_redo());
        });
    }
    // Duplicate-and-transform at identity is an intentional copy, unlike lift.
    press("ctrl-alt-t enter", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 2);
        assert_eq!(e.editor.doc.next_id, original.next_id + 1);
        assert_eq!(e.editor.doc.node(source), original.node(source));
        assert!(e.editor.doc.selection.is_none());
        assert_ne!(e.selected, Some(source));
        assert_eq!(e.editor.history.len(), 1);
        assert!(!e.photo_transform_active());
    });
    press("ctrl-z", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc, original);
        assert_eq!(e.editor.doc.next_id, original.next_id);
    });
}

#[gpui_kit::test]
fn photo_text_edge_reflow_preserves_again_on_cancel_or_noop_and_clears_it_on_commit(
    cx: &mut TestAppContext,
) {
    let mut original = Document::new(384, 256);
    let text = add(
        &mut original,
        Node::text(
            0,
            "Paragraph",
            emulsion_core::text::TextSpec {
                text: "Several words remain editable in this paragraph frame".into(),
                x: 100.,
                y: 70.,
                size: 16.,
                width: Some(110.),
                height: Some(100.),
                ..Default::default()
            },
            384,
            256,
        ),
        None,
    );
    let (view, cx) = setup(cx, original);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(10., 4.)), cx));
            e.commit_photo_transform(cx);
            // Use exact document/screen units so returning to the same pointer
            // position tests a true no-op, not f32 viewport rounding.
            e.zoom_100(cx);
        })
    });
    cx.run_until_parked();
    for outcome in ["cancel", "out-and-back", "commit"] {
        let (before, start, end) = cx.update(|_, cx| {
            let e = view.read(cx);
            let NodeKind::Text { spec, .. } = &e.editor.doc.node(text).unwrap().kind else {
                unreachable!()
            };
            let to_window = |width| {
                let p = spec.transform().transform_point2(dvec2(width, 50.));
                e.doc_to_window((p.x, p.y)).unwrap()
            };
            (e.editor.doc.clone(), to_window(110.), to_window(160.))
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(e.has_active_gesture());
            assert!(
                !e.photo_transform_active(),
                "this is ordinary frame reflow, not affine Free Transform"
            );
            let NodeKind::Text { spec, .. } = &e.editor.doc.node(text).unwrap().kind else {
                unreachable!()
            };
            assert!((spec.width.unwrap() - 160.).abs() < 0.01);
            assert_eq!((spec.scale_x, spec.scale_y), (1., 1.));
        });
        match outcome {
            "cancel" => {
                press("escape", cx);
                cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
            }
            "out-and-back" => {
                cx.simulate_mouse_move(start, Some(MouseButton::Left), Modifiers::none());
                cx.simulate_mouse_up(start, MouseButton::Left, Modifiers::none());
            }
            _ => cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none()),
        }
        cx.run_until_parked();
        let after = cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(!e.editor.in_transaction());
            if outcome != "commit" {
                assert_eq!(
                    e.editor.doc, before,
                    "{outcome} must restore unchanged text frame data"
                );
                assert_eq!(e.editor.history.len(), 1);
            } else {
                assert_ne!(e.editor.doc, before);
                assert_eq!(e.editor.history.len(), 2);
            }
            e.editor.doc.clone()
        });
        press("ctrl-shift-t", cx);
        cx.update(|_, cx| {
            let e = view.read(cx);
            if outcome == "commit" {
                assert_eq!(
                    e.editor.doc, after,
                    "committed reflow must not replay an older affine recipe"
                );
                assert_eq!(e.editor.history.len(), 2);
            } else {
                let NodeKind::Text { spec: actual, .. } = &e.editor.doc.node(text).unwrap().kind
                else {
                    unreachable!()
                };
                let NodeKind::Text { spec: previous, .. } = &after.node(text).unwrap().kind else {
                    unreachable!()
                };
                assert_matrix(
                    actual.transform(),
                    DAffine2::from_translation(dvec2(10., 4.)) * previous.transform(),
                );
                assert_eq!(
                    e.editor.history.len(),
                    2,
                    "{outcome} must retain the prior affine recipe"
                );
            }
        });
        if outcome != "commit" {
            press("ctrl-z", cx);
            cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, before));
        } else {
            press("ctrl-alt-shift-t", cx);
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert_eq!(e.editor.doc, after);
                assert_eq!(e.editor.doc.next_id, after.next_id);
                assert_eq!(e.editor.history.len(), 2);
            });
        }
    }
}

#[gpui_kit::test]
fn photo_redo_mid_reflow_commits_the_frame_and_invalidates_affine_again(cx: &mut TestAppContext) {
    let mut document = Document::new(384, 256);
    let text = add(
        &mut document,
        Node::text(
            0,
            "Paragraph",
            emulsion_core::text::TextSpec {
                text: "Editable paragraph".into(),
                x: 100.,
                y: 70.,
                size: 16.,
                width: Some(110.),
                height: Some(100.),
                ..Default::default()
            },
            384,
            256,
        ),
        None,
    );
    let (view, cx) = setup(cx, document);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(DAffine2::from_translation(dvec2(10., 4.)), cx));
            e.commit_photo_transform(cx);
            e.zoom_100(cx);
        })
    });
    cx.run_until_parked();
    let (start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(text).unwrap().kind else {
            unreachable!()
        };
        let at = |width| {
            let p = spec.transform().transform_point2(dvec2(width, 50.));
            e.doc_to_window((p.x, p.y)).unwrap()
        };
        (at(110.), at(160.))
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| assert!(view.read(cx).has_active_gesture()));
    press("ctrl-shift-z", cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let committed = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.editor.in_transaction());
        assert!(!e.has_active_gesture());
        assert_eq!(e.editor.history.len(), 2);
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(text).unwrap().kind else {
            unreachable!()
        };
        assert!((spec.width.unwrap() - 160.).abs() < 0.01);
        e.editor.doc.clone()
    });
    press("ctrl-shift-t ctrl-alt-shift-t", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc, committed);
        assert_eq!(e.editor.doc.next_id, committed.next_id);
        assert_eq!(e.editor.history.len(), 2);
    });
}

fn nonlinear_selected_artwork() -> Document {
    let mut doc = Document::new(256, 192);
    add(
        &mut doc,
        Node::raster(
            0,
            "Selected source",
            Arc::new(Raster::from_fn(128, 96, [0; 4], |x, y| {
                [x as u16 * 511, y as u16 * 683, 18000, 65535]
            })),
            Placement::at(32., 32.),
        ),
        None,
    );
    // At 100% zoom, adjacent Warp lattice controls are farther apart than
    // their hit targets, making pointer previews target a single exact point.
    doc.selection = Some(Arc::new(emulsion_raster::select::rect(
        256, 192, 48., 48., 72., 54.,
    )));
    doc
}

struct NonlinearLiftBaseline {
    doc: Document,
    selected: Option<NodeId>,
    selected_ids: Vec<NodeId>,
    revision: u64,
    saved_revision: u64,
    modified: bool,
    history: Vec<(String, u64)>,
    can_redo: bool,
}

impl NonlinearLiftBaseline {
    fn capture(e: &EditorView) -> Self {
        Self {
            doc: e.editor.doc.clone(),
            selected: e.selected,
            selected_ids: e.selected_layer_ids(),
            revision: e.editor.revision,
            saved_revision: e.editor.saved_revision(),
            modified: e.editor.is_modified(),
            history: e
                .editor
                .history
                .steps()
                .map(|s| (s.name.clone(), s.revision_before))
                .collect(),
            can_redo: e.editor.history.can_redo(),
        }
    }

    fn assert_restored(&self, e: &EditorView) {
        assert_eq!(
            e.editor.doc, self.doc,
            "pixels, selection and all original nodes must be restored by identity"
        );
        assert_eq!(e.editor.doc.next_id, self.doc.next_id);
        assert_eq!(e.selected, self.selected);
        assert_eq!(e.selected_layer_ids(), self.selected_ids);
        assert_eq!(e.editor.revision, self.revision);
        assert_eq!(e.editor.saved_revision(), self.saved_revision);
        assert_eq!(
            e.editor.is_modified(),
            self.modified,
            "restore the original saved/dirty state"
        );
        assert_eq!(
            e.editor
                .history
                .steps()
                .map(|s| (s.name.clone(), s.revision_before))
                .collect::<Vec<_>>(),
            self.history
        );
        assert_eq!(e.editor.history.can_redo(), self.can_redo);
        assert!(e.tools.transform_lift.is_none());
        assert!(e.warp.is_none());
        assert!(!e.has_active_gesture());
        assert!(!e.photo_transform_active());
        assert!(!e.editor.in_transaction());
    }
}

fn dispatch_nonlinear_transform(mode: &str, cx: &mut VisualTestContext) {
    match mode {
        "warp" => cx.dispatch_action(actions::TransformWarp),
        "distort" => cx.dispatch_action(actions::TransformDistort),
        _ => unreachable!(),
    }
    cx.run_until_parked();
}

fn nonlinear_modifiers(mode: &str) -> Modifiers {
    Modifiers {
        control: mode == "distort",
        ..Modifiers::none()
    }
}

#[gpui_kit::test]
fn photo_selected_warp_and_distort_escape_restore_lift_and_existing_redo(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, nonlinear_selected_artwork());
    let (baseline, redo_doc) = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            e.execute(Command::SetOpacity { id, opacity: 0.5 }, cx);
            let redo_doc = e.editor.doc.clone();
            e.undo(cx);
            e.zoom_100(cx);
            assert!(!e.editor.is_modified());
            assert!(e.editor.history.can_redo());
            (NonlinearLiftBaseline::capture(e), redo_doc)
        })
    });
    cx.run_until_parked();
    for mode in ["warp", "distort"] {
        for pointer_preview in [false, true] {
            dispatch_nonlinear_transform(mode, cx);
            let (start, end) = cx.update(|_, cx| {
                let e = view.read(cx);
                assert_eq!(e.editor.doc.nodes.len(), baseline.doc.nodes.len() + 1);
                assert!(e.editor.doc.selection.is_none());
                assert!(
                    e.tools.transform_lift.is_some(),
                    "the production {mode} entry must capture its lift baseline"
                );
                assert!(!e.photo_transform_active());
                assert_eq!(e.warp.is_some(), mode == "warp");
                (
                    e.doc_to_window((48., 48.)).unwrap(),
                    e.doc_to_window((40., 42.)).unwrap(),
                )
            });
            if pointer_preview {
                cx.simulate_mouse_down(start, MouseButton::Left, nonlinear_modifiers(mode));
                cx.simulate_mouse_move(end, Some(MouseButton::Left), nonlinear_modifiers(mode));
                cx.run_until_parked();
                cx.update(|_, cx| {
                    let e = view.read(cx);
                    assert!(e.has_active_gesture());
                    assert_eq!(
                        e.editor.history.len(),
                        1,
                        "geometry preview must not resample yet"
                    );
                    if mode == "warp" {
                        assert_eq!(e.warp.as_ref().unwrap().grid[0], (40., 42.));
                    }
                });
            }
            press("escape", cx);
            if pointer_preview {
                // Mouse-up after Escape must not start a stale resampling job.
                cx.simulate_mouse_up(end, MouseButton::Left, nonlinear_modifiers(mode));
            }
            cx.run_until_parked();
            cx.update(|_, cx| baseline.assert_restored(view.read(cx)));
            press("ctrl-shift-z", cx);
            cx.update(|_, cx| {
                assert_eq!(
                    view.read(cx).editor.doc,
                    redo_doc,
                    "cancel must preserve the real old Redo contents"
                );
            });
            press("ctrl-z", cx);
            cx.update(|_, cx| baseline.assert_restored(view.read(cx)));
        }
    }
}

#[gpui_kit::test]
fn photo_selected_warp_apply_and_distort_noop_restore_modified_baseline_and_redo(
    cx: &mut TestAppContext,
) {
    let (view, cx) = setup(cx, nonlinear_selected_artwork());
    let (baseline, redo_doc) = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            e.execute(Command::SetOpacity { id, opacity: 0.8 }, cx);
            e.execute(Command::SetOpacity { id, opacity: 0.6 }, cx);
            let redo_doc = e.editor.doc.clone();
            e.undo(cx);
            e.zoom_100(cx);
            assert!(e.editor.is_modified());
            assert_eq!(e.editor.history.len(), 1);
            assert!(e.editor.history.can_redo());
            (NonlinearLiftBaseline::capture(e), redo_doc)
        })
    });
    cx.run_until_parked();
    for mode in ["warp", "distort"] {
        for outcome in ["enter-before-pointer", "untouched-release", "out-and-back"] {
            dispatch_nonlinear_transform(mode, cx);
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert!(e.tools.transform_lift.is_some());
                assert_eq!(e.editor.history.len(), 2);
            });
            if outcome != "enter-before-pointer" {
                let (start, end) = cx.update(|_, cx| {
                    let e = view.read(cx);
                    (
                        e.doc_to_window((48., 48.)).unwrap(),
                        e.doc_to_window((40., 42.)).unwrap(),
                    )
                });
                cx.simulate_mouse_down(start, MouseButton::Left, nonlinear_modifiers(mode));
                if outcome == "out-and-back" {
                    cx.simulate_mouse_move(end, Some(MouseButton::Left), nonlinear_modifiers(mode));
                    cx.simulate_mouse_move(
                        start,
                        Some(MouseButton::Left),
                        nonlinear_modifiers(mode),
                    );
                }
                cx.simulate_mouse_up(start, MouseButton::Left, nonlinear_modifiers(mode));
                cx.run_until_parked();
            }
            if mode == "warp" || outcome == "enter-before-pointer" {
                press("enter", cx);
            }
            cx.run_until_parked();
            cx.update(|_, cx| baseline.assert_restored(view.read(cx)));
            press("ctrl-shift-z", cx);
            cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, redo_doc));
            press("ctrl-z", cx);
            cx.update(|_, cx| baseline.assert_restored(view.read(cx)));
        }
    }
}

#[gpui_kit::test]
fn photo_selected_degenerate_warp_and_singular_distort_roll_back_the_lift(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, nonlinear_selected_artwork());
    let baseline = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.zoom_100(cx);
            NonlinearLiftBaseline::capture(e)
        })
    });
    cx.run_until_parked();
    for mode in ["warp", "distort"] {
        dispatch_nonlinear_transform(mode, cx);
        let (start, end) = cx.update(|_, cx| {
            let e = view.read(cx);
            let invalid = if mode == "warp" {
                (72., 48.) // Collapse a lattice edge onto its neighboring control.
            } else {
                (120., 48.) // Two identical quad corners have no homography.
            };
            (
                e.doc_to_window((48., 48.)).unwrap(),
                e.doc_to_window(invalid).unwrap(),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, nonlinear_modifiers(mode));
        cx.simulate_mouse_move(end, Some(MouseButton::Left), nonlinear_modifiers(mode));
        cx.simulate_mouse_up(end, MouseButton::Left, nonlinear_modifiers(mode));
        cx.run_until_parked();
        if mode == "warp" {
            press("enter", cx);
        }
        cx.run_until_parked();
        cx.update(|_, cx| baseline.assert_restored(view.read(cx)));
    }
}

#[gpui_kit::test]
fn photo_selected_warp_and_distort_async_commit_undo_in_two_accurate_steps(
    cx: &mut TestAppContext,
) {
    let original = nonlinear_selected_artwork();
    let source_id = original.nodes[0].id;
    let (view, cx) = setup(cx, original.clone());
    let original_revision = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.zoom_100(cx);
            e.editor.revision
        })
    });
    cx.run_until_parked();
    for mode in ["warp", "distort"] {
        dispatch_nonlinear_transform(mode, cx);
        let (lifted, lift_revision, selected, start, end) = cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.history.len(), 1);
            assert_eq!(e.editor.doc.nodes.len(), 2);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.selected.unwrap(),
                e.doc_to_window((48., 48.)).unwrap(),
                e.doc_to_window((40., 42.)).unwrap(),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, nonlinear_modifiers(mode));
        cx.simulate_mouse_move(end, Some(MouseButton::Left), nonlinear_modifiers(mode));
        cx.simulate_mouse_up(end, MouseButton::Left, nonlinear_modifiers(mode));
        cx.run_until_parked();
        if mode == "warp" {
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert!(e.warp.is_some());
                assert_eq!(
                    e.editor.doc, lifted,
                    "Warp mouse-up only changes the lattice"
                );
                assert_eq!(e.editor.history.len(), 1);
            });
            press("enter", cx);
        }
        cx.run_until_parked();
        let committed = cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(
                e.editor.history.len(),
                2,
                "legacy non-affine transform retains separate Lift and {mode} steps"
            );
            assert!(e.editor.is_modified());
            assert!(e.warp.is_none());
            assert!(!e.has_active_gesture());
            assert!(!e.editor.in_transaction());
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert_eq!(e.editor.doc.next_id, lifted.next_id);
            assert_eq!(
                e.editor.doc.node(source_id),
                lifted.node(source_id),
                "resampling must only replace the lifted pixels"
            );
            assert_eq!(e.selected, Some(selected));
            let NodeKind::Raster { raster: actual, .. } =
                &e.editor.doc.node(selected).unwrap().kind
            else {
                unreachable!()
            };
            let NodeKind::Raster { raster: before, .. } = &lifted.node(selected).unwrap().kind
            else {
                unreachable!()
            };
            assert!(
                !Arc::ptr_eq(actual, before),
                "successful {mode} must publish its asynchronous result"
            );
            assert_ne!(e.editor.doc.node(selected), lifted.node(selected));
            e.editor.doc.clone()
        });
        press("ctrl-z", cx);
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc, lifted);
            assert_eq!(e.editor.doc.next_id, lifted.next_id);
            assert_eq!(e.editor.revision, lift_revision);
            assert_eq!(e.editor.history.len(), 1);
        });
        press("ctrl-z", cx);
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.doc.next_id, original.next_id);
            assert_eq!(e.editor.revision, original_revision);
            assert!(!e.editor.is_modified());
            assert_eq!(e.selected, Some(source_id));
            assert!(e.editor.history.is_empty());
        });
        press("ctrl-shift-z ctrl-shift-z", cx);
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).editor.doc, committed);
            assert_eq!(view.read(cx).editor.history.len(), 2);
        });
        press("ctrl-z ctrl-z", cx);
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    }
}
