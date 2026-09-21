use super::*;
use crate::editor::Tool;
use emulsion_core::{NodeKind, text::TextSpec};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{MouseButton, point, px};

fn text_document() -> Document {
    let mut document = Document::new(256, 192);
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Sample",
            TextSpec {
                text: "Sample".into(),
                color: [80, 30, 20, 255],
                x: 20.,
                y: 30.,
                ..Default::default()
            },
            256,
            192,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut document)
    .unwrap();
    document
}

#[gpui_kit::test]
fn text_color_picker_recolors_existing_text_in_type_and_move(cx: &mut TestAppContext) {
    let original = text_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for tool in [Tool::Type, Tool::Move] {
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.selected = Some(id);
                e.set_tool(tool, cx);
            });
        });
        cx.run_until_parked();
        let swatch = cx.update(|window, _| window.find("fg-swatch").bounds().center());
        cx.simulate_click(swatch, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                editor.read(cx).editor.doc,
                original,
                "opening the foreground picker must not activate the overlapping background swatch"
            );
        });
        let red = cx.update(|window, _| window.find(("sw", 3usize)).bounds().center());
        cx.simulate_click(red, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.selected, Some(id));
            assert_eq!(e.editor.doc.nodes.len(), 1);
            let NodeKind::Text { spec, .. } = &e.editor.doc.node(id).unwrap().kind else {
                panic!("recoloring must retain editable text");
            };
            assert_eq!(spec.text, "Sample");
            assert_eq!(spec.color, [0xD9, 0x3A, 0x1E, 255]);
            editor.update(cx, |e, cx| e.undo(cx));
            assert_eq!(editor.read(cx).editor.doc, original);
        });
        // Clear the popup before changing tools for the next case.
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.tools.picker = false;
                cx.notify();
            })
        });
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn text_color_control_opens_picker_with_selected_text_color(cx: &mut TestAppContext) {
    let original = text_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.selected = Some(id);
            e.set_tool(Tool::Type, cx);
        });
    });
    cx.run_until_parked();
    let control = cx.update(|window, _| window.find("type-colour").bounds().center());
    cx.simulate_click(control, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("picker-sv").bounds().size.width > px(0.));
        let e = editor.read(cx);
        assert_eq!(e.tools.fg, [80, 30, 20, 255]);
        assert_eq!(e.editor.doc, original);
        assert_eq!(e.editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn text_color_picker_drag_is_one_undo_step(cx: &mut TestAppContext) {
    let original = text_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.selected = Some(id);
            e.set_tool(Tool::Type, cx);
        });
    });
    cx.run_until_parked();
    let control = cx.update(|window, _| window.find("type-colour").bounds().center());
    cx.simulate_click(control, Default::default());
    cx.run_until_parked();
    let bounds = cx.update(|window, _| window.find("picker-sv").bounds());
    let start = bounds.origin + point(px(20.), px(20.));
    let middle = bounds.origin + point(px(70.), px(40.));
    let end = bounds.origin + point(px(150.), px(70.));
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(middle, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_ne!(e.editor.doc, original);
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 1);
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });

    // Escape during a second gesture restores the pre-drag text and closes
    // the transaction instead of absorbing the next edit into it.
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(middle, Some(MouseButton::Left), Default::default());
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(middle, MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, original);
        assert!(!e.editor.in_transaction());
    });
}

#[gpui_kit::test]
fn foreground_changes_preserve_brush_targets_and_locked_text(cx: &mut TestAppContext) {
    let original = text_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.selected = Some(id);
            e.set_tool(Tool::Brush, cx);
            e.set_fg([255, 0, 0, 255], cx);
            assert_eq!(e.editor.doc, original);
            e.execute(Command::SetLocked { id, locked: true }, cx);
            let locked = e.editor.doc.clone();
            e.set_tool(Tool::Type, cx);
            e.set_fg([0, 255, 0, 255], cx);
            assert_eq!(e.editor.doc, locked);
            assert!(!e.editor.in_transaction());
        });
    });
}
