//! Photo's locked-layer Eraser is a background-color stroke, not a paint blend.
use super::*;
use crate::editor::{EditorView, MaskEditTarget, PaintKind};
use emulsion_core::NodeKind;
use emulsion_raster::paint::{Brush, BrushBlend};
use emulsion_raster::{Mask, color};
use gpui_kit::{Modifiers, MouseButton};

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

fn configure(e: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.set_paint(PaintKind::Eraser, cx);
            e.tools.brush = Brush {
                size: 200.,
                hardness: 1.,
                ..Brush::default()
            };
            e.tools.quick_shape = false;
            e.tools.fg = [255, 0, 0, 255];
            e.tools.bg = [30, 180, 220, 0];
            window.focus(&e.canvas_focus, cx);
        });
    });
    cx.run_until_parked();
}

fn click(e: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let at = cx.update(|_, cx| e.read(cx).doc_to_window((128., 96.)).unwrap());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn edges() -> Raster {
    Raster::from_fn(256, 192, [0; 4], |x, _| {
        let alpha = if x < 64 {
            0
        } else if x < 128 {
            18000
        } else {
            65535
        };
        [alpha / 3, alpha / 5, alpha / 7, alpha]
    })
}

fn expected_background(prior: [u16; 4], background: [u8; 4], coverage: f32) -> [u16; 4] {
    let b = color::px_to_f(prior);
    let mut result = prior;
    for ch in 0..3 {
        let pigment = color::srgb_to_linear(background[ch] as f32 / 255.0);
        result[ch] = color::f_to_u16(pigment * b[3] * coverage + b[ch] * (1.0 - coverage));
    }
    result
}

#[gpui_kit::test]
fn photo_locked_eraser_uses_background_not_foreground_or_saved_brush_blend(
    cx: &mut TestAppContext,
) {
    let original = edges();
    let mut document = doc(&["Locked photo"], Some(original.clone()));
    document.nodes[0].locks.transparency = true;
    let (ws, cx) = open(cx, document);
    let e = editor(&ws, cx);
    configure(&e, cx);
    for blend in [
        BrushBlend::Normal,
        BrushBlend::Multiply,
        BrushBlend::Dissolve,
        BrushBlend::Clear,
        BrushBlend::Behind,
    ] {
        let saved = cx.update(|_, cx| {
            e.update(cx, |e, _| {
                assert!(e.is_photo_workflow());
                e.tools.brush.blend = blend;
                e.tools.brush.color_jitter = 1.;
                e.tools.brush.wetness = 0.8;
                e.tools.brush.relief = 0.6;
                e.tools.brush
            })
        });
        click(&e, cx);
        let painted = pixels(&e, cx);
        for x in [48, 96, 144, 192] {
            assert_eq!(
                painted.get(x, 96),
                expected_background(original.get(x, 96), [30, 180, 220, 0], 1.),
                "{blend:?} at {x}"
            );
        }
        for (prior, after) in original.to_pixels().iter().zip(painted.to_pixels()) {
            assert_eq!(prior[3], after[3]);
        }
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert_eq!(e.tools.brush, saved, "settings must not be rewritten");
                assert_eq!(e.editor.history.len(), 1);
                assert!(
                    e.editor.doc.colors.is_empty(),
                    "Eraser must not log foreground paint"
                );
                e.undo(cx);
            })
        });
        assert_eq!(pixels(&e, cx).to_pixels(), original.to_pixels());
        cx.update(|_, cx| e.update(cx, |e, cx| e.redo(cx)));
        assert_eq!(pixels(&e, cx).to_pixels(), painted.to_pixels());
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    }
}

