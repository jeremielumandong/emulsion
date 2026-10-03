//! The Stage's layout in the real window: a new storyboard opens fitted, the
//! bars under the picture never overlap, playback keeps the burn-in visible,
//! the canvas quick-tool column stays off the Shot Generator and the Board,
//! and photo suggestions stay off drawn panels.
use super::*;
use crate::playback::clock::FakeClock;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::command::Slot;
use emulsion_core::creation::CanvasKind;
use emulsion_core::{Command, Node};
use emulsion_raster::{Placement, Raster};
use gpui_kit::test::TestWindowExt;
use std::sync::Arc;

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    cx.run_until_parked();
}

/// File › New storyboard › Create in a `size` window, with the default
/// compact chrome and the Ask AI hint, as a first run shows them.
fn new_storyboard(
    cx: &mut TestAppContext,
    size: (f32, f32),
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(size.0), px(size.1)));
    cx.update(|_, cx| {
        crate::app_state::update_settings(cx, |s| {
            s.compact_chrome = true;
            s.ai_hint_dismissed = false;
        })
    });
    settle(cx);
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_new_canvas_kind(CanvasKind::Storyboard, window, cx)
        })
    });
    settle(cx);
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    // Only the frames the app asks for itself, as on a real display.
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    assert_fitted(&editor, cx);
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.player.fake_clock = Some(FakeClock::default());
            e.set_tool(Tool::Brush, cx);
        })
    });
    settle(cx);
    (ws, editor, cx)
}

fn bounds(id: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.update(|window, _| {
        let found = window.find(id);
        assert!(found.visible(), "{id} is shown");
        found.bounds()
    })
}

/// The Stage toolbar, the tool bar and the transport, pairwise apart.
fn assert_bars_apart(what: &str, cx: &mut VisualTestContext) {
    let bars = [
        "storyboard-stage-toolbar",
        "contextual-taskbar",
        "storyboard-transport",
    ]
    .map(|id| (id, bounds(id, cx)));
    for (i, (a, ab)) in bars.iter().enumerate() {
        for (b, bb) in &bars[i + 1..] {
            assert!(!ab.intersects(bb), "{what}: {a} {ab:?} overlaps {b} {bb:?}");
        }
    }
}

/// The view fits the panel and its overscan, and the zoom control says so.
fn assert_fitted(e: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let e = e.read(cx);
        assert!(e.editor.storyboard().is_some());
        let canvas = e.canvas_bounds().expect("the canvas is laid out");
        let (w, h) = e.stage_fit_size().expect("the Stage frames its overscan");
        let mut fitted = e.view;
        fitted.fit(w, h, &canvas);
        assert!(
            (e.view.zoom - fitted.zoom).abs() < 1e-9 && e.view.zoom < 1.,
            "fitted, not 100%: {:?} vs {:?}",
            e.view,
            fitted
        );
        let (dw, dh) = (
            f64::from(e.editor.doc.width),
            f64::from(e.editor.doc.height),
        );
        assert_eq!(e.view.center, (dw / 2., dh / 2.), "centred on the panel");
        for corner in [(0., 0.), (dw, dh)] {
            let at = e.doc_to_window(corner).unwrap();
            assert!(
                canvas.contains(&at),
                "{corner:?} at {at:?} is in {canvas:?}"
            );
        }
        assert_eq!(
            window.find("zoom").label(),
            Some(format!("{:.0}%", e.view.zoom * 100.).as_str()),
            "the zoom control shows the fit"
        );
    });
}

#[gpui_kit::test]
fn a_new_storyboard_opens_fitted(cx: &mut TestAppContext) {
    // `new_storyboard` checks the fit straight after Create.
    let (_ws, e, cx) = new_storyboard(cx, (1600., 1000.));
    assert_fitted(&e, cx);
    // A panel not shown yet is fitted when it first shows.
    let first = cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let first = e.editor.active_page();
            e.zoom_100(cx);
            e.add_project_page(false, cx);
            first
        })
    });
    cx.run_until_parked();
    assert_ne!(cx.update(|_, cx| e.read(cx).editor.active_page()), first);
    assert_fitted(&e, cx);
}

