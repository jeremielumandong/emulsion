//! Native component targeting, Pen coordinates and gesture ownership.
use super::*;
use ::core::prelude::v1::test;
use emulsion_core::{EmptyVectorCoverage, VectorMask};
use emulsion_raster::{
    Mask,
    vector::{Anchor, Path, SubPath},
};
use glam::{DAffine2, dvec2};
use gpui_kit::{TestAppContext, VisualTestContext, test::TestWindowExt};

fn path(points: &[(f64, f64)]) -> Arc<Path> {
    Arc::new(Path {
        subpaths: vec![SubPath {
            anchors: points.iter().copied().map(Anchor::corner).collect(),
            closed: false,
        }],
    })
}
fn document(smart: bool) -> Document {
    let mut doc = Document::new(300, 220);
    let source = Arc::new(Raster::solid(80, 60, [0.2, 0.3, 0.4, 1.]));
    let placement = Placement {
        x: 50.,
        y: 40.,
        scale_x: 1.5,
        scale_y: 0.8,
        rotation: 27.,
        ..Default::default()
    };
    let mut node = if smart {
        Node::smart(1, "Smart", source, Vec::new(), placement)
    } else {
        Node::raster(1, "Photo", source, placement)
    };
    node.mask = Some(Arc::new(Mask::white(80, 60)));
    let mut mask = VectorMask::empty(EmptyVectorCoverage::HideAll);
    mask.transform =
        (DAffine2::from_translation(dvec2(6., 4.)) * DAffine2::from_angle(0.15)).to_cols_array();
    node.vector_mask = Some(mask);
    if let NodeKind::Smart { cache, offset, .. } = &mut node.kind {
        *cache = Arc::new(Raster::solid(100, 84, [0.2, 0.3, 0.4, 1.]));
        *offset = (-10, -12);
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
fn setup(cx: &mut TestAppContext, doc: Document) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = crate::tests::open(cx, doc);
    cx.simulate_resize(size(px(1440.), px(1100.)));
    let editor = cx.update(|window, cx| {
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| {
            e.select_layer_vector_mask(1, cx);
            e.snap = false;
            window.focus(&e.canvas_focus, cx);
        });
        editor
    });
    cx.run_until_parked();
    (editor, cx)
}
fn near(actual: DAffine2, expected: DAffine2) {
    for (a, b) in actual
        .to_cols_array()
        .into_iter()
        .zip(expected.to_cols_array())
    {
        assert!((a - b).abs() < 1e-8, "{a} != {b}");
    }
}

#[gpui_kit::test]
fn both_mask_thumbnails_choose_independent_focus_inspection_and_enable(cx: &mut TestAppContext) {
    let original = document(false);
    let (editor, cx) = setup(cx, original.clone());
    for (name, target) in [
        ("layer-mask", MaskEditTarget::RasterMask),
        ("layer-vector-mask", MaskEditTarget::VectorMask),
    ] {
        let point = cx.update(|window, _| window.find((name, 1_u64)).bounds().center());
        cx.simulate_click(
            point,
            Modifiers {
                alt: true,
                ..Default::default()
            },
        );
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.tools.mask_edit_target, target);
            assert_eq!(e.mask_view.target, Some((1, target)));
            assert!(e.mask_view_snapshot().unwrap().is_some());
            assert_eq!(e.editor.doc, original);
        });
    }
    let point = cx.update(|window, _| window.find(("layer-vector-mask", 1_u64)).bounds().center());
    cx.simulate_click(
        point,
        Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let n = e.editor.doc.node(1).unwrap();
        assert!(!n.vector_mask.as_ref().unwrap().enabled);
        assert!(n.mask_enabled);
        assert_eq!(n.kind, original.nodes[0].kind);
    });
}

