//! Geometry tools exercised through actual canvas events and history actions.
use super::*;
use crate::editor::{EditorView, SelectShape, Tool};
use emulsion_core::NodeKind;
use gpui_kit::{Modifiers, MouseButton, Pixels, Point};

fn setup(cx: &mut TestAppContext, tool: Tool) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(tool, cx)));
    cx.run_until_parked();
    (e, cx)
}
fn at(e: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) -> Point<Pixels> {
    cx.update(|_, cx| e.read(cx).doc_to_window(p).unwrap())
}
fn stroke(
    e: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    points: &[(f64, f64)],
    modifiers: Modifiers,
) {
    let first = at(e, cx, points[0]);
    cx.simulate_mouse_down(first, MouseButton::Left, modifiers);
    for &point in &points[1..] {
        let point = at(e, cx, point);
        cx.simulate_mouse_move(point, Some(MouseButton::Left), modifiers);
    }
    let last = at(e, cx, *points.last().unwrap());
    cx.simulate_mouse_up(last, MouseButton::Left, modifiers);
    cx.run_until_parked();
}
fn click(e: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) {
    let p = at(e, cx, p);
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn ellipse_marquee_selects_center_excludes_corners_and_redoes(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, Tool::Select);
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_select(SelectShape::Ellipse, cx)));
    stroke(&e, cx, &[(30., 30.), (130., 110.)], Modifiers::none());
    let selected = cx.update(|_, cx| {
        let e = e.read(cx);
        let mask = e.editor.doc.selection.as_ref().unwrap();
        assert_eq!(mask.get(80, 70), 255);
        assert_eq!(mask.get(31, 31), 0);
        assert_eq!(mask.get(180, 70), 0);
        e.editor.doc.clone()
    });
    cx.simulate_keystrokes("ctrl-z");
    assert!(cx.update(|_, cx| e.read(cx).editor.doc.selection.is_none()));
    cx.simulate_keystrokes("ctrl-shift-z");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), selected);
}

#[gpui_kit::test]
fn freehand_and_polygon_lassos_select_the_drawn_region(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, Tool::Select);
    for shape in [SelectShape::Lasso, SelectShape::Polygon] {
        cx.update(|_, cx| e.update(cx, |e, cx| e.set_select(shape, cx)));
        let points = [(30., 30.), (150., 30.), (30., 150.), (30., 30.)];
        if shape == SelectShape::Lasso {
            stroke(&e, cx, &points, Modifiers::none());
        } else {
            for point in points {
                click(&e, cx, point);
            }
        }
        cx.update(|_, cx| {
            let e = e.read(cx);
            let mask = e
                .editor
                .doc
                .selection
                .as_ref()
                .expect("closed lasso selection");
            assert_eq!(mask.get(50, 50), 255);
            assert_eq!(
                mask.get(130, 130),
                0,
                "triangle excludes bounding-box corner"
            );
            assert_eq!(e.editor.history.len(), 1, "one selection gesture");
        });
        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| e.read(cx).editor.doc.selection.is_none()));
    }
}

#[gpui_kit::test]
fn marquee_shift_add_and_alt_subtract_change_only_the_target_region(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, Tool::Select);
    stroke(&e, cx, &[(20., 20.), (80., 80.)], Modifiers::none());
    stroke(
        &e,
        cx,
        &[(100., 20.), (160., 80.)],
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
    );
    let added = cx.update(|_, cx| {
        let e = e.read(cx);
        let mask = e.editor.doc.selection.as_ref().unwrap();
        assert_eq!(mask.get(40, 40), 255);
        assert_eq!(mask.get(120, 40), 255);
        assert_eq!(mask.get(90, 40), 0);
        e.editor.doc.clone()
    });
    stroke(
        &e,
        cx,
        &[(30., 30.), (60., 60.)],
        Modifiers {
            alt: true,
            ..Modifiers::none()
        },
    );
    cx.update(|_, cx| {
        let e = e.read(cx);
        let mask = e.editor.doc.selection.as_ref().unwrap();
        assert_eq!(mask.get(40, 40), 0);
        assert_eq!(mask.get(70, 70), 255);
        assert_eq!(mask.get(120, 40), 255);
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), added);
}

#[gpui_kit::test]
fn crop_preview_cancels_and_committed_crop_undo_restores_pixels(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, Tool::Crop);
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    stroke(&e, cx, &[(40., 30.), (160., 130.)], Modifiers::none());
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
    cx.simulate_keystrokes("escape enter");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
    stroke(&e, cx, &[(40., 30.), (160., 130.)], Modifiers::none());
    cx.simulate_keystrokes("enter");
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!((e.editor.doc.width, e.editor.doc.height), (120, 100));
        assert_eq!(e.editor.history.len(), 1);
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
}

#[gpui_kit::test]
fn rectangle_and_ellipse_shapes_render_distinct_masks_and_undo(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, Tool::Shape);
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    // Focus the canvas before exercising the public shape shortcuts.
    click(&e, cx, (20., 20.));
    for (shortcut, name, corner_inside) in [("u", "Rectangle", true), ("shift-u", "Ellipse", false)]
    {
        cx.simulate_keystrokes(shortcut);
        cx.update(|_, cx| e.update(cx, |e, cx| e.set_fg([255, 0, 0, 255], cx)));
        stroke(&e, cx, &[(40., 40.), (140., 120.)], Modifiers::none());
        cx.update(|_, cx| {
            let e = e.read(cx);
            let node = e.editor.doc.nodes.last().unwrap();
            assert_eq!(node.name, name);
            let mask = node.mask.as_ref().unwrap();
            assert_eq!(mask.get(90, 80), 255);
            assert_eq!(mask.get(41, 41) > 127, corner_inside);
            let rendered = emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0);
            assert!(rendered.get(90, 80)[0] > 64000, "shape fill renders red");
            assert_eq!(
                rendered.get(180, 80),
                emulsion_raster::composite::flatten(&before.composite_tree(), 0).get(180, 80)
            );
        });
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
    }
}

#[gpui_kit::test]
fn pen_enter_commits_open_path_once_and_escape_discards_next_draft(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, Tool::Pen);
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    click(&e, cx, (40., 40.));
    click(&e, cx, (160., 100.));
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
    cx.simulate_keystrokes("enter");
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Path { path, .. } = &e.editor.doc.nodes.last().unwrap().kind else {
            panic!("path expected")
        };
        assert_eq!(path.anchor_count(), 2);
        assert!(!path.subpaths[0].closed);
        assert_eq!(e.editor.history.len(), 1);
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
    click(&e, cx, (60., 60.));
    click(&e, cx, (180., 120.));
    cx.simulate_keystrokes("escape enter");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
}

#[gpui_kit::test]
fn typing_session_undo_removes_layer_and_redo_restores_it(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, Tool::Type);
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    click(&e, cx, (40., 40.));
    cx.simulate_keystrokes("H e l l o ctrl-enter");
    cx.run_until_parked();
    let typed = cx.update(|_, cx| {
        let e = e.read(cx);
        let n = e.editor.doc.nodes.last().unwrap();
        let NodeKind::Text { spec, .. } = &n.kind else {
            panic!("text layer expected")
        };
        assert_eq!(spec.text, "Hello");
        assert_eq!(n.name, "Hello");
        assert_eq!(
            e.editor.history.len(),
            1,
            "creation and typing form one session"
        );
        e.editor.doc.clone()
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), before);
    cx.simulate_keystrokes("ctrl-shift-z");
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.clone()), typed);
}
