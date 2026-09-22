use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::{Document, Node};
use emulsion_raster::vector::PathStyle;
use gpui_kit::component::color_picker::ColorPickerState;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton};

fn click(cx: &mut VisualTestContext, id: &'static str) {
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        window.click(id, cx);
    });
    cx.run_until_parked();
}

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, Document, &mut VisualTestContext) {
    let mut original = Document::new(128, 96);
    for id in [1, 2] {
        original.nodes.push(Node::path(
            id,
            "Shape",
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                20., 20., 60., 50.,
            )),
            PathStyle {
                fill: Some([200, 40, 20, 255]),
                stroke: None,
                ..Default::default()
            },
            128,
            96,
        ));
    }
    original.next_id = 3;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![1], Some(1));
            e.set_tool(Tool::Shape, cx);
        })
    });
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(2200.)));
    click(cx, "sidebar-properties");
    (editor, original, cx)
}

fn open_picker(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Entity<ColorPickerState> {
    let picker = cx.update(|_, cx| editor.read(cx).shape_color_picker("fill"));
    // Choose HSLA tab before opening, then exercise native slider pointer input.
    cx.update(|_, cx| picker.update(cx, |p, cx| p.set_active_tab(1, cx)));
    click(cx, "shape-fill-color");
    assert!(cx.update(|_, cx| picker.read(cx).is_open()));
    picker
}

fn drag_hue(picker: &Entity<ColorPickerState>, cx: &mut VisualTestContext) {
    let bounds = cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        picker.read(cx).sliders().hue().read(cx).bounds()
    });
    assert!(bounds.size.width > gpui_kit::px(0.));
    let p = |fraction: f32| {
        gpui_kit::point(
            bounds.left() + bounds.size.width * fraction,
            bounds.center().y,
        )
    };
    cx.simulate_mouse_down(p(0.2), MouseButton::Left, Modifiers::none());
    for i in 3..8 {
        cx.simulate_mouse_move(
            p(i as f32 / 10.),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.run_until_parked();
    }
    cx.simulate_mouse_up(p(0.7), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn native_shape_color_drag_is_live_and_creates_one_history_step_on_close(cx: &mut TestAppContext) {
    let (editor, original, cx) = setup(cx);
    let picker = open_picker(&editor, cx);
    drag_hue(&picker, cx);
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_ne!(e.editor.doc, original);
        assert!(e.editor.in_transaction());
        assert_eq!(
            e.editor.history.len(),
            0,
            "drag previews do not retain undo snapshots"
        );
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(!e.editor.in_transaction());
        assert!(!picker.read(cx).is_open());
        assert_eq!(e.editor.history.len(), 1);
    });
    cx.update(|_, cx| editor.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        original
    );
}

#[gpui_kit::test]
fn shape_color_selection_and_tool_navigation_resolve_transaction(cx: &mut TestAppContext) {
    let (editor, _, cx) = setup(cx);
    let picker = open_picker(&editor, cx);
    drag_hue(&picker, cx);
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![2], Some(2));
            assert!(!e.editor.in_transaction());
            assert_eq!(e.editor.history.len(), 1);
            cx.notify();
        })
    });
    cx.run_until_parked();
    let picker = open_picker(&editor, cx);
    drag_hue(&picker, cx);
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_tool(Tool::Hand, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(!editor.read(cx).editor.in_transaction());
        assert!(!picker.read(cx).is_open());
        assert_eq!(editor.read(cx).editor.history.len(), 2);
    });
}

#[gpui_kit::test]
fn undo_during_shape_color_preview_closes_popup_and_restores_shape(cx: &mut TestAppContext) {
    let (editor, original, cx) = setup(cx);
    let picker = open_picker(&editor, cx);
    drag_hue(&picker, cx);
    cx.update(|window, cx| {
        let focus = editor.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, original);
        assert!(!e.editor.in_transaction());
        assert!(!picker.read(cx).is_open());
    });
}
