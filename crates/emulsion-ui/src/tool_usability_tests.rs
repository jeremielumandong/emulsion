use super::*;
use crate::editor::{EditorView, PaintKind, SelectShape, ShapeKind, Tool};
use gpui_kit::Modifiers;
use gpui_kit::test::TestWindowExt;

fn setup(cx: &mut TestAppContext, tool: Tool) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_tool(tool, cx);
            window.focus(&editor.canvas_focus, cx);
        })
    });
    cx.run_until_parked();
    (editor, cx)
}

#[gpui_kit::test]
fn zoom_shift_changes_indicator_without_moving_and_preserves_click_anchor(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Zoom);
    let at = cx.update(|_, cx| editor.read(cx).doc_to_window((80., 60.)).unwrap());
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-in").is_some()));
    let shift = Modifiers {
        shift: true,
        ..Modifiers::none()
    };
    cx.simulate_modifiers_change(shift);
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-out").is_some()));
    let zoom = cx.update(|_, cx| editor.read(cx).view.zoom);
    cx.simulate_click(at, shift);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.view.zoom, zoom * 0.5);
        let after = editor.doc_to_window((80., 60.)).unwrap();
        assert!((f32::from(after.x - at.x)).abs() < 0.01);
        assert!((f32::from(after.y - at.y)).abs() < 0.01);
    });
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-in").is_some()));
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).view.zoom), zoom);
    let alt = Modifiers {
        alt: true,
        ..Modifiers::none()
    };
    cx.simulate_modifiers_change(alt);
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-out").is_some()));
    cx.simulate_click(at, alt);
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).view.zoom), zoom * 0.5);
}

#[gpui_kit::test]
fn explicit_subtool_shortcuts_reach_the_advertised_tools(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Hand);
    for (keys, shape) in [
        ("shift-m", SelectShape::Ellipse),
        ("shift-l", SelectShape::Polygon),
        ("alt-l", SelectShape::Magnetic),
        ("shift-w", SelectShape::Quick),
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Select);
            assert_eq!(editor.read(cx).tools.select, shape);
        });
    }
    for (keys, kind) in [
        ("shift-b", PaintKind::Smudge),
        ("shift-j", PaintKind::Liquify),
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).tools.paint), kind);
    }
    for (keys, kind) in [("shift-u", ShapeKind::Ellipse), ("u", ShapeKind::Rect)] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Shape);
            assert_eq!(editor.read(cx).tools.shape, kind);
        });
    }
    cx.simulate_keystrokes("q");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tool), Tool::Mask);
    cx.simulate_keystrokes("shift-q");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tool), Tool::Grade);
}

#[gpui_kit::test]
fn brush_option_slider_supports_keyboard_limits_without_editing_pixels(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|window, cx| window.click("ToolHardness", cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("home");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tools.brush.hardness), 0.);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!((cx.update(|_, cx| editor.read(cx).tools.brush.hardness) - 0.01).abs() < 0.0001);
    cx.simulate_keystrokes("shift-right");
    cx.run_until_parked();
    assert!((cx.update(|_, cx| editor.read(cx).tools.brush.hardness) - 0.11).abs() < 0.0001);
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tools.brush.hardness), 1.);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        before
    );
}

#[gpui_kit::test]
fn brush_quick_controls_open_at_pointer_and_change_settings_without_painting(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, Tool::Brush);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(3840.), gpui_kit::px(2160.)));
    cx.run_until_parked();
    let (before, position) = cx.update(|_, cx| {
        let editor = editor.read(cx);
        let canvas = editor.canvas_bounds.get().unwrap();
        (
            editor.editor.doc.clone(),
            canvas.origin + gpui_kit::point(gpui_kit::px(200.), gpui_kit::px(220.)),
        )
    });
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let panel = window.find("brush-quick-controls").bounds();
        assert!((f32::from(panel.origin.x - position.x)).abs() < 80.);
        assert!(panel.right() < window.viewport_size().width / 2.);
        assert!(window.try_find("brush-settings-panel").is_none());
        let size = window.find("QuickBrushSize").bounds();
        let toolbar_size = window.find("ToolSize").bounds();
        assert_ne!(size, toolbar_size);
        window.drag(
            gpui_kit::point(size.origin.x + size.size.width * 0.2, size.center().y),
            gpui_kit::point(size.origin.x + size.size.width * 0.8, size.center().y),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(editor.read(cx).tools.brush.size > 250.);
        assert!(window.find("brush-quick-controls").visible());
        window.click("QuickBrushHardness", cx);
        assert!(
            !editor.read(cx).has_active_gesture(),
            "popup click must finish slider gesture"
        );
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("home");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tools.brush.hardness), 0.);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!((cx.update(|_, cx| editor.read(cx).tools.brush.hardness) - 0.01).abs() < 0.0001);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-quick-controls").is_none());
        let editor = editor.read(cx);
        assert!(editor.canvas_focus.is_focused(window));
        assert!(!editor.has_active_gesture());
        assert_eq!(editor.editor.doc, before);
        assert!(editor.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn brush_quick_controls_stay_with_toolbar_and_do_not_replace_other_tool_menus(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, Tool::Brush);
    let canvas_before = cx.update(|window, cx| {
        let bounds = window.find("editor-canvas-column").bounds();
        window.click("brush-settings", cx);
        bounds
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.find("brush-quick-controls").visible());
        assert!(window.find("QuickBrushOpacity").visible());
        assert!(window.find("QuickBrushFlow").visible());
        assert_eq!(window.find("editor-canvas-column").bounds(), canvas_before);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let position = cx.update(|_, cx| {
        editor.update(cx, |editor, cx| editor.set_tool(Tool::Move, cx));
        editor.read(cx).canvas_bounds.get().unwrap().center()
    });
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.find("popup-menu").visible());
        assert!(window.try_find("brush-quick-controls").is_none());
    });
}

