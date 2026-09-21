//! Paint tools exercised through real canvas input, with pixel and history assertions.
use super::*;
use crate::editor::{EditorView, PaintKind, Tool};
use emulsion_core::NodeKind;
use emulsion_raster::paint::Brush;

fn editor(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    cx.update(|_, cx| ws.read(cx).editor.clone().unwrap())
}

fn pixels(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Arc<Raster> {
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!("raster target")
        };
        raster.clone()
    })
}

fn click(e: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) {
    let p = cx.update(|_, cx| e.read(cx).doc_to_window(p).unwrap());
    cx.simulate_click(p, gpui_kit::Modifiers::none());
    cx.run_until_parked();
}

fn drag(e: &Entity<EditorView>, cx: &mut VisualTestContext, a: (f64, f64), b: (f64, f64)) {
    let (a, b) = cx.update(|_, cx| {
        let e = e.read(cx);
        (e.doc_to_window(a).unwrap(), e.doc_to_window(b).unwrap())
    });
    cx.simulate_mouse_down(a, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
    for i in 1..=8 {
        cx.simulate_mouse_move(
            a + (b - a) * (i as f32 / 8.0),
            Some(gpui_kit::MouseButton::Left),
            gpui_kit::Modifiers::none(),
        );
    }
    cx.simulate_mouse_up(b, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
    cx.run_until_parked();
}

fn set_paint(e: &Entity<EditorView>, cx: &mut VisualTestContext, kind: PaintKind) {
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_paint(kind, cx);
            e.tools.brush = Brush {
                size: 32.0,
                hardness: 1.0,
                ..Brush::default()
            };
            e.tools.quick_shape = false;
            e.set_fg([255, 0, 0, 255], cx);
        })
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn brush_and_eraser_change_only_selected_pixels_and_undo_one_gesture(cx: &mut TestAppContext) {
    let original = Raster::solid(256, 192, [0., 0., 1., 1.]);
    let (ws, cx) = open(cx, doc(&["Photo"], Some(original.clone())));
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(emulsion_raster::select::rect(
                        256, 192, 0., 0., 128., 192.,
                    ))),
                },
                cx,
            );
        })
    });
    set_paint(&e, cx, PaintKind::Brush);
    drag(&e, cx, (80., 96.), (160., 96.));
    let painted = pixels(&e, cx);
    assert_eq!(painted.get(100, 96), [65535, 0, 0, 65535]);
    assert_eq!(painted.get(140, 96), original.get(140, 96));
    assert_eq!(painted.get(100, 40), original.get(100, 40));
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.history.len(), 2);
            e.undo(cx);
        })
    });
    assert_eq!(
        pixels(&e, cx).read_rect(original.bounds()),
        original.read_rect(original.bounds())
    );
    set_paint(&e, cx, PaintKind::Eraser);
    drag(&e, cx, (80., 96.), (160., 96.));
    assert_eq!(pixels(&e, cx).get(100, 96), [0; 4]);
    assert_eq!(pixels(&e, cx).get(140, 96), original.get(140, 96));
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(
        pixels(&e, cx).read_rect(original.bounds()),
        original.read_rect(original.bounds())
    );
}

#[gpui_kit::test]
fn smudge_transports_existing_color_and_undo_restores_boundary(cx: &mut TestAppContext) {
    let original = Raster::from_fn(256, 192, [0; 4], |x, _| {
        if x < 128 {
            [65535, 0, 0, 65535]
        } else {
            [0, 0, 65535, 65535]
        }
    });
    let (ws, cx) = open(cx, doc(&["Photo"], Some(original.clone())));
    let e = editor(&ws, cx);
    set_paint(&e, cx, PaintKind::Smudge);
    // Foreground is deliberately unrelated: smudge must transport source pigment.
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_fg([0, 255, 0, 255], cx)));
    drag(&e, cx, (112., 96.), (160., 96.));
    let painted = pixels(&e, cx);
    assert!(painted.get(134, 96)[0] > 0);
    assert_eq!(painted.get(134, 96)[1], 0);
    assert_eq!(painted.get(200, 40), original.get(200, 40));
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
        })
    });
    assert_eq!(
        pixels(&e, cx).read_rect(original.bounds()),
        original.read_rect(original.bounds())
    );
}

#[gpui_kit::test]
fn bucket_respects_contiguity_and_selection_and_undo(cx: &mut TestAppContext) {
    let original = Raster::from_fn(256, 192, [0; 4], |x, _| {
        if (120..136).contains(&x) {
            [0, 0, 65535, 65535]
        } else {
            [65535; 4]
        }
    });
    let (ws, cx) = open(cx, doc(&["Photo"], Some(original.clone())));
    let e = editor(&ws, cx);
    set_paint(&e, cx, PaintKind::Bucket);
    click(&e, cx, (60., 96.));
    let filled = pixels(&e, cx);
    assert_eq!(filled.get(60, 96), [65535, 0, 0, 65535]);
    assert_eq!(filled.get(180, 96), [65535; 4]);
    assert_eq!(filled.get(128, 96), [0, 0, 65535, 65535]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.undo(cx);
            e.tools.contiguous = false;
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(emulsion_raster::select::rect(
                        256, 192, 0., 0., 256., 96.,
                    ))),
                },
                cx,
            );
        })
    });
    click(&e, cx, (60., 40.));
    let filled = pixels(&e, cx);
    assert_eq!(filled.get(180, 40), [65535, 0, 0, 65535]);
    assert_eq!(filled.get(60, 140), [65535; 4]);
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(
        pixels(&e, cx).read_rect(original.bounds()),
        original.read_rect(original.bounds())
    );
}

