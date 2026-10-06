//! Regression inventory for independent Smart filter-mask routing. Native
//! renderer/GPU and file roundtrip verification are separate integration gates.
use super::*;
use ::core::prelude::v1::test;
use emulsion_core::{EmptyVectorCoverage, MaskProperties, SmartFilterMask, VectorMask};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::{IRect, Mask, paint::Brush, select};
use glam::{DAffine2, dvec2};
use gpui_kit::{TestAppContext, VisualTestContext, test::TestWindowExt};

pub(super) fn document() -> Document {
    let mut doc = Document::new(160, 120);
    let mut node = Node::smart(
        1,
        "Smart",
        Arc::new(Raster::solid(48, 32, [0.2, 0.3, 0.4, 1.])),
        vec![Filter::GaussianBlur { radius: 2. }],
        Placement::at(40., 36.),
    );
    node.mask = Some(Arc::new(Mask::white(48, 32)));
    node.vector_mask = Some(VectorMask::empty(EmptyVectorCoverage::RevealAll));
    if let NodeKind::Smart { filter_mask, .. } = &mut node.kind {
        *filter_mask = Some(SmartFilterMask::new(Arc::new(Mask::white(48, 32))));
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
pub(super) fn mask(doc: &Document) -> &SmartFilterMask {
    smart_filter_mask_ui::descriptor(doc.node(1).unwrap()).unwrap()
}
pub(super) fn setup(
    cx: &mut TestAppContext,
    doc: Document,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = crate::tests::open(cx, doc);
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|window, cx| {
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| {
            e.select_smart_filter_mask(1, cx);
            e.tools.quick_shape = false;
            e.tools.brush = Brush {
                size: 6.,
                hardness: 1.,
                stabilizer: 0.,
                taper_end: 0.,
                ..Default::default()
            };
            e.set_fg([0, 0, 0, 255], cx);
            e.snap = false;
            window.focus(&e.canvas_focus, cx);
        });
        editor
    });
    cx.run_until_parked();
    (editor, cx)
}
pub(super) fn drag(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    a: (f64, f64),
    b: (f64, f64),
) {
    let (a, b) = cx.update(|_, cx| {
        let e = editor.read(cx);
        (e.doc_to_window(a).unwrap(), e.doc_to_window(b).unwrap())
    });
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    for i in 1..=8 {
        cx.simulate_mouse_move(
            a + (b - a) * (i as f32 / 8.),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
    }
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}
pub(super) fn unchanged_source_and_layer_masks(doc: &Document, original: &Document) {
    let (a, b) = (doc.node(1).unwrap(), original.node(1).unwrap());
    // These are untouched in-memory planes, so retain their shared allocation.
    assert_eq!(
        a.mask.as_ref().map(Arc::as_ptr),
        b.mask.as_ref().map(Arc::as_ptr)
    );
    assert_eq!(a.mask_transform, b.mask_transform);
    assert_eq!(a.mask_properties, b.mask_properties);
    assert_eq!(a.vector_mask, b.vector_mask);
    let (
        NodeKind::Smart {
            source: sa,
            filters: fa,
            filter_styles: styles_a,
            placement: pa,
            cache: ca,
            offset: oa,
            ..
        },
        NodeKind::Smart {
            source: sb,
            filters: fb,
            filter_styles: styles_b,
            placement: pb,
            cache: cb,
            offset: ob,
            ..
        },
    ) = (&a.kind, &b.kind)
    else {
        panic!("Smart content must stay editable")
    };
    assert!(Arc::ptr_eq(sa, sb));
    assert!(
        Arc::ptr_eq(ca, cb),
        "mask edits never rerun or overwrite the raw filter cache"
    );
    assert_eq!((fa, styles_a, pa, oa), (fb, styles_b, pb, ob));
}
fn near(a: DAffine2, b: DAffine2) {
    for (a, b) in a.to_cols_array().into_iter().zip(b.to_cols_array()) {
        assert!((a - b).abs() < 1e-8, "{a} != {b}");
    }
}

#[test]
fn first_mask_projects_captured_selection_into_expanded_cache_grid() {
    let selection = select::rect(20, 12, 2., 3., 7., 5.);
    let mask = smart_filter_mask_ui::initial_mask(
        12,
        8,
        (-2, -1),
        DAffine2::from_translation(dvec2(3., 3.)),
        Some(&selection),
        false,
    )
    .unwrap();
    assert_eq!(
        mask.transform.affine().unwrap().to_cols_array(),
        DAffine2::from_translation(dvec2(-2., -1.)).to_cols_array()
    );
    assert_eq!(mask.pixels.fill(), 0);
    assert_eq!(mask.pixels.get(0, 0), 0);
    assert_eq!(mask.pixels.get(1, 1), 255);
    assert_eq!(mask.pixels.get(8, 1), 0);
    assert_eq!(mask.properties, MaskProperties::default());
    assert!(mask.enabled && mask.linked);
    let white = smart_filter_mask_ui::initial_mask(4, 3, (-1, -1), DAffine2::IDENTITY, None, false)
        .unwrap();
    let black =
        smart_filter_mask_ui::initial_mask(4, 3, (-1, -1), DAffine2::IDENTITY, None, true).unwrap();
    assert_eq!(white.pixels.fill(), 255);
    assert_eq!(black.pixels.fill(), 0);
    let large =
        smart_filter_mask_ui::initial_mask(5_000, 4_000, (-4, -4), DAffine2::IDENTITY, None, false)
            .unwrap();
    assert_eq!(
        large.transform.affine().unwrap().to_cols_array(),
        DAffine2::IDENTITY.to_cols_array()
    );
    assert_eq!(large.pixels.fill(), 255);
    assert!(
        smart_filter_mask_ui::initial_mask(30_001, 1, (0, 0), DAffine2::IDENTITY, None, false)
            .is_err()
    );
    assert!(
        smart_filter_mask_ui::initial_mask(
            4_001,
            4_000,
            (0, 0),
            DAffine2::IDENTITY,
            Some(&selection),
            false
        )
        .is_err()
    );
}

#[gpui_kit::test]
fn filter_header_click_alt_shift_distinguish_all_three_components(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    for (name, target) in [
        ("layer-mask", MaskEditTarget::RasterMask),
        ("layer-vector-mask", MaskEditTarget::VectorMask),
        ("smart-filter-mask", MaskEditTarget::SmartFilterMask),
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
    let point = cx.update(|window, _| window.find(("smart-filter-mask", 1_u64)).bounds().center());
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
        assert!(!mask(&e.editor.doc).enabled);
        assert!(e.editor.doc.node(1).unwrap().mask_enabled);
        assert!(
            e.editor
                .doc
                .node(1)
                .unwrap()
                .vector_mask
                .as_ref()
                .unwrap()
                .enabled
        );
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
    });
}

