//! Deterministic first-mask capture coverage across coalescing and slider previews.
use super::super::smart_filter_mask_tests::{mask, setup};
use super::*;
use ::core::prelude::v1::test;
use emulsion_core::{EmptyVectorCoverage, SmartFilterMask, VectorMask};
use emulsion_raster::select;
use gpui_kit::TestAppContext;

fn document(smart: bool) -> Document {
    let mut doc = Document::new(100, 80);
    let mut node = Node::raster(
        1,
        "Photo",
        Arc::new(Raster::from_fn(32, 24, [0; 4], |x, y| {
            [x as u16 * 1000, y as u16 * 1000, 30000, 65535]
        })),
        Placement::at(20., 20.),
    );
    node.mask = Some(Arc::new(Mask::white(32, 24)));
    node.vector_mask = Some(VectorMask::empty(EmptyVectorCoverage::RevealAll));
    doc.nodes.push(node);
    doc.next_id = 2;
    doc.selection = Some(Arc::new(select::rect(100, 80, 15., 18., 20., 28.)));
    if smart {
        Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
    }
    doc
}

fn newer_selection() -> Option<Arc<Mask>> {
    Some(Arc::new(select::rect(100, 80, 40., 32., 12., 10.)))
}

fn exact_mask(actual: &SmartFilterMask, expected: &SmartFilterMask) {
    assert_eq!(actual.transform, expected.transform);
    assert_eq!(actual.properties, expected.properties);
    assert_eq!(
        (actual.enabled, actual.linked),
        (expected.enabled, expected.linked)
    );
    assert_eq!(actual.pixels.bounds(), expected.pixels.bounds());
    assert_eq!(actual.pixels.fill(), expected.pixels.fill());
    assert_eq!(
        actual.pixels.read_rect(actual.pixels.bounds()),
        expected.pixels.read_rect(expected.pixels.bounds())
    );
}

fn exact_baseline_nodes(actual: &Document, expected: &Document) {
    assert_eq!(actual.nodes, expected.nodes);
    if let NodeKind::Smart {
        cache: expected_cache,
        offset: expected_offset,
        ..
    } = &expected.node(1).unwrap().kind
    {
        let NodeKind::Smart { cache, offset, .. } = &actual.node(1).unwrap().kind else {
            panic!("empty Smart baseline restored")
        };
        assert!(Arc::ptr_eq(cache, expected_cache));
        assert_eq!(offset, expected_offset);
    }
}

fn verify(
    actual: &Document,
    original: &Document,
    captured_selection: Option<&Mask>,
    current_selection: &Option<Arc<Mask>>,
    radius: f32,
    opacity: f32,
) -> Arc<Raster> {
    let node = actual.node(1).unwrap();
    let before = original.node(1).unwrap();
    let NodeKind::Smart {
        source,
        cache,
        offset,
        filters,
        filter_styles,
        filters_enabled,
        ..
    } = &node.kind
    else {
        panic!("first filter remains editable")
    };
    let original_source = match &before.kind {
        NodeKind::Raster { raster, .. } => raster,
        NodeKind::Smart { source, .. } => source,
        _ => unreachable!(),
    };
    assert!(Arc::ptr_eq(source, original_source));
    assert_eq!(filters, &[Filter::GaussianBlur { radius }]);
    assert_eq!(
        filter_styles,
        &[FilterStyle {
            opacity,
            ..Default::default()
        }]
    );
    assert!(*filters_enabled);
    let (expected_cache, expected_offset) =
        emulsion_core::smart::render_stack(original_source, filters, filter_styles, true);
    assert_eq!(*offset, expected_offset);
    assert_eq!(
        cache.read_rect(cache.bounds()),
        expected_cache.read_rect(expected_cache.bounds())
    );
    let expected_mask = smart_filter_mask_ui::initial_mask(
        cache.width(),
        cache.height(),
        *offset,
        emulsion_core::transform::local_to_document(before)
            .unwrap()
            .require_affine("affine fixture")
            .unwrap(),
        captured_selection,
        false,
    )
    .unwrap();
    exact_mask(mask(actual), &expected_mask);
    assert_eq!(
        node.mask.as_ref().map(Arc::as_ptr),
        before.mask.as_ref().map(Arc::as_ptr)
    );
    assert_eq!(node.mask_transform, before.mask_transform);
    assert_eq!(node.mask_properties, before.mask_properties);
    assert_eq!(node.vector_mask, before.vector_mask);
    assert_eq!(
        actual.selection.as_ref().map(Arc::as_ptr),
        current_selection.as_ref().map(Arc::as_ptr),
    );
    cache.clone()
}

