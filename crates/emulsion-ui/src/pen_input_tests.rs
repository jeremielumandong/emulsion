//! Drive native pen data through the real canvas callbacks on every platform.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::NodeKind;
use emulsion_raster::paint::Brush;
use gpui_kit::{Modifiers, MouseButton, PenInput, with_pen_input};

#[gpui_kit::test]
fn native_pen_draws_with_pressure_and_stops_on_tip_up(cx: &mut TestAppContext) {
    let base = Raster::transparent(256, 192);
    let (workspace, cx) = open(cx, doc(&["Ink"], Some(base.clone())));
    cx.run_until_parked();
    let editor = cx.update(|window, cx| {
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |editor: &mut EditorView, cx| {
            editor.tool = Tool::Brush;
            editor.tools.brush = Brush {
                size: 32.,
                hardness: 1.,
                spacing: 0.05,
                size_pressure: 1.,
                flow_pressure: 0.,
                stabilizer: 0.,
                ..Default::default()
            };
            editor.tools.quick_shape = false;
            window.focus(&editor.canvas_focus, cx);
            cx.notify();
        });
        editor
    });
    cx.run_until_parked();
    let points = [
        ((40., 96.), 0.2),
        ((80., 96.), 0.2),
        ((180., 96.), 0.9),
        ((220., 96.), 0.9),
    ];
    let positions =
        points.map(|(point, _)| cx.update(|_, cx| editor.read(cx).doc_to_window(point).unwrap()));
    with_pen_input(PenInput::new(Some(0.8), (15., -20.), false), || {
        cx.simulate_mouse_move(positions[0], None, Modifiers::none());
    });
    cx.update(|_, cx| {
        assert!(editor.read(cx).editor.history.is_empty());
        let NodeKind::Raster { raster, .. } = &editor.read(cx).editor.doc.nodes[0].kind else {
            panic!("ink layer")
        };
        assert!(
            (0..192).all(|y| (0..256).all(|x| raster.get(x, y)[3] == 0)),
            "hover must not paint"
        );
    });
    with_pen_input(PenInput::new(Some(0.2), (15., -20.), true), || {
        cx.simulate_mouse_down(positions[0], MouseButton::Left, Modifiers::none());
    });
    for index in 1..points.len() {
        with_pen_input(
            PenInput::new(Some(points[index].1), (15., -20.), true),
            || {
                cx.simulate_mouse_move(
                    positions[index],
                    Some(MouseButton::Left),
                    Modifiers::none(),
                );
            },
        );
    }
    with_pen_input(PenInput::new(Some(0.), (0., 0.), false), || {
        cx.simulate_mouse_up(positions[3], MouseButton::Left, Modifiers::none());
    });
    cx.run_until_parked();
    let painted = cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.editor.history.len(), 1, "one stroke, one undo step");
        let NodeKind::Raster { raster, .. } = &editor.editor.doc.nodes[0].kind else {
            panic!("ink layer")
        };
        let width = |x| (0..192).filter(|&y| raster.get(x, y)[3] > 6553).count();
        assert!(width(60) > 0, "light-pressure ink is visible");
        assert!(
            width(200) > width(60) * 2,
            "pressure changes rendered brush width"
        );
        editor.editor.doc.clone()
    });
    // Subsequent hovering must not extend the completed stroke.
    with_pen_input(PenInput::new(Some(0.9), (0., 0.), false), || {
        cx.simulate_mouse_move(positions[0], None, Modifiers::none());
    });
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, painted));
}