#[gpui_kit::test]
fn brush_eraser_pad_halo_and_clip_only_filter_mask_as_one_undo(cx: &mut TestAppContext) {
    for kind in [PaintKind::Brush, PaintKind::Eraser] {
        let mut original = document();
        original.selection = Some(Arc::new(select::rect(160, 120, 34., 36., 35., 32.)));
        let (editor, cx) = setup(cx, original.clone());
        cx.update(|_, cx| editor.update(cx, |e, cx| e.set_paint(kind, cx)));
        // Start inside the result halo, then cross the selection's right edge.
        drag(&editor, cx, (38., 48.), (78., 48.));
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                let n = e.editor.doc.node(1).unwrap();
                let m = mask(&e.editor.doc);
                let inverse = MaskEditTarget::SmartFilterMask
                    .to_document(n)
                    .unwrap()
                    .inverse();
                let black = inverse.transform_point2(dvec2(44.5, 48.5));
                let clipped = inverse.transform_point2(dvec2(76.5, 48.5));
                assert_eq!(m.pixels.get(black.x as u32, black.y as u32), 0);
                assert_eq!(m.pixels.get(clipped.x as u32, clipped.y as u32), 255);
                assert!(m.pixels.width() > 48 && m.pixels.height() > 32);
                unchanged_source_and_layer_masks(&e.editor.doc, &original);
                assert_eq!(e.editor.history.len(), 1);
                e.undo(cx);
                assert_eq!(e.editor.doc, original);
                e.redo(cx);
                assert!(mask(&e.editor.doc).pixels.width() > 48);
            })
        });
    }
}

