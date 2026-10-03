//! Clipboard tests use GPUI's simulated OS clipboard, never the real pasteboard.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::{NodeId, NodeKind};
use emulsion_raster::{Mask, select};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{ClipboardEntry, ClipboardItem};

fn editor(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    cx.update(|_, cx| ws.read(cx).editor.clone().unwrap())
}

#[gpui_kit::test]
fn clipboard_shortcuts_work_after_opening_and_switching_tabs(cx: &mut TestAppContext) {
    let source_doc = doc(&["Source"], None);
    let source_id = source_doc.nodes[0].id;
    let (ws, cx) = open(cx, source_doc.clone());
    let source = editor(&ws, cx);
    cx.update(|_, cx| source.update(cx, |e, _| e.selected = Some(source_id)));
    let copy = if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    };
    let paste = if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    };
    let undo = if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    };
    cx.simulate_keystrokes(copy);
    cx.run_until_parked();
    let target_doc = Document::new(256, 192);
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                target_doc.clone(),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    let target = editor(&ws, cx);
    cx.simulate_keystrokes(paste);
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(target.read(cx).editor.doc.nodes.len(), 1);
        assert_eq!(target.read(cx).editor.history.len(), 1);
        assert_eq!(source.read(cx).editor.doc, source_doc);
    });
    cx.simulate_keystrokes(undo);
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(target.read(cx).editor.doc, target_doc));

    // Switching away and back must restore canvas shortcuts without a canvas click.
    for index in [0, 1] {
        cx.simulate_keystrokes("ctrl-tab");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(ws.read(cx).active_tab(), Some(index)));
    }
    cx.simulate_keystrokes(paste);
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(target.read(cx).editor.doc.nodes.len(), 1);
        assert_eq!(source.read(cx).editor.doc, source_doc);
        assert_eq!(source.read(cx).editor.history.len(), 0);
    });
    cx.simulate_keystrokes(undo);
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(target.read(cx).editor.doc, target_doc));
}

#[gpui_kit::test]
fn clipboard_selection_buttons_restore_canvas_shortcuts(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Source"], None));
    let source = editor(&ws, cx);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.simulate_keystrokes("m");
    cx.run_until_parked();
    cx.update(|window, cx| window.click("sel-all", cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-c");
    cx.run_until_parked();
    let selected_doc = cx.update(|_, cx| {
        assert!(matches!(
            cx.read_from_clipboard().unwrap().entries[0],
            ClipboardEntry::Image(_)
        ));
        let e = source.read(cx);
        assert!(e.editor.doc.selection.is_some());
        e.editor.doc.clone()
    });
    cx.update(|window, cx| window.click("sel-none", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert!(source.read(cx).editor.doc.selection.is_none()));
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(source.read(cx).editor.doc, selected_doc));
}

#[gpui_kit::test]
fn clipboard_rectangle_selection_deselect_and_undo_across_tabs(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Source"], None));
    let source = editor(&ws, cx);
    cx.update(|_, cx| source.update(cx, |e, _| e.selected = None));
    cx.simulate_keystrokes("m m");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(source.read(cx).tool, Tool::Select);
        assert_eq!(
            source.read(cx).select_shape(),
            crate::editor::SelectShape::Rect
        );
    });
    let start = cx.update(|_, cx| source.read(cx).doc_to_window((20., 20.)).unwrap());
    let end = cx.update(|_, cx| source.read(cx).doc_to_window((100., 80.)).unwrap());
    cx.simulate_mouse_down(
        start,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::none(),
    );
    cx.simulate_mouse_move(
        end,
        Some(gpui_kit::MouseButton::Left),
        gpui_kit::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        end,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::none(),
    );
    cx.run_until_parked();
    let selected_doc = cx.update(|_, cx| {
        let e = source.read(cx);
        let mask = e.editor.doc.selection.as_ref().unwrap();
        assert_eq!(mask.get(25, 25), 255, "rectangle includes its corner");
        assert_eq!(mask.get(150, 100), 0);
        e.editor.doc.clone()
    });
    cx.simulate_keystrokes("ctrl-c ctrl-d");
    cx.run_until_parked();
    cx.update(|_, cx| assert!(source.read(cx).editor.doc.selection.is_none()));
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(source.read(cx).editor.doc, selected_doc));
    cx.simulate_keystrokes("ctrl-shift-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert!(source.read(cx).editor.doc.selection.is_none()));
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(source.read(cx).editor.doc, selected_doc));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| assert!(source.read(cx).editor.doc.selection.is_none()));
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(source.read(cx).editor.doc, selected_doc));
    let target_doc = Document::new(256, 192);
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                target_doc.clone(),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    let target = editor(&ws, cx);
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = target.read(cx);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert!((raster.width() as i32 - 80).abs() <= 2);
        assert!((raster.height() as i32 - 60).abs() <= 2);
        assert_eq!(source.read(cx).editor.doc, selected_doc);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(target.read(cx).editor.doc, target_doc));
    cx.simulate_keystrokes("ctrl-tab ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(ws.read(cx).active_tab(), Some(0));
        assert!(source.read(cx).editor.doc.selection.is_none());
    });
}

