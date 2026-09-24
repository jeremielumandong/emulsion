//! Canvas-only frames must reuse panels while document and panel changes stay live.
use super::*;
use crate::editor::{EditorView, SidebarTab, Tool};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, Pixels, Point, point, px};
use std::time::Duration;

fn setup(cx: &mut TestAppContext, compact: bool) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = compact;
        let editor = workspace.read(cx).editor.clone().unwrap();
        let focus = editor.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
        window.refresh();
        editor
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("b");
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Brush));
    (editor, cx)
}

fn counts(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize) {
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.canvas_view.read(cx).render_count,
            editor.sidebar_view.read(cx).render_count,
        )
    })
}

fn canvas_point(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
    cx.update(|_, cx| editor.read(cx).doc_to_window((128., 96.)).unwrap())
}

#[gpui_kit::test]
fn brush_pointer_frames_reuse_sidebar_in_both_layouts(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        let start = canvas_point(&editor, cx);
        // Entering the canvas may legitimately change hover styles. Measure the
        // continuous movement after that first frame has settled.
        cx.simulate_mouse_move(start, None, Modifiers::none());
        let before = counts(&editor, cx);
        let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        assert!(before.0 > 0 && before.1 > 0);
        for offset in [8., 16., 24., 32.] {
            let position = start + point(px(offset), px(12.));
            cx.simulate_mouse_move(position, None, Modifiers::none());
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.tools.pointer, Some(position));
                assert_eq!(editor.editor.doc, original);
            });
        }
        let after = counts(&editor, cx);
        assert!(after.0 >= before.0 + 4, "each pointer position must render");
        assert_eq!(after.1, before.1, "brush hover rebuilt the sidebar");
    }
}

#[gpui_kit::test]
fn marching_ants_frames_reuse_sidebar_in_both_layouts(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        cx.update(|_, cx| cx.set_reduce_motion(false));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        let (phase, original) = cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.editor.doc.selection.is_some());
            (editor.tools.ants_phase, editor.editor.doc.clone())
        });
        let before = counts(&editor, cx);
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_ne!(editor.tools.ants_phase, phase);
            assert_eq!(editor.editor.doc, original);
        });
        let after = counts(&editor, cx);
        assert!(after.0 > before.0, "the selection outline must animate");
        assert_eq!(after.1, before.1, "marching ants rebuilt the sidebar");
    }
}

#[gpui_kit::test]
fn document_commands_refresh_cached_layers_and_canvas(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        let before = counts(&editor, cx);
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-shift-n"
        } else {
            "ctrl-shift-n"
        });
        let after = counts(&editor, cx);
        assert!(after.0 > before.0, "document changes must reach the canvas");
        assert!(after.1 > before.1, "document changes must reach Layers");
        cx.update(|window, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.editor.doc.nodes.len(), 2);
            let selected = editor.selected.unwrap();
            assert_eq!(editor.editor.doc.node(selected).unwrap().name, "Layer 2");
            assert!(window.find(("row", selected)).visible());
        });
    }
}

#[gpui_kit::test]
fn visible_info_panel_still_refreshes_for_pointer_movement(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, false);
    cx.simulate_keystrokes("f8");
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert!(editor.sidebar_tab == SidebarTab::Info);
        assert!(editor.panels.info);
    });
    let start = canvas_point(&editor, cx);
    cx.simulate_mouse_move(start, None, Modifiers::none());
    let before = counts(&editor, cx);
    let position = start + point(px(17.), px(9.));
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.panels.pointer, Some(position));
        assert_eq!(editor.tools.pointer, Some(position));
    });
    let after = counts(&editor, cx);
    assert!(after.0 > before.0, "the brush cursor must move");
    assert!(after.1 > before.1, "visible Info values must stay current");
}