#[gpui_kit::test]
fn pen_vector_mask_placed_rotated_smart_coordinates_and_last_anchor_safety(
    cx: &mut TestAppContext,
) {
    for smart in [false, true] {
        let original = document(smart);
        let (editor, cx) = setup(cx, original.clone());
        let to_doc = MaskEditTarget::VectorMask
            .to_document(&original.nodes[0])
            .unwrap();
        let points = [(8., 8.), (35., 8.), (24., 28.)];
        for p in points {
            let d = to_doc.transform_point2(dvec2(p.0, p.1));
            let screen = cx.update(|_, cx| editor.read(cx).doc_to_window((d.x, d.y)).unwrap());
            cx.simulate_click(screen, Modifiers::none());
            cx.run_until_parked();
        }
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert_eq!(e.editor.doc.nodes.len(), 1);
                let n = e.editor.doc.node(1).unwrap();
                assert_eq!(n.kind, original.nodes[0].kind);
                assert!(Arc::ptr_eq(
                    n.mask.as_ref().unwrap(),
                    original.nodes[0].mask.as_ref().unwrap()
                ));
                let actual = &n.vector_mask.as_ref().unwrap().path.subpaths[0].anchors;
                assert_eq!(actual.len(), 3);
                for (a, p) in actual.iter().zip(points) {
                    assert!((a.p.0 - p.0).abs() < 0.1);
                    assert!((a.p.1 - p.1).abs() < 0.1);
                }
                let geometry = n.vector_mask.as_ref().unwrap().path.clone();
                e.tools.pen.fill_on = true;
                e.tools.pen.stroke_on = true;
                e.pen_restyle(cx);
                assert_eq!(
                    e.editor
                        .doc
                        .node(1)
                        .unwrap()
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .path,
                    geometry
                );
                for _ in 0..3 {
                    e.tools.pen.selected = Some((0, 0));
                    assert!(e.pen_delete(cx));
                }
                let n = e.editor.doc.node(1).unwrap();
                assert_eq!(n.kind, original.nodes[0].kind);
                assert!(n.vector_mask.as_ref().unwrap().path.is_empty());
                assert_eq!(
                    n.vector_mask.as_ref().unwrap().empty_coverage,
                    EmptyVectorCoverage::HideAll
                );
                assert_eq!(e.editor.doc.nodes.len(), 1);
                e.undo(cx);
                assert!(
                    !e.editor
                        .doc
                        .node(1)
                        .unwrap()
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .path
                        .is_empty()
                );
            })
        });
    }
}