#[gpui_kit::test]
fn leaving_canvas_focus_releases_temporary_pan(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(cx.update(|_, cx| editor.read(cx).space_held));
    cx.update(|window, cx| {
        let focus = editor.read(cx).panel_focus.clone();
        window.focus(&focus, cx);
        window.render_frame(cx);
    });
    cx.run_until_parked();
    assert!(!cx.update(|_, cx| editor.read(cx).space_held));
}

#[gpui_kit::test]
fn brush_settings_and_presets_stay_in_sidebar_without_shrinking_canvas(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.update(|_, cx| crate::app_state::update_settings(cx, |s| s.advanced_tools = true));
    cx.run_until_parked();
    let before = cx.update(|window, cx| {
        assert!(window.try_find("advanced").is_none());
        assert!(window.try_find("brush-settings-panel").is_none());
        let bar = window.find("editor-tool-options").bounds();
        assert!(f32::from(bar.size.height) < 100.);
        let before = (
            window.find("editor-canvas-column").bounds(),
            editor.read(cx).editor.doc.clone(),
        );
        window.click("brush-settings", cx);
        before
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("brush-quick-controls").visible());
        window.within("popup-menu").click(2usize, cx); // All brush settings.
    });
    cx.run_until_parked();
    for tab in [
        "brush-settings-tip",
        "brush-settings-texture",
        "brush-settings-dynamics",
        "brush-settings-drawing",
    ] {
        cx.update(|window, cx| window.click(tab, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("brush-settings-panel").visible());
            assert_eq!(window.find("editor-canvas-column").bounds(), before.0);
            assert_eq!(editor.read(cx).editor.doc, before.1);
        });
    }
    cx.update(|window, cx| window.click("brush-settings-presets", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("brush-settings-panel").visible());
        // On a short window the library uses the sidebar's existing scroll area.
        window.scroll(
            ("sidebar-content", editor.read(cx).sidebar_tab as usize),
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-200.))),
            cx,
        );
        assert!(window.find("brush-presets-panel").visible());
        assert!(window.try_find("preset-close").is_none());
        window.click(("preset-b", 1usize), cx);
    });
    cx.run_until_parked();
    let selected_brush = cx.update(|window, cx| {
        assert!(window.find("brush-settings-panel").visible());
        assert!(editor.read(cx).presets.current.is_some());
        assert_eq!(window.find("editor-canvas-column").bounds(), before.0);
        let brush = editor.read(cx).brush();
        window.scroll(
            ("sidebar-content", editor.read(cx).sidebar_tab as usize),
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(200.))),
            cx,
        );
        window.click("brush-settings-tip", cx);
        brush
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-presets-panel").is_none());
        assert_eq!(editor.read(cx).brush(), selected_brush);
        assert_eq!(editor.read(cx).editor.doc, before.1);
        window.click("brush-settings-close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-history-content").visible());
        window.click("presets", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let library = window.find("brush-presets-panel").bounds();
        let dock = window.find("node-panel").bounds();
        assert!(library.origin.x >= dock.origin.x);
        assert!(library.right() <= dock.right());
        assert_eq!(window.find("editor-canvas-column").bounds(), before.0);
        window.click("preset-close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-history-content").visible());
        assert_eq!(editor.read(cx).editor.doc, before.1);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}
