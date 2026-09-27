use super::*;
use emulsion_core::creation::{Background, CanvasKind};
use gpui_kit::test::TestWindowExt;

fn type_field(cx: &mut VisualTestContext, label: &'static str, value: &str) {
    cx.update(|window, cx| {
        window.click(
            gpui_kit::SharedString::from(format!("new-canvas-field-{label}")),
            cx,
        )
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input(value);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn new_canvas_dialog_validates_before_creating_and_preserves_existing_tabs(
    cx: &mut TestAppContext,
) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.new_document(window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("new-canvas-form").visible());
        assert_eq!(ws.read(cx).tabs.len(), 1);
        window.click(("new-canvas-kind", CanvasKind::Paint as usize), cx);
    });
    cx.run_until_parked();
    type_field(cx, "Name", "Local painting");
    type_field(cx, "Width", "NaN");
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("new-canvas-form").visible());
        assert_eq!(ws.read(cx).tabs.len(), 1);
    });
    type_field(cx, "Width", "64");
    type_field(cx, "Height", "48");
    cx.update(|window, cx| {
        window.click(
            ("new-canvas-background", Background::Transparent as usize),
            cx,
        )
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-save-preset", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("new-canvas-form").is_none());
        let workspace = ws.read(cx);
        assert_eq!(workspace.tabs.len(), 2);
        assert_eq!(workspace.tabs[0].read(cx).editor.doc, original);
        let editor = workspace.editor.as_ref().unwrap().read(cx);
        assert_eq!(editor.name, "Local painting");
        assert_eq!(
            (editor.editor.doc.width, editor.editor.doc.height),
            (64, 48)
        );
        assert!(editor.draw_mode);
        let settings = &cx.global::<AppSettings>().0;
        assert_eq!(settings.canvas_presets[0].name, "Local painting");
        assert_eq!(
            settings.recent_canvases[0].background,
            Background::Transparent
        );
    });
}

#[gpui_kit::test]
fn cancelling_new_canvas_does_not_change_document_or_saved_presets(cx: &mut TestAppContext) {
    let original = doc(&["Existing"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.new_document(window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-cancel", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("new-canvas-form").is_none());
        assert_eq!(ws.read(cx).tabs.len(), 1);
        assert_eq!(
            ws.read(cx).editor.as_ref().unwrap().read(cx).editor.doc,
            original
        );
        assert!(cx.global::<AppSettings>().0.recent_canvases.is_empty());
    });
}
