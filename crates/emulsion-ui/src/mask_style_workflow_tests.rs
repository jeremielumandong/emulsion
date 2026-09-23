use super::*;
use crate::editor::{PaintKind, Tool};
use emulsion_core::styles::LayerStyle;
use emulsion_raster::{Mask, paint::Brush};

#[gpui_kit::test]
fn mask_taskbar_paints_inverts_and_toggles_view(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    let document = doc(&["Photo"], None);
    let id = document.nodes[0].id;
    let original_pixels = document.nodes[0].kind.clone();
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    cx.update(|window, cx| window.click("layers-add-mask", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(editor.read(cx).tools.mask_edit);
        window.click("mask-subtract-paint", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tool, Tool::Brush);
        assert_eq!(e.tools.paint, PaintKind::Brush);
        assert_eq!(e.tools.fg, [0, 0, 0, 255]);
        assert!(e.tools.mask_edit);
    });
    cx.update(|window, cx| window.click("mask-add-paint", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).tools.fg, [255; 4]));
    cx.update(|window, cx| window.click("mask-taskbar-invert", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(
            e.editor
                .doc
                .node(id)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .get(64, 64),
            0
        );
        assert_eq!(e.editor.doc.node(id).unwrap().kind, original_pixels);
    });
    for expected in [Some(id), None] {
        cx.update(|window, cx| window.click("mask-taskbar-view", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).mask_view.layer, expected));
    }
}

