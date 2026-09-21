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