#[gpui_kit::test]
fn clipboard_select_all_without_active_layer_copies_across_tabs(cx: &mut TestAppContext) {
    let source_doc = doc(&["Source"], None);
    let source_id = source_doc.nodes[0].id;
    let (ws, cx) = open(cx, source_doc);
    let source = editor(&ws, cx);
    for panel in [false, true] {
        cx.update(|window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string("replace me".into()));
            source.update(cx, |e, cx| {
                e.selected = None;
                window.focus(
                    if panel {
                        &e.panel_focus
                    } else {
                        &e.canvas_focus
                    },
                    cx,
                );
            });
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a cmd-c"
        } else {
            "ctrl-a ctrl-c"
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = source.read(cx);
            assert_eq!(e.selected, Some(source_id));
            assert!(e.editor.doc.selection.is_some());
            assert!(matches!(
                cx.read_from_clipboard().unwrap().entries[0],
                ClipboardEntry::Image(_)
            ));
        });
    }
    let selected_doc = cx.update(|_, cx| source.read(cx).editor.doc.clone());
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(256, 192),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    let target = editor(&ws, cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(target.read(cx).editor.doc.nodes.len(), 1);
        assert_eq!(source.read(cx).editor.doc, selected_doc);
    });
}

#[gpui_kit::test]
fn clipboard_layer_panel_shortcuts_copy_cut_and_paste(cx: &mut TestAppContext) {
    let source_doc = doc(&["Source"], None);
    let source_id = source_doc.nodes[0].id;
    let (ws, cx) = open(cx, source_doc.clone());
    let e = editor(&ws, cx);
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(source_id);
            window.focus(&e.panel_focus, cx);
        })
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c cmd-v"
    } else {
        "ctrl-c ctrl-v"
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc.nodes.len(), 2));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    });
    cx.run_until_parked();
    cx.update(|_, cx| e.update(cx, |e, _| e.selected = Some(source_id)));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-x"
    } else {
        "ctrl-x"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(source_id).unwrap().kind else {
            panic!()
        };
        assert_eq!(raster.get(0, 0)[3], 0);
    });
}

#[gpui_kit::test]
fn clipboard_cross_tab_paste_centers_in_smaller_document(cx: &mut TestAppContext) {
    let mut source_doc = Document::new(256, 192);
    let source_id = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Source",
            Arc::new(Raster::solid(8, 12, [1., 0., 0., 1.])),
            Placement::at(100., 100.),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut source_doc)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, source_doc.clone());
    let source = editor(&ws, cx);
    cx.update(|_, cx| {
        source.update(cx, |e, cx| {
            e.selected = Some(source_id);
            e.copy_pixels(cx);
        })
    });
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(32, 32),
                None,
                None,
                None,
                "Small target".into(),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    let target = editor(&ws, cx);
    cx.update(|_, cx| target.update(cx, |e, cx| e.paste_pixels(cx)));
    cx.update(|_, cx| {
        let e = target.read(cx);
        let NodeKind::Raster { raster, placement } =
            &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(*placement, Placement::at(12., 10.));
        assert_eq!((raster.width(), raster.height()), (8, 12));
        assert_eq!(source.read(cx).editor.doc, source_doc);
    });
}