#[gpui_kit::test]
fn mask_defaults_swap_and_gradient_keep_source_pixels(cx: &mut TestAppContext) {
    let mut document = doc(&["Photo"], None);
    let id = document.nodes[0].id;
    document.nodes[0].mask = Some(Arc::new(Mask::white(256, 192)));
    let original = document.clone();
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_tool(Tool::Mask, cx);
            e.default_colors(cx);
            assert_eq!(e.tools.fg, [255; 4]);
            assert_eq!(e.tools.bg, [0, 0, 0, 255]);
            e.swap_colors(cx);
            assert_eq!(e.tools.fg, [0, 0, 0, 255]);
            assert_eq!(e.tools.bg, [255; 4]);
            e.set_paint(PaintKind::Gradient, cx);
            assert!(e.tools.mask_edit);
        })
    });
    cx.run_until_parked();
    let (a, b) = cx.update(|_, cx| {
        let e = editor.read(cx);
        (
            e.doc_to_window((32., 96.)).unwrap(),
            e.doc_to_window((224., 96.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(a, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(b, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(b, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 1);
        let node = e.editor.doc.node(id).unwrap();
        assert_eq!(node.kind, original.node(id).unwrap().kind);
        let mask = node.mask.as_ref().unwrap();
        assert_eq!(mask.get(16, 96), 0);
        assert!((120..136).contains(&mask.get(128, 96)));
        assert_eq!(mask.get(240, 96), 255);
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn footer_add_mask_supports_alt_inversion_and_undo(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    for selected in [false, true] {
        for inverted in [false, true] {
            let mut document = doc(&["Photo"], None);
            let id = document.nodes[0].id;
            if selected {
                document.selection = Some(Arc::new(Mask::from_fn(256, 192, 0, |x, _| {
                    if x < 128 { 96 } else { 0 }
                })));
            }
            let original = document.clone();
            let (ws, cx) = open(cx, document);
            let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
            cx.run_until_parked();
            let point = cx.update(|window, _| window.find("layers-add-mask").bounds().center());
            cx.simulate_click(
                point,
                gpui_kit::Modifiers {
                    alt: inverted,
                    ..Default::default()
                },
            );
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = editor.read(cx);
                let mask = e.editor.doc.node(id).unwrap().mask.as_ref().unwrap();
                let expected = if selected { 96 } else { 255 };
                assert_eq!(
                    mask.get(32, 32),
                    if inverted { 255 - expected } else { expected }
                );
                let outside = if selected { 0 } else { 255 };
                assert_eq!(
                    mask.get(200, 32),
                    if inverted { 255 - outside } else { outside }
                );
                assert_eq!(e.editor.history.len(), 1);
                if !selected {
                    assert_eq!(mask.tile_count(), 0);
                }
                editor.update(cx, |e, cx| e.undo(cx));
                assert_eq!(editor.read(cx).editor.doc, original);
            });
        }
    }
}

#[gpui_kit::test]
fn add_mask_preserves_existing_masks_and_rejects_locked_layers(cx: &mut TestAppContext) {
    for existing in [false, true] {
        let mut document = doc(&["Photo"], None);
        if existing {
            document.nodes[0].mask = Some(Arc::new(Mask::from_fn(256, 192, 0, |_, _| 96)));
        } else {
            document.nodes[0].locked = true;
        }
        let original = document.clone();
        let (ws, cx) = open(cx, document);
        cx.update(|_, cx| {
            let editor = ws.read(cx).editor.clone().unwrap();
            editor.update(cx, |e, cx| e.add_mask_inverted(true, cx));
            assert_eq!(editor.read(cx).editor.doc, original);
            assert_eq!(editor.read(cx).editor.history.len(), 0);
        });
    }
}

#[gpui_kit::test]
fn add_mask_preserves_source_shares_aligned_selection_and_undo(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    for selected in [false, true] {
        let mut document = doc(&["Photo"], None);
        let id = document.nodes[0].id;
        let selection = Arc::new(Mask::from_fn(256, 192, 0, |x, y| {
            if (16..240).contains(&x) && (16..176).contains(&y) {
                255
            } else {
                0
            }
        }));
        if selected {
            document.selection = Some(selection.clone());
        }
        let original = document.clone();
        let (ws, cx) = open(cx, document);
        let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.set_layer_selection(vec![id], Some(id));
                e.select_sidebar(crate::editor::SidebarTab::Properties, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("mask-add", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            let node = e.editor.doc.node(id).unwrap();
            let mask = node.mask.as_ref().unwrap();
            assert_eq!(node.kind, original.node(id).unwrap().kind);
            assert_eq!(e.editor.history.len(), 1);
            if selected {
                assert!(
                    Arc::ptr_eq(mask, &selection),
                    "aligned selection needs no pixel copy"
                );
                assert_eq!(
                    emulsion_raster::select::bounds(mask),
                    emulsion_raster::IRect::new(16, 16, 224, 160)
                );
            } else {
                assert_eq!(mask.fill(), 255);
                assert_eq!(mask.tile_count(), 0, "reveal-all mask is sparse");
                assert_eq!(emulsion_raster::select::bounds(mask), mask.bounds());
            }
            editor.update(cx, |e, cx| e.undo(cx));
            assert_eq!(editor.read(cx).editor.doc, original);
        });
    }
}

#[gpui_kit::test]
fn mask_brush_uses_gray_foreground_and_keeps_pixels_untouched(cx: &mut TestAppContext) {
    let mut document = doc(&["Pixels"], None);
    let id = document.nodes[0].id;
    document.nodes[0].mask = Some(Arc::new(Mask::white(256, 192)));
    let original = document.nodes[0].kind.clone();
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for (foreground, expected) in [
        ([128, 128, 128, 255], 128u8),
        ([0, 0, 0, 255], 0),
        ([255; 4], 255),
    ] {
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.set_tool(Tool::Mask, cx);
                e.tools.brush = Brush {
                    size: 32.,
                    hardness: 1.,
                    opacity: 1.,
                    flow: 1.,
                    ..Default::default()
                };
                e.set_fg(foreground, cx);
            })
        });
        cx.run_until_parked();
        let point = cx.update(|_, cx| editor.read(cx).doc_to_window((80., 80.)).unwrap());
        cx.simulate_click(point, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let node = editor.read(cx).editor.doc.node(id).unwrap();
            let coverage = node.mask.as_ref().unwrap().get(80, 80);
            assert!(
                (coverage as i16 - expected as i16).abs() <= 1,
                "expected {expected}, got {coverage}"
            );
            assert_eq!(node.kind, original);
        });
    }
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_paint(PaintKind::Smudge, cx);
            assert!(
                e.tools.mask_edit,
                "changing brush mode keeps the mask target"
            );
        })
    });
}

