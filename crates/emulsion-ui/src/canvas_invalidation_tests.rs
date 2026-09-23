//! Canvas-only frames must reuse panels while document and panel changes stay live.
use super::*;
use crate::editor::{EditorView, SidebarTab, Tool};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, Pixels, Point, point, px};
use std::time::Duration;

fn setup(cx: &mut TestAppContext, compact: bool) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = compact;
        let editor = workspace.read(cx).editor.clone().unwrap();
        let focus = editor.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
        window.refresh();
        editor
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("b");
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Brush));
    (editor, cx)
}

fn counts(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize) {
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.canvas_view.read(cx).render_count,
            editor.sidebar_view.read(cx).render_count,
        )
    })
}

fn canvas_point(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
    cx.update(|_, cx| editor.read(cx).doc_to_window((128., 96.)).unwrap())
}

#[gpui_kit::test]
fn brush_pointer_frames_reuse_sidebar_in_both_layouts(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        let start = canvas_point(&editor, cx);
        // Entering the canvas may legitimately change hover styles. Measure the
        // continuous movement after that first frame has settled.
        cx.simulate_mouse_move(start, None, Modifiers::none());
        let before = counts(&editor, cx);
        let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        assert!(before.0 > 0 && before.1 > 0);
        for offset in [8., 16., 24., 32.] {
            let position = start + point(px(offset), px(12.));
            cx.simulate_mouse_move(position, None, Modifiers::none());
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.tools.pointer, Some(position));
                assert_eq!(editor.editor.doc, original);
            });
        }
        let after = counts(&editor, cx);
        assert!(after.0 >= before.0 + 4, "each pointer position must render");
        assert_eq!(after.1, before.1, "brush hover rebuilt the sidebar");
    }
}

#[gpui_kit::test]
fn marching_ants_frames_reuse_sidebar_in_both_layouts(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        cx.update(|_, cx| cx.set_reduce_motion(false));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        let (phase, original) = cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.editor.doc.selection.is_some());
            (editor.tools.ants_phase, editor.editor.doc.clone())
        });
        let before = counts(&editor, cx);
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_ne!(editor.tools.ants_phase, phase);
            assert_eq!(editor.editor.doc, original);
        });
        let after = counts(&editor, cx);
        assert!(after.0 > before.0, "the selection outline must animate");
        assert_eq!(after.1, before.1, "marching ants rebuilt the sidebar");
    }
}

#[gpui_kit::test]
fn document_commands_refresh_cached_layers_and_canvas(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        let before = counts(&editor, cx);
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-shift-n"
        } else {
            "ctrl-shift-n"
        });
        let after = counts(&editor, cx);
        assert!(after.0 > before.0, "document changes must reach the canvas");
        assert!(after.1 > before.1, "document changes must reach Layers");
        cx.update(|window, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.editor.doc.nodes.len(), 2);
            let selected = editor.selected.unwrap();
            assert_eq!(editor.editor.doc.node(selected).unwrap().name, "Layer 2");
            assert!(window.find(("row", selected)).visible());
        });
    }
}

#[gpui_kit::test]
fn visible_info_panel_still_refreshes_for_pointer_movement(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, false);
    cx.simulate_keystrokes("f8");
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert!(editor.sidebar_tab == SidebarTab::Info);
        assert!(editor.panels.info);
    });
    let start = canvas_point(&editor, cx);
    cx.simulate_mouse_move(start, None, Modifiers::none());
    let before = counts(&editor, cx);
    let position = start + point(px(17.), px(9.));
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.panels.pointer, Some(position));
        assert_eq!(editor.tools.pointer, Some(position));
    });
    let after = counts(&editor, cx);
    assert!(after.0 > before.0, "the brush cursor must move");
    assert!(after.1 > before.1, "visible Info values must stay current");
}

#[gpui_kit::test]
fn hidden_info_tracks_pointer_without_rebuilding_sidebar(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        cx.simulate_keystrokes("f8");
        cx.update(|window, cx| window.click("sidebar-properties", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.panels.info);
            assert!(editor.sidebar_tab == SidebarTab::Properties);
        });
        let start = canvas_point(&editor, cx);
        cx.simulate_mouse_move(start, None, Modifiers::none());
        let before = counts(&editor, cx);
        let position = start + point(px(19.), px(11.));
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.panels.pointer, Some(position));
            assert_eq!(editor.tools.pointer, Some(position));
        });
        let after = counts(&editor, cx);
        assert!(after.0 > before.0);
        assert_eq!(after.1, before.1, "hidden Info rebuilt the sidebar");
    }
}

#[gpui_kit::test]
fn collapsed_info_tracks_pointer_without_rebuilding_sidebar(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, true);
    cx.simulate_keystrokes("f8");
    cx.update(|window, cx| window.click("sidebar-collapse", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-collapsed").visible());
        assert!(editor.read(cx).sidebar_tab == SidebarTab::Info);
    });
    let start = canvas_point(&editor, cx);
    cx.simulate_mouse_move(start, None, Modifiers::none());
    let before = counts(&editor, cx);
    let position = start + point(px(21.), px(13.));
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.panels.pointer, Some(position));
        assert_eq!(editor.tools.pointer, Some(position));
    });
    let after = counts(&editor, cx);
    assert!(after.0 > before.0);
    assert_eq!(after.1, before.1, "collapsed Info rebuilt the sidebar");
}

#[gpui_kit::test]
fn zoom_cursor_modifiers_repaint_canvas_without_rebuilding_sidebar(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, false);
    cx.simulate_keystrokes("z");
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Zoom));
    let position = canvas_point(&editor, cx);
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.update(|window, _| assert!(window.find("zoom-cursor-in").visible()));
    let before = counts(&editor, cx);
    for alt in [true, false] {
        cx.simulate_modifiers_change(Modifiers {
            alt,
            ..Modifiers::none()
        });
        cx.update(|window, _| {
            let (visible, absent) = if alt {
                ("zoom-cursor-out", "zoom-cursor-in")
            } else {
                ("zoom-cursor-in", "zoom-cursor-out")
            };
            assert!(window.find(visible).visible());
            assert!(window.try_find(absent).is_none());
        });
    }
    let after = counts(&editor, cx);
    assert!(after.0 >= before.0 + 2, "each cursor change must render");
    assert_eq!(after.1, before.1, "zoom cursor rebuilt the sidebar");
}