#[gpui_kit::test]
fn photo_locked_eraser_honors_ancestor_lock_selection_and_opacity(cx: &mut TestAppContext) {
    let original = edges();
    let mut document = Document::new(256, 192);
    let mut parent = Node::group(0, "Transparency-locked group");
    parent.locks.transparency = true;
    let parent = Command::AddNode {
        node: Box::new(parent),
        slot: Slot::TOP,
    }
    .apply(&mut document)
    .unwrap()
    .unwrap();
    let target = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Photo",
            Arc::new(original.clone()),
            Placement::default(),
        )),
        slot: Slot::top_of(Some(parent)),
    }
    .apply(&mut document)
    .unwrap()
    .unwrap();
    document.selection = Some(Arc::new(Mask::from_fn(256, 192, 0, |x, _| {
        if x < 128 { 128 } else { 0 }
    })));
    let (ws, cx) = open(cx, document.clone());
    let e = editor(&ws, cx);
    configure(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            e.selected = Some(target);
            assert!(!e.editor.doc.node(target).unwrap().locks.transparency);
            assert!(e.editor.doc.layer_locks(target).transparency);
            e.tools.brush.opacity = 0.5;
        })
    });
    click(&e, cx);
    let painted = pixels(&e, cx);
    assert_eq!(
        painted.get(96, 96),
        expected_background(original.get(96, 96), [30, 180, 220, 0], 0.5 * 128. / 255.)
    );
    assert_eq!(painted.get(144, 96), original.get(144, 96));
    assert_eq!(painted.get(48, 96), [0; 4]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(Arc::ptr_eq(
                e.editor.doc.selection.as_ref().expect("retained selection"),
                document.selection.as_ref().expect("original selection"),
            ));
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, document);
        })
    });
}

#[gpui_kit::test]
fn photo_locked_eraser_captures_background_and_preserves_cancel_and_focus_lifecycle(
    cx: &mut TestAppContext,
) {
    let original = edges();
    let mut document = doc(&["Locked photo"], Some(original.clone()));
    document.nodes[0].locks.transparency = true;
    let (ws, cx) = open(cx, document.clone());
    let e = editor(&ws, cx);
    configure(&e, cx);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let at = cx.update(|_, cx| e.read(cx).doc_to_window((128., 96.)).unwrap());
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_ne!(pixels(&e, cx).to_pixels(), original.to_pixels());
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.doc, document);
        assert!(e.editor.history.is_empty());
        assert!(!e.has_active_gesture());
        assert!(!e.editor.in_transaction());
    });
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(pixels(&e, cx).get(240, 96), original.get(240, 96));
    cx.update(|_, cx| e.update(cx, |e, _| e.tools.bg = [255, 0, 255, 255]));
    // Newly covered pixels from later samples must use the stroke's captured
    // background, even though the swatch has changed since pointer-down.
    let end = cx.update(|_, cx| e.read(cx).doc_to_window((208., 96.)).unwrap());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        pixels(&e, cx).get(240, 96),
        expected_background(original.get(240, 96), [30, 180, 220, 0], 1.)
    );
    cx.deactivate_window();
    cx.run_until_parked();
    let painted = pixels(&e, cx);
    assert_eq!(
        painted.get(144, 96),
        expected_background(original.get(144, 96), [30, 180, 220, 0], 1.)
    );
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(!e.has_active_gesture());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 1);
    });
    cx.update(|window, _| window.activate_window());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(pixels(&e, cx).to_pixels(), painted.to_pixels());
    // A new gesture captures the new swatch rather than retaining the old one.
    click(&e, cx);
    assert_eq!(
        pixels(&e, cx).get(144, 96),
        expected_background(original.get(144, 96), [255, 0, 255, 255], 1.)
    );
    assert_eq!(pixels(&e, cx).get(240, 96), painted.get(240, 96));
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.history.len(), 2);
            e.undo(cx);
        })
    });
    assert_eq!(pixels(&e, cx).to_pixels(), painted.to_pixels());
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(pixels(&e, cx).to_pixels(), original.to_pixels());
}

