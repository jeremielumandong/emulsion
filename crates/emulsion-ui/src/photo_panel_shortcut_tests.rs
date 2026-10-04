//! Real Photo panel focus, tool dispatch and text/IME ownership regressions.
use super::*;
use crate::editor::{EditorView, MaskEditTarget, PaintKind, SidebarTab, Tool};
use gpui_kit::component::WindowExt;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{EntityInputHandler, Modifiers, px, size};

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let mut document = doc(&["Photo"], None);
    document.nodes[0].mask = Some(Arc::new(emulsion_raster::Mask::empty(256, 192, 255)));
    let (workspace, cx) = open(cx, document);
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_tool(Tool::Hand, cx)));
    cx.run_until_parked();
    (editor, cx)
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn thumbnail(editor: &Entity<EditorView>, mask: bool, cx: &mut VisualTestContext) {
    let at = cx.update(|window, cx| {
        let id = editor.read(cx).editor.doc.nodes[0].id;
        window
            .find((if mask { "layer-mask" } else { "layer-content" }, id))
            .bounds()
            .center()
    });
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| assert!(editor.read(cx).panel_focus.is_focused(window)));
}

#[gpui_kit::test]
fn photo_panel_content_and_mask_thumbnails_accept_move_and_gradient(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    for compact in [false, true] {
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            window.refresh();
        });
        cx.run_until_parked();
        for mask in [false, true] {
            thumbnail(&editor, mask, cx);
            press("v", cx);
            cx.update(|window, cx| {
                assert_eq!(editor.read(cx).tool, Tool::Move);
                assert!(editor.read(cx).panel_focus.is_focused(window));
                assert_eq!(
                    editor.read(cx).tools.mask_edit_target,
                    if mask {
                        MaskEditTarget::RasterMask
                    } else {
                        MaskEditTarget::Content
                    }
                );
            });
            press("g", cx);
            cx.update(|window, cx| {
                let e = editor.read(cx);
                assert_eq!(e.tool, Tool::Brush);
                assert_eq!(e.tools.paint, PaintKind::Gradient);
                assert_eq!(
                    e.tools.mask_edit_target,
                    if mask {
                        MaskEditTarget::RasterMask
                    } else {
                        MaskEditTarget::Content
                    }
                );
                assert!(e.panel_focus.is_focused(window));
                assert_eq!(e.editor.doc, before);
                assert!(e.editor.history.is_empty());
            });
        }
    }
}

#[gpui_kit::test]
fn photo_panel_tool_groups_recall_and_cycle_without_refocusing_canvas(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    thumbnail(&editor, false, cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    for (keys, name) in [
        ("m shift-m v m m", "Elliptical marquee"),
        ("shift-l v l l", "Polygonal lasso"),
        ("shift-l v l", "Magnetic lasso"),
        ("shift-l l", "Lasso"),
        ("w shift-w v w w", "Quick select (AI)"),
        ("g shift-g v g g", "Paint bucket"),
        ("j shift-j v j j", "Remove"),
        ("shift-j v j", "Heal"),
        ("p shift-p v p p", "Free Pen"),
        ("shift-p v p", "Curvature Pen"),
        ("t shift-t v t t", "Vertical Type Tool"),
        ("u shift-u v u u", "Ellipse"),
        ("shift-u v u", "Rectangle"),
        ("shift-b", "Smudge"),
        ("ctrl-shift-x", "Liquify"),
        ("h", "Hand"),
        ("r", "Rotate View"),
        ("z", "Zoom"),
    ] {
        press(keys, cx);
        cx.update(|window, cx| {
            let e = editor.read(cx);
            assert_eq!(e.active_tool_name(), name, "{keys}");
            assert!(e.panel_focus.is_focused(window), "{keys}");
            assert_eq!(e.editor.doc, before, "{keys}");
            assert!(e.editor.history.is_empty(), "{keys}");
        });
    }
}

#[gpui_kit::test]
fn photo_panel_rename_and_search_keep_typed_tool_letters_and_ime(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    thumbnail(&editor, false, cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    press("f2 ctrl-a", cx);
    cx.simulate_input("vgb-test");
    cx.run_until_parked();
    let rename = cx.update(|window, cx| {
        let e = editor.read(cx);
        let field = e.renaming.as_ref().unwrap().1.clone();
        assert_eq!(field.read(cx).value().as_str(), "vgb-test");
        assert_eq!(e.tool, Tool::Hand);
        assert!(!e.panel_focus.is_focused(window));
        assert_eq!(e.editor.doc, before);
        field
    });
    cx.update(|window, cx| {
        rename.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(Some(0..3), "変換", Some(2..2), window, cx);
            assert!(input.marked_text_range(window, cx).is_some());
            input.replace_text_in_range(None, "変換済", window, cx);
        })
    });
    cx.simulate_input("vgb");
    cx.update(|_, cx| {
        assert_eq!(rename.read(cx).value().as_str(), "変換済vgb-test");
        assert_eq!(editor.read(cx).tool, Tool::Hand);
    });
    press("enter", cx);
    cx.update(|window, cx| editor.update(cx, |e, cx| e.find_layers(window, cx)));
    cx.run_until_parked();
    press("ctrl-a", cx);
    cx.simulate_input("vgb-test");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let input_focused = window.focused_input(cx).is_some();
        let e = editor.read(cx);
        assert_eq!(
            e.layer_panel
                .search
                .as_ref()
                .unwrap()
                .0
                .read(cx)
                .value()
                .as_str(),
            "vgb-test"
        );
        assert_eq!(e.tool, Tool::Hand);
        assert!(input_focused);
        assert_eq!(
            e.editor.history.len(),
            1,
            "only the explicit rename committed"
        );
    });
}