#[gpui_kit::test]
fn hidden_info_tracks_pointer_without_rebuilding_sidebar(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        cx.simulate_keystrokes("f8");
        cx.update(|window, cx| window.click("sidebar-properties", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.panels.info);
            assert!(editor.sidebar_tab == SidebarTab::Properties);
        });
        let start = canvas_point(&editor, cx);
        cx.simulate_mouse_move(start, None, Modifiers::none());
        let before = counts(&editor, cx);
        let position = start + point(px(19.), px(11.));
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.panels.pointer, Some(position));
            assert_eq!(editor.tools.pointer, Some(position));
        });
        let after = counts(&editor, cx);
        assert!(after.0 > before.0);
        assert_eq!(after.1, before.1, "hidden Info rebuilt the sidebar");
    }
}

#[gpui_kit::test]
fn collapsed_info_tracks_pointer_without_rebuilding_sidebar(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, true);
    cx.simulate_keystrokes("f8");
    cx.update(|window, cx| window.click("sidebar-collapse", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-collapsed").visible());
        assert!(editor.read(cx).sidebar_tab == SidebarTab::Info);
    });
    let start = canvas_point(&editor, cx);
    cx.simulate_mouse_move(start, None, Modifiers::none());
    let before = counts(&editor, cx);
    let position = start + point(px(21.), px(13.));
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.panels.pointer, Some(position));
        assert_eq!(editor.tools.pointer, Some(position));
    });
    let after = counts(&editor, cx);
    assert!(after.0 > before.0);
    assert_eq!(after.1, before.1, "collapsed Info rebuilt the sidebar");
}

#[gpui_kit::test]
fn zoom_cursor_modifiers_repaint_canvas_without_rebuilding_sidebar(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, false);
    cx.simulate_keystrokes("z");
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Zoom));
    let position = canvas_point(&editor, cx);
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.update(|window, _| assert!(window.find("zoom-cursor-in").visible()));
    let before = counts(&editor, cx);
    for alt in [true, false] {
        cx.simulate_modifiers_change(Modifiers {
            alt,
            ..Modifiers::none()
        });
        cx.update(|window, _| {
            let (visible, absent) = if alt {
                ("zoom-cursor-out", "zoom-cursor-in")
            } else {
                ("zoom-cursor-in", "zoom-cursor-out")
            };
            assert!(window.find(visible).visible());
            assert!(window.try_find(absent).is_none());
        });
    }
    let after = counts(&editor, cx);
    assert!(after.0 >= before.0 + 2, "each cursor change must render");
    assert_eq!(after.1, before.1, "zoom cursor rebuilt the sidebar");
}

fn navigation_point(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
    cx.update(|_, cx| editor.read(cx).canvas_bounds.get().unwrap().center())
}

fn wheel_navigation(cx: &mut VisualTestContext, position: Point<Pixels>, zoom: bool) {
    cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(point(px(16.), px(24.))),
        modifiers: Modifiers {
            control: zoom,
            ..Modifiers::none()
        },
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    cx.run_until_parked();
}

fn pinch_navigation(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(gpui_kit::PinchEvent {
        position,
        delta: 0.1,
        modifiers: Modifiers::none(),
        phase: gpui_kit::TouchPhase::Moved,
    });
    cx.run_until_parked();
}

fn assert_navigation_controls(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let view = editor.read(cx).view;
        assert_eq!(
            window.find("zoom").label(),
            Some(format!("{:.0}%", view.zoom * 100.).as_str())
        );
        assert_eq!(
            window.find("rot").label(),
            Some(format!("{:.0}\u{b0}", view.rotation).as_str())
        );
    });
}

