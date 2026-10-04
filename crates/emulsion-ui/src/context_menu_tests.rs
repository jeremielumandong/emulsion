//! Exercise clipboard menus through actual right-clicks and menu keyboard navigation.
use super::*;
use gpui_kit::MouseButton;
use gpui_kit::test::TestWindowExt;

fn click_menu_item(cx: &mut VisualTestContext, index: usize) {
    let position = cx.update(|window, _| window.within("popup-menu").find(index).bounds().center());
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
}

/// A long layer menu can open a submenu beneath the pointer as it fits into the
/// window. Match only the clicked row's top-level popup items: both menus reuse
/// the same popup ID and small integer item IDs.
pub(super) fn layer_menu_item_position(
    cx: &mut VisualTestContext,
    id: emulsion_core::NodeId,
    index: usize,
    label: &str,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    cx.update(|window, _| {
        let row = window.find(("row", id));
        let popup = gpui_kit::ElementId::from("popup-menu");
        let submenu = gpui_kit::ElementId::from("submenu");
        let item_id = gpui_kit::ElementId::from(index);
        let mut matches: Vec<_> = gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .filter(|item| {
                item.path().starts_with(row.path())
                    && item.path().contains(&popup)
                    && !item.path().contains(&submenu)
                    && item.path().last() == Some(&item_id)
            })
            .collect();
        assert_eq!(matches.len(), 1, "one top-level layer menu item {label}");
        let item = matches.pop().unwrap();
        assert_eq!(item.label(), Some(label));
        assert!(item.visible(), "{label} must be visible");
        item.bounds().center()
    })
}

fn click_layer_menu_item(
    cx: &mut VisualTestContext,
    id: emulsion_core::NodeId,
    index: usize,
    label: &str,
) {
    let position = layer_menu_item_position(cx, id, index, label);
    cx.simulate_mouse_move(position, None, Default::default());
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn layer_context_menu_opens_blending_for_clicked_layer(cx: &mut TestAppContext) {
    let original = doc(&["First", "Second"], None);
    let first = original.nodes[0].id;
    let second = original.nodes[1].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(1200.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| editor.update(cx, |e, _| e.selected = Some(first)));
    cx.update(|window, cx| window.right_click(("row", second), cx));
    cx.run_until_parked();
    click_layer_menu_item(cx, second, 11, &t!("editor.layer_menu.blending_options"));
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.find("layer-style-dialog").visible());
        assert_eq!(editor.read(cx).selected, Some(second));
        assert_eq!(editor.read(cx).editor.doc, original);
        assert!(editor.read(cx).editor.history.is_empty());
    });
    cx.update(|window, cx| window.click(("style-kind", 0usize), cx));
    cx.run_until_parked();
    let ok = cx.update(|window, _| window.find("style-dialog-ok").bounds().center());
    cx.simulate_click(ok, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            editor
                .read(cx)
                .editor
                .doc
                .node(second)
                .unwrap()
                .styles
                .len(),
            1
        );
        assert!(
            editor
                .read(cx)
                .editor
                .doc
                .node(first)
                .unwrap()
                .styles
                .is_empty()
        );
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn context_menu_copies_clicked_layer_and_pastes_into_other_tab(cx: &mut TestAppContext) {
    let mut source_doc = doc(&["First", "Second"], None);
    if let emulsion_core::NodeKind::Raster { raster, .. } = &mut source_doc.nodes[1].kind {
        *raster = Arc::new(Raster::solid(256, 192, [1., 0., 0., 1.]));
    }
    let first = source_doc.nodes[0].id;
    let second = source_doc.nodes[1].id;
    let (ws, cx) = open(cx, source_doc.clone());
    let source = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| source.update(cx, |e, _| e.selected = Some(first)));
    cx.update(|window, cx| window.right_click(("row", second), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    cx.update(|_, cx| assert_eq!(source.read(cx).selected, Some(second)));
    click_layer_menu_item(cx, second, 1, &t!("edit.copy"));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(cx.read_from_clipboard().is_some());
        assert!(source.read(cx).panel_focus.is_focused(window));
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(256, 192),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            );
        });
    });
    cx.run_until_parked();
    let target = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let position = cx.update(|_, cx| target.read(cx).canvas_bounds.get().unwrap().center());
    cx.simulate_mouse_down(position, MouseButton::Right, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    click_menu_item(cx, 2);
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(target.read(cx).editor.doc.nodes.len(), 1);
        assert_eq!(target.read(cx).editor.history.len(), 1);
        let emulsion_core::NodeKind::Raster { raster, .. } =
            &target.read(cx).editor.doc.nodes[0].kind
        else {
            panic!("paste creates a pixel layer");
        };
        assert_eq!(raster.get(0, 0), [u16::MAX, 0, 0, u16::MAX]);
        assert_eq!(source.read(cx).editor.doc, source_doc);
        assert!(target.read(cx).canvas_focus.is_focused(window));
    });
}