#[gpui_kit::test]
fn gradient_mask_capture_and_undo_never_create_an_artwork_layer(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_paint(PaintKind::Gradient, cx);
            e.tools.bg = [255; 4];
        })
    });
    drag(&editor, cx, (40., 48.), (88., 48.));
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 1);
            let m = mask(&e.editor.doc);
            assert!(
                m.pixels
                    .read_rect(m.pixels.bounds())
                    .iter()
                    .any(|value| *value > 0 && *value < 255)
            );
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            let captured = tools::ToolDrag::Gradient {
                start: (40., 48.),
                end: (88., 48.),
                target: tools::PaintTarget::Component(MaskEditTarget::SmartFilterMask),
                node: Some(1),
            };
            e.select_layer_mask(1, cx);
            e.tool_up(captured, cx);
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
        })
    });
}

#[gpui_kit::test]
fn filter_mask_properties_invert_delete_readd_are_component_only(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            for (property, value) in [
                (photo_masks::MaskProperty::Density, 35.),
                (photo_masks::MaskProperty::Feather, 1.5),
            ] {
                e.apply_mask_property(1, MaskEditTarget::SmartFilterMask, property, value, cx);
            }
            let props = mask(&e.editor.doc).properties;
            assert_eq!(
                props,
                MaskProperties {
                    density: 0.35,
                    feather: 1.5
                }
            );
            let before = e.editor.history.len();
            e.apply_mask_property(
                1,
                MaskEditTarget::SmartFilterMask,
                photo_masks::MaskProperty::Density,
                35.,
                cx,
            );
            assert_eq!(e.editor.history.len(), before);
            e.invert_smart_filter_mask(1, cx);
            assert_eq!(mask(&e.editor.doc).properties, props);
            assert_eq!(mask(&e.editor.doc).pixels.fill(), 0);
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
            e.delete_smart_filter_mask(1, cx);
            assert_eq!(e.tools.mask_edit_target, MaskEditTarget::Content);
            assert!(smart_filter_mask_ui::descriptor(e.editor.doc.node(1).unwrap()).is_none());
            e.add_smart_filter_mask(1, false, false, cx);
            assert_eq!(mask(&e.editor.doc).pixels.fill(), 255);
            assert_eq!(mask(&e.editor.doc).properties, MaskProperties::default());
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
        })
    });
}

#[gpui_kit::test]
fn filter_mask_transform_cancel_commit_repeat_copy_rejection_and_nudge(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let start = MaskEditTarget::SmartFilterMask
                .to_document(e.editor.doc.node(1).unwrap())
                .unwrap();
            let delta = DAffine2::from_translation(dvec2(9., -3.));
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_active());
            assert!(e.photo_transform_delta(delta, cx));
            near(
                MaskEditTarget::SmartFilterMask
                    .to_document(e.editor.doc.node(1).unwrap())
                    .unwrap(),
                delta * start,
            );
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
            assert!(e.cancel_photo_transform(cx));
            assert_eq!(e.editor.doc, original);
            e.begin_photo_transform(false, cx);
            assert!(e.photo_transform_delta(delta, cx));
            assert!(e.commit_photo_transform(cx));
            assert_eq!(e.editor.history.len(), 1);
            e.repeat_photo_transform(false, cx);
            near(
                MaskEditTarget::SmartFilterMask
                    .to_document(e.editor.doc.node(1).unwrap())
                    .unwrap(),
                delta * delta * start,
            );
            assert_eq!(e.editor.history.len(), 2);
            let before = e.editor.doc.clone();
            e.begin_photo_transform(true, cx);
            e.repeat_photo_transform(true, cx);
            assert!(!e.photo_transform_active());
            assert_eq!(e.editor.doc, before);
            e.undo(cx);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.nudge_selected(2., 1., cx);
            near(
                MaskEditTarget::SmartFilterMask
                    .to_document(e.editor.doc.node(1).unwrap())
                    .unwrap(),
                DAffine2::from_translation(dvec2(2., 1.)) * start,
            );
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
        })
    });
}