#[gpui_kit::test]
fn clipboard_cut_paste_preserves_placement_mask_and_single_undo(cx: &mut TestAppContext) {
    let mut d = Document::new(64, 48);
    let mut layer = Node::raster(
        0,
        "Placed",
        Arc::new(Raster::solid(16, 12, [1., 0., 0., 1.])),
        Placement::at(10., 8.),
    );
    layer.mask = Some(Arc::new(Mask::from_fn(16, 12, 0, |x, _| {
        if x < 8 { 255 } else { 0 }
    })));
    let id = Command::AddNode {
        node: Box::new(layer),
        slot: Slot::TOP,
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    Command::SetSelection {
        selection: Some(Arc::new(select::rect(64, 48, 12., 10., 8., 6.))),
    }
    .apply(&mut d)
    .unwrap();
    let original = d.clone();
    let (ws, cx) = open(cx, d);
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            e.cut_pixels(cx);
        })
    });
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.history.len(), 1);
        let NodeKind::Raster { raster, placement } = &e.editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(*placement, Placement::at(10., 8.));
        assert_eq!(raster.get(2, 2)[3], 0);
        assert_eq!(raster.get(0, 0)[3], 65535);
        let item = cx.read_from_clipboard().expect("portable image clipboard");
        let ClipboardEntry::Image(image) = &item.entries[0] else {
            panic!()
        };
        let png = image::load_from_memory(&image.bytes).unwrap().to_rgba8();
        assert_eq!(png.dimensions(), (8, 6));
        assert_eq!(png.get_pixel(0, 0)[3], 255);
        assert_eq!(
            png.get_pixel(7, 0)[3],
            0,
            "layer mask must be preserved in copied pixels"
        );
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.paste_pixels(cx)));
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.history.len(), 2);
        assert!(e.editor.doc.selection.is_none());
        assert_eq!(e.tool, Tool::Move);
        let NodeKind::Raster { raster, placement } =
            &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(*placement, Placement::at(12., 10.));
        assert_eq!((raster.width(), raster.height()), (8, 6));
    });
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.undo(cx);
            e.undo(cx);
        })
    });
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
}

#[gpui_kit::test]
fn clipboard_free_transform_lifts_selection_without_touching_clipboard(cx: &mut TestAppContext) {
    let mut d = doc(&["Photo"], None);
    let id = d.nodes[0].id;
    let selection = Mask::from_fn(d.width, d.height, 0, |x, y| {
        if (20..44).contains(&x) && (16..28).contains(&y) {
            if x == 20 { 128 } else { 255 }
        } else {
            0
        }
    });
    d.selection = Some(Arc::new(selection));
    let original = d.clone();
    let (ws, cx) = open(cx, d);
    let e = editor(&ws, cx);
    cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("keep this".into()));
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            window.focus(&e.canvas_focus, cx);
        });
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-t"
    } else {
        "ctrl-t"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.history.len(), 1);
        assert_eq!(e.editor.doc.nodes.len(), 2);
        assert!(e.editor.doc.selection.is_none());
        assert_eq!(e.tool, Tool::Move);
        let NodeKind::Raster { raster, placement } =
            &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(*placement, Placement::at(20., 16.));
        assert_eq!((raster.width(), raster.height()), (24, 12));
        assert!((raster.get(0, 0)[3] as i32 - 32896).abs() <= 1);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("keep this")
        );
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
}

#[gpui_kit::test]
fn clipboard_destructive_operations_respect_locked_parent(cx: &mut TestAppContext) {
    let mut d = doc(&["Photo"], None);
    let id = d.nodes[0].id;
    let group = Command::Group {
        ids: vec![id],
        name: "Protected".into(),
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    Command::SetLocked {
        id: group,
        locked: true,
    }
    .apply(&mut d)
    .unwrap();
    d.selection = Some(Arc::new(select::rect(
        d.width, d.height, 10., 10., 20., 20.,
    )));
    let original = d.clone();
    let (ws, cx) = open(cx, d);
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("unchanged".into()));
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            e.cut_pixels(cx);
            e.clear_pixels(cx);
            e.transform_pixels(cx);
        });
        assert_eq!(e.read(cx).editor.doc, original);
        assert_eq!(e.read(cx).editor.history.len(), 0);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("unchanged")
        );
    });
}