#[gpui_kit::test]
fn vector_mask_transform_preview_cancel_commit_repeat_and_component_independence(
    cx: &mut TestAppContext,
) {
    let mut original = document(true);
    original.nodes[0].vector_mask.as_mut().unwrap().path = path(&[(5., 5.), (35., 5.), (30., 30.)]);
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let start = MaskEditTarget::VectorMask
                .to_document(e.editor.doc.node(1).unwrap())
                .unwrap();
            let delta = DAffine2::from_translation(dvec2(9., -3.));
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_active());
            assert!(e.photo_transform_delta(delta, cx));
            near(
                MaskEditTarget::VectorMask
                    .to_document(e.editor.doc.node(1).unwrap())
                    .unwrap(),
                delta * start,
            );
            assert_eq!(e.editor.doc.nodes[0].kind, original.nodes[0].kind);
            assert_eq!(
                e.editor.doc.nodes[0].mask_transform,
                original.nodes[0].mask_transform
            );
            let preview = e.editor.doc.clone();
            e.remove_vector_mask(cx);
            e.invert_vector_mask(cx);
            e.set_mask_edit_target(MaskEditTarget::RasterMask, cx);
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.tools.mask_edit_target, MaskEditTarget::VectorMask);
            assert!(e.cancel_photo_transform(cx));
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(delta, cx));
            assert!(e.commit_photo_transform(cx));
            assert_eq!(e.editor.history.len(), 1);
            e.repeat_photo_transform(false, cx);
            near(
                MaskEditTarget::VectorMask
                    .to_document(e.editor.doc.node(1).unwrap())
                    .unwrap(),
                delta * delta * start,
            );
            assert_eq!(e.editor.history.len(), 2);
            e.undo(cx);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn vector_mask_locks_tool_gates_and_failed_rasterization_preserve_both_components(
    cx: &mut TestAppContext,
) {
    let mut original = document(false);
    original.nodes[0].vector_mask.as_mut().unwrap().path = path(&[(8., 8.), (35., 8.), (24., 28.)]);
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.rasterize_vector_mask(cx);
            assert_eq!(e.editor.doc, original);
            e.tools.pen.selected = Some((0, 0));
            e.editor.doc.node_mut(1).unwrap().locks.pixels = true;
            assert!(e.pen_delete(cx));
            assert_eq!(
                e.editor
                    .doc
                    .node(1)
                    .unwrap()
                    .vector_mask
                    .as_ref()
                    .unwrap()
                    .path
                    .anchor_count(),
                2
            );
            e.undo(cx);
            e.editor.doc.node_mut(1).unwrap().locks.position = true;
            let locked = e.editor.doc.clone();
            e.tools.pen.selected = Some((0, 0));
            assert!(
                e.pen_delete(cx),
                "locked mask deletion is consumed without touching artwork"
            );
            e.toggle_vector_mask_link(cx);
            e.remove_vector_mask(cx);
            e.begin_photo_transform(false, cx);
            assert_eq!(e.editor.doc, locked);
            assert!(!e.photo_transform_active());
            e.editor.doc.node_mut(1).unwrap().locks.position = false;
            e.editor.doc.node_mut(1).unwrap().locked = true;
            let locked = e.editor.doc.clone();
            e.invert_vector_mask(cx);
            e.toggle_vector_mask(cx);
            assert_eq!(e.editor.doc, locked);
            e.editor.doc.node_mut(1).unwrap().locked = false;
            let before = e.editor.doc.clone();
            e.set_paint(PaintKind::Brush, cx);
            assert_eq!(e.tools.mask_edit_target, MaskEditTarget::VectorMask);
            assert!(e.paint_target(cx).is_none());
            e.pen_paint_along(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[test]
fn vector_mask_unrepresentable_handle_bounds_never_overflow_or_clip_geometry() {
    let mut doc = document(false);
    let mask = doc.nodes[0].vector_mask.as_mut().unwrap();
    mask.path = path(&[(-1e9, -1e9), (1e9, 1e9)]);
    mask.transform = DAffine2::from_scale(dvec2(100., 100.)).to_cols_array();
    let old = mask.path.clone();
    assert!(
        MaskEditTarget::VectorMask
            .bounds(&doc, &doc.nodes[0])
            .is_none()
    );
    assert!(Arc::ptr_eq(
        &doc.nodes[0].vector_mask.as_ref().unwrap().path,
        &old
    ));
}

#[gpui_kit::test]
fn vector_mask_pen_noop_and_single_anchor_drag_preserve_untouched_intrinsic_geometry(
    cx: &mut TestAppContext,
) {
    let mut original = document(false);
    let mask = original.nodes[0].vector_mask.as_mut().unwrap();
    mask.path = Arc::new(Path {
        subpaths: vec![
            SubPath {
                anchors: vec![
                    Anchor::corner((7.125, 8.375)),
                    Anchor::corner((39.625, 8.125)),
                    Anchor::corner((23.375, 35.625)),
                ],
                closed: true,
            },
            SubPath {
                anchors: vec![
                    Anchor::corner((16.125, 15.375)),
                    Anchor::corner((19.625, 20.125)),
                    Anchor::corner((13.375, 20.625)),
                ],
                closed: true,
            },
        ],
    });
    let local = mask.path.subpaths[0].anchors[0].p;
    let expected = mask.path.clone();
    let to_doc = MaskEditTarget::VectorMask
        .to_document(&original.nodes[0])
        .unwrap();
    let point = to_doc.transform_point2(dvec2(local.0, local.1));
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let event = MouseDownEvent {
                position: gpui_kit::point(px(0.), px(0.)),
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            };
            e.pen_down((point.x, point.y), &event, cx);
            let Some(Drag::Tool(tools::ToolDrag::Pen(drag))) = e.drag.take() else {
                panic!("anchor drag")
            };
            e.pen_move((point.x, point.y), drag.clone(), cx);
            e.pen_up(drag, cx);
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
            e.pen_down((point.x, point.y), &event, cx);
            let Some(Drag::Tool(tools::ToolDrag::Pen(drag))) = e.drag.take() else {
                panic!("anchor drag")
            };
            e.pen_move((point.x + 4., point.y - 3.), drag.clone(), cx);
            e.pen_up(drag, cx);
            let path = &e
                .editor
                .doc
                .node(1)
                .unwrap()
                .vector_mask
                .as_ref()
                .unwrap()
                .path;
            assert_eq!(
                &path.subpaths[0].anchors[1..],
                &expected.subpaths[0].anchors[1..]
            );
            assert_eq!(path.subpaths[1], expected.subpaths[1]);
            assert_ne!(path.subpaths[0].anchors[0], expected.subpaths[0].anchors[0]);
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn vector_mask_delete_keys_canvas_panel_repeated_empty_and_position_locked_are_safe(
    cx: &mut TestAppContext,
) {
    for panel in [false, true] {
        for locked in [false, true] {
            let mut original = document(false);
            original.nodes[0].vector_mask.as_mut().unwrap().path = path(&[(7.125, 8.375)]);
            original.nodes[0].locks.position = locked;
            let (editor, cx) = setup(cx, original.clone());
            cx.update(|window, cx| {
                editor.update(cx, |e, cx| {
                    e.tools.pen.selected = None;
                    window.focus(
                        if panel {
                            &e.panel_focus
                        } else {
                            &e.canvas_focus
                        },
                        cx,
                    );
                })
            });
            // No anchor selected must never fall back to deleting the layer.
            cx.simulate_keystrokes("delete backspace");
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |e, _| {
                    assert_eq!(e.editor.doc, original);
                    assert!(e.editor.history.is_empty());
                    e.tools.pen.selected = Some((0, 0));
                })
            });
            cx.simulate_keystrokes("backspace delete backspace delete");
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = editor.read(cx);
                assert_eq!(e.tools.mask_edit_target, MaskEditTarget::VectorMask);
                assert_eq!(e.editor.doc.nodes.len(), 1);
                let node = e.editor.doc.node(1).unwrap();
                assert_eq!(node.kind, original.nodes[0].kind);
                assert!(Arc::ptr_eq(
                    node.mask.as_ref().unwrap(),
                    original.nodes[0].mask.as_ref().unwrap()
                ));
                assert_eq!(node.mask_properties, original.nodes[0].mask_properties);
                assert_eq!(node.mask_transform, original.nodes[0].mask_transform);
                if locked {
                    assert_eq!(e.editor.doc, original);
                    assert!(e.editor.history.is_empty());
                } else {
                    assert!(node.vector_mask.as_ref().unwrap().path.is_empty());
                    assert_eq!(
                        node.vector_mask.as_ref().unwrap().empty_coverage,
                        EmptyVectorCoverage::HideAll
                    );
                    assert_eq!(e.editor.history.len(), 1);
                }
            });
        }
    }
}

