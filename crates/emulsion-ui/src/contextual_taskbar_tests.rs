//! The contextual bar follows tool changes and runs real canvas commands.
use super::*;
use crate::editor::{ShapeKind, Tool};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn contextual_taskbar_follows_every_tool(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1000.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for (tool, control) in [
        (Tool::Hand, "context-fit"),
        (Tool::Move, "context-transform"),
        (Tool::Select, "context-subject"),
        (Tool::Mask, "mask-taskbar-view"),
        (Tool::Brush, "context-brush-settings"),
        (Tool::Heal, "context-brush-settings"),
        (Tool::Clone, "context-clone-source"),
        (Tool::Grade, "context-hsl"),
        (Tool::Type, "context-text-properties"),
        (Tool::Crop, "context-crop-apply"),
        (Tool::Shape, "context-shape-ellipse"),
        (Tool::Pen, "context-pen-finish"),
        (Tool::Eyedropper, "context-default-colors"),
        (Tool::Zoom, "context-zoom-in"),
    ] {
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.tools.mask_edit = false;
                editor.set_tool(tool, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            assert!(window.find("contextual-taskbar").bounds().size.height > gpui_kit::px(0.));
            assert!(
                window.find(control).bounds().size.width > gpui_kit::px(0.),
                "{tool:?}"
            );
        });
    }
}

#[gpui_kit::test]
fn contextual_taskbar_shape_color_and_quick_mask_actions(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| {
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |editor, cx| editor.set_tool(Tool::Shape, cx));
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("context-shape-ellipse", cx));
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.tools.shape, ShapeKind::Ellipse);
            editor.tools.fg = [20, 40, 60, 255];
            editor.tools.bg = [80, 100, 120, 255];
            editor.set_tool(Tool::Eyedropper, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("context-swap-colors", cx));
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.tools.fg, [80, 100, 120, 255]);
            assert_eq!(editor.tools.bg, [20, 40, 60, 255]);
            editor.toggle_quick_mask(cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("context-quick-mask-done", cx));
    cx.update(|_, cx| assert!(!editor.read(cx).tools.quick_mask));
}