#[gpui_kit::test]
fn mask_locks_and_unsupported_tools_cannot_fall_through_to_source(cx: &mut TestAppContext) {
    let mut original = document();
    original.nodes[0].locks.pixels = true;
    original.nodes[0].locks.transparency = true;
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.apply_mask_property(
                1,
                MaskEditTarget::SmartFilterMask,
                photo_masks::MaskProperty::Density,
                50.,
                cx,
            );
            assert_eq!(mask(&e.editor.doc).properties.density, 0.5);
            e.undo(cx);
            e.editor.doc.node_mut(1).unwrap().locks.position = true;
            let before = e.editor.doc.clone();
            e.nudge_selected(4., 2., cx);
            e.begin_photo_transform(false, cx);
            assert_eq!(e.editor.doc, before);
            assert!(!e.photo_transform_active());
            e.set_tool(Tool::Pen, cx);
        })
    });
    let p = cx.update(|_, cx| editor.read(cx).doc_to_window((55., 48.)).unwrap());
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    for kind in [PaintKind::Smudge, PaintKind::Bucket, PaintKind::Liquify] {
        cx.update(|_, cx| editor.update(cx, |e, cx| e.set_paint(kind, cx)));
        drag(&editor, cx, (50., 48.), (60., 48.));
    }
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 1);
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
            assert_eq!(mask(&e.editor.doc), mask(&original));
            e.editor.doc.node_mut(1).unwrap().locked = true;
            let before = e.editor.doc.clone();
            e.invert_smart_filter_mask(1, cx);
            e.delete_smart_filter_mask(1, cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn first_filter_captures_selection_then_commits_mask_and_conversion_in_one_undo(
    cx: &mut TestAppContext,
) {
    let mut original = Document::new(100, 80);
    let mut node = Node::raster(
        1,
        "Photo",
        Arc::new(Raster::solid(32, 24, [0.2, 0.3, 0.4, 1.])),
        Placement::at(20., 20.),
    );
    node.mask = Some(Arc::new(Mask::white(32, 24)));
    original.nodes.push(node);
    original.next_id = 2;
    original.selection = Some(Arc::new(select::rect(100, 80, 15., 18., 20., 28.)));
    for newer_selection in [
        None,
        Some(Arc::new(select::rect(100, 80, 40., 32., 12., 10.))),
    ] {
        let (editor, cx) = setup(cx, original.clone());
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                // Selection is deliberately different by the time the worker returns.
                e.execute(
                    Command::SetSelection {
                        selection: newer_selection.clone(),
                    },
                    cx,
                );
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                let n = e.editor.doc.node(1).unwrap();
                let NodeKind::Smart {
                    cache,
                    offset,
                    filters,
                    filter_styles,
                    ..
                } = &n.kind
                else {
                    panic!("conversion")
                };
                assert_eq!(filters.len(), 1);
                assert_eq!(filter_styles.len(), 1);
                let expected = smart_filter_mask_ui::initial_mask(
                    cache.width(),
                    cache.height(),
                    *offset,
                    DAffine2::from_translation(dvec2(20., 20.)),
                    original.selection.as_deref(),
                    false,
                )
                .unwrap();
                let actual = mask(&e.editor.doc);
                assert_eq!(actual.transform, expected.transform);
                assert_eq!(
                    actual.pixels.read_rect(actual.pixels.bounds()),
                    expected.pixels.read_rect(expected.pixels.bounds())
                );
                assert_eq!(
                    n.mask.as_ref().map(Arc::as_ptr),
                    original.nodes[0].mask.as_ref().map(Arc::as_ptr)
                );
                assert_eq!(
                    e.editor.doc.selection.as_ref().map(Arc::as_ptr),
                    newer_selection.as_ref().map(Arc::as_ptr)
                );
                assert_eq!(
                    e.editor.history.len(),
                    2,
                    "selection edit plus one atomic filter application"
                );
                let committed = e.editor.doc.clone();
                e.undo(cx);
                assert_eq!(e.editor.doc.nodes, original.nodes);
                assert_eq!(
                    e.editor.doc.selection.as_ref().map(Arc::as_ptr),
                    newer_selection.as_ref().map(Arc::as_ptr)
                );
                if newer_selection.is_none() {
                    assert!(e.editor.doc.selection.is_none());
                }
                e.undo(cx);
                assert_eq!(e.editor.doc, original);
                e.redo(cx);
                e.redo(cx);
                assert_eq!(e.editor.doc, committed);
            })
        });
    }
}