#[gpui_kit::test]
fn photo_panel_numeric_fields_receive_letters_without_changing_tool_or_artwork(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx);
    thumbnail(&editor, false, cx);
    press("v", cx);
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        editor.update(cx, |e, cx| e.show_sidebar_tab(SidebarTab::Properties, cx));
        window.refresh();
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window
            .within("sidebar-properties-content")
            .click("photo-transform-X", cx)
    });
    cx.run_until_parked();
    let (field, before) = cx.update(|window, cx| {
        (
            window.focused_input(cx).unwrap(),
            editor.read(cx).editor.doc.clone(),
        )
    });
    press("ctrl-a", cx);
    cx.simulate_input("vgb");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(field.value(cx).as_str(), "vgb");
        assert_eq!(window.focused_input(cx).as_ref(), Some(&field));
        assert_eq!(editor.read(cx).tool, Tool::Move);
        assert_eq!(editor.read(cx).editor.doc, before);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_panel_preserves_clipboard_delete_numbers_and_navigation(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    thumbnail(&editor, false, cx);
    press("g", cx);
    let (before, brush) = cx.update(|_, cx| {
        (
            editor.read(cx).editor.doc.clone(),
            editor.read(cx).tools.brush,
        )
    });
    press("5 shift-6 left right up down ctrl-c", cx);
    cx.update(|window, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, before);
        assert_eq!(e.tools.brush, brush);
        assert!(e.panel_focus.is_focused(window));
        assert!(e.editor.history.is_empty());
    });
    press("delete", cx);
    cx.update(|_, cx| {
        assert!(editor.read(cx).editor.doc.nodes.is_empty());
        assert_eq!(
            editor.read(cx).editor.history.len(),
            1,
            "panel delete removes the layer"
        );
    });
    press("ctrl-z", cx);
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, before));
}

#[gpui_kit::test]
fn photo_panel_tool_keys_cannot_escape_modal_transform(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    thumbnail(&editor, false, cx);
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    press("ctrl-alt-t", cx);
    let before = cx.update(|window, cx| {
        let e = editor.read(cx);
        assert!(e.photo_transform_active());
        // Starting a modal transform intentionally transfers focus to the
        // canvas. Explicitly test the panel binding context after that handoff,
        // rather than incorrectly assuming the start action keeps panel focus.
        assert!(e.canvas_focus.is_focused(window));
        let before = e.editor.doc.clone();
        let focus = e.panel_focus.clone();
        window.focus(&focus, cx);
        assert!(editor.read(cx).panel_focus.is_focused(window));
        before
    });
    for key in [
        "g", "b", "m", "shift-m", "shift-l", "shift-w", "shift-b", "shift-u", "p", "shift-p", "t",
        "shift-t", "j", "shift-j",
    ] {
        press(key, cx);
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert!(e.photo_transform_active(), "{key}");
            assert_eq!(e.tool, Tool::Move, "{key}");
            assert_eq!(e.editor.doc, before, "{key}");
            assert!(e.editor.history.is_empty(), "{key}");
            assert_eq!(e.editor.transaction_depth(), 1, "{key}");
        });
    }
    press("escape", cx);
    cx.update(|_, cx| {
        assert!(!editor.read(cx).photo_transform_active());
        assert_eq!(editor.read(cx).editor.doc, original);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_panel_tool_inheritance_does_not_change_paint_panels(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    thumbnail(&editor, false, cx);
    cx.update(|_, cx| editor.update(cx, |e, cx| e.toggle_draw_mode(cx)));
    cx.run_until_parked();
    let before = cx.update(|_, cx| editor.read(cx).tool);
    press("v g shift-m shift-u", cx);
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).tool, before);
        assert!(editor.read(cx).panel_focus.is_focused(window));
    });
}

#[gpui_kit::test]
fn photo_panel_saved_keymap_remaps_and_unbindings_dispatch_without_stealing_input(
    cx: &mut TestAppContext,
) {
    struct RestoreKeymap(std::path::PathBuf, Option<Vec<u8>>);
    impl Drop for RestoreKeymap {
        fn drop(&mut self) {
            if let Some(bytes) = &self.1 {
                std::fs::write(&self.0, bytes).unwrap();
            } else {
                let _ = std::fs::remove_file(&self.0);
            }
        }
    }
    let path = actions::keymap_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let _restore = RestoreKeymap(path.clone(), std::fs::read(&path).ok());
    std::fs::write(
        &path,
        r#"[canvas]
ToolMove = "k"
ToolGradient = []
ToolEllipseMarquee = "f12"
[panel]
ToolEllipseMarquee = []
[photo_panel]
ToolBrush = "v"
"#,
    )
    .unwrap();
    let (editor, cx) = setup(cx);
    thumbnail(&editor, false, cx);
    press("g f12", cx);
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Hand));
    press("k", cx);
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Move));
    press("v", cx);
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).tool, Tool::Brush);
        assert_eq!(editor.read(cx).tools.paint, PaintKind::Brush);
        assert!(editor.read(cx).panel_focus.is_focused(window));
    });
    press("f2 ctrl-a", cx);
    cx.simulate_input("kvgb");
    press("f12", cx);
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tool, Tool::Brush);
        assert_eq!(e.tools.paint, PaintKind::Brush);
        assert_eq!(
            e.renaming.as_ref().unwrap().1.read(cx).value().as_str(),
            "kvgb"
        );
        assert!(e.editor.history.is_empty());
    });
}
