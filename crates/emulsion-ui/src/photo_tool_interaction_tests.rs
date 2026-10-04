//! Photo tool-group memory and switching use the real scoped action dispatch.
use super::*;
use crate::editor::{EditorView, SelectShape, Tool};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, point, px, size};

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|window, cx| {
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |editor, cx| {
            editor.set_tool(Tool::Move, cx);
            window.focus(&editor.canvas_focus, cx);
        });
        editor
    });
    cx.run_until_parked();
    (editor, cx)
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn photo_plain_letters_recall_subtools_without_cycling(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    for (keys, name) in [
        ("m shift-m v m m", "Elliptical marquee"),
        ("l l", "Lasso"),
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
    ] {
        press(keys, cx);
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.active_tool_name(), name, "{keys}");
            assert_eq!(
                editor.editor.doc, before,
                "tool choice must not edit artwork"
            );
            assert!(editor.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn photo_tool_choice_from_flyout_and_shortcut_agree(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    cx.update(|window, cx| {
        window
            .within("tool-rail")
            .right_click("Rectangular marquee", cx)
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("rail-flyout-item", 17usize), cx));
    press("v m", cx);
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).tools.select, SelectShape::Ellipse);
        assert_eq!(
            window
                .within("tool-rail")
                .find("Elliptical marquee")
                .selected(),
            Some(true)
        );
        assert!(editor.read(cx).canvas_focus.is_focused(window));
    });
    press("shift-m v", cx);
    cx.update(|window, _| {
        assert!(
            window
                .within("tool-rail")
                .find("Rectangular marquee")
                .visible()
        );
    });
}

#[gpui_kit::test]
fn photo_alt_click_cycles_group_and_returns_focus_without_opening_menu(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    for (shown, next) in [
        ("Rectangular marquee", "Elliptical marquee"),
        ("Elliptical marquee", "Rectangular marquee"),
        ("Lasso", "Polygonal lasso"),
        ("Polygonal lasso", "Magnetic lasso"),
        ("Magnetic lasso", "Lasso"),
    ] {
        let at = cx.update(|window, _| {
            window.within("tool-rail").find(shown).bounds().origin + point(px(8.), px(8.))
        });
        cx.simulate_click(
            at,
            Modifiers {
                alt: true,
                ..Modifiers::none()
            },
        );
        cx.run_until_parked();
        cx.update(|window, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.active_tool_name(), next);
            assert!(editor.rail.flyout.is_none());
            assert!(editor.canvas_focus.is_focused(window));
            assert_eq!(editor.editor.doc, before);
            assert!(editor.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn photo_group_switch_cancels_pending_selection_and_keeps_undo_clean(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    press("shift-l", cx);
    let at = cx.update(|_, cx| editor.read(cx).doc_to_window((40., 40.)).unwrap());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert!(!cx.update(|_, cx| editor.read(cx).tools.polygon.is_empty()));
    press("shift-l", cx);
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert!(editor.tools.polygon.is_empty());
        assert!(editor.editor.doc.selection.is_none());
        assert!(editor.editor.history.is_empty());
    });
    press("escape ctrl-z", cx);
    assert!(cx.update(|_, cx| editor.read(cx).editor.history.is_empty()));
}

#[gpui_kit::test]
fn photo_memory_does_not_change_paint_legacy_shortcuts(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    press("shift-m shift-l v", cx);
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.toggle_draw_mode(cx)));
    press("m", cx);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).tools.select),
        SelectShape::Rect
    );
    press("l l", cx);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).tools.select),
        SelectShape::Polygon
    );
    press("shift-u u", cx);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).tools.shape),
        crate::editor::ShapeKind::Rect
    );
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.toggle_draw_mode(cx)));
    press("m", cx);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).tools.select),
        SelectShape::Ellipse
    );
    // A Paint-carried Rect cannot overwrite Photo's remembered Ellipse.
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.toggle_draw_mode(cx)));
    press("m", cx);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).tools.select),
        SelectShape::Rect
    );
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.toggle_draw_mode(cx)));
    press("m", cx);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).tools.select),
        SelectShape::Ellipse
    );
}