#[gpui_kit::test]
fn selection_change_does_not_revive_filter_superseded_by_newer_job(cx: &mut TestAppContext) {
    let original = document();
    let selection = Arc::new(select::rect(160, 120, 40., 36., 12., 10.));
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_filters_enabled(1, false, cx);
            let newer_job = e.begin_edit_job().unwrap();
            e.execute(
                Command::SetSelection {
                    selection: Some(selection.clone()),
                },
                cx,
            );
            assert!(!e.edit_is_current(newer_job));
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert!(!e.smart.has_pending());
            assert_eq!(e.editor.doc.nodes, original.nodes);
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
            assert!(Arc::ptr_eq(
                e.editor.doc.selection.as_ref().unwrap(),
                &selection
            ));
            assert_eq!(e.editor.history.len(), 1, "only the selection edit commits");
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn identical_filters_retain_exact_style_after_async_remove_and_dormant_mask_returns(
    cx: &mut TestAppContext,
) {
    let mut original = document();
    let styles = vec![
        FilterStyle {
            enabled: true,
            opacity: 0.25,
            blend: emulsion_raster::BlendMode::Multiply,
        },
        FilterStyle {
            enabled: true,
            opacity: 0.75,
            blend: emulsion_raster::BlendMode::Screen,
        },
    ];
    Command::SetFilterStack {
        id: 1,
        filters: vec![Filter::GaussianBlur { radius: 2. }; 2],
        styles: styles.clone(),
    }
    .apply(&mut original)
    .unwrap();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| editor.update(cx, |e, cx| e.remove_filter(1, 0, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let NodeKind::Smart { filter_styles, .. } = &e.editor.doc.node(1).unwrap().kind else {
                panic!()
            };
            assert_eq!(filter_styles, &styles[1..]);
            assert_eq!(mask(&e.editor.doc), mask(&original));
            e.remove_filter(1, 0, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .find(("smart-filter-mask", 1_u64))
                .bounds()
                .size
                .width
                > px(0.)
        );
        editor.update(cx, |e, cx| {
            let NodeKind::Smart { filters, .. } = &e.editor.doc.node(1).unwrap().kind else {
                panic!()
            };
            assert!(filters.is_empty());
            assert_eq!(mask(&e.editor.doc), mask(&original));
            e.add_filter(1, Filter::GaussianBlur { radius: 1. }, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(mask(&editor.read(cx).editor.doc), mask(&original)));
}

#[gpui_kit::test]
fn target_switch_and_source_lock_cancel_pending_filter_without_orphan_mask(
    cx: &mut TestAppContext,
) {
    for mode in 0..3 {
        let mut original = document();
        Command::SetFilters {
            id: 1,
            filters: Vec::new(),
        }
        .apply(&mut original)
        .unwrap();
        Command::SetSmartFilterMask { id: 1, mask: None }
            .apply(&mut original)
            .unwrap();
        let (editor, cx) = setup(cx, original.clone());
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.add_filter(1, Filter::GaussianBlur { radius: 3. }, cx);
                match mode {
                    0 => e.select_layer_mask(1, cx),
                    1 => e.editor.doc.node_mut(1).unwrap().locks.pixels = true,
                    _ => e.undo(cx),
                }
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            let NodeKind::Smart {
                filters,
                filter_mask,
                ..
            } = &e.editor.doc.node(1).unwrap().kind
            else {
                panic!()
            };
            assert!(filters.is_empty());
            assert!(filter_mask.is_none());
            assert!(e.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn late_stroke_component_and_missing_mask_do_not_write_layer_mask(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.select_layer_mask(1, cx);
            e.commit_stroke(
                1,
                Raster::solid(48, 32, [0., 0., 0., 1.]),
                IRect::new(0, 0, 10, 10),
                "Paint filter mask",
                tools::PaintTarget::Component(MaskEditTarget::SmartFilterMask),
                cx,
            );
            assert_eq!(e.editor.doc, original);
            e.select_smart_filter_mask(1, cx);
            e.delete_smart_filter_mask(1, cx);
            let deleted = e.editor.doc.clone();
            e.tools.mask_edit_target = MaskEditTarget::SmartFilterMask;
            e.commit_stroke(
                1,
                Raster::solid(48, 32, [0., 0., 0., 1.]),
                IRect::new(0, 0, 10, 10),
                "Paint filter mask",
                tools::PaintTarget::Component(MaskEditTarget::SmartFilterMask),
                cx,
            );
            assert_eq!(e.editor.doc, deleted);
        })
    });
}

#[gpui_kit::test]
fn filter_mask_numeric_enter_escape_invalid_and_target_switch_capture(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    let key = SliderKey::MaskProperty(
        1,
        MaskEditTarget::SmartFilterMask,
        photo_masks::MaskProperty::Density,
        photo_masks::MaskControlSurface::Taskbar,
    );
    let edit = |cx: &mut VisualTestContext, text: &str| {
        let point = cx.update(|window, _| {
            window
                .find(SharedString::from(format!("mask-value-{key:?}")))
                .bounds()
                .center()
        });
        cx.simulate_click(point, Modifiers::none());
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(text);
        cx.run_until_parked();
    };
    edit(cx, "37");
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(mask(&e.editor.doc).properties.density, 0.37);
        assert_eq!(e.editor.history.len(), 1);
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
    });
    edit(cx, "NaN");
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(editor.read(cx).tools.photo_masks.edit.is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.tools.photo_masks.edit.is_none());
        assert_eq!(e.editor.history.len(), 1);
    });
    edit(cx, "12");
    cx.update(|_, cx| editor.update(cx, |e, cx| e.select_layer_mask(1, cx)));
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.tools.photo_masks.edit.is_none());
        assert_eq!(mask(&e.editor.doc).properties.density, 0.37);
        assert_eq!(
            e.editor.doc.node(1).unwrap().mask_properties,
            MaskProperties::default()
        );
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn filter_mask_slider_one_gesture_undo_and_escape(cx: &mut TestAppContext) {
    let original = document();
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let key = SliderKey::MaskProperty(
                1,
                MaskEditTarget::SmartFilterMask,
                photo_masks::MaskProperty::Density,
                photo_masks::MaskControlSurface::Taskbar,
            );
            let start = |e: &mut EditorView, cx: &mut Context<EditorView>| {
                e.tracks.entry(key).or_default().set(Some(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(100.), px(24.)),
                )));
                e.slider_down(
                    key,
                    (0., 100., 1.),
                    &MouseDownEvent {
                        position: point(px(50.), px(12.)),
                        button: MouseButton::Left,
                        modifiers: Modifiers::none(),
                        click_count: 1,
                        first_mouse: false,
                    },
                    cx,
                );
            };
            start(e, cx);
            e.apply_slider(key, 20., cx);
            e.apply_slider(key, 40., cx);
            assert!(e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
            e.drag_end(cx);
            assert_eq!(e.editor.history.len(), 1);
            assert_eq!(mask(&e.editor.doc).properties.density, 0.4);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            start(e, cx);
            e.apply_slider(key, 20., cx);
            assert!(e.tool_cancel(cx));
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.in_transaction());
        })
    });
}

