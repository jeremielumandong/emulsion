use super::*;
use crate::editor::Tool;
use emulsion_core::NodeKind;
use emulsion_raster::auto::AutoCorrection;
use emulsion_raster::{composite::flatten, select};
use gpui_kit::test::TestWindowExt;

fn correction_document() -> Document {
    doc(
        &["Photo"],
        Some(Raster::from_fn(256, 192, [0; 4], |x, _| {
            let value = 8000 + x as u16 * 90;
            [value + 4000, value, value - 2000, 65535]
        })),
    )
}

#[gpui_kit::test]
fn auto_correction_shortcuts_create_editable_layers_and_undo_once(cx: &mut TestAppContext) {
    let document = correction_document();
    let (ws, cx) = open(cx, document.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for (shortcut, name) in [
        ("ctrl-shift-l", "Auto Tone"),
        ("ctrl-alt-shift-l", "Auto Contrast"),
        ("ctrl-shift-b", "Auto Color"),
    ] {
        cx.update(|window, cx| {
            let focus = editor.read(cx).canvas_focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes(shortcut);
        cx.run_until_parked();
        let corrected = cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.doc.nodes.len(), 2, "{shortcut}");
            let adjustment = e.editor.doc.nodes.last().unwrap();
            assert_eq!(adjustment.name, name);
            assert!(matches!(adjustment.kind, NodeKind::Adjust(_)));
            assert!(adjustment.parent.is_none());
            assert!(adjustment.clip_to.is_none());
            assert_eq!(e.editor.doc.nodes[0], document.nodes[0]);
            e.editor.doc.clone()
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, document));
        cx.update(|_, cx| editor.update(cx, |e, cx| e.redo(cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).editor.doc, corrected);
            editor.update(cx, |e, cx| e.undo(cx));
        });
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn auto_correction_taskbar_preserves_pixels_outside_selection(cx: &mut TestAppContext) {
    let mut document = correction_document();
    document.selection = Some(Arc::new(select::rect(256, 192, 0., 0., 128., 192.)));
    let before = flatten(&document.composite_tree(), 0);
    let (ws, cx) = open(cx, document.clone());
    let editor = cx.update(|_, cx| {
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| e.set_tool(Tool::Grade, cx));
        editor
    });
    for button in [
        "context-auto-tone",
        "context-auto-contrast",
        "context-auto-color",
    ] {
        cx.run_until_parked();
        cx.update(|window, cx| window.click(button, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert_eq!(e.editor.doc.nodes.len(), 2, "{button}");
                let adjustment = e.editor.doc.nodes.last().unwrap();
                assert!(Arc::ptr_eq(
                    adjustment.mask.as_ref().unwrap(),
                    document.selection.as_ref().unwrap()
                ));
                assert_eq!(e.editor.doc.nodes[0], document.nodes[0]);
                let after = flatten(&e.editor.doc.composite_tree(), 0);
                assert_ne!(after.get(30, 90), before.get(30, 90), "{button}");
                assert_eq!(after.get(200, 90), before.get(200, 90), "{button}");
                e.undo(cx);
                assert_eq!(e.editor.doc, document);
            });
        });
    }
}

#[gpui_kit::test]
fn auto_correction_pending_result_is_discarded_after_undo(cx: &mut TestAppContext) {
    let document = correction_document();
    let (ws, cx) = open(cx, document.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.auto_correct(AutoCorrection::Tone, cx);
            e.undo(cx);
            e.redo(cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, document));
}

#[gpui_kit::test]
fn auto_correction_pending_result_cannot_overwrite_newer_edit(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, correction_document());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let edited = cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.auto_correct(AutoCorrection::Color, cx);
            let raster = Arc::new(Raster::solid(256, 192, [0., 1., 0., 1.]));
            e.execute(
                Command::ReplacePixels {
                    id: e.editor.doc.nodes[0].id,
                    dirty: raster.bounds(),
                    raster,
                    label: "Newer edit".into(),
                },
                cx,
            );
            e.editor.doc.clone()
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, edited));
}

#[gpui_kit::test]
fn auto_correction_refuses_busy_document_and_active_transaction(cx: &mut TestAppContext) {
    let document = correction_document();
    let (ws, cx) = open(cx, document.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.generate.busy = true;
            e.auto_correct(AutoCorrection::Contrast, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).editor.doc, document);
        editor.update(cx, |e, cx| {
            e.generate.busy = false;
            e.editor.begin("Active brush stroke");
            e.auto_correct(AutoCorrection::Tone, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, document);
        assert!(e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn auto_correction_image_menu_dispatches_with_canvas_context(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, correction_document());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        window.click("image-menu", cx);
        assert_eq!(
            window.within("popup-menu").find(3usize).label(),
            Some("Auto Contrast")
        );
        window.within("popup-menu").click(3usize, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 2);
        assert_eq!(e.editor.doc.nodes.last().unwrap().name, "Auto Contrast");
        assert_eq!(e.editor.history.len(), 1);
    });
}