#[gpui_kit::test]
fn photo_locked_eraser_leaves_raster_vector_and_quick_mask_semantics_intact(
    cx: &mut TestAppContext,
) {
    let original = edges();
    let mut document = doc(&["Locked photo"], Some(original));
    document.nodes[0].locks.transparency = true;
    document.nodes[0].mask = Some(Arc::new(Mask::white(256, 192)));
    document.nodes[0].vector_mask = Some(emulsion_core::VectorMask::default());
    let (ws, cx) = open(cx, document.clone());
    let e = editor(&ws, cx);
    configure(&e, cx);
    let original = pixels(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            e.tools.alpha_lock = true;
            e.tools.mask_edit_target = MaskEditTarget::RasterMask;
        })
    });
    click(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes[0].mask.as_ref().unwrap().get(128, 96), 0);
            assert_eq!(
                e.editor.doc.nodes[0].vector_mask,
                document.nodes[0].vector_mask
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, document);
            e.tools.mask_edit_target = MaskEditTarget::VectorMask;
        })
    });
    click(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(
                e.editor.doc, document,
                "raster Eraser cannot edit vector geometry"
            );
            assert!(e.editor.history.is_empty());
            e.tools.mask_edit_target = MaskEditTarget::Content;
            e.toggle_quick_mask(cx);
            e.editor.doc.selection = Some(Arc::new(Mask::empty(256, 192, 0)));
            e.set_paint(PaintKind::Eraser, cx);
            e.tools.brush = Brush {
                size: 200.,
                hardness: 1.,
                ..Brush::default()
            };
        })
    });
    click(&e, cx);
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(
            e.editor.doc.selection.as_ref().unwrap().get(128, 96),
            255,
            "Quick Mask Eraser reveals"
        );
        assert_eq!(e.editor.doc.nodes, document.nodes);
    });
    assert!(Arc::ptr_eq(&pixels(&e, cx), &original));
}

#[gpui_kit::test]
fn photo_unlocked_and_tool_only_locked_erasers_keep_their_existing_behavior(
    cx: &mut TestAppContext,
) {
    let original = Raster::solid(256, 192, [0.2, 0.1, 0., 0.5]);
    let (ws, cx) = open(cx, doc(&["Photo"], Some(original.clone())));
    let e = editor(&ws, cx);
    configure(&e, cx);
    cx.update(|_, cx| e.update(cx, |e, _| e.tools.alpha_lock = true));
    click(&e, cx);
    assert_eq!(pixels(&e, cx).to_pixels(), original.to_pixels());
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            assert!(e.editor.history.is_empty());
            e.tools.alpha_lock = false;
        })
    });
    click(&e, cx);
    assert_eq!(pixels(&e, cx).get(128, 96), [0; 4]);
}

#[gpui_kit::test]
fn locked_eraser_background_policy_is_not_enabled_in_paint_design_or_storyboard(
    cx: &mut TestAppContext,
) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    let original = Raster::solid(256, 192, [0.2, 0.1, 0., 0.5]);
    let mut document = doc(&["Other workspace"], Some(original.clone()));
    document.nodes[0].locks.transparency = true;
    let (ws, cx) = open(cx, document.clone());
    let e = editor(&ws, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_draw_mode(cx)));
    configure(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            assert!(!e.is_photo_workflow());
            e.tools.alpha_lock = true;
        })
    });
    click(&e, cx);
    assert_eq!(pixels(&e, cx).to_pixels(), original.to_pixels());
    for kind in [ProjectKind::Design, ProjectKind::Storyboard] {
        let project = ProjectEditor::new_project(kind, document.clone()).unwrap();
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Other Eraser workspace".into(), window, cx)
            })
        });
        let e = editor(&ws, cx);
        configure(&e, cx);
        cx.update(|_, cx| {
            e.update(cx, |e, _| {
                assert!(!e.is_photo_workflow());
                // Installing a project clears the page-local layer selection.
                // Explicitly target the locked source; an unselected Eraser
                // otherwise creates a new empty layer under existing behavior.
                let target = document.nodes[0].id;
                e.set_layer_selection(vec![target], Some(target));
                assert!(e.editor.doc.layer_locks(target).transparency);
                assert_eq!(e.editor.doc, document, "{kind:?} source before stroke");
                e.tools.alpha_lock = true;
            })
        });
        click(&e, cx);
        assert_eq!(pixels(&e, cx).to_pixels(), original.to_pixels(), "{kind:?}");
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(e.selected, Some(document.nodes[0].id));
            assert_eq!(e.editor.doc, document, "{kind:?} must preserve the source");
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.in_transaction());
        });
    }
}