#[gpui_kit::test]
fn clipped_first_dab_preserves_live_flags_when_padding_is_later_published(cx: &mut TestAppContext) {
    let mut original = document();
    original.selection = Some(Arc::new(select::rect(160, 120, 50., 36., 30., 32.)));
    let (editor, cx) = setup(cx, original.clone());
    let (a, b) = cx.update(|_, cx| {
        let e = editor.read(cx);
        (
            e.doc_to_window((38., 48.)).unwrap(),
            e.doc_to_window((60., 48.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(
                mask(&e.editor.doc).pixels.width(),
                48,
                "a clipped dab does not publish padding"
            );
            e.execute(
                Command::SetSmartFilterMaskEnabled {
                    id: 1,
                    enabled: false,
                },
                cx,
            );
            e.execute(
                Command::SetSmartFilterMaskLinked {
                    id: 1,
                    linked: false,
                },
                cx,
            );
            e.execute(
                Command::SetSmartFilterMaskProperties {
                    id: 1,
                    properties: MaskProperties {
                        density: 0.4,
                        feather: 1.5,
                    },
                },
                cx,
            );
        })
    });
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let mask = mask(&e.editor.doc);
        assert!(mask.pixels.width() > 48);
        assert!(!mask.enabled && !mask.linked);
        assert_eq!(
            mask.properties,
            MaskProperties {
                density: 0.4,
                feather: 1.5
            }
        );
        assert!(mask.pixels.read_rect(mask.pixels.bounds()).contains(&0));
        unchanged_source_and_layer_masks(&e.editor.doc, &original);
    });
}

