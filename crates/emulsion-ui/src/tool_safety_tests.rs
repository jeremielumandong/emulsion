//! Regression coverage for jobs that finish after newer user intent.
use super::*;
use crate::editor::{SelectShape, Tool};
use emulsion_core::NodeKind;
use emulsion_filters::Filter;
use emulsion_raster::select;

#[gpui_kit::test]
fn pending_fill_cannot_replace_later_pixels_or_join_a_gesture(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let replacement = Arc::new(Raster::solid(256, 192, [0., 1., 0., 1.]));
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            e.set_fg([255, 0, 0, 255], cx);
            e.fill_selection(cx);
            e.editor.begin("Newer stroke");
            e.execute(
                Command::ReplacePixels {
                    id,
                    raster: replacement.clone(),
                    dirty: replacement.bounds(),
                    label: "Newer stroke".into(),
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.nodes[0].kind else {
            panic!()
        };
        assert_eq!(raster.get(100, 100), [0, 65535, 0, 65535]);
        assert!(
            e.editor.in_transaction(),
            "the worker must not close another gesture"
        );
    });
}

#[gpui_kit::test]
fn undo_cancels_a_pending_fill_even_without_a_history_step(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_fg([255, 0, 0, 255], cx);
            e.fill_selection(cx);
            e.undo(cx);
            e.redo(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, before));
}

#[gpui_kit::test]
fn deselect_cancels_pending_selection_refinement(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(select::rect(256, 192, 20., 20., 100., 100.))),
                },
                cx,
            );
            e.modify_selection(5, 12., cx);
            e.deselect(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(e.read(cx).editor.doc.selection.is_none()));
}

#[gpui_kit::test]
fn quick_selection_new_replaces_the_previous_region(cx: &mut TestAppContext) {
    let raster = Raster::from_fn(256, 192, [0; 4], |x, _| {
        if x < 128 {
            [65535, 0, 0, 65535]
        } else {
            [0, 0, 65535, 65535]
        }
    });
    let (ws, cx) = open(cx, doc(&["Photo"], Some(raster)));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(select::rect(256, 192, 0., 0., 100., 192.))),
                },
                cx,
            );
            e.set_select(SelectShape::Quick, cx);
        })
    });
    cx.run_until_parked();
    let point = cx.update(|_, cx| e.read(cx).doc_to_window((190., 96.)).unwrap());
    cx.simulate_click(point, gpui_kit::Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        let selection = e.editor.doc.selection.as_ref().unwrap();
        assert_eq!(selection.get(40, 96), 0);
        assert!(selection.get(190, 96) > 0);
    });
}

#[gpui_kit::test]
fn magnetic_lasso_rejects_pre_resize_cache(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_select(SelectShape::Magnetic, cx)));
    cx.run_until_parked();
    let point = cx.update(|_, cx| e.read(cx).doc_to_window((20., 20.)).unwrap());
    cx.simulate_click(point, gpui_kit::Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::ImageSize {
                    width: 512,
                    height: 384,
                },
                cx,
            );
            e.magnetic_track((500., 350.), cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| e.update(cx, |e, cx| e.magnetic_track((500., 350.), cx)));
}

fn smart_doc() -> Document {
    let mut d = doc(&["A", "B"], None);
    let ids: Vec<_> = d.nodes.iter().map(|n| n.id).collect();
    for id in ids {
        Command::ConvertToSmart { id }.apply(&mut d).unwrap();
    }
    d
}

#[gpui_kit::test]
fn filters_on_separate_layers_both_finish_and_fast_edits_compose(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, smart_doc());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let a = e.editor.doc.nodes[0].id;
            let b = e.editor.doc.nodes[1].id;
            e.add_filter(a, Filter::GaussianBlur { radius: 2. }, cx);
            e.add_filter(a, Filter::HighPass { radius: 1. }, cx);
            e.add_filter(b, Filter::GaussianBlur { radius: 3. }, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        for (node, expected) in e.editor.doc.nodes.iter().zip([2, 1]) {
            let NodeKind::Smart { filters, .. } = &node.kind else {
                panic!()
            };
            assert_eq!(filters.len(), expected);
        }
    });
}

#[gpui_kit::test]
fn undo_cancels_pending_smart_filters(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, smart_doc());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let id = e.editor.doc.nodes[0].id;
            e.add_filter(id, Filter::GaussianBlur { radius: 2. }, cx);
            e.undo(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        for node in &e.read(cx).editor.doc.nodes {
            let NodeKind::Smart { filters, .. } = &node.kind else {
                panic!()
            };
            assert!(filters.is_empty());
        }
    });
}

#[gpui_kit::test]
fn smart_warp_is_explicitly_unavailable_and_stale_distort_does_not_commit(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, smart_doc());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Move, cx);
            e.start_warp(cx);
            assert!(e.warp.is_none());
            assert!(e.status.as_ref().unwrap().0.contains("Rasterize"));
            e.convert_smart(cx);
            let id = e.selected.unwrap();
            e.finish_distort(id, [(0., 0.), (250., 20.), (230., 190.), (10., 180.)], cx);
            e.execute(
                Command::SetPlacement {
                    id,
                    placement: Placement::at(40., 50.),
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { placement, .. } =
            &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(*placement, Placement::at(40., 50.));
    });
}