fn numeric_enter(cx: &mut VisualTestContext) {
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn vector_mask_numeric_transform_repeated_enter_blur_keeps_one_affine_and_history_step(
    cx: &mut TestAppContext,
) {
    for modal in [false, true] {
        for field in ["Angle", "X", "Y", "W", "H"] {
            let mut original = document(false);
            original.nodes[0].vector_mask.as_mut().unwrap().path =
                path(&[(7., 8.), (39., 8.), (24., 30.)]);
            let (editor, cx) = setup(cx, original.clone());
            cx.update(|window, cx| {
                editor.update(cx, |e, cx| {
                    e.set_tool(Tool::Move, cx);
                    e.show_sidebar_tab(SidebarTab::Properties, cx);
                    if modal {
                        e.begin_photo_transform(false, cx);
                    }
                    window.focus(&e.canvas_focus, cx);
                })
            });
            cx.run_until_parked();
            let (w, h, start) = cx.update(|_, cx| {
                let (_, w, h, p) = editor.read(cx).transformable().unwrap();
                (w, h, p)
            });
            let (value, mut expected_frame) = match field {
                "Angle" => (30., start),
                "X" => (start.x + 9., start),
                "Y" => (start.y + 7., start),
                "W" => (w as f64 * 1.25, start),
                _ => (h as f64 * 1.5, start),
            };
            match field {
                "Angle" => expected_frame.rotation = value,
                "X" => expected_frame.x = value,
                "Y" => expected_frame.y = value,
                "W" => expected_frame.scale_x = value / w as f64,
                _ => expected_frame.scale_y = value / h as f64,
            }
            let expected = expected_frame.to_doc(w, h)
                * start.to_doc(w, h).inverse()
                * MaskEditTarget::VectorMask
                    .to_document(&original.nodes[0])
                    .unwrap();
            let field_id = format!("photo-transform-{field}");
            cx.update(|window, cx| {
                window
                    .within("sidebar-properties-content")
                    .click(SharedString::from(field_id.clone()), cx)
            });
            cx.run_until_parked();
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input(&format!("{value}"));
            numeric_enter(cx);
            let (accepted, revision) = cx.update(|_, cx| {
                let e = editor.read(cx);
                near(
                    MaskEditTarget::VectorMask
                        .to_document(e.editor.doc.node(1).unwrap())
                        .unwrap(),
                    expected,
                );
                (e.editor.doc.clone(), e.editor.revision)
            });
            numeric_enter(cx);
            numeric_enter(cx);
            cx.update(|window, cx| {
                let focus = editor.read(cx).canvas_focus.clone();
                window.focus(&focus, cx);
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = editor.read(cx);
                assert_eq!(
                    e.editor.doc, accepted,
                    "repeated Enter/Blur changed {field}, modal={modal}"
                );
                assert_eq!(e.editor.revision, revision);
                assert_eq!(e.editor.history.len(), usize::from(!modal));
                assert_eq!(e.editor.doc.nodes[0].kind, original.nodes[0].kind);
                assert!(
                    match (&e.editor.doc.nodes[0].mask, &original.nodes[0].mask) {
                        (Some(after), Some(before)) => Arc::ptr_eq(after, before),
                        (None, None) => true,
                        _ => false,
                    }
                );
            });
            let cancel = modal && field == "Angle";
            if cancel {
                cx.simulate_keystrokes("escape");
                cx.run_until_parked();
            } else if modal {
                numeric_enter(cx);
            }
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    assert!(!e.photo_transform_active());
                    assert_eq!(e.editor.history.len(), usize::from(!cancel));
                    if !cancel {
                        e.undo(cx);
                    }
                    assert_eq!(e.editor.doc, original);
                })
            });
        }
    }
}