#[gpui_kit::test]
fn first_mask_capture_matches_before_and_after_coalesced_publication(cx: &mut TestAppContext) {
    for smart in [false, true] {
        for selection in [None, newer_selection()] {
            let original = document(smart);
            let mut first_mask = None;
            for publish_first in [false, true] {
                let (editor, cx) = setup(cx, original.clone());
                let (ready, release) = cx.update(|_, cx| {
                    editor.update(cx, |e, cx| {
                        let barrier = e.smart.pause_next_render();
                        e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                        e.execute(
                            Command::SetSelection {
                                selection: selection.clone(),
                            },
                            cx,
                        );
                        barrier
                    })
                });
                cx.run_until_parked();
                ready
                    .try_recv()
                    .expect("first render held before publication");
                let mut release = Some(release);
                if publish_first {
                    drop(release.take());
                    cx.run_until_parked();
                }
                cx.update(|_, cx| {
                    editor.update(cx, |e, cx| {
                        e.set_filters_enabled(1, false, cx);
                        e.set_filter_enabled(1, 0, false, cx);
                        e.set_filter_style(1, 0, None, Some(0.25), cx);
                        e.set_filter_enabled(1, 0, true, cx);
                        e.set_filters_enabled(1, true, cx);
                    })
                });
                cx.run_until_parked();
                let (committed, cache) = cx.update(|_, cx| {
                    let e = editor.read(cx);
                    assert!(!e.smart.has_pending());
                    assert_eq!(e.editor.history.len(), if publish_first { 3 } else { 2 });
                    let cache = verify(
                        &e.editor.doc,
                        &original,
                        original.selection.as_deref(),
                        &selection,
                        2.,
                        0.25,
                    );
                    if let Some(expected) = &first_mask {
                        exact_mask(mask(&e.editor.doc), expected);
                    } else {
                        first_mask = Some(mask(&e.editor.doc).clone());
                    }
                    (e.editor.doc.clone(), cache)
                });
                drop(release);
                cx.run_until_parked();
                cx.update(|_, cx| {
                    editor.update(cx, |e, cx| {
                        assert_eq!(
                            e.editor.doc, committed,
                            "held older generation cannot publish"
                        );
                        let NodeKind::Smart { cache: actual, .. } =
                            &e.editor.doc.node(1).unwrap().kind
                        else {
                            panic!()
                        };
                        assert!(Arc::ptr_eq(actual, &cache));
                        let edits = if publish_first { 2 } else { 1 };
                        for _ in 0..edits {
                            e.undo(cx);
                        }
                        exact_baseline_nodes(&e.editor.doc, &original);
                        assert_eq!(
                            e.editor.doc.selection.as_ref().map(Arc::as_ptr),
                            selection.as_ref().map(Arc::as_ptr)
                        );
                        e.undo(cx);
                        assert_eq!(e.editor.doc, original);
                        for _ in 0..=edits {
                            e.redo(cx);
                        }
                        assert_eq!(e.editor.doc, committed);
                    })
                });
            }
        }
    }
}