#[gpui_kit::test]
fn clipboard_canvas_delete_clears_selection_then_deletes_layer_without_selection(
    cx: &mut TestAppContext,
) {
    let mut d = doc(&["Photo"], None);
    let id = d.nodes[0].id;
    d.selection = Some(Arc::new(select::rect(
        d.width, d.height, 10., 10., 20., 20.,
    )));
    let (ws, cx) = open(cx, d);
    let e = editor(&ws, cx);
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            window.focus(&e.canvas_focus, cx);
        })
    });
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 1);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(raster.get(15, 15)[3], 0);
        assert_eq!(raster.get(0, 0)[3], 65535);
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.deselect(cx)));
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    cx.update(|_, cx| assert!(e.read(cx).editor.doc.nodes.is_empty()));
}

#[gpui_kit::test]
fn clipboard_shortcuts_in_text_input_do_not_edit_canvas_pixels(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = editor(&ws, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(Tool::Type, cx)));
    cx.run_until_parked();
    let p = cx.update(|_, cx| e.read(cx).doc_to_window((30., 20.)).unwrap());
    cx.simulate_click(p, gpui_kit::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_keystrokes("H i");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-c"
    } else {
        "ctrl-a ctrl-c"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let item = cx.read_from_clipboard().unwrap();
        assert_eq!(item.text().as_deref(), Some("Hi"));
        assert!(
            item.entries
                .iter()
                .all(|e| !matches!(e, ClipboardEntry::Image(_)))
        );
        assert!(
            e.read(cx).editor.doc.selection.is_none(),
            "text Select All must not select the canvas"
        );
    });
}

#[gpui_kit::test]
fn clipboard_clear_handles_implicit_fill_and_retains_off_canvas_pixels(cx: &mut TestAppContext) {
    let mut d = Document::new(16, 16);
    let source = Raster::empty(32, 16, [65535, 0, 0, 65535]);
    assert!(
        source.tile_bounds().is_empty(),
        "fixture has no allocated tiles"
    );
    let id = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Sparse fill",
            Arc::new(source),
            Placement::at(-8., 0.),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, d);
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            e.clear_pixels(cx);
        })
    });
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(raster.get(10, 8)[3], 0, "visible implicit fill clears");
        assert_eq!(raster.get(2, 8)[3], 65535, "left off-canvas pixels survive");
        assert_eq!(
            raster.get(30, 8)[3],
            65535,
            "right off-canvas pixels survive"
        );
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn clipboard_and_internal_lift_retain_16_bit_precision(cx: &mut TestAppContext) {
    let pixel = [12345, 23456, 34567, 65535];
    let mut d = Document::new(16, 16);
    let id = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Precise color",
            Arc::new(Raster::empty(16, 16, pixel)),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    d.selection = Some(Arc::new(select::rect(16, 16, 2., 3., 6., 7.)));
    let (ws, cx) = open(cx, d);
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            e.copy_pixels(cx);
        })
    });
    cx.update(|_, cx| {
        let item = cx.read_from_clipboard().unwrap();
        let ClipboardEntry::Image(image) = &item.entries[0] else {
            panic!()
        };
        assert_eq!(
            image::load_from_memory(&image.bytes).unwrap().color(),
            image::ColorType::Rgba16
        );
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.paste_pixels(cx)));
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        for (actual, expected) in raster.get(0, 0).into_iter().zip(pixel) {
            assert!(
                (actual as i32 - expected as i32).abs() <= 2,
                "16-bit sRGB clipboard roundtrip: {actual} vs {expected}"
            );
        }
    });
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.undo(cx);
            e.selected = Some(id);
            e.transform_pixels(cx);
        })
    });
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(
            raster.get(0, 0),
            pixel,
            "internal lift must stay in linear 16-bit pixels"
        );
    });
}