#[gpui_kit::test]
fn vector_mask_smooth_handle_noop_alt_conversion_and_cancel_preserve_intrinsic_points(
    cx: &mut TestAppContext,
) {
    for reflected in [false, true] {
        for out in [false, true] {
            let mut original = document(false);
            if let NodeKind::Raster { placement, .. } = &mut original.nodes[0].kind {
                placement.flip_x = reflected;
            }
            original.nodes[0].vector_mask.as_mut().unwrap().path = Arc::new(Path {
                subpaths: vec![SubPath {
                    anchors: vec![
                        Anchor {
                            p: (7.125, 8.375),
                            h_in: (3.125, 5.375),
                            h_out: (13.525, 13.175),
                            smooth: true,
                        },
                        Anchor::corner((39., 35.)),
                        Anchor::corner((6., 35.)),
                    ],
                    closed: false,
                }],
            });
            let (editor, cx) = setup(cx, original.clone());
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    let (_, projected, _) = e.pen_target().unwrap();
                    let a = projected.subpaths[0].anchors[0];
                    let point = if out { a.h_out } else { a.h_in };
                    let drag = pen::PenDrag::Handle {
                        si: 0,
                        ai: 0,
                        out,
                        alt: false,
                        last: point,
                    };
                    e.tools.pen.edit_target = Some((1, MaskEditTarget::VectorMask));
                    let revision = e.editor.revision;
                    e.editor.begin("Adjust handle");
                    e.pen_move(point, drag.clone(), cx);
                    e.pen_move(point, drag.clone(), cx);
                    e.pen_up(drag.clone(), cx);
                    assert_eq!(e.editor.doc, original);
                    assert_eq!(e.editor.revision, revision);
                    assert!(e.editor.history.is_empty());
                    // Window coordinates may round the original grab slightly
                    // away from the geometric handle. An unchanged pointer
                    // must still keep both baseline handles exactly.
                    let rounded = (point.0 + 0.00001, point.1 - 0.00001);
                    let rounded_drag = pen::PenDrag::Handle {
                        si: 0,
                        ai: 0,
                        out,
                        alt: false,
                        last: rounded,
                    };
                    e.editor.begin("Adjust handle");
                    e.pen_move(rounded, rounded_drag.clone(), cx);
                    e.pen_up(rounded_drag, cx);
                    assert_eq!(e.editor.doc, original);
                    assert!(e.editor.history.is_empty());
                    e.editor.begin("Adjust handle");
                    e.drag = Some(Drag::Tool(tools::ToolDrag::Pen(drag.clone())));
                    e.pen_move((point.0 + 8., point.1 - 4.), drag, cx);
                    assert_ne!(e.editor.doc, original);
                    assert!(e.tool_cancel(cx));
                    assert_eq!(e.editor.doc, original);
                    assert!(e.editor.history.is_empty());
                    e.tools.pen.edit_target = Some((1, MaskEditTarget::VectorMask));
                    let alt = pen::PenDrag::Handle {
                        si: 0,
                        ai: 0,
                        out,
                        alt: true,
                        last: point,
                    };
                    e.editor.begin("Convert handle");
                    e.pen_move(point, alt.clone(), cx);
                    e.pen_up(alt, cx);
                    let a = e
                        .editor
                        .doc
                        .node(1)
                        .unwrap()
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .path
                        .subpaths[0]
                        .anchors[0];
                    let old = original.nodes[0]
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .path
                        .subpaths[0]
                        .anchors[0];
                    assert!(!a.smooth);
                    assert_eq!(a.p, old.p);
                    assert_eq!(a.h_in, old.h_in);
                    assert_eq!(a.h_out, old.h_out);
                    assert_eq!(e.editor.history.len(), 1);
                })
            });
        }
    }
}