#[gpui_kit::test]
fn first_mask_capture_survives_filter_preview_release_and_throttle_flush(cx: &mut TestAppContext) {
    for smart in [false, true] {
        for publish_preview in [false, true] {
            for flush in [false, true] {
                let original = document(smart);
                let selection = newer_selection();
                let (editor, cx) = setup(cx, original.clone());
                let (ready, first_release) = cx.update(|_, cx| {
                    editor.update(cx, |e, cx| {
                        let barrier = e.smart.pause_next_render();
                        e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                        e.execute(
                            Command::SetSelection {
                                selection: selection.clone(),
                            },
                            cx,
                        );
                        barrier
                    })
                });
                cx.run_until_parked();
                ready.try_recv().expect("initial add held");
                let (ready, release) = cx.update(|_, cx| {
                    editor.update(cx, |e, cx| {
                        let barrier = e.smart.pause_next_render();
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
                        e.set_filter_param(1, 0, "radius", 7., false, cx);
                        assert_eq!(e.smart.pending, Some((1, 0, "radius", 7.)));
                        assert!(e.editor.in_transaction());
                        barrier
                    })
                });
                cx.run_until_parked();
                ready
                    .try_recv()
                    .expect("slider render held before publication");
                let mut release = Some(release);
                if publish_preview {
                    drop(release.take());
                    cx.run_until_parked();
                    cx.update(|_, cx| {
                        let e = editor.read(cx);
                        verify(
                            &e.editor.doc,
                            &original,
                            original.selection.as_deref(),
                            &selection,
                            4.,
                            1.,
                        );
                        assert!(e.editor.in_transaction());
                        assert_eq!(e.editor.history.len(), 1);
                    });
                }
                cx.update(|_, cx| {
                    editor.update(cx, |e, cx| {
                        if flush {
                            e.flush_filter_param(cx);
                        }
                        e.drag_end(cx);
                        assert!(!e.editor.in_transaction());
                        assert!(e.smart.gesture_initial_mask.is_none());
                    })
                });
                cx.run_until_parked();
                let (committed, cache) = cx.update(|_, cx| {
                    let e = editor.read(cx);
                    assert!(!e.smart.has_pending());
                    assert_eq!(e.editor.history.len(), 2);
                    let cache = verify(
                        &e.editor.doc,
                        &original,
                        original.selection.as_deref(),
                        &selection,
                        7.,
                        1.,
                    );
                    (e.editor.doc.clone(), cache)
                });
                drop(release);
                drop(first_release);
                cx.run_until_parked();
                cx.update(|_, cx| {
                    editor.update(cx, |e, cx| {
                        assert_eq!(e.editor.doc, committed);
                        let NodeKind::Smart { cache: actual, .. } =
                            &e.editor.doc.node(1).unwrap().kind
                        else {
                            panic!()
                        };
                        assert!(Arc::ptr_eq(actual, &cache));
                        e.undo(cx);
                        exact_baseline_nodes(&e.editor.doc, &original);
                        assert_eq!(
                            e.editor.doc.selection.as_ref().map(Arc::as_ptr),
                            selection.as_ref().map(Arc::as_ptr)
                        );
                        e.undo(cx);
                        assert_eq!(e.editor.doc, original);
                        e.redo(cx);
                        e.redo(cx);
                        assert_eq!(e.editor.doc, committed);
                    })
                });
            }
        }
    }
}

#[gpui_kit::test]
fn invalidated_or_removed_first_filter_cannot_donate_its_capture(cx: &mut TestAppContext) {
    for smart in [false, true] {
        for invalidation in 0..8 {
            let original = document(smart);
            let selection = newer_selection();
            let (editor, cx) = setup(cx, original.clone());
            let (ready, release) = cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    let barrier = e.smart.pause_next_render();
                    e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                    e.execute(
                        Command::SetSelection {
                            selection: selection.clone(),
                        },
                        cx,
                    );
                    barrier
                })
            });
            cx.run_until_parked();
            ready.try_recv().expect("old initialization held");
            let (before_history, baseline) = cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    match invalidation {
                        0 => {
                            e.begin_edit_job().unwrap();
                        }
                        1 => {
                            e.editor.doc.node_mut(1).unwrap().name = "Changed source node".into();
                        }
                        2 => {
                            assert!(e.cancel_filter_edits(cx));
                        }
                        3 => {
                            e.undo(cx);
                            e.execute(
                                Command::SetSelection {
                                    selection: selection.clone(),
                                },
                                cx,
                            );
                        }
                        4 => {
                            e.set_layer_selection(vec![1], Some(1));
                        }
                        5 => {
                            e.remove_filter(1, 0, cx);
                        }
                        6 => {
                            let pixels = Arc::new(Raster::solid(32, 24, [0.9, 0.1, 0.2, 1.]));
                            match &mut e.editor.doc.node_mut(1).unwrap().kind {
                                NodeKind::Raster { raster, .. } => *raster = pixels,
                                NodeKind::Smart { source, .. } => *source = pixels,
                                _ => unreachable!(),
                            }
                        }
                        _ => match &mut e.editor.doc.node_mut(1).unwrap().kind {
                            NodeKind::Raster { placement, .. } => placement.x += 4.,
                            NodeKind::Smart { cache, offset, .. } => {
                                *cache = Arc::new(Raster::solid(32, 24, [0.1, 0.9, 0.2, 1.]));
                                *offset = (1, 2);
                            }
                            _ => unreachable!(),
                        },
                    }
                    let before_history = e.editor.history.len();
                    let baseline = e.editor.doc.clone();
                    if !(2..6).contains(&invalidation) {
                        e.set_filter_style(1, 0, None, Some(0.25), cx);
                    } else {
                        e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                    }
                    (before_history, baseline)
                })
            });
            cx.run_until_parked();
            let (committed, cache) = cx.update(|_, cx| {
                let e = editor.read(cx);
                assert!(!e.smart.has_pending());
                assert_eq!(e.editor.history.len(), before_history + 1);
                let cache = verify(
                    &e.editor.doc,
                    &baseline,
                    selection.as_deref(),
                    &selection,
                    2.,
                    if !(2..6).contains(&invalidation) {
                        0.25
                    } else {
                        1.
                    },
                );
                (e.editor.doc.clone(), cache)
            });
            drop(release);
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = editor.read(cx);
                assert_eq!(e.editor.doc, committed);
                let NodeKind::Smart { cache: actual, .. } = &e.editor.doc.node(1).unwrap().kind
                else {
                    panic!()
                };
                assert!(Arc::ptr_eq(actual, &cache));
            });
        }
    }
}