#[gpui_kit::test]
fn linear_and_radial_gradients_create_selected_layers_with_expected_colors(
    cx: &mut TestAppContext,
) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = editor(&ws, cx);
    set_paint(&e, cx, PaintKind::Gradient);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.tools.bg = [0, 0, 255, 255];
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(emulsion_raster::select::rect(
                        256, 192, 0., 32., 256., 128.,
                    ))),
                },
                cx,
            );
        })
    });
    for radial in [false, true] {
        cx.update(|_, cx| e.update(cx, |e, _| e.tools.radial = radial));
        drag(&e, cx, (64., 96.), (192., 96.));
        let gradient = pixels(&e, cx);
        assert!(gradient.get(64, 96)[0] > 64000);
        assert_eq!(gradient.get(192, 96), [0, 0, 65535, 65535]);
        assert_eq!(gradient.get(128, 10), [0; 4]);
        if radial {
            assert!(gradient.get(64, 150)[2] > 20000);
        } else {
            assert!(gradient.get(64, 150)[2] < 1000);
        }
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert_eq!(e.editor.doc.nodes.len(), 2);
                e.undo(cx);
                assert_eq!(e.editor.doc.nodes.len(), 1);
            })
        });
    }
}

#[gpui_kit::test]
fn mask_hide_and_reveal_change_mask_without_replacing_layer_pixels(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = editor(&ws, cx);
    let original = pixels(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Mask, cx);
            e.tools.brush = Brush {
                size: 32.,
                hardness: 1.,
                ..Brush::default()
            };
            e.tools.mask_reveal = false;
        })
    });
    cx.run_until_parked();
    click(&e, cx, (100., 96.));
    cx.update(|_, cx| {
        let e = e.read(cx);
        let mask = e.editor.doc.nodes[0].mask.as_ref().unwrap();
        assert_eq!(mask.get(100, 96), 0);
        assert_eq!(mask.get(180, 96), 255);
    });
    assert!(Arc::ptr_eq(&original, &pixels(&e, cx)));
    cx.update(|_, cx| e.update(cx, |e, _| e.tools.mask_reveal = true));
    click(&e, cx, (100., 96.));
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(
                e.editor.doc.nodes[0].mask.as_ref().unwrap().get(100, 96),
                255
            );
            e.undo(cx);
            assert_eq!(e.editor.doc.nodes[0].mask.as_ref().unwrap().get(100, 96), 0);
        })
    });
    assert!(Arc::ptr_eq(&original, &pixels(&e, cx)));
}

#[gpui_kit::test]
fn heal_removes_an_isolated_speck_and_undo_restores_original(cx: &mut TestAppContext) {
    let original = Raster::from_fn(256, 192, [0; 4], |x, y| {
        if (126..131).contains(&x) && (94..99).contains(&y) {
            [65535; 4]
        } else {
            [12000, 12000, 12000, 65535]
        }
    });
    let (ws, cx) = open(cx, doc(&["Photo"], Some(original.clone())));
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Heal, cx);
            e.tools.brush = Brush {
                size: 24.,
                hardness: 1.,
                ..Brush::default()
            };
        })
    });
    cx.run_until_parked();
    click(&e, cx, (128., 96.));
    let healed = pixels(&e, cx);
    assert!(
        healed.get(128, 96)[0] < 30000,
        "healing should replace the isolated bright defect"
    );
    assert_eq!(healed.get(180, 96), original.get(180, 96));
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
        })
    });
    assert_eq!(
        pixels(&e, cx).read_rect(original.bounds()),
        original.read_rect(original.bounds())
    );
}

#[gpui_kit::test]
fn locked_raster_rejects_destructive_paint_tools_without_history(cx: &mut TestAppContext) {
    let mut document = doc(&["Locked photo"], None);
    document.nodes[0].locked = true;
    let original = document.clone();
    let (ws, cx) = open(cx, document);
    let e = editor(&ws, cx);
    for kind in [
        PaintKind::Brush,
        PaintKind::Eraser,
        PaintKind::Smudge,
        PaintKind::Bucket,
        PaintKind::Liquify,
    ] {
        set_paint(&e, cx, kind);
        drag(&e, cx, (100., 96.), (150., 96.));
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(
                e.editor.doc, original,
                "locked layer changed using {kind:?}"
            );
            assert_eq!(e.editor.history.len(), 0);
            assert!(!e.editor.in_transaction());
        });
    }
    for tool in [Tool::Clone, Tool::Heal, Tool::Mask] {
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.set_tool(tool, cx);
                e.tools.clone_source = Some((60., 96.));
            })
        });
        cx.run_until_parked();
        click(&e, cx, (160., 96.));
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(
                e.editor.doc, original,
                "locked layer changed using {tool:?}"
            );
            assert_eq!(e.editor.history.len(), 0);
            assert!(!e.editor.in_transaction());
        });
    }
}