#[gpui_kit::test]
fn selection_replaces_only_filter_mask_and_loading_ignores_enabled_flag(cx: &mut TestAppContext) {
    let mut original = document();
    original.selection = Some(Arc::new(select::rect(160, 120, 40., 36., 20., 32.)));
    let (editor, cx) = setup(cx, original.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.add_smart_filter_mask(1, false, true, cx);
            assert_eq!(e.editor.history.len(), 1);
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
            let effective = MaskEditTarget::SmartFilterMask
                .inspection(&e.editor.doc, e.editor.doc.node(1).unwrap())
                .unwrap()
                .unwrap();
            assert!(effective.read_rect(effective.bounds()).contains(&0));
            e.execute(
                Command::SetSmartFilterMaskEnabled {
                    id: 1,
                    enabled: false,
                },
                cx,
            );
            e.execute(Command::SetSelection { selection: None }, cx);
            e.component_mask_to_selection(1, MaskEditTarget::SmartFilterMask, cx);
            let selection = e.editor.doc.selection.as_ref().unwrap();
            assert_eq!(selection.get(45, 42), 255);
            assert_eq!(selection.get(70, 42), 0);
            assert!(!mask(&e.editor.doc).enabled);
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
            let count = e.editor.doc.nodes.len();
            e.quick_adjust("brightness_contrast", cx);
            e.quick_desaturate(cx);
            e.apply_filter_key("gaussian_blur", cx);
            assert_eq!(e.editor.doc.nodes.len(), count);
            unchanged_source_and_layer_masks(&e.editor.doc, &original);
        })
    });
}