fn text_clipboard_document() -> (Document, NodeId, emulsion_core::text::TextSpec) {
    let mut d = Document::new(600, 400);
    let mut spec = emulsion_core::text::TextSpec {
        text: "Crisp text\nMixed café".into(),
        font: "Geist".into(),
        size: 38.0,
        x: 90.25,
        y: 110.5,
        rotation: 8.0,
        scale_x: 1.15,
        bold: true,
        color: [30, 60, 120, 255],
        ..Default::default()
    };
    spec.apply_style(6..10, |style| {
        style.italic = true;
        style.color = [180, 35, 80, 255];
        style.baseline = 4.5;
        style.size = 44.;
    });
    let id = Command::AddNode {
        node: Box::new(Node::text(0, "Heading", spec.clone(), d.width, d.height)),
        slot: Slot::TOP,
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    (d, id, spec)
}

#[gpui_kit::test]
fn clipboard_text_stays_editable_and_preserves_exact_spec_and_undo(cx: &mut TestAppContext) {
    let (d, id, spec) = text_clipboard_document();
    let (ws, cx) = open(cx, d.clone());
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            e.copy_pixels(cx);
            e.paste_pixels(cx);
            let pasted = e.selected.unwrap();
            assert_ne!(pasted, id);
            let NodeKind::Text {
                spec: actual,
                cache,
            } = &e.editor.doc.node(pasted).unwrap().kind
            else {
                panic!("pasted text was rasterized");
            };
            assert_eq!(**actual, spec);
            assert_eq!(cache.size(), (600, 400));
            assert_eq!(e.editor.history.len(), 1);
            assert!(matches!(
                cx.read_from_clipboard().unwrap().entries[0],
                ClipboardEntry::Image(_)
            ));
            e.undo(cx);
            assert_eq!(e.editor.doc, d);
            e.redo(cx);
            assert!(matches!(
                e.editor.doc.node(pasted).unwrap().kind,
                NodeKind::Text { .. }
            ));
        })
    });
}

#[gpui_kit::test]
fn clipboard_design_background_into_photo_keeps_ordinary_layer_semantics(cx: &mut TestAppContext) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};

    let mut source_editor = emulsion_core::Editor::new(Document::new(80, 60), None);
    emulsion_core::design_background::set_color(&mut source_editor, [25, 50, 75, 255]).unwrap();
    let source_doc = source_editor.doc;
    let fill = source_doc.design.page_background.unwrap().fill;
    let (ws, cx) = open(cx, Document::new(80, 60));
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install_project(
                ProjectEditor::new_project(ProjectKind::Design, source_doc.clone()).unwrap(),
                "Design source".into(),
                window,
                cx,
            );
        });
    });
    let source = editor(&ws, cx);
    cx.update(|_, cx| {
        source.update(cx, |e, cx| {
            e.set_layer_selection(vec![fill], Some(fill));
            e.copy_pixels(cx);
        });
    });
    let target_doc = Document::new(80, 60);
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                target_doc.clone(),
                None,
                None,
                None,
                "Photo destination".into(),
                window,
                cx,
            );
        });
    });
    cx.run_until_parked();
    let target = editor(&ws, cx);
    cx.update(|_, cx| {
        target.update(cx, |e, cx| {
            assert_eq!(e.editor.kind(), None);
            e.paste_pixels(cx);
            assert_eq!(e.editor.doc.nodes.len(), 1);
            assert!(e.editor.doc.design.page_background.is_none());
            assert!(matches!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().kind,
                NodeKind::Fill {
                    rgba: [25, 50, 75, 255]
                }
            ));
            assert_eq!(e.editor.history.len(), 1);
            let pasted = e.editor.doc.clone();
            e.undo(cx);
            assert_eq!(e.editor.doc, target_doc);
            e.redo(cx);
            assert_eq!(e.editor.doc, pasted);
        });
        assert_eq!(source.read(cx).editor.doc, source_doc);
    });
}