#[gpui_kit::test]
fn context_menu_escape_returns_focus_without_changing_canvas(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let position = cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.selected = Some(id);
            window.focus(&e.panel_focus, cx);
            e.canvas_bounds.get().unwrap().center()
        })
    });
    cx.simulate_mouse_down(position, MouseButton::Right, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = editor.read(cx);
        assert!(e.canvas_focus.is_focused(window));
        assert_eq!(e.selected, Some(id));
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn context_menu_cut_is_one_undoable_edit(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| window.right_click(("row", id), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    click_layer_menu_item(cx, id, 0, &t!("edit.cut"));
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.history.len(), 1);
        let emulsion_core::NodeKind::Raster { raster, .. } = &e.editor.doc.node(id).unwrap().kind
        else {
            panic!("cut retains the layer");
        };
        assert_eq!(raster.get(0, 0)[3], 0);
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
}

#[gpui_kit::test]
fn context_menu_selection_and_history_commands_work_by_mouse(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for index in [4, 5, 6, 10, 11] {
        let position = cx.update(|_, cx| editor.read(cx).canvas_bounds.get().unwrap().center());
        cx.simulate_mouse_down(position, MouseButton::Right, Default::default());
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        click_menu_item(cx, index);
        cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(e.canvas_focus.is_focused(window));
            if index == 4 {
                assert_eq!(e.tool, crate::editor::Tool::Select);
                assert_eq!(e.tools.select, crate::editor::SelectShape::Rect);
            } else {
                assert_eq!(e.editor.doc.selection.is_some(), matches!(index, 5 | 10));
            }
        });
    }
}

#[gpui_kit::test]
fn context_menu_transform_rotates_and_flips_clicked_layer(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for index in [6usize, 9usize] {
        cx.update(|window, cx| window.right_click(("row", id), cx));
        cx.run_until_parked();
        let position = layer_menu_item_position(cx, id, 9, &t!("editor.clipboard.transform"));
        cx.simulate_mouse_move(position, None, Default::default());
        cx.run_until_parked();
        cx.simulate_keystrokes("right");
        cx.run_until_parked();
        let position = cx.update(|window, _| {
            let item = window.within("submenu").find(index);
            let label = if index == 6 {
                t!("editor.clipboard.rotate_90_cw")
            } else {
                t!("editor.clipboard.flip_horizontal")
            };
            assert_eq!(item.label(), Some(label.as_ref()));
            assert!(item.visible());
            item.bounds().center()
        });
        cx.simulate_mouse_move(position, None, Default::default());
        cx.simulate_click(position, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            let emulsion_core::NodeKind::Raster { placement, .. } =
                &e.editor.doc.node(id).unwrap().kind
            else {
                panic!("transform retains the pixel layer");
            };
            assert_eq!(placement.rotation, if index == 6 { 90. } else { 0. });
            assert_eq!(placement.flip_x, index == 9);
            assert_eq!(e.editor.history.len(), 1);
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-z"
        } else {
            "ctrl-z"
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
    }
}

#[gpui_kit::test]
fn transform_escape_restores_lifted_selection_and_pixels(cx: &mut TestAppContext) {
    let mut original = doc(&["Photo"], None);
    original.selection = Some(Arc::new(emulsion_raster::select::rect(
        256, 192, 40., 40., 60., 50.,
    )));
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for mode in ["idle", "drag", "warp"] {
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| {
                e.selected = Some(id);
                window.focus(&e.canvas_focus, cx);
            })
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-t"
        } else {
            "ctrl-t"
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc.nodes.len(), 2));
        if mode == "warp" {
            cx.update(|_, cx| editor.update(cx, |e, cx| e.start_warp(cx)));
        }
        if mode == "drag" {
            let start = cx.update(|_, cx| editor.read(cx).doc_to_window((40., 40.)).unwrap());
            cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
            cx.simulate_mouse_move(
                start + gpui_kit::point(gpui_kit::px(-20.), gpui_kit::px(-20.)),
                Some(MouseButton::Left),
                Default::default(),
            );
        }
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.doc, original, "cancel {mode}");
            assert_eq!(e.selected, Some(id));
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.history.can_redo());
            assert!(!e.editor.in_transaction());
        });
    }
}