#[gpui_kit::test]
fn wheel_and_pinch_navigation_reuse_sidebar_and_update_controls(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        let position = navigation_point(&editor, cx);
        cx.simulate_mouse_move(position, None, Modifiers::none());
        let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        let before = counts(&editor, cx);
        let initial = cx.update(|_, cx| editor.read(cx).view);
        wheel_navigation(cx, position, false);
        let panned = cx.update(|_, cx| editor.read(cx).view);
        assert_ne!(panned.center, initial.center);
        assert_eq!(panned.zoom, initial.zoom);
        wheel_navigation(cx, position, true);
        let zoomed = cx.update(|_, cx| editor.read(cx).view);
        assert!(zoomed.zoom > panned.zoom);
        assert_navigation_controls(&editor, cx);
        pinch_navigation(cx, position);
        assert!(cx.update(|_, cx| editor.read(cx).view.zoom) > zoomed.zoom);
        assert_navigation_controls(&editor, cx);
        let after = counts(&editor, cx);
        assert!(
            after.0 >= before.0 + 3,
            "navigation did not redraw the canvas"
        );
        assert_eq!(after.1, before.1, "navigation rebuilt a static sidebar");
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.editor.doc, original);
            assert!(editor.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn continuous_pan_and_rotation_drags_reuse_sidebar(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        for rotate in [false, true] {
            cx.simulate_keystrokes(if rotate { "r" } else { "h" });
            let center = navigation_point(&editor, cx);
            let start = if rotate {
                center + point(px(90.), px(0.))
            } else {
                center
            };
            cx.simulate_mouse_move(start, None, Modifiers::none());
            cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Modifiers::none());
            // Beginning/ending a gesture can change focus or tool controls.
            // Isolate its continuous frames, which should reuse static panels.
            let before = counts(&editor, cx);
            let initial = cx.update(|_, cx| editor.read(cx).view);
            let offsets = if rotate {
                [
                    point(px(80.), px(35.)),
                    point(px(50.), px(70.)),
                    point(px(0.), px(90.)),
                ]
            } else {
                [
                    point(px(10.), px(4.)),
                    point(px(20.), px(8.)),
                    point(px(30.), px(12.)),
                ]
            };
            for offset in offsets {
                cx.simulate_mouse_move(
                    center + offset,
                    Some(gpui_kit::MouseButton::Left),
                    Modifiers::none(),
                );
            }
            let after = counts(&editor, cx);
            let final_view = cx.update(|_, cx| editor.read(cx).view);
            assert!(after.0 >= before.0 + 3);
            assert_eq!(
                after.1, before.1,
                "continuous navigation rebuilt static panels"
            );
            if rotate {
                assert!((final_view.rotation - initial.rotation - 90.).abs() < 0.001);
                assert_eq!(final_view.center, initial.center);
            } else {
                assert_ne!(final_view.center, initial.center);
                assert_eq!(final_view.rotation, initial.rotation);
            }
            assert_navigation_controls(&editor, cx);
            cx.simulate_mouse_up(
                center + offsets[2],
                gpui_kit::MouseButton::Left,
                Modifiers::none(),
            );
        }
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.editor.doc, original);
            assert!(editor.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn visible_info_refreshes_on_navigation_with_a_stationary_pointer(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        cx.simulate_keystrokes("f8");
        let position = navigation_point(&editor, cx);
        cx.simulate_mouse_move(position, None, Modifiers::none());
        let initial_center = cx.update(|_, cx| editor.read(cx).view.center);
        for zoom in [false, true] {
            let before = counts(&editor, cx);
            wheel_navigation(cx, position, zoom);
            let after = counts(&editor, cx);
            assert!(after.0 > before.0);
            assert!(
                after.1 > before.1,
                "visible Info did not refresh its view values"
            );
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert!(editor.sidebar_tab == SidebarTab::Info);
                assert_eq!(editor.panels.pointer, Some(position));
            });
        }
        assert_ne!(
            cx.update(|_, cx| editor.read(cx).view.center),
            initial_center
        );
        let before = counts(&editor, cx);
        pinch_navigation(cx, position);
        assert!(counts(&editor, cx).1 > before.1);
        assert_navigation_controls(&editor, cx);
    }
}