#[gpui_kit::test]
fn styles_copy_across_tabs_and_defaults_roundtrip(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Source"], None));
    let source = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let mut style = LayerStyle::catalogue()[0].clone();
    style.set_param("size", 27.0);
    cx.update(|_, cx| {
        source.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            e.execute(
                Command::SetStyles {
                    id,
                    styles: vec![style.clone()],
                },
                cx,
            );
            e.copy_layer_style(cx);
            e.save_style_default(id, 0, cx);
            let settings = crate::app_state::settings(cx);
            let restored: emulsion_io::settings::Settings =
                serde_json::from_str(&serde_json::to_string(settings).unwrap()).unwrap();
            assert_eq!(restored.layer_style_defaults, vec![style.clone()]);
        })
    });
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                doc(&["Target"], None),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            )
        })
    });
    let target = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        target.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            e.paste_layer_style(cx);
            assert_eq!(e.editor.doc.node(id).unwrap().styles, vec![style.clone()]);
            e.undo(cx);
            assert!(e.editor.doc.node(id).unwrap().styles.is_empty());
            e.add_style(id, LayerStyle::catalogue()[0].clone(), cx);
            assert_eq!(e.editor.doc.node(id).unwrap().styles, vec![style]);
            e.reset_style_default(id, 0, cx);
            assert_eq!(
                e.editor.doc.node(id).unwrap().styles,
                vec![LayerStyle::catalogue()[0].clone()]
            );
            assert!(
                crate::app_state::settings(cx)
                    .layer_style_defaults
                    .is_empty()
            );
        })
    });
}

#[gpui_kit::test]
fn style_and_mask_actions_do_not_commit_an_in_progress_gesture(cx: &mut TestAppContext) {
    let mut original = doc(&["Source", "Target"], None);
    let (source, target) = (original.nodes[0].id, original.nodes[1].id);
    original.nodes[0].styles = vec![LayerStyle::catalogue()[0].clone()];
    original.nodes[1].mask = Some(Arc::new(Mask::white(256, 192)));
    let (ws, cx) = open(cx, original);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![source], Some(source));
            e.copy_layer_style(cx);
            e.set_layer_selection(vec![target], Some(target));
            e.editor.begin("Unfinished move");
            e.execute(
                Command::SetOpacity {
                    id: target,
                    opacity: 0.5,
                },
                cx,
            );
            let before = e.editor.doc.clone();
            let history = e.editor.history.len();
            e.paste_layer_style(cx);
            e.transfer_layer_style(source, target, true, cx);
            e.apply_layer_mask(cx);
            assert!(e.editor.in_transaction());
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), history);
            e.editor.cancel();
        })
    });
}

#[gpui_kit::test]
fn replacing_moved_mask_resets_geometry_and_select_mask_ignores_layer_alpha(
    cx: &mut TestAppContext,
) {
    let mut document = doc(&["Transparent pixels"], Some(Raster::transparent(256, 192)));
    let id = document.nodes[0].id;
    let original_mask = Arc::new(Mask::white(256, 192));
    document.nodes[0].mask = Some(original_mask.clone());
    document.nodes[0].mask_linked = false;
    document.nodes[0].mask_enabled = false;
    document.nodes[0].locks.position = true;
    document.nodes[0].mask_transform = [1., 0., 0., 1., 100., 20.];
    document.selection = Some(Arc::new(emulsion_raster::select::rect(
        256, 192, 50., 40., 10., 10.,
    )));
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.add_mask(cx);
            let node = e.editor.doc.node(id).unwrap();
            assert_eq!(node.mask_transform, [1., 0., 0., 1., 0., 0.]);
            assert!(!node.mask_linked);
            assert!(node.mask_enabled);
            assert_eq!(node.mask.as_ref().unwrap().get(15, 25), 0);
            assert_eq!(node.mask.as_ref().unwrap().get(55, 45), 255);
            e.undo(cx);
            assert!(Arc::ptr_eq(
                e.editor.doc.node(id).unwrap().mask.as_ref().unwrap(),
                &original_mask
            ));
            assert_eq!(
                e.editor.doc.node(id).unwrap().mask_transform,
                [1., 0., 0., 1., 100., 20.]
            );
            e.redo(cx);
            e.execute(Command::SetMaskEnabled { id, enabled: false }, cx);
            e.execute(Command::SetSelection { selection: None }, cx);
            e.mask_to_selection(cx);
            let selection = e.editor.doc.selection.as_ref().unwrap();
            assert_eq!(
                emulsion_raster::select::bounds(selection),
                emulsion_raster::IRect::new(50, 40, 10, 10)
            );
            assert_eq!(
                selection.get(55, 45),
                255,
                "disabled mask selected independently of fully transparent pixels"
            );
        })
    });
}