#[gpui_kit::test]
fn vector_mask_focused_numeric_draft_rejects_intervening_revision(cx: &mut TestAppContext) {
    let mut original = document(false);
    original.nodes[0].vector_mask.as_mut().unwrap().path = path(&[(7., 8.), (39., 8.), (24., 30.)]);
    let (editor, cx) = setup(cx, original);
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_tool(Tool::Move, cx);
            e.show_sidebar_tab(SidebarTab::Properties, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window
            .within("sidebar-properties-content")
            .click("photo-transform-Angle", cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("30");
    cx.run_until_parked();
    let concurrent = cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.execute(
                Command::SetVectorMaskInverted {
                    id: 1,
                    inverted: true,
                },
                cx,
            );
            e.editor.doc.clone()
        })
    });
    numeric_enter(cx);
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, concurrent);
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn vector_mask_ordinary_move_scale_rotate_return_to_origin_restore_exact_affine_and_revision(
    cx: &mut TestAppContext,
) {
    for gesture in ["move", "scale", "rotate"] {
        let mut original = document(false);
        original.nodes[0].vector_mask.as_mut().unwrap().path =
            path(&[(7., 8.), (39., 8.), (24., 35.)]);
        let (editor, cx) = setup(cx, original.clone());
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.set_tool(Tool::Move, cx);
                e.snap = false;
            })
        });
        cx.run_until_parked();
        let (start, end, revision) = cx.update(|_, cx| {
            let e = editor.read(cx);
            let b = e.transform_box().unwrap();
            let start = match gesture {
                "move" => e
                    .doc_to_window(((b[0].0 + b[2].0) / 2., (b[0].1 + b[2].1) / 2.))
                    .unwrap(),
                "scale" => e.doc_to_window(b[2]).unwrap(),
                _ => {
                    let p = e.doc_to_window(b[0]).unwrap();
                    gpui_kit::point(p.x - px(12.), p.y - px(12.))
                }
            };
            (
                start,
                gpui_kit::point(start.x + px(22.), start.y + px(11.)),
                e.editor.revision,
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_ne!(e.editor.doc, original, "{gesture} should preview an edit");
            assert!(e.editor.in_transaction());
        });
        cx.simulate_mouse_move(start, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(start, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(
                e.editor.doc, original,
                "{gesture} must restore raw affine exactly"
            );
            assert!(e.editor.history.is_empty());
            assert_eq!(e.editor.revision, revision);
        });
    }
}

#[gpui_kit::test]
fn locked_disabled_vector_mask_to_selection_reads_only_independent_coverage(
    cx: &mut TestAppContext,
) {
    let mut original = document(false);
    original.nodes[0].locked = true;
    original.nodes[0].mask = Some(Arc::new(Mask::empty(80, 60, 0)));
    let mask = original.nodes[0].vector_mask.as_mut().unwrap();
    mask.path = path(&[(8., 8.), (35., 8.), (35., 30.), (8., 30.)]);
    mask.enabled = false;
    let sample = MaskEditTarget::VectorMask
        .to_document(&original.nodes[0])
        .unwrap()
        .transform_point2(dvec2(20., 18.));
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.vector_mask_to_selection(cx);
            let selection = e
                .editor
                .doc
                .selection
                .as_ref()
                .expect("locked coverage can be inspected");
            assert!(selection.get(sample.x.floor() as u32, sample.y.floor() as u32) > 0);
            let mut result = e.editor.doc.clone();
            result.selection = None;
            assert_eq!(result, original);
            assert_eq!(e.editor.history.len(), 1);
        })
    });
}