#[gpui_kit::test]
fn photo_group_changes_leave_design_and_storyboard_shortcuts_unchanged(cx: &mut TestAppContext) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    for kind in [ProjectKind::Design, ProjectKind::Storyboard] {
        let project = ProjectEditor::new_project(kind, Document::new(256, 192)).unwrap();
        let editor = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(project, "Other workspace".into(), window, cx);
            });
            let editor = workspace.read(cx).editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_tool(Tool::Move, cx);
                window.focus(&editor.canvas_focus, cx);
            });
            editor
        });
        cx.run_until_parked();
        press("shift-m v m", cx);
        assert_eq!(
            cx.update(|_, cx| editor.read(cx).tools.select),
            SelectShape::Rect
        );
        press("l l", cx);
        assert_eq!(
            cx.update(|_, cx| editor.read(cx).tools.select),
            SelectShape::Polygon
        );
        press("shift-u u", cx);
        assert_eq!(
            cx.update(|_, cx| editor.read(cx).tools.shape),
            crate::editor::ShapeKind::Rect
        );
    }
}

#[gpui_kit::test]
fn photo_group_shortcuts_do_not_escape_layer_name_input(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    press("shift-m v", cx);
    let before = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.rename_layer(window, cx));
        editor.read(cx).editor.doc.clone()
    });
    cx.run_until_parked();
    press("ctrl-a m l w g j p t u shift-m shift-l", cx);
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.tool, Tool::Move);
        assert_eq!(editor.editor.doc, before);
    });
    press("escape", cx);
    cx.update(|window, cx| editor.read(cx).canvas_focus.clone().focus(window, cx));
    press("m", cx);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).tools.select),
        SelectShape::Ellipse
    );
}

#[gpui_kit::test]
fn photo_shape_group_switch_cancels_incomplete_shape(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    press("u", cx);
    let (start, end) = cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.doc_to_window((40., 40.)).unwrap(),
            editor.doc_to_window((100., 90.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.update(|_, cx| editor.read(cx).has_active_gesture()));
    press("shift-u", cx);
    cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.active_tool_name(), "Ellipse");
        assert!(!editor.has_active_gesture());
        assert_eq!(editor.editor.doc, before);
        assert!(editor.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_pen_alt_cycle_skips_anchor_editing_modes(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    for (shown, next) in [
        ("Pen", "Free Pen"),
        ("Free Pen", "Curvature Pen"),
        ("Curvature Pen", "Pen"),
    ] {
        let at = cx.update(|window, _| {
            window.within("tool-rail").find(shown).bounds().origin + point(px(8.), px(8.))
        });
        cx.simulate_click(
            at,
            Modifiers {
                alt: true,
                ..Modifiers::none()
            },
        );
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).active_tool_name()), next);
    }
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.activate_rail_item(12, 3, cx)));
    cx.run_until_parked();
    let at = cx.update(|window, _| {
        window
            .within("tool-rail")
            .find("Add Anchor Point")
            .bounds()
            .origin
            + point(px(8.), px(8.))
    });
    cx.simulate_click(
        at,
        Modifiers {
            alt: true,
            ..Modifiers::none()
        },
    );
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).active_tool_name()),
        "Add Anchor Point"
    );
    press("escape p", cx);
    assert_eq!(cx.update(|_, cx| editor.read(cx).active_tool_name()), "Pen");
}

#[gpui_kit::test]
fn photo_explicit_rectangle_menu_does_not_recall_ellipse(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    press("shift-m v", cx);
    let at = cx.update(|_, cx| editor.read(cx).canvas_bounds.get().unwrap().center());
    cx.simulate_mouse_down(at, gpui_kit::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    let at = cx.update(|window, _| window.within("popup-menu").find(4usize).bounds().center());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.tool, Tool::Select);
        assert_eq!(editor.tools.select, SelectShape::Rect);
    });
}

#[gpui_kit::test]
fn photo_heal_group_switch_discards_incomplete_remove_mask(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    press("shift-j", cx);
    let (start, end) = cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.doc_to_window((40., 40.)).unwrap(),
            editor.doc_to_window((100., 90.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.update(|_, cx| editor.read(cx).has_active_gesture()));
    press("shift-j", cx);
    cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.active_tool_name(), "Heal");
        assert!(!editor.has_active_gesture());
        assert_eq!(editor.editor.doc, before);
        assert!(editor.editor.history.is_empty());
        assert!(!editor.editor.in_transaction());
    });
}
