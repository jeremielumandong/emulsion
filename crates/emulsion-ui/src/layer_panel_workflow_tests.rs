use super::*;
use crate::editor::Tool;
use emulsion_core::node::LayerColor;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn photoshop_shortcuts_cycle_selected_layer_blend_mode(cx: &mut TestAppContext) {
    let document = doc(&["Layer"], None);
    let id = document.nodes[0].id;
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        editor.update(cx, |e, _| e.selected = Some(id));
        let focus = editor.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("shift-=");
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.node(id).unwrap().blend),
        emulsion_raster::BlendMode::Dissolve
    );
    cx.simulate_keystrokes("ctrl-z shift--");
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.node(id).unwrap().blend),
        emulsion_raster::BlendMode::Luminosity
    );
}

#[gpui_kit::test]
fn layer_search_reaches_collapsed_children_without_changing_document(cx: &mut TestAppContext) {
    let mut document = doc(&["Photo", "Title"], None);
    let title = document.nodes[1].id;
    let group = Command::AddNode {
        node: Box::new(Node::group(0, "Group")),
        slot: Slot::TOP,
    }
    .apply(&mut document)
    .unwrap()
    .unwrap();
    Command::MoveNode {
        id: title,
        slot: Slot::top_of(Some(group)),
    }
    .apply(&mut document)
    .unwrap();
    Command::SetCollapsed {
        id: group,
        collapsed: true,
    }
    .apply(&mut document)
    .unwrap();
    let (ws, cx) = open(cx, document.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(900.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    let field = cx.update(|_, cx| {
        editor
            .read(cx)
            .layer_panel
            .search
            .as_ref()
            .unwrap()
            .0
            .clone()
    });
    cx.update(|window, cx| field.update(cx, |s, cx| s.focus(window, cx)));
    cx.simulate_input("TITLE");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(
            e.filtered_layer_rows()
                .iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            vec![title]
        );
        assert_eq!(e.editor.doc, document);
    });
    cx.simulate_keystrokes("ctrl-a backspace");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(
            !editor
                .read(cx)
                .filtered_layer_rows()
                .iter()
                .any(|row| row.id == title)
        )
    });
}

#[gpui_kit::test]
fn mask_and_content_thumbnails_choose_independent_edit_targets(cx: &mut TestAppContext) {
    let mut document = doc(&["Masked"], None);
    let id = document.nodes[0].id;
    document.nodes[0].mask = Some(Arc::new(emulsion_raster::Mask::empty(256, 192, 255)));
    document.nodes[0].color_label = LayerColor::Blue;
    let (ws, cx) = open(cx, document.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1100.), gpui_kit::px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    let mask = cx.update(|window, _| window.find(("layer-mask", id)).bounds().center());
    cx.simulate_click(mask, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.selected, Some(id));
        assert_eq!(e.tool, Tool::Mask);
        assert!(e.tools.mask_edit);
        assert_eq!(e.editor.doc, document);
    });
    let alt = gpui_kit::Modifiers {
        alt: true,
        ..Default::default()
    };
    cx.simulate_click(mask, alt);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.mask_view.layer, Some(id));
        assert_eq!(e.editor.doc, document);
    });
    cx.simulate_click(mask, alt);
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).mask_view.layer, None));
    cx.simulate_click(mask, alt);
    cx.run_until_parked();
    let content = cx.update(|window, _| window.find(("layer-content", id)).bounds().center());
    cx.simulate_click(content, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.selected, Some(id));
        assert_eq!(e.tool, Tool::Brush);
        assert!(!e.tools.mask_edit);
        assert_eq!(e.mask_view.layer, None);
        assert_eq!(e.editor.doc, document);
    });
    let shift = gpui_kit::Modifiers {
        shift: true,
        ..Default::default()
    };
    cx.simulate_click(mask, shift);
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!editor.read(cx).editor.doc.node(id).unwrap().mask_enabled);
        assert!(window.find(("layer-mask-disabled", id)).visible());
    });
    cx.simulate_click(mask, shift);
    cx.run_until_parked();
    cx.update(|_, cx| assert!(editor.read(cx).editor.doc.node(id).unwrap().mask_enabled));
    let mask = cx.update(|window, _| window.find(("layer-mask", id)).bounds().center());
    cx.simulate_mouse_down(mask, gpui_kit::MouseButton::Right, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            window.within("popup-menu").find(0usize).label(),
            Some("Delete Layer Mask")
        );
        window.within("popup-menu").click(0usize, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(editor.read(cx).editor.doc.node(id).unwrap().mask.is_none()));
}

