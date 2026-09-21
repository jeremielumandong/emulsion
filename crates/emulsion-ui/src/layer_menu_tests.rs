use super::*;
use gpui_kit::ClipboardItem;
use gpui_kit::test::TestWindowExt;

fn open_menu(cx: &mut VisualTestContext, id: emulsion_core::NodeId) {
    let position = cx.update(|window, _| window.find(("row", id)).bounds().center());
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Right, Default::default());
    cx.run_until_parked();
}

fn click_menu(cx: &mut VisualTestContext, index: usize) {
    let point = cx.update(|window, _| window.within("popup-menu").find(index).bounds().center());
    cx.simulate_click(point, Default::default());
    cx.run_until_parked();
}

fn open_submenu(cx: &mut VisualTestContext, index: usize) {
    cx.update(|window, cx| {
        _ = window.draw(cx);
        window.within("popup-menu").hover(index, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
}

fn click_submenu(cx: &mut VisualTestContext, index: usize) {
    let point = cx.update(|window, _| window.within("submenu").find(index).bounds().center());
    cx.simulate_click(point, Default::default());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn layer_menu_duplicate_mask_and_color_are_undoable(cx: &mut TestAppContext) {
    let original = doc(&["Photo", "Caption"], None);
    let id = original.nodes[1].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1200.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    open_menu(cx, id);
    click_menu(cx, 13);
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).editor.doc.nodes.len(), 3);
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
    cx.run_until_parked();
    open_menu(cx, id);
    open_submenu(cx, 24);
    click_submenu(cx, 0);
    cx.update(|_, cx| {
        assert!(editor.read(cx).editor.doc.node(id).unwrap().mask.is_some());
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
        let ids = original.nodes.iter().map(|n| n.id).collect();
        editor.update(cx, |e, _| e.set_layer_selection(ids, Some(id)));
    });
    cx.run_until_parked();
    open_menu(cx, id);
    open_submenu(cx, 26);
    click_submenu(cx, 1);
    cx.update(|_, cx| {
        assert!(
            editor
                .read(cx)
                .editor
                .doc
                .nodes
                .iter()
                .all(|n| n.color_label == emulsion_core::node::LayerColor::Red)
        );
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(
            editor.read(cx).editor.doc,
            original,
            "one undo restores both labels"
        );
    });
}

#[gpui_kit::test]
fn layer_merge_shortcut_keeps_appearance_and_is_one_undo(cx: &mut TestAppContext) {
    let mut original = doc(&["Photo", "Overlay"], None);
    original.nodes[1].opacity = 0.4;
    let id = original.nodes[1].id;
    let expected = emulsion_raster::composite::flatten(&original.composite_tree(), 0);
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            window.focus(&e.panel_focus, cx);
        })
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-e"
    } else {
        "ctrl-e"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 1);
        let actual = emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0);
        assert_eq!(actual.to_srgba16(), expected.to_srgba16());
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn merge_visible_preserves_hidden_layers_and_disabled_merge_preserves_locks(
    cx: &mut TestAppContext,
) {
    let mut original = doc(&["Photo", "Overlay", "Hidden"], None);
    original.nodes[1].blend = emulsion_raster::BlendMode::Multiply;
    original.nodes[2].visible = false;
    let hidden = original.nodes[2].id;
    let expected = emulsion_raster::composite::flatten(&original.composite_tree(), 0);
    let id = original.nodes[1].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            e.merge_layers(false, cx);
            assert_eq!(
                e.editor.doc, original,
                "backdrop-dependent partial merge stays disabled"
            );
            e.merge_layers(true, cx);
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert_eq!(e.editor.doc.node(hidden), original.node(hidden));
            assert_eq!(
                emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0).to_srgba16(),
                expected.to_srgba16()
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.editor.doc.node_mut(id).unwrap().locked = true;
            let locked = e.editor.doc.clone();
            e.merge_layers(true, cx);
            assert_eq!(e.editor.doc, locked);
        })
    });
}

#[gpui_kit::test]
fn multiselected_pixel_cut_copies_every_layer_and_undo_restores_all(cx: &mut TestAppContext) {
    let mut original = doc(&["Photo", "Overlay"], None);
    if let emulsion_core::NodeKind::Raster { raster, placement } = &mut original.nodes[1].kind {
        *raster = Arc::new(Raster::solid(20, 20, [0., 0., 1., 1.]));
        *placement = emulsion_raster::Placement::at(30., 25.);
    }
    let expected = emulsion_raster::composite::flatten(&original.composite_tree(), 0).to_srgba8();
    let ids: Vec<_> = original.nodes.iter().map(|n| n.id).collect();
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(ids.clone(), ids.last().copied());
            e.cut_pixels(cx);
            for id in &ids {
                let emulsion_core::NodeKind::Raster { raster, .. } =
                    &e.editor.doc.node(*id).unwrap().kind
                else {
                    panic!("raster");
                };
                assert!(
                    raster
                        .to_srgba8()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[3] == 0)
                );
            }
            e.undo(cx);
            assert_eq!(
                e.editor.doc, original,
                "one undo restores every selected layer"
            );
            e.paste_pixels(cx);
            let emulsion_core::NodeKind::Raster { raster, .. } =
                &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
            else {
                panic!("pasted pixels");
            };
            assert_eq!(raster.to_srgba8(), expected);
            e.undo(cx);
            e.set_layer_selection(ids.clone(), ids.last().copied());
            e.editor.doc.node_mut(ids[0]).unwrap().locks.pixels = true;
            let locked = e.editor.doc.clone();
            cx.write_to_clipboard(ClipboardItem::new_string("keep this".into()));
            e.cut_pixels(cx);
            assert_eq!(e.editor.doc, locked);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some("keep this")
            );
        })
    });
}
