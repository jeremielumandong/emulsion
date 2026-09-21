//! Behavior checks for view-only tools and canvas shortcut dispatch.
use super::*;
use crate::editor::{EditorView, PaintKind, Tool};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton, point, px};

fn setup(
    cx: &mut TestAppContext,
    pixels: Option<Raster>,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], pixels));
    let e = cx.update(|window, cx| {
        let e = ws.read(cx).editor.clone().unwrap();
        let focus = e.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
        e
    });
    (e, cx)
}

#[gpui_kit::test]
fn hand_and_temporary_pan_move_the_view_without_editing_artwork(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, None);
    let original = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    for temporary in [false, true] {
        cx.update(|window, cx| {
            e.update(cx, |e, cx| {
                e.set_tool(if temporary { Tool::Brush } else { Tool::Hand }, cx);
                e.view.zoom = 2.0;
                e.view.rotation = 30.0;
                window.focus(&e.canvas_focus, cx);
            })
        });
        cx.run_until_parked();
        if temporary {
            cx.simulate_keystrokes("space");
            cx.run_until_parked();
        }
        let anchor = cx.update(|_, cx| e.read(cx).doc_to_window((128.0, 96.0)).unwrap());
        let end = anchor + point(px(35.0), px(19.0));
        cx.update(|window, cx| window.drag(anchor, end, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            let actual = e.doc_to_window((128.0, 96.0)).unwrap();
            assert!(f32::from(actual.x - end.x).abs() < 0.01);
            assert!(f32::from(actual.y - end.y).abs() < 0.01);
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.in_transaction());
        });
        // Releasing canvas focus also releases the temporary space modifier.
        cx.update(|window, cx| {
            let focus = e.read(cx).panel_focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn rotate_view_shortcut_drag_reset_and_hand_do_not_edit_artwork(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, None);
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.simulate_keystrokes("r");
    cx.run_until_parked();
    let center = cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tool, Tool::Hand);
        assert!(e.tools.rotate_view);
        assert_eq!(e.view.rotation, 0.0);
        e.canvas_bounds.get().unwrap().center()
    });
    let start = center + point(px(90.), px(0.));
    let end = center + point(px(0.), px(90.));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!((editor.read(cx).view.rotation - 90.).abs() < 0.001);
        window.click("reset-view-rotation", cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("h");
    cx.run_until_parked();
    let before = cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(!e.tools.rotate_view);
        assert_eq!(e.view.rotation, 0.0);
        e.view.center
    });
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        center + point(px(30.), px(10.)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        center + point(px(30.), px(10.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_ne!(e.view.center, before);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn rotate_view_rail_group_snaps_and_escape_cancels_rotation(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, None);
    // Give the navigation tools enough vertical space to remain visible.
    cx.simulate_resize(gpui_kit::size(px(1000.), px(1000.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("Hand", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("rail-flyout-item", 16usize * 16 + 1), cx));
    cx.run_until_parked();
    let center = cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.tools.rotate_view);
        e.canvas_bounds.get().unwrap().center()
    });
    let shift = Modifiers {
        shift: true,
        ..Modifiers::none()
    };
    cx.simulate_mouse_down(center + point(px(100.), px(0.)), MouseButton::Left, shift);
    cx.simulate_mouse_move(
        center + point(px(100.), px(40.)),
        Some(MouseButton::Left),
        shift,
    );
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).view.rotation, 15.));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.simulate_mouse_up(
        center + point(px(100.), px(40.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.view.rotation, 0.);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn eyedropper_samples_foreground_background_and_ignores_transparency(cx: &mut TestAppContext) {
    let raster = Raster::from_fn(256, 192, [0; 4], |x, _| match x {
        0..64 => [65535, 0, 0, 65535],
        64..128 => [0, 0, 65535, 65535],
        _ => [0; 4],
    });
    let (e, cx) = setup(cx, Some(raster));
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(Tool::Eyedropper, cx)));
    cx.run_until_parked();
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    for (x, alt) in [(30.0, false), (90.0, true), (190.0, false), (190.0, true)] {
        let at = cx.update(|_, cx| e.read(cx).doc_to_window((x, 96.0)).unwrap());
        cx.simulate_click(
            at,
            Modifiers {
                alt,
                ..Modifiers::none()
            },
        );
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(e.tools.fg, [255, 0, 0, 255]);
            assert_eq!(
                e.tools.hue, 0.0,
                "background sampling preserves the foreground picker hue"
            );
            if x != 30.0 {
                assert_eq!(e.tools.bg, [0, 0, 255, 255]);
            }
            assert_eq!(e.editor.doc, before);
            assert!(e.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn primary_tool_shortcuts_reach_tools_without_modifying_document(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, None);
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    for (key, tool) in [
        ("h", Tool::Hand),
        ("v", Tool::Move),
        ("m", Tool::Select),
        ("l", Tool::Select),
        ("w", Tool::Select),
        ("b", Tool::Brush),
        ("j", Tool::Heal),
        ("s", Tool::Clone),
        ("c", Tool::Crop),
        ("u", Tool::Shape),
        ("p", Tool::Pen),
        ("t", Tool::Type),
        ("i", Tool::Eyedropper),
        ("z", Tool::Zoom),
    ] {
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(e.tool, tool, "shortcut {key}");
            assert_eq!(e.editor.doc, before);
            assert!(e.editor.history.is_empty());
        });
    }
    for (key, kind) in [
        ("e", PaintKind::Eraser),
        ("g", PaintKind::Bucket),
        ("shift-g", PaintKind::Gradient),
    ] {
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| e.read(cx).tools.paint), kind);
    }
}

#[gpui_kit::test]
fn zoom_double_click_resets_scale_without_document_history(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, None);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Zoom, cx);
            e.view.zoom = 4.0;
        })
    });
    cx.run_until_parked();
    let at = cx.update(|_, cx| e.read(cx).doc_to_window((128.0, 96.0)).unwrap());
    cx.simulate_event(gpui_kit::MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        click_count: 2,
        modifiers: Modifiers::none(),
        ..Default::default()
    });
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.view.zoom, 1.0);
        assert!(e.editor.history.is_empty());
    });
}
