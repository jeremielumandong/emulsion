//! Workbook text workflows through real canvas gestures and native Properties controls.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::{NodeKind, text::TextSpec};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{MouseButton, Pixels, Point};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    text: Option<&str>,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let mut doc = Document::new(600, 400);
    if let Some(text) = text {
        Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Title",
                TextSpec {
                    text: text.into(),
                    x: 30.,
                    y: 40.,
                    size: 24.,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
    }
    let (ws, cx) = open(cx, doc);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.set_tool(Tool::Type, cx);
            window.focus(&e.canvas_focus, cx);
        })
    });
    cx.run_until_parked();
    (editor, cx)
}
fn point(e: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) -> Point<Pixels> {
    cx.update(|_, cx| e.read(cx).doc_to_window(p).unwrap())
}
fn spec(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> TextSpec {
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!("editable text")
        };
        (**spec).clone()
    })
}
fn control(cx: &mut VisualTestContext, id: &'static str) {
    cx.update(|w, cx| {
        w.render_frame(cx);
        w.render_frame(cx);
        w.click(id, cx);
    });
    cx.run_until_parked();
}
fn field(cx: &mut VisualTestContext, id: &'static str, value: &str) {
    control(cx, id);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(value);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}
fn drag(cx: &mut VisualTestContext, a: Point<Pixels>, b: Point<Pixels>) {
    cx.simulate_mouse_down(a, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_up(b, MouseButton::Left, Default::default());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn paragraph_text_drag_creates_frame_reflows_and_resizes_without_scaling(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, None);
    let a = point(&editor, cx, (30., 40.));
    let b = point(&editor, cx, (270., 200.));
    drag(cx, a, b);
    cx.simulate_input("A paragraph with several words that wrap into lines.");
    cx.simulate_keystrokes("ctrl-enter");
    let before = spec(&editor, cx);
    assert_eq!(before.width, Some(240.));
    assert_eq!(before.height, Some(160.));
    let corner = point(&editor, cx, (270., 200.));
    let end = point(&editor, cx, (180., 260.));
    drag(cx, corner, end);
    let after = spec(&editor, cx);
    assert_eq!(after.width, Some(150.));
    assert_eq!(after.height, Some(220.));
    assert_eq!(after.size, before.size);
    assert_eq!(after.scale_x, 1.);
    let away = point(&editor, cx, (350., 300.));
    cx.simulate_mouse_move(away, None, Default::default());
    assert_eq!(spec(&editor, cx), after);
    cx.update(|w, cx| {
        let f = editor.read(cx).canvas_focus.clone();
        w.focus(&f, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(spec(&editor, cx), before);
}

#[gpui_kit::test]
fn selected_characters_keep_scope_when_native_character_field_takes_focus(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Some("Hello world"));
    let (start, end) = cx.update(|_, cx| {
        let e = editor.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.nodes[0].kind else {
            panic!()
        };
        let layout = emulsion_core::text::layout(spec);
        let at = |byte| {
            let c = layout.caret(byte);
            e.doc_to_window((
                spec.x as f64 + c.x as f64,
                spec.y as f64 + (c.y + c.height * 0.5) as f64,
            ))
            .unwrap()
        };
        (at(6), at(11))
    });
    drag(cx, start, end);
    field(cx, "text-size", "40");
    let s = spec(&editor, cx);
    assert_eq!(s.style_at(0).size, 24.);
    assert_eq!(s.style_at(6).size, 40.);
    assert_eq!(s.text, "Hello world");
    field(cx, "text-tracking", "3");
    let s = spec(&editor, cx);
    assert_eq!(s.style_at(0).letter_spacing, 0.);
    assert_eq!(s.style_at(6).letter_spacing, 3.);
    cx.update(|w, cx| {
        let f = editor.read(cx).canvas_focus.clone();
        w.focus(&f, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    let s = spec(&editor, cx);
    assert_eq!(s.style_at(6).size, 40.);
    assert_eq!(s.style_at(6).letter_spacing, 0.);
    control(cx, "type-colour");
    let bounds = cx.update(|window, _| window.find("picker-sv").bounds());
    let a = bounds.origin + gpui_kit::point(gpui_kit::px(20.), gpui_kit::px(20.));
    let b = bounds.origin + gpui_kit::point(gpui_kit::px(120.), gpui_kit::px(50.));
    drag(cx, a, b);
    let s = spec(&editor, cx);
    assert_eq!(s.style_at(0).color, [0, 0, 0, 255]);
    assert_ne!(s.style_at(6).color, [0, 0, 0, 255]);
}

#[gpui_kit::test]
fn point_text_click_does_not_create_paragraph_frame_and_properties_are_undoable(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, None);
    let at = point(&editor, cx, (40., 50.));
    cx.simulate_click(at, Default::default());
    cx.simulate_input("Title");
    cx.simulate_keystrokes("ctrl-enter");
    let original = spec(&editor, cx);
    assert!(original.width.is_none());
    assert!(original.height.is_none());
    field(cx, "text-leading", "1.8");
    assert_eq!(spec(&editor, cx).line_height, 1.8);
    field(cx, "text-width", "180");
    assert_eq!(spec(&editor, cx).width, Some(180.));
    cx.update(|w, cx| {
        let f = editor.read(cx).canvas_focus.clone();
        w.focus(&f, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    assert!(spec(&editor, cx).width.is_none());
}

#[gpui_kit::test]
fn text_path_handle_drag_changes_offset_and_flip_in_one_undo(cx: &mut TestAppContext) {
    use emulsion_core::text_effects::{TextPath, TextPathMode};
    let mut document = Document::new(600, 400);
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Path title",
            TextSpec {
                text: "Along a line".into(),
                x: 40.,
                y: 100.,
                size: 24.,
                text_path: Some(TextPath {
                    path: emulsion_raster::vector::Path::from_svg("M0 0 L400 0").unwrap(),
                    mode: TextPathMode::Follow,
                    offset: 0.,
                    flip: false,
                    inset: 0.,
                }),
                ..Default::default()
            },
            600,
            400,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut document)
    .unwrap();
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|w, cx| {
        editor.update(cx, |e, cx| {
            e.set_tool(Tool::Type, cx);
            e.selected = Some(e.editor.doc.nodes[0].id);
            w.focus(&e.canvas_focus, cx);
        })
    });
    cx.run_until_parked();
    let before = spec(&editor, cx);
    let a = point(&editor, cx, (40., 100.));
    let b = point(&editor, cx, (130., 120.));
    drag(cx, a, b);
    let after = spec(&editor, cx);
    let path = after.text_path.as_ref().unwrap();
    assert!((path.offset - 90.).abs() < 1.);
    assert!(path.flip);
    assert_eq!(after.text, before.text);
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(spec(&editor, cx), before);
}
