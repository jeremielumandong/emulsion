//! Crop presets and dimensions exercised through toolbar and canvas input.
use super::*;
use crate::editor::{EditorView, Tool, crop::CropMode};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton};

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_tool(Tool::Crop, cx)));
    cx.run_until_parked();
    (editor, cx)
}

fn mode(cx: &mut VisualTestContext, index: usize) {
    cx.update(|window, cx| {
        window.click("crop-mode", cx);
        window.within("popup-menu").click(index, cx);
    });
    cx.run_until_parked();
}

fn field(cx: &mut VisualTestContext, name: &'static str, value: &str) {
    cx.update(|window, cx| {
        window.click(name, cx);
        window.press(
            if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            },
            cx,
        );
        window.input(value, cx);
    });
    cx.run_until_parked();
}

fn drag(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    start: (f64, f64),
    end: (f64, f64),
) {
    let (start, end) = cx.update(|_, cx| {
        let e = editor.read(cx);
        (
            e.doc_to_window(start).unwrap(),
            e.doc_to_window(end).unwrap(),
        )
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    let preview = cx.update(|_, cx| editor.update(cx, |e, _| e.overlay(1.).crop));
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).tools.crop, preview));
}

#[gpui_kit::test]
fn crop_apply_cancel_and_mode_fit_compact_window(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.run_until_parked();
    cx.update(|window, _| {
        for id in ["crop-apply", "crop-cancel", "crop-mode"] {
            let control = window.find(id);
            assert!(control.visible(), "{id} must stay visible");
            assert!(f32::from(control.bounds().right()) <= 800.);
        }
    });
}

#[gpui_kit::test]
fn crop_dropdown_updates_preview_and_apply_undo_restores_document(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    drag(&editor, cx, (20., 20.), (180., 80.));
    mode(cx, 7); // 16:9
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tools.crop_options.mode, CropMode::Ratio(16, 9));
        assert_eq!(e.tools.crop, Some((20., 20., 160., 90.)));
        assert_eq!(e.editor.doc, before);
    });
    // Choosing a preset must return shortcuts to the canvas, without another click.
    cx.simulate_keystrokes("enter");
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!((e.editor.doc.width, e.editor.doc.height), (160, 90));
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
    drag(&editor, cx, (20., 20.), (180., 80.));
    cx.update(|window, cx| window.click("crop-apply", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!((e.editor.doc.width, e.editor.doc.height), (160, 90));
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn crop_fixed_size_fields_validate_update_preview_and_cancel(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    drag(&editor, cx, (20., 20.), (100., 80.));
    mode(cx, 2);
    field(cx, "crop-width", "120");
    field(cx, "crop-height", "80");
    cx.update(|_, cx| assert_eq!(editor.read(cx).tools.crop, Some((20., 20., 120., 80.))));
    for invalid in ["0", "-2", "NaN", "1.5", "999999999"] {
        field(cx, "crop-width", invalid);
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert!(!e.tools.crop_options.valid);
                e.tool_commit(cx);
                assert_eq!(e.editor.doc, before);
                assert_eq!(e.editor.history.len(), 0);
            })
        });
    }
    field(cx, "crop-width", "96");
    cx.update(|_, cx| assert_eq!(editor.read(cx).tools.crop, Some((20., 20., 96., 80.))));
    cx.update(|window, cx| window.click("crop-cancel", cx));
    cx.update(|_, cx| {
        assert!(editor.read(cx).tools.crop.is_none());
        assert_eq!(editor.read(cx).editor.doc, before);
    });
    drag(&editor, cx, (140., 130.), (90., 90.));
    cx.simulate_keystrokes("enter");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!((e.editor.doc.width, e.editor.doc.height), (96, 80));
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn crop_original_and_custom_ratio_follow_live_settings(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    mode(cx, 3);
    drag(&editor, cx, (20., 20.), (180., 80.));
    cx.update(|_, cx| assert_eq!(editor.read(cx).tools.crop, Some((20., 20., 160., 120.))));
    mode(cx, 1);
    field(cx, "crop-width", "3");
    field(cx, "crop-height", "2");
    cx.update(|_, cx| {
        let (_, _, w, h) = editor.read(cx).tools.crop.unwrap();
        assert!((w / 1.5 - h).abs() <= 1.);
    });
    field(cx, "crop-height", "1e-300");
    cx.update(|_, cx| assert!(!editor.read(cx).tools.crop_options.valid));
    mode(cx, 0);
    drag(&editor, cx, (20., 20.), (100., 70.));
    cx.update(|_, cx| assert_eq!(editor.read(cx).tools.crop, Some((20., 20., 80., 50.))));
}