#[gpui_kit::test]
fn clipboard_text_cross_tab_uses_destination_cache_and_centers(cx: &mut TestAppContext) {
    let (d, id, spec) = text_clipboard_document();
    let bounds = emulsion_core::geometry::node_bounds(&d, id).unwrap();
    let (ws, cx) = open(cx, d);
    let source = editor(&ws, cx);
    cx.update(|_, cx| {
        source.update(cx, |e, cx| {
            e.selected = Some(id);
            e.copy_pixels(cx);
        })
    });
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(800, 500),
                None,
                None,
                None,
                "Destination".into(),
                window,
                cx,
            );
        })
    });
    cx.run_until_parked();
    let target = editor(&ws, cx);
    cx.update(|_, cx| {
        target.update(cx, |e, cx| {
            e.paste_pixels(cx);
            let NodeKind::Text {
                spec: actual,
                cache,
            } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
            else {
                panic!("cross-tab text was rasterized");
            };
            assert_eq!(cache.size(), (800, 500));
            let mut expected = spec.clone();
            expected.x += (800.0 - bounds.w as f32) / 2.0 - bounds.x as f32;
            expected.y += (500.0 - bounds.h as f32) / 2.0 - bounds.y as f32;
            assert!((actual.x - expected.x).abs() < 0.001);
            assert!((actual.y - expected.y).abs() < 0.001);
            assert_eq!(actual.text, spec.text);
            assert_eq!(actual.size, spec.size);
            assert_eq!(actual.font, spec.font);
            assert_eq!(actual.runs, spec.runs);
            assert_eq!(actual.rotation, spec.rotation);
            assert_eq!(actual.scale_x, spec.scale_x);
            assert_eq!(e.editor.history.len(), 1);
        })
    });
}

#[gpui_kit::test]
fn clipboard_text_selection_and_replaced_clipboard_still_paste_pixels(cx: &mut TestAppContext) {
    let (d, id, _) = text_clipboard_document();
    let (ws, cx) = open(cx, d);
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(id);
            e.copy_pixels(cx);
            // A different clipboard image must never resurrect our cached text.
            let bytes = emulsion_io::export::png16(1, 1, &[65535, 0, 0, 65535]).unwrap();
            let image = gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes);
            cx.write_to_clipboard(ClipboardItem::new_image(&image));
            e.paste_pixels(cx);
            assert!(matches!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().kind,
                NodeKind::Raster { .. }
            ));
            e.set_layer_selection(vec![id], Some(id));
            e.editor.doc.selection =
                Some(Arc::new(select::rect(600, 400, 90.0, 110.0, 180.0, 180.0)));
            e.copy_pixels(cx);
            e.paste_pixels(cx);
            assert!(matches!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().kind,
                NodeKind::Raster { .. }
            ));
        })
    });
}

#[gpui_kit::test]
fn paste_in_place_keeps_the_copied_position_in_another_document(cx: &mut TestAppContext) {
    let mut source_doc = Document::new(256, 192);
    let small = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Small",
            Arc::new(Raster::solid(20, 10, [1., 0., 0., 1.])),
            Placement::at(40., 30.),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut source_doc)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, source_doc);
    let source = editor(&ws, cx);
    cx.update(|_, cx| source.update(cx, |e, _| e.selected = Some(small)));
    cx.simulate_keystrokes("ctrl-c");
    cx.run_until_parked();
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(256, 192),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    let target = editor(&ws, cx);
    let top_position = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let doc = &target.read(cx).editor.doc;
            let top = *doc.children(None).last().unwrap();
            let NodeKind::Raster { placement, .. } = &doc.node(top).unwrap().kind else {
                panic!("pasted a raster layer")
            };
            (placement.x, placement.y)
        })
    };
    // Paste in Place (Edit menu, Ctrl+Shift+V) keeps the copied position.
    cx.simulate_keystrokes("ctrl-shift-v");
    cx.run_until_parked();
    assert_eq!(top_position(cx), (40., 30.));
    assert_eq!(cx.update(|_, cx| target.read(cx).editor.history.len()), 1);
    // A plain paste into another document centres the pixels.
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    assert_eq!(top_position(cx), (118., 91.));
}