#[gpui_kit::test]
fn numeric_transform_rejected_anisotropic_path_edit_restores_accepted_values_and_applies(
    cx: &mut TestAppContext,
) {
    let click_field = |id: &'static str, cx: &mut VisualTestContext| {
        // Path style controls precede Transform in Properties. Reach its fields
        // through the real scroll container before dispatching a pointer click.
        cx.update(|window, cx| {
            let viewport = window
                .find(("sidebar-content", SidebarTab::Properties as usize))
                .bounds();
            let field = window
                .within("sidebar-properties-content")
                .find(id)
                .bounds();
            window.scroll(
                ("sidebar-content", SidebarTab::Properties as usize),
                gpui::ScrollDelta::Pixels(point(px(0.), viewport.center().y - field.center().y)),
                cx,
            );
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let viewport = window
                .find(("sidebar-content", SidebarTab::Properties as usize))
                .bounds();
            let field = window.within("sidebar-properties-content").find(id);
            assert!(field.visible());
            assert!(field.bounds().top() >= viewport.top());
            assert!(field.bounds().bottom() <= viewport.bottom());
            window.within("sidebar-properties-content").click(id, cx);
        });
        cx.run_until_parked();
    };
    for translate_first in [false, true] {
        let mut original = Document::new(300, 220);
        original.nodes.push(Node::path(
            1,
            "Stroked path",
            path(&[(50., 50.), (140., 50.), (120., 100.)]),
            emulsion_raster::vector::PathStyle {
                stroke: Some([20, 50, 180, 255]),
                width: 3.,
                fill: None,
                ..Default::default()
            },
            300,
            220,
        ));
        original.next_id = 2;
        let (editor, cx) = setup(cx, original.clone());
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| {
                e.select_layer_content(1, cx);
                e.set_tool(Tool::Move, cx);
                e.show_sidebar_tab(SidebarTab::Properties, cx);
                e.begin_photo_transform(false, cx);
                assert_eq!(e.tool, Tool::Move);
                assert!(e.photo_transform_active());
                window.focus(&e.canvas_focus, cx);
            })
        });
        cx.run_until_parked();
        let (_, w, _, frame) = cx.update(|_, cx| editor.read(cx).transformable().unwrap());
        if translate_first {
            click_field("photo-transform-X", cx);
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input(&format!("{}", frame.x + 9.));
            numeric_enter(cx);
        }
        let accepted = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        let width = w as f64 * frame.scale_x;
        click_field("photo-transform-W", cx);
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(&format!("{}", width + 13.));
        numeric_enter(cx);
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(
                e.editor.doc, accepted,
                "anisotropic stroked-path edit must reject"
            );
            assert!(e.photo_transform_active());
            assert!(e.editor.history.is_empty());
        });
        // Actual Apply is blocked while the numeric proposal is rejected.
        cx.update(|window, cx| {
            let focus = editor.read(cx).canvas_focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        numeric_enter(cx);
        cx.update(|_, cx| assert!(editor.read(cx).photo_transform_active()));
        click_field("photo-transform-W", cx);
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(&format!("{width}"));
        numeric_enter(cx);
        cx.update(|window, cx| {
            let focus = editor.read(cx).canvas_focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        numeric_enter(cx);
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert!(
                    !e.photo_transform_active(),
                    "restored accepted numbers must allow Apply"
                );
                assert_eq!(e.editor.doc, accepted);
                assert_eq!(e.editor.history.len(), usize::from(translate_first));
                if translate_first {
                    e.undo(cx);
                }
                assert_eq!(e.editor.doc, original);
            })
        });
    }
}

