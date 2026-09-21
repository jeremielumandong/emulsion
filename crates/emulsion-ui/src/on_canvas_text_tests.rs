use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::{NodeKind, text::TextSpec};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{EntityInputHandler, MouseButton, MouseDownEvent, Pixels, Point};

fn setup_text<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let mut document = Document::new(512, 256);
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Editable",
            TextSpec {
                text: text.into(),
                x: 20.,
                y: 30.,
                size: 24.,
                ..Default::default()
            },
            512,
            256,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut document)
    .unwrap();
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_tool(Tool::Type, cx)));
    cx.run_until_parked();
    (editor, cx)
}

fn text_point(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    byte: usize,
) -> Point<Pixels> {
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.nodes[0].kind else {
            panic!()
        };
        let caret = emulsion_core::text::layout(spec).caret(byte);
        let doc = spec.transform().transform_point2(glam::dvec2(
            caret.x as f64,
            (caret.y + caret.height * 0.5) as f64,
        ));
        e.doc_to_window((doc.x, doc.y)).unwrap()
    })
}

fn contents(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        let NodeKind::Text { spec, .. } = &editor.read(cx).editor.doc.nodes[0].kind else {
            panic!()
        };
        spec.text.clone()
    })
}

#[gpui_kit::test]
fn direct_history_undo_closes_text_session_before_restoring_document(cx: &mut TestAppContext) {
    let (editor, cx) = setup_text(cx, "Short");
    let end = text_point(&editor, cx, 5);
    cx.simulate_click(end, Default::default());
    cx.simulate_input(" longer text");
    cx.update(|_, cx| editor.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(contents(&editor, cx), "Short");
    cx.update(|_, cx| assert!(editor.read(cx).type_tool.field.is_none()));
    cx.update(|_, cx| editor.update(cx, |e, cx| e.redo(cx)));
    assert_eq!(contents(&editor, cx), "Short longer text");
    cx.update(|_, cx| assert!(editor.read(cx).type_tool.field.is_none()));
}

#[gpui_kit::test]
fn canvas_text_mouse_places_caret_and_drag_selects_range(cx: &mut TestAppContext) {
    let (editor, cx) = setup_text(cx, "Hello world");
    let point = text_point(&editor, cx, 5);
    cx.simulate_click(point, Default::default());
    cx.simulate_input("!");
    assert_eq!(contents(&editor, cx), "Hello! world");
    let from = text_point(&editor, cx, 7);
    let to = text_point(&editor, cx, 12);
    cx.simulate_mouse_down(from, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_up(to, MouseButton::Left, Default::default());
    cx.simulate_input("canvas");
    cx.simulate_keystrokes("ctrl-enter");
    assert_eq!(contents(&editor, cx), "Hello! canvas");
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(contents(&editor, cx), "Hello world");
}

#[gpui_kit::test]
fn canvas_text_double_click_selects_word_and_enter_inserts_line(cx: &mut TestAppContext) {
    let (editor, cx) = setup_text(cx, "Hello world");
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_tool(Tool::Move, cx)));
    cx.run_until_parked();
    let point = text_point(&editor, cx, 8);
    cx.simulate_event(MouseDownEvent {
        position: point,
        button: MouseButton::Left,
        modifiers: Default::default(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_up(point, MouseButton::Left, Default::default());
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Type));
    cx.simulate_input("earth");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Next");
    assert_eq!(contents(&editor, cx), "Hello earth\nNext");
    cx.simulate_keystrokes("escape");
    assert_eq!(contents(&editor, cx), "Hello world");
    cx.update(|_, cx| assert!(!editor.read(cx).editor.in_transaction()));
}

#[gpui_kit::test]
fn canvas_text_unicode_delete_selection_and_ime_use_native_ranges(cx: &mut TestAppContext) {
    let (editor, cx) = setup_text(cx, "A🙂e\u{301}Z");
    let point = text_point(&editor, cx, 0);
    cx.simulate_click(point, Default::default());
    cx.simulate_keystrokes("end left backspace");
    assert_eq!(contents(&editor, cx), "A🙂Z");
    cx.simulate_keystrokes("shift-left");
    cx.simulate_input("é");
    assert_eq!(contents(&editor, cx), "AéZ");
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.replace_and_mark_text_in_range(Some(1..2), "界", Some(1..1), window, cx);
            assert_eq!(e.marked_text_range(window, cx), Some(1..2));
            assert_eq!(
                e.selected_text_range(false, window, cx).unwrap().range,
                2..2
            );
            e.replace_text_in_range(None, "語", window, cx);
        })
    });
    assert_eq!(contents(&editor, cx), "A語Z");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Edited on canvas");
    cx.simulate_keystrokes("ctrl-enter");
    assert_eq!(contents(&editor, cx), "Edited on canvas");
}

#[gpui_kit::test]
fn new_canvas_text_escape_removes_draft(cx: &mut TestAppContext) {
    let (editor, cx) = setup_text(cx, "Existing");
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    let point = cx.update(|_, cx| editor.read(cx).doc_to_window((300., 180.)).unwrap());
    cx.simulate_click(point, Default::default());
    cx.simulate_input("Draft");
    cx.simulate_keystrokes("escape");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, before);
        assert!(e.type_tool.field.is_none());
        assert!(!e.editor.in_transaction());
    });
}

#[gpui_kit::test]
fn canvas_text_cancel_button_does_not_commit_on_focus_change(cx: &mut TestAppContext) {
    let (editor, cx) = setup_text(cx, "Original");
    let point = text_point(&editor, cx, 0);
    cx.simulate_click(point, Default::default());
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Cancelled");
    cx.run_until_parked();
    let cancel = cx.update(|window, _| window.find("type-cancel").bounds().center());
    cx.simulate_click(cancel, Default::default());
    assert_eq!(contents(&editor, cx), "Original");
    cx.update(|_, cx| assert!(editor.read(cx).type_tool.field.is_none()));
}
