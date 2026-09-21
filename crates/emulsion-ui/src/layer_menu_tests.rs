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
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert_eq!(
                emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0).to_srgba16(),
                expected.to_srgba16()
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
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
fn flatten_discards_hidden_content_fills_transparency_and_undo_restores_everything(
    cx: &mut TestAppContext,
) {
    let mut original = doc(
        &["Paint", "Hidden"],
        Some(Raster::solid(20, 20, [0.5, 0., 0., 0.5])),
    );
    original.nodes[1].visible = false;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.flatten_image(cx);
            assert_eq!(e.editor.doc.nodes.len(), 1);
            let rendered =
                emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0).to_srgba8();
            let outside = (100 * e.editor.doc.width as usize + 100) * 4;
            assert_eq!(&rendered[outside..outside + 4], &[255; 4]);
            assert!(rendered.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.editor.doc.nodes[1].locked = true;
            let locked = e.editor.doc.clone();
            e.flatten_image(cx);
            assert_eq!(
                e.editor.doc, locked,
                "hidden locked content cannot be discarded"
            );
        })
    });
}

#[gpui_kit::test]
fn merge_groups_effects_and_clipped_layers_preserves_composite(cx: &mut TestAppContext) {
    let mut original = doc(&["Base", "Overlay"], None);
    let base = original.nodes[0].id;
    original.nodes[1].clip_to = Some(base);
    original.nodes[1].blend = emulsion_raster::BlendMode::Screen;
    original.nodes[1]
        .styles
        .push(emulsion_core::styles::LayerStyle::ColorOverlay {
            color: [255, 50, 20],
            opacity: 0.5,
        });
    let group = Command::AddNode {
        node: Box::new(Node::group(0, "Group")),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    let child = Node::raster(
        0,
        "Child",
        Arc::new(Raster::solid(16, 16, [0., 1., 0., 1.])),
        Placement::at(25., 25.),
    );
    Command::AddNode {
        node: Box::new(child),
        slot: Slot::top_of(Some(group)),
    }
    .apply(&mut original)
    .unwrap();
    let ids = original.children(None);
    let expected = emulsion_raster::composite::flatten(&original.composite_tree(), 0).to_srgba16();
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(ids, Some(group));
            e.merge_layers(false, cx);
            assert_eq!(e.editor.doc.nodes.len(), 1);
            assert_eq!(
                emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0).to_srgba16(),
                expected
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
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

#[gpui_kit::test]
fn noncontiguous_merge_keeps_unselected_layer_and_uses_top_selected_position(
    cx: &mut TestAppContext,
) {
    let original = doc(&["Bottom", "Middle", "Top"], None);
    let ids: Vec<_> = original.nodes.iter().map(|node| node.id).collect();
    let middle = original.nodes[1].clone();
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![ids[0], ids[2]], Some(ids[2]));
            e.merge_layers(false, cx);
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert_eq!(e.editor.doc.nodes[0], middle);
            assert_eq!(e.editor.doc.nodes[1].name, "Top");
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn merge_rejects_clip_dependencies_crossing_selection_boundary(cx: &mut TestAppContext) {
    let mut original = doc(&["Base", "Clipped", "Top"], None);
    let ids: Vec<_> = original.nodes.iter().map(|node| node.id).collect();
    original.nodes[1].clip_to = Some(ids[0]);
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for selection in [vec![ids[0], ids[2]], vec![ids[1], ids[2]]] {
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.set_layer_selection(selection, Some(ids[2]));
                e.merge_layers(false, cx);
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.history.len(), 0);
            })
        });
    }
}

#[gpui_kit::test]
fn alt_drag_copies_effects_and_advanced_blending_without_reordering(cx: &mut TestAppContext) {
    let mut original = doc(&["Target", "Source"], None);
    original.nodes[1].styles = vec![emulsion_core::styles::LayerStyle::catalogue()[0].clone()];
    original.nodes[1].blend = emulsion_raster::BlendMode::Multiply;
    original.nodes[1].opacity = 0.6;
    original.nodes[1].blending.fill_opacity = 0.3;
    original.nodes[1].blending.blend_if.source.black_fade = 0.25;
    let (target, source) = (original.nodes[0].id, original.nodes[1].id);
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    let (start, end) = cx.update(|window, _| {
        (
            window.find(("row", source)).bounds().center(),
            window.find(("row", target)).bounds().center(),
        )
    });
    let modifiers = gpui_kit::Modifiers {
        alt: true,
        ..Default::default()
    };
    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, modifiers);
    cx.simulate_mouse_move(
        start + gpui_kit::point(gpui_kit::px(10.), gpui_kit::px(0.)),
        Some(gpui_kit::MouseButton::Left),
        modifiers,
    );
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), modifiers);
    cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, modifiers);
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.children(None), vec![target, source]);
            let applied = e.editor.doc.node(target).unwrap();
            assert_eq!(applied.styles, original.nodes[1].styles);
            assert_eq!(applied.blend, original.nodes[1].blend);
            assert_eq!(applied.opacity, original.nodes[1].opacity);
            assert_eq!(applied.blending, original.nodes[1].blending);
            assert_eq!(e.editor.doc.node(source).unwrap(), &original.nodes[1]);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn merged_layer_keeps_only_a_shared_link_with_an_external_partner(cx: &mut TestAppContext) {
    let mut original = doc(&["Partner", "Merge A", "Merge B"], None);
    for node in &mut original.nodes {
        node.link_group = Some(42);
    }
    let ids: Vec<_> = original.nodes.iter().map(|node| node.id).collect();
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![ids[1], ids[2]], Some(ids[2]));
            e.merge_layers(false, cx);
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert_eq!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().link_group,
                Some(42)
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.editor.doc.node_mut(ids[2]).unwrap().link_group = Some(43);
            e.set_layer_selection(vec![ids[1], ids[2]], Some(ids[2]));
            e.merge_layers(false, cx);
            assert_eq!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().link_group,
                None
            );
        })
    });
}
