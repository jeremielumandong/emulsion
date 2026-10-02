//! Native touch navigation through the real canvas, without mouse promotion.
use super::*;
use crate::editor::{EditorView, Tool};
use gpui_kit::{PlatformInput, TouchEvent, TouchId, TouchPhase, point, px};

#[gpui_kit::test]
fn touch_input_keeps_overlaid_controls_tappable(cx: &mut TestAppContext) {
    use gpui_kit::{
        App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
        StatefulInteractiveElement as _, Styled as _, Window, div,
    };
    use std::cell::Cell;
    struct CanvasWithControl {
        clicks: Rc<Cell<usize>>,
        touches: Rc<Cell<usize>>,
    }
    impl Render for CanvasWithControl {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.clicks.clone();
            let touches = self.touches.clone();
            div()
                .size(px(200.))
                .on_touch(move |_, window, cx: &mut App| {
                    touches.set(touches.get() + 1);
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .id("overlay-control")
                        .size(px(50.))
                        .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                )
        }
    }
    let clicks = Rc::new(Cell::new(0));
    let touches = Rc::new(Cell::new(0));
    let window: gpui_kit::AnyWindowHandle = cx
        .add_window({
            let clicks = clicks.clone();
            let touches = touches.clone();
            move |_, _| CanvasWithControl { clicks, touches }
        })
        .into();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    for (id, position) in [
        (20, point(px(20.), px(20.))),
        (21, point(px(150.), px(150.))),
    ] {
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            cx.update_window(window, |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::Touch(TouchEvent {
                        id: TouchId(id),
                        phase,
                        position,
                        ..Default::default()
                    }),
                    cx,
                );
            })
            .unwrap();
        }
    }
    assert_eq!(
        clicks.get(),
        1,
        "touching a child control activates it once"
    );
    assert_eq!(
        touches.get(),
        2,
        "only touches on the exposed canvas are claimed"
    );
}

#[gpui_kit::test]
fn touch_input_pans_pinches_and_releases_without_painting(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, doc(&["Ink"], Some(Raster::transparent(256, 192))));
    cx.run_until_parked();
    let editor = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            // Test the exposed canvas, after the startup splash is dismissed.
            workspace.splash = false;
            cx.notify();
        });
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |editor: &mut EditorView, cx| {
            editor.tool = Tool::Brush;
            window.focus(&editor.canvas_focus, cx);
            cx.notify();
        });
        editor
    });
    cx.run_until_parked();
    let (bounds, before, document) = cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.canvas_bounds.get().unwrap(),
            editor.view,
            editor.editor.doc.clone(),
        )
    });
    let center = bounds.center();
    let first = center - point(px(40.), px(0.));
    let second = center + point(px(40.), px(0.));
    let dispatch = |cx: &mut VisualTestContext, id, phase, position| {
        cx.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::Touch(TouchEvent {
                    id: TouchId(id),
                    phase,
                    position,
                    ..Default::default()
                }),
                cx,
            );
        });
    };
    dispatch(cx, 10, TouchPhase::Started, first);
    dispatch(cx, 10, TouchPhase::Moved, first + point(px(24.), px(18.)));
    let panned = cx.update(|_, cx| editor.read(cx).view);
    assert_ne!(panned.center.0, before.center.0);
    assert_ne!(
        panned.center.1, before.center.1,
        "diagonal pan is not axis-locked"
    );
    assert_eq!(panned.zoom, before.zoom);
    dispatch(cx, 10, TouchPhase::Ended, first + point(px(24.), px(18.)));
    dispatch(cx, 11, TouchPhase::Started, first);
    dispatch(cx, 12, TouchPhase::Started, second);
    let pinch_start = cx.update(|_, cx| editor.read(cx).view);
    let anchor = (f32::from(center.x) as f64, f32::from(center.y) as f64);
    let anchored_doc = pinch_start.screen_to_doc(anchor, &bounds);
    dispatch(cx, 11, TouchPhase::Moved, first - point(px(40.), px(0.)));
    dispatch(cx, 12, TouchPhase::Moved, second + point(px(40.), px(0.)));
    let zoomed = cx.update(|_, cx| editor.read(cx).view);
    assert!((zoomed.zoom / pinch_start.zoom - 2.).abs() < 1e-6);
    let retained = zoomed.screen_to_doc(anchor, &bounds);
    assert!((retained.0 - anchored_doc.0).abs() < 1e-6);
    assert!((retained.1 - anchored_doc.1).abs() < 1e-6);
    dispatch(cx, 11, TouchPhase::Cancelled, first);
    dispatch(cx, 12, TouchPhase::Cancelled, second);
    dispatch(cx, 13, TouchPhase::Started, center);
    dispatch(cx, 13, TouchPhase::Ended, center);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(
            editor.view, zoomed,
            "cancellation and a new tap do not jump the view"
        );
        assert_eq!(editor.editor.doc, document, "fingers never paint");
        assert!(editor.editor.history.is_empty());
        assert!(!editor.has_active_gesture());
    });
}
