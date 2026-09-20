//! Painting regressions; included as a child of the shared headless test module.
use super::*;
use crate::editor::{EditorView, PaintKind, Tool};
use emulsion_core::NodeKind;

fn editor(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    cx.update(|_, cx| ws.read(cx).editor.clone().unwrap())
}

fn click(e: &Entity<EditorView>, cx: &mut VisualTestContext, point: (f64, f64), alt: bool) {
    let point = cx.update(|_, cx| e.read(cx).doc_to_window(point).unwrap());
    cx.simulate_click(
        point,
        gpui_kit::Modifiers {
            alt,
            ..Default::default()
        },
    );
}

fn pixels(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Arc<Raster> {
    cx.update(|_, cx| {
        let NodeKind::Raster { raster, .. } = &e.read(cx).editor.doc.nodes[0].kind else {
            panic!("raster");
        };
        raster.clone()
    })
}

#[gpui_kit::test]
fn clone_click_copies_the_source_and_undo_restores_it(cx: &mut TestAppContext) {
    let raster = Raster::from_fn(256, 192, [0; 4], |x, _| {
        if x < 128 {
            [65535, 0, 0, 65535]
        } else {
            [0, 0, 65535, 65535]
        }
    });
    let (ws, cx) = open(cx, doc(&["Photo"], Some(raster)));
    let e = editor(&ws, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(Tool::Clone, cx)));
    cx.run_until_parked();
    click(&e, cx, (40.0, 96.0), true);
    click(&e, cx, (180.0, 96.0), false);
    cx.run_until_parked();
    assert_eq!(pixels(&e, cx).get(180, 96), [65535, 0, 0, 65535]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
        })
    });
    assert_eq!(pixels(&e, cx).get(180, 96), [0, 0, 65535, 65535]);
}

fn speck() -> Raster {
    Raster::from_fn(256, 192, [0; 4], |x, y| {
        if (178..183).contains(&x) && (94..99).contains(&y) {
            [65535; 4]
        } else {
            [12000, 12000, 12000, 65535]
        }
    })
}

#[gpui_kit::test]
fn heal_outside_selection_leaves_pixels_and_history_unchanged(cx: &mut TestAppContext) {
    let original = speck();
    let (ws, cx) = open(cx, doc(&["Photo"], Some(original.clone())));
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(emulsion_raster::select::rect(
                        256, 192, 0.0, 0.0, 128.0, 192.0,
                    ))),
                },
                cx,
            );
            e.set_tool(Tool::Heal, cx);
        })
    });
    cx.run_until_parked();
    click(&e, cx, (180.0, 96.0), false);
    cx.run_until_parked();
    assert_eq!(
        pixels(&e, cx).read_rect(original.bounds()),
        original.read_rect(original.bounds())
    );
    cx.update(|_, cx| {
        assert_eq!(e.read(cx).editor.history.len(), 1);
        assert!(!e.read(cx).editor.in_transaction());
    });
}

#[gpui_kit::test]
fn stale_heal_preserves_a_later_edit_and_its_open_transaction(cx: &mut TestAppContext) {
    use gpui_kit::InputEvent as _;
    let (ws, cx) = open(cx, doc(&["Photo"], Some(speck())));
    let e = editor(&ws, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(Tool::Heal, cx)));
    cx.run_until_parked();
    let replacement = Arc::new(Raster::solid(256, 192, [0.0, 0.8, 0.0, 1.0]));
    cx.update(|window, cx| {
        let position = e.read(cx).doc_to_window((180.0, 96.0)).unwrap();
        // simulate_click drains background jobs after each event. Dispatch
        // inside one update so the later edit really precedes heal completion.
        window.dispatch_event(
            gpui_kit::MouseDownEvent {
                position,
                button: gpui_kit::MouseButton::Left,
                modifiers: gpui_kit::Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            gpui_kit::MouseUpEvent {
                position,
                button: gpui_kit::MouseButton::Left,
                modifiers: gpui_kit::Modifiers::none(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        e.update(cx, |e, cx| {
            assert!(
                !e.editor.in_transaction(),
                "heal releases its preview transaction before work"
            );
            assert_eq!(e.editor.history.len(), 0, "heal is still pending");
            e.editor.begin("later stroke");
            e.execute(
                Command::ReplacePixels {
                    id: e.editor.doc.nodes[0].id,
                    raster: replacement.clone(),
                    dirty: replacement.bounds(),
                    label: "later stroke".into(),
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    assert!(Arc::ptr_eq(&pixels(&e, cx), &replacement));
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            assert!(
                e.editor.in_transaction(),
                "a stale heal must not end the later stroke"
            );
            e.editor.end();
            assert_eq!(e.editor.history.len(), 1);
        })
    });
}

#[gpui_kit::test]
fn liquify_restore_uses_the_session_original_and_selection_clips_warps(cx: &mut TestAppContext) {
    let original = Raster::from_fn(256, 192, [0; 4], |x, _| {
        if x < 128 {
            [0, 0, 0, 65535]
        } else {
            [65535; 4]
        }
    });
    let (ws, cx) = open(cx, doc(&["Photo"], Some(original.clone())));
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_paint(PaintKind::Liquify, cx);
            e.tools.brush.size = 100.0;
            e.tools.brush.flow = 1.0;
        })
    });
    cx.run_until_parked();
    let (a, b) = cx.update(|_, cx| {
        let e = e.read(cx);
        (
            e.doc_to_window((118.0, 96.0)).unwrap(),
            e.doc_to_window((150.0, 96.0)).unwrap(),
        )
    });
    cx.simulate_mouse_down(a, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
    cx.simulate_mouse_move(
        b,
        Some(gpui_kit::MouseButton::Left),
        gpui_kit::Modifiers::none(),
    );
    cx.simulate_mouse_up(b, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
    cx.run_until_parked();
    let difference = |r: &Raster| -> u64 {
        r.read_rect(r.bounds())
            .iter()
            .zip(original.read_rect(original.bounds()))
            .map(|(a, b)| a[0].abs_diff(b[0]) as u64)
            .sum()
    };
    let warped = pixels(&e, cx);
    let error = difference(&warped);
    assert!(error > 0);
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            e.tools.liquify = emulsion_raster::liquify::Mode::Restore
        })
    });
    click(&e, cx, (150.0, 96.0), false);
    cx.run_until_parked();
    let restored = pixels(&e, cx);
    assert!(
        difference(&restored) < error,
        "Restore must approach the image before Push"
    );
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(emulsion_raster::select::rect(
                        256, 192, 0.0, 0.0, 40.0, 192.0,
                    ))),
                },
                cx,
            );
            e.tools.liquify = emulsion_raster::liquify::Mode::Twirl { cw: true };
        })
    });
    click(&e, cx, (150.0, 96.0), false);
    cx.run_until_parked();
    assert_eq!(
        pixels(&e, cx).read_rect(original.bounds()),
        restored.read_rect(original.bounds())
    );
}

#[gpui_kit::test]
fn preset_tool_switches_preserve_the_previous_brush_and_leave_liquify(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_paint(PaintKind::Brush, cx);
            e.tools.brush.size = 37.0;
            e.tools.brush.size_pressure = 0.73;
            let brush = e.brush();
            assert!(e.apply_preset_named("Soft eraser", cx));
            assert_eq!(e.paint_kind(), PaintKind::Eraser);
            e.set_paint(PaintKind::Brush, cx);
            assert_eq!(e.brush(), brush);
            e.set_paint(PaintKind::Liquify, cx);
            assert!(e.apply_preset_named("Chalk", cx));
            assert_eq!(e.paint_kind(), PaintKind::Brush);
        })
    });
}