#[gpui_kit::test]
fn layer_lock_header_applies_to_multiselection_in_one_undo(cx: &mut TestAppContext) {
    let document = doc(&["One", "Two"], None);
    let ids: Vec<_> = document.nodes.iter().map(|n| n.id).collect();
    let (ws, cx) = open(cx, document.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1100.), gpui_kit::px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.select_layer_row(ids[0], false, false, cx);
            e.select_layer_row(ids[1], true, false, cx);
        })
    });
    cx.run_until_parked();
    let position = cx.update(|window, _| window.find(("layer-lock", 2usize)).bounds().center());
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(
            editor
                .read(cx)
                .editor
                .doc
                .nodes
                .iter()
                .all(|n| n.locks.position)
        );
        assert_eq!(editor.read(cx).editor.history.len(), 1);
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, document);
    });
}

#[gpui_kit::test]
fn default_layer_dock_keeps_three_rows_inside_clickable_list(cx: &mut TestAppContext) {
    let document = doc(&["One", "Two", "Three"], None);
    let ids: Vec<_> = document.nodes.iter().map(|n| n.id).collect();
    let (ws, cx) = open(cx, document);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(720.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    for id in ids {
        let point = cx.update(|window, _| {
            let list = window.find("sidebar-layers-list").bounds();
            let row = window.find(("row", id)).bounds();
            assert!(
                list.contains(&row.center()),
                "row must be inside scrolling list: row={row:?} list={list:?}"
            );
            assert!(
                row.top() >= list.top() && row.bottom() <= list.bottom(),
                "whole row must remain visible"
            );
            row.center()
        });
        cx.simulate_click(point, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).selected, Some(id)));
    }
}

#[gpui_kit::test]
fn layer_header_sliders_have_independent_tracks_and_batch_undo(cx: &mut TestAppContext) {
    let document = doc(&["One", "Two"], None);
    let ids: Vec<_> = document.nodes.iter().map(|n| n.id).collect();
    let id = ids[1];
    let (ws, cx) = open(cx, document.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1100.), gpui_kit::px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.select_layer_row(ids[0], false, false, cx);
            e.select_layer_row(ids[1], true, false, cx);
            e.select_sidebar(crate::editor::SidebarTab::Properties, cx);
        })
    });
    cx.run_until_parked();
    for (header, inspector, fill) in [
        ("LayerOpacity", "Opacity", false),
        ("LayerFillOpacity", "LayerOpacity", true),
    ] {
        let point = cx.update(|window, _| {
            let header = window
                .find(gpui_kit::SharedString::from(format!("{header}({id})")))
                .bounds();
            let inspector = window
                .find(gpui_kit::SharedString::from(format!("{inspector}({id})")))
                .bounds();
            assert_ne!(header.origin, inspector.origin);
            gpui_kit::point(header.left() + header.size.width * 0.25, header.center().y)
        });
        cx.simulate_click(point, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            let values: Vec<_> = e
                .editor
                .doc
                .nodes
                .iter()
                .map(|n| {
                    if fill {
                        n.blending.fill_opacity
                    } else {
                        n.opacity
                    }
                })
                .collect();
            assert_eq!(values[0], values[1], "both selected layers change together");
            assert!(
                e.editor.doc.nodes.iter().all(|node| if fill {
                    node.opacity == 1.0
                } else {
                    node.blending.fill_opacity == 1.0
                }),
                "the other header control remains unchanged"
            );
            assert!(
                values[0] > 0.1 && values[0] < 0.4,
                "clicked header quarter: {values:?}"
            );
            assert_eq!(e.editor.history.len(), 1);
            editor.update(cx, |e, cx| e.undo(cx));
            assert_eq!(editor.read(cx).editor.doc, document);
        });
    }
}

#[gpui_kit::test]
fn compact_layer_dock_divider_changes_visible_height(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["One", "Two", "Three"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.run_until_parked();
    let (from, before) = cx.update(|window, _| {
        (
            window.find("layers-resize").bounds().center(),
            window.find("sidebar-layers-dock").bounds().size.height,
        )
    });
    let to = from + gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(35.));
    cx.simulate_mouse_down(from, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(to, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(to, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let after = window.find("sidebar-layers-dock").bounds().size.height;
        assert!(
            after < before - gpui_kit::px(25.),
            "divider changes actual height: {before:?} -> {after:?}"
        );
        let editor = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert!(editor.editor.history.is_empty());
    });
}
