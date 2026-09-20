//! Clipboard tests use GPUI's simulated OS clipboard, never the real pasteboard.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::NodeKind;
use emulsion_raster::{Mask, select};
use gpui_kit::{ClipboardEntry, ClipboardItem};

fn editor(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    cx.update(|_, cx| ws.read(cx).editor.clone().unwrap())
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