#[gpui_kit::test]
fn canceled_or_superseded_preview_cannot_donate_initial_mask_capture(cx: &mut TestAppContext) {
    for smart in [false, true] {
        for invalidation in 0..3 {
            let original = document(smart);
            let selection = newer_selection();
            let (editor, cx) = setup(cx, original.clone());
            let (ready, release) = cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    let barrier = e.smart.pause_next_render();
                    e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                    e.execute(
                        Command::SetSelection {
                            selection: selection.clone(),
                        },
                        cx,
                    );
                    barrier
                })
            });
            cx.run_until_parked();
            ready.try_recv().expect("initial add held");
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
                            position: point(px(40.), px(12.)),
                            button: MouseButton::Left,
                            modifiers: Modifiers::none(),
                            click_count: 1,
                            first_mouse: false,
                        },
                        cx,
                    );
                })
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    verify(
                        &e.editor.doc,
                        &original,
                        original.selection.as_deref(),
                        &selection,
                        4.,
                        1.,
                    );
                    assert!(e.smart.gesture_initial_mask.is_some());
                    assert!(e.editor.in_transaction());
                    match invalidation {
                        0 => {
                            assert!(e.cancel_filter_edits(cx));
                            assert!(e.smart.gesture_initial_mask.is_none());
                            e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                        }
                        1 => {
                            e.begin_edit_job().unwrap();
                            e.set_filter_param(1, 0, "radius", 6., true, cx);
                            e.drag_end(cx);
                        }
                        _ => {
                            e.remove_filter(1, 0, cx);
                            e.add_filter(1, Filter::GaussianBlur { radius: 2. }, cx);
                            e.drag_end(cx);
                        }
                    }
                    assert!(!e.editor.in_transaction());
                    assert!(e.smart.gesture_initial_mask.is_none());
                })
            });
            cx.run_until_parked();
            let (committed, cache) = cx.update(|_, cx| {
                let e = editor.read(cx);
                assert!(!e.smart.has_pending());
                assert_eq!(e.editor.history.len(), 2);
                let radius = if invalidation == 1 { 6. } else { 2. };
                let cache = verify(
                    &e.editor.doc,
                    &original,
                    selection.as_deref(),
                    &selection,
                    radius,
                    1.,
                );
                (e.editor.doc.clone(), cache)
            });
            drop(release);
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    assert_eq!(e.editor.doc, committed);
                    let NodeKind::Smart { cache: actual, .. } = &e.editor.doc.node(1).unwrap().kind
                    else {
                        panic!()
                    };
                    assert!(Arc::ptr_eq(actual, &cache));
                    e.undo(cx);
                    exact_baseline_nodes(&e.editor.doc, &original);
                    assert_eq!(
                        e.editor.doc.selection.as_ref().map(Arc::as_ptr),
                        selection.as_ref().map(Arc::as_ptr)
                    );
                    e.undo(cx);
                    assert_eq!(e.editor.doc, original);
                })
            });
        }
    }
}