#[gpui_kit::test]
fn vector_mask_delete_keys_after_move_or_transform_commit_never_delete_artwork(
    cx: &mut TestAppContext,
) {
    for panel in [false, true] {
        for transform_first in [false, true] {
            let mut original = document(false);
            original.nodes[0].vector_mask.as_mut().unwrap().path =
                path(&[(7., 8.), (39., 8.), (24., 30.)]);
            let (editor, cx) = setup(cx, original);
            cx.update(|window, cx| {
                editor.update(cx, |e, cx| {
                    if transform_first {
                        e.begin_photo_transform(false, cx);
                        assert!(
                            e.photo_transform_delta(DAffine2::from_translation(dvec2(9., 3.)), cx)
                        );
                        assert!(e.commit_photo_transform(cx));
                    } else {
                        e.set_tool(Tool::Move, cx);
                    }
                    assert_eq!(e.tool, Tool::Move);
                    assert_eq!(e.tools.mask_edit_target, MaskEditTarget::VectorMask);
                    window.focus(
                        if panel {
                            &e.panel_focus
                        } else {
                            &e.canvas_focus
                        },
                        cx,
                    );
                })
            });
            cx.run_until_parked();
            let (before, revision, history) = cx.update(|_, cx| {
                let e = editor.read(cx);
                (
                    e.editor.doc.clone(),
                    e.editor.revision,
                    e.editor.history.len(),
                )
            });
            cx.simulate_keystrokes("delete backspace delete backspace");
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = editor.read(cx);
                assert_eq!(e.editor.doc, before);
                assert_eq!(e.editor.revision, revision);
                assert_eq!(e.editor.history.len(), history);
                assert_eq!(e.editor.doc.nodes.len(), 1);
                assert_eq!(e.tools.mask_edit_target, MaskEditTarget::VectorMask);
            });
        }
    }
}

#[gpui_kit::test]
fn explicit_delete_layer_action_remains_distinct_from_mask_keyboard_delete(
    cx: &mut TestAppContext,
) {
    for target in [MaskEditTarget::RasterMask, MaskEditTarget::VectorMask] {
        let original = document(false);
        let (editor, cx) = setup(cx, original.clone());
        cx.update(|window, cx| {
            if target == MaskEditTarget::RasterMask {
                editor.update(cx, |e, cx| e.select_layer_mask(1, cx));
            }
            let focus = editor.read(cx).panel_focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("delete");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
        cx.update(|window, cx| window.dispatch_action(Box::new(crate::actions::DeleteNode), cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert!(e.editor.doc.nodes.is_empty());
                assert_eq!(e.editor.history.len(), 1);
                e.undo(cx);
                assert_eq!(e.editor.doc, original);
            })
        });
    }
}

#[gpui_kit::test]
fn raster_mask_delete_keys_without_pixel_selection_are_safe_across_tools_and_committed_transform(
    cx: &mut TestAppContext,
) {
    for panel in [false, true] {
        for transform_first in [false, true] {
            for tool in [Tool::Mask, Tool::Brush, Tool::Move] {
                let original = document(false);
                let (editor, cx) = setup(cx, original);
                cx.update(|window, cx| {
                    editor.update(cx, |e, cx| {
                        if transform_first {
                            // This fixture retains a linked raster mask. Finish
                            // a content transform, then select its raster component
                            // for this key test without unlinking it.
                            e.select_layer_content(1, cx);
                            e.begin_photo_transform(false, cx);
                            assert!(e.photo_transform_delta(
                                DAffine2::from_translation(dvec2(9., 3.)),
                                cx
                            ));
                            assert!(e.commit_photo_transform(cx));
                        }
                        e.select_layer_mask(1, cx);
                        e.set_tool(tool, cx);
                        assert_eq!(e.tools.mask_edit_target, MaskEditTarget::RasterMask);
                        assert!(e.editor.doc.selection.is_none());
                        window.focus(
                            if panel {
                                &e.panel_focus
                            } else {
                                &e.canvas_focus
                            },
                            cx,
                        );
                    })
                });
                cx.run_until_parked();
                let (before, revision, history) = cx.update(|_, cx| {
                    let e = editor.read(cx);
                    (
                        e.editor.doc.clone(),
                        e.editor.revision,
                        e.editor.history.len(),
                    )
                });
                cx.simulate_keystrokes("delete backspace delete backspace");
                cx.run_until_parked();
                cx.update(|_, cx| {
                    let e = editor.read(cx);
                    assert_eq!(e.editor.doc, before);
                    assert_eq!(e.editor.revision, revision);
                    assert_eq!(e.editor.history.len(), history);
                    assert_eq!(e.editor.doc.nodes.len(), 1);
                    assert_eq!(e.tools.mask_edit_target, MaskEditTarget::RasterMask);
                });
            }
        }
    }
}
