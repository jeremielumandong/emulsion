//! Pen family workflows driven by canvas pointer events.
use super::*;
use crate::editor::{EditorView, PenMode};
use emulsion_core::NodeKind;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton, Pixels, Point};

fn setup(cx: &mut TestAppContext, mode: PenMode) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_pen_mode(mode, cx)));
    cx.run_until_parked();
    (editor, cx)
}

fn at(editor: &Entity<EditorView>, cx: &mut VisualTestContext, point: (f64, f64)) -> Point<Pixels> {
    cx.update(|_, cx| editor.read(cx).doc_to_window(point).unwrap())
}

fn click(editor: &Entity<EditorView>, cx: &mut VisualTestContext, point: (f64, f64)) {
    let point = at(editor, cx, point);
    cx.simulate_click(point, Modifiers::none());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn free_pen_rail_choice_draws_one_editable_path_per_gesture(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, PenMode::Pen);
    cx.update(|window, cx| window.click("Pen", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("rail-flyout-item", 12usize * 16 + 1), cx));
    cx.run_until_parked();
    let before = cx.update(|_, cx| {
        assert_eq!(editor.read(cx).tools.pen.mode, PenMode::Free);
        editor.read(cx).editor.doc.clone()
    });
    let points = [
        (30., 50.),
        (60., 30.),
        (100., 70.),
        (140., 40.),
        (180., 70.),
    ];
    let start = at(&editor, cx, points[0]);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    for point in &points[1..] {
        let position = at(&editor, cx, *point);
        cx.simulate_mouse_move(position, Some(MouseButton::Left), Modifiers::none());
    }
    let end = at(&editor, cx, points[points.len() - 1]);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), before.nodes.len() + 1);
        assert_eq!(e.editor.history.len(), 1);
        assert!(e.tools.pen.building.is_none());
        let NodeKind::Path { path, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!("editable vector path expected")
        };
        assert_eq!(path.subpaths[0].anchors.len(), points.len());
        assert!(path.subpaths[0].anchors.iter().all(|a| a.smooth));
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, before));
}

#[gpui_kit::test]
fn curvature_pen_places_smooth_points_without_dragging(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, PenMode::Curvature);
    for point in [(40., 90.), (100., 30.), (170., 100.)] {
        click(&editor, cx, point);
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let NodeKind::Path { path, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        let anchors = &path.subpaths[0].anchors;
        assert_eq!(anchors.len(), 3);
        assert!(anchors.iter().all(|a| a.smooth && a.has_handles()));
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn free_pen_escape_discards_unfinished_gesture(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, PenMode::Free);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    let start = at(&editor, cx, (40., 40.));
    let end = at(&editor, cx, (150., 100.));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc, before);
        assert!(e.editor.history.is_empty());
        assert!(e.tools.pen.building.is_none());
    });
}

#[gpui_kit::test]
fn anchor_tools_add_convert_and_delete_without_creating_extra_paths(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, PenMode::Pen);
    for point in [(40., 50.), (160., 50.), (160., 150.)] {
        click(&editor, cx, point);
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    for (mode, point, count, smooth) in [
        (PenMode::AddAnchor, (100., 50.), 4, false),
        (PenMode::ConvertPoint, (40., 50.), 4, true),
        (PenMode::DeleteAnchor, (100., 50.), 3, true),
    ] {
        cx.update(|_, cx| editor.update(cx, |e, cx| e.set_pen_mode(mode, cx)));
        click(&editor, cx, point);
        cx.update(|_, cx| {
            let e = editor.read(cx);
            let NodeKind::Path { path, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
            else {
                panic!()
            };
            assert_eq!(path.subpaths[0].anchors.len(), count);
            assert_eq!(path.subpaths[0].anchors[0].smooth, smooth);
            assert_eq!(e.editor.doc.nodes.len(), original.nodes.len());
        });
    }
    for _ in 0..3 {
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-z"
        } else {
            "ctrl-z"
        });
    }
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
}