#[gpui_kit::test]
fn stage_bars_never_overlap_and_playback_keeps_the_burn_in(cx: &mut TestAppContext) {
    let (_ws, e, cx) = new_storyboard(cx, (1280., 720.));
    assert_bars_apart("1280×720", cx);
    // The light table's controls make the Stage toolbar its widest.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.update_light_table(|t| t.enabled = true, cx);
        })
    });
    settle(cx);
    assert_bars_apart("1280×720, light table", cx);
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1000.)));
    settle(cx);
    assert_bars_apart("1600×1000", cx);

    // The playing picture, burn-in at its foot, is clear of every bar.
    cx.update(|window, cx| window.click("transport-play", cx));
    settle(cx);
    assert!(cx.update(|_, cx| e.read(cx).player.showing));
    let picture = bounds("storyboard-player", cx);
    for id in ["storyboard-transport", "storyboard-stage-toolbar"] {
        let bar = bounds(id, cx);
        assert!(!picture.intersects(&bar), "{id} {bar:?} covers {picture:?}");
    }
    let transport = bounds("storyboard-transport", cx);
    assert!(transport.origin.y >= picture.bottom_left().y);
    // The transport still takes its own clicks over the player.
    cx.update(|window, cx| window.click("transport-play", cx));
    settle(cx);
    assert!(!cx.update(|_, cx| e.read(cx).transport.playing));
}

#[gpui_kit::test]
fn the_quick_tool_column_leaves_the_shot_generator_and_board(cx: &mut TestAppContext) {
    let (_ws, e, cx) = new_storyboard(cx, (1600., 1000.));
    let column = |cx: &mut VisualTestContext| {
        cx.update(|window, _| window.try_find("photo-shortcut-strip").is_some())
    };
    assert!(column(cx), "the Stage has its quick tools");
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_shot_generator(cx)));
    settle(cx);
    assert!(cx.update(|_, cx| e.read(cx).shot_generator_open()));
    assert!(!column(cx), "not over the Shot Generator");
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_shot_generator(cx)));
    settle(cx);
    assert!(column(cx));
    cx.update(|window, cx| e.update(cx, |e, cx| e.toggle_storyboard_board(window, cx)));
    settle(cx);
    assert!(cx.update(|_, cx| e.read(cx).board_open()));
    assert!(!column(cx), "not over the Board");
}

/// White paper with a black cross: nearly all of it "clipped" to a photo.
fn white_with_strokes(w: u32, h: u32) -> Raster {
    let data: Vec<u8> = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| {
                let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
                if (u - v).abs() < 0.02 || (u + v - 1.).abs() < 0.02 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                }
            })
        })
        .collect();
    Raster::from_srgba8(w, h, &data)
}

fn suggestions(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    settle(cx);
    cx.update(|_, cx| {
        e.read(cx)
            .suggestions
            .iter()
            .map(|s| s.label.clone())
            .collect()
    })
}

#[gpui_kit::test]
fn drawn_panels_and_paint_get_no_photo_suggestions(cx: &mut TestAppContext) {
    // A photo of the same picture is offered highlight recovery...
    let mut doc = Document::new(256, 144);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Scan",
            Arc::new(white_with_strokes(256, 144)),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    let (ws, cx) = open(cx, doc);
    let photo = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let offered = suggestions(&photo, cx);
    assert!(
        offered.iter().any(|s| s.starts_with("Recover highlights")),
        "{offered:?}"
    );
    // ...but not in Paint.
    cx.update(|_, cx| photo.update(cx, |e, cx| e.toggle_draw_mode(cx)));
    assert_eq!(suggestions(&photo, cx), Vec::<String>::new(), "Paint");

    // Nor on a storyboard panel with strokes on its white paper.
    let project = emulsion_core::project::ProjectEditor::new_project(
        emulsion_core::project::ProjectKind::Storyboard,
        Document::new(256, 144),
    )
    .unwrap();
    let e = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        "Layer 1",
                        Arc::new(white_with_strokes(256, 144)),
                        Placement::default(),
                    )),
                    slot: Slot::TOP,
                },
                cx,
            )
            .expect("strokes drawn");
        })
    });
    assert_eq!(suggestions(&e, cx), Vec::<String>::new(), "storyboard");
}