fn navigator_rectangles(cx: &mut VisualTestContext) -> Vec<gpui_kit::Bounds<Pixels>> {
    cx.update(|window, cx| {
        let accent = theme::palette(cx).accent;
        let fill: gpui_kit::Background = accent.opacity(0.12).into();
        let scale = window.scale_factor();
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.border_color == accent && quad.background == fill)
            .map(|quad| quad.bounds.map(|coordinate| px(coordinate.0 / scale)))
            .collect()
    })
}

#[gpui_kit::test]
fn visible_navigator_repaints_its_viewport_after_pan_and_zoom(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (editor, cx) = setup(cx, compact);
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.toggle_navigator(cx);
                // Make the visible document rectangle smaller than the thumbnail,
                // so pan/zoom changes cannot disappear behind edge clamping.
                editor.view.zoom = 8.;
                cx.notify();
            })
        });
        cx.run_until_parked();
        let position = navigation_point(&editor, cx);
        cx.simulate_mouse_move(position, None, Modifiers::none());
        let original = navigator_rectangles(cx);
        assert_eq!(original.len(), 1, "expected the Navigator viewport outline");
        let before = counts(&editor, cx);
        wheel_navigation(cx, position, false);
        let panned = navigator_rectangles(cx);
        assert_eq!(panned.len(), 1);
        assert_ne!(panned[0].origin, original[0].origin);
        assert!(counts(&editor, cx).1 > before.1);
        let before = counts(&editor, cx);
        wheel_navigation(cx, position, true);
        let zoomed = navigator_rectangles(cx);
        assert_eq!(zoomed.len(), 1);
        assert!(zoomed[0].size.width < panned[0].size.width);
        assert!(zoomed[0].size.height < panned[0].size.height);
        assert!(counts(&editor, cx).1 > before.1);
        for rotate in [false, true] {
            cx.simulate_keystrokes(if rotate { "r" } else { "h" });
            let center = navigation_point(&editor, cx);
            let start = if rotate {
                center + point(px(80.), px(0.))
            } else {
                center
            };
            let end = if rotate {
                center + point(px(80.), px(20.))
            } else {
                center + point(px(20.), px(10.))
            };
            cx.simulate_mouse_move(start, None, Modifiers::none());
            cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Modifiers::none());
            let before = counts(&editor, cx);
            let rectangle = navigator_rectangles(cx);
            cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Modifiers::none());
            let after = counts(&editor, cx);
            assert!(after.0 > before.0);
            assert!(
                after.1 > before.1,
                "Navigator missed a continuous navigation frame"
            );
            assert_ne!(navigator_rectangles(cx), rectangle);
            cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Modifiers::none());
        }
    }
}

#[gpui_kit::test]
fn collapsed_view_panels_reuse_sidebar_during_navigation(cx: &mut TestAppContext) {
    for navigator in [false, true] {
        for narrow_window in [false, true] {
            let (editor, cx) = setup(cx, true);
            if navigator {
                cx.update(|_, cx| editor.update(cx, |editor, cx| editor.toggle_navigator(cx)));
            } else {
                cx.simulate_keystrokes("f8");
            }
            cx.run_until_parked();
            if narrow_window {
                cx.simulate_resize(gpui_kit::size(px(460.), px(900.)));
            } else {
                cx.update(|window, cx| window.click("sidebar-collapse", cx));
            }
            cx.run_until_parked();
            cx.update(|window, _| assert!(window.find("sidebar-collapsed").visible()));
            let position = navigation_point(&editor, cx);
            cx.simulate_mouse_move(position, None, Modifiers::none());
            let before = counts(&editor, cx);
            let initial = cx.update(|_, cx| editor.read(cx).view);
            wheel_navigation(cx, position, false);
            wheel_navigation(cx, position, true);
            pinch_navigation(cx, position);
            let after = counts(&editor, cx);
            assert!(after.0 >= before.0 + 3);
            assert_eq!(
                after.1, before.1,
                "collapsed view panel rebuilt during navigation"
            );
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert_ne!(editor.view.center, initial.center);
                assert!(editor.view.zoom > initial.zoom);
            });
        }
    }
}
