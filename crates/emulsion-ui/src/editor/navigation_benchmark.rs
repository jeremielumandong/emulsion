//! Opt-in benchmarks for real editor navigation and cached panel ownership.
//! The executable isolates app data before main and uses memory-only settings.

use super::*;
use criterion::{BenchmarkId, Criterion};
use emulsion_core::command::Slot;
use emulsion_io::settings::Settings;
use emulsion_raster::{Placement, Raster};
use gpui_kit::component::Root;
use gpui_kit::{BenchAppContext, BenchReport, Global};
use std::cell::Cell;

/// Stops native tablet hooks and recurring editor services in this benchmark.
/// Normal applications never install this global.
#[doc(hidden)]
pub struct NavigationBenchmark;
impl Global for NavigationBenchmark {}

const LAYERS: usize = 64;

fn document() -> Document {
    let mut document = Document::new(256, 192);
    let pixels = Arc::new(Raster::solid(256, 192, [0.2, 0.3, 0.4, 1.0]));
    for index in 0..LAYERS {
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                format!("Benchmark layer {index}"),
                pixels.clone(),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut document)
        .expect("benchmark document must be valid");
    }
    document
}

/// Measures bounded hand-tool pans using the real EditorView drag handler.
/// The legacy mode adds the former owner notification after the same pan.
/// Native text, bundled SVG assets, and warm canvas tiles participate; GPU
/// submission is excluded by the headless benchmark platform.
#[doc(hidden)]
pub fn benchmark(criterion: &mut Criterion) {
    let retained_first =
        std::env::var("EMULSION_LAYOUT_BENCH_RETAINED_FIRST").is_ok_and(|value| value == "1");
    let layout_modes = if retained_first {
        [true, false]
    } else {
        [false, true]
    };
    let targeted_first =
        std::env::var("EMULSION_NAVIGATION_BENCH_TARGETED_FIRST").is_ok_and(|value| value == "1");
    let notification_modes = if targeted_first {
        [true, false]
    } else {
        [false, true]
    };
    println!(
        "editor navigation: {} {} debug_assertions={} layers={} GPU=excluded SVG_assets=bundled native_text=true layout_retained_first={} targeted_first={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(debug_assertions),
        LAYERS,
        retained_first,
        targeted_first,
    );
    let native = gpui_kit::platform::current_platform(true);
    let mut group = criterion.benchmark_group("editor_navigation");
    for compact in [false, true] {
        let chrome = if compact { "compact" } else { "roomy" };
        for retained in layout_modes {
            let layout = if retained { "retained" } else { "cold" };
            for targeted in notification_modes {
                let notification = if targeted { "targeted" } else { "legacy_owner" };
                let name = format!("{chrome}/{layout}/{notification}");
                let report = BenchReport::default();
                let last_counts = Cell::new(None);
                group.bench_function(BenchmarkId::from_parameter(&name), |bencher| {
                    let mut cx = BenchAppContext::new_with_platform_and_report(
                        gpui_kit::bench_platform(None, native.text_system()),
                        Some("editor_navigation"),
                        bencher,
                        report.clone(),
                    )
                    .with_assets(gpui_kit::assets::Assets);
                    cx.update(|cx| {
                        gpui_kit::init(cx);
                        cx.set_reduce_motion(true);
                        theme::install(cx);
                        crate::actions::bind(cx);
                        cx.set_global(NavigationBenchmark);
                        cx.set_global(crate::app_state::AppSettings(Settings {
                            jev_api_key: None,
                            compact_chrome: compact,
                            ai_hint_dismissed: true,
                            ..Settings::default()
                        }));
                        cx.set_global(crate::app_state::Capabilities {
                            cli: crate::app_state::CliStatus::Missing,
                        });
                    });
                    let mut window = cx.add_empty_window();
                    let editor = window.update(|window, cx| {
                        window.set_layout_reuse_enabled(retained);
                        let editor = cx.new(|cx| {
                            EditorView::new(
                                document(),
                                None,
                                None,
                                None,
                                "Navigation benchmark".into(),
                                cx,
                            )
                        });
                        editor.update(cx, |editor, _| {
                            // Exercise the real Layers dock with a static upper
                            // sidebar, never the intentionally live Info panel.
                            editor.sidebar_tab = SidebarTab::History;
                            editor.dock_tab = DockTab::Layers;
                            editor.sidebar_layout.collapsed = false;
                            editor.tool = Tool::Hand;
                        });
                        window.replace_root(cx, |window, cx| Root::new(editor.clone(), window, cx));
                        editor
                    });
                    // Complete tile/thumbnail work before measuring repeated pans
                    // over the same small, fully cached document.
                    cx.settle();
                    let anchor = window.update(|window, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.view.zoom = 1.0;
                            editor.view.rotation = 0.0;
                            editor.fit_pending = false;
                            let anchor = editor
                                .canvas_bounds()
                                .expect("editor canvas must be laid out")
                                .center();
                            editor.drag = Some(Drag::Pan { last: anchor });
                            editor.notify_canvas_navigation(window, cx);
                            anchor
                        })
                    });
                    cx.settle();
                    // Compact toolbars measure their geometry after painting and
                    // defer an owner notification when that geometry changes.
                    // Settle both endpoints before timing; executor idleness alone
                    // does not establish that these scheduled frames converged.
                    let mut stable_round_trips = 0;
                    for _ in 0..8 {
                        let sidebar_before = window.update(|_, cx| {
                            editor.read(cx).sidebar_view.read(cx).render_count
                        });
                        for position in [anchor + point(px(2.), px(0.)), anchor] {
                            window.update(|window, cx| {
                                editor.update(cx, |editor, cx| editor.drag_move(position, window, cx));
                            });
                            cx.settle();
                        }
                        let sidebar_after = window.update(|_, cx| {
                            editor.read(cx).sidebar_view.read(cx).render_count
                        });
                        if sidebar_after == sidebar_before {
                            stable_round_trips += 1;
                            if stable_round_trips == 2 {
                                break;
                            }
                        } else {
                            stable_round_trips = 0;
                        }
                    }
                    assert_eq!(stable_round_trips, 2, "targeted pan fixture must reach two stable sidebar round trips before timing");
                    let (anchor, canvas_before, sidebar_before, initial_view) =
                        window.update(|_, cx| {
                            let editor = editor.read(cx);
                            (
                                anchor,
                                editor.canvas_view.read(cx).render_count,
                                editor.sidebar_view.read(cx).render_count,
                                editor.view,
                            )
                        });
                    assert!(
                        canvas_before > 0 && sidebar_before > 0,
                        "real editor regions must render during setup"
                    );
                    let mut turns = 0_usize;
                    let mut expected = initial_view;
                    cx.bench_renderer(editor.clone(), |editor, window, cx| {
                        let dx = if turns.is_multiple_of(2) { 2.0 } else { -2.0 };
                        let position = if turns.is_multiple_of(2) {
                            anchor + point(px(2.), px(0.))
                        } else {
                            anchor
                        };
                        expected.pan(dx, 0.0);
                        editor.drag_move(position, window, cx);
                        if !targeted {
                            // Reproduce the former broad invalidation while
                            // preserving the exact same navigation operation.
                            cx.notify();
                        }
                        turns += 1;
                    });
                    window.update(|_, cx| {
                        let editor = editor.read(cx);
                        assert_eq!(
                            editor.view, expected,
                            "every drag must update the view transform"
                        );
                        let canvas = editor.canvas_view.read(cx).render_count - canvas_before;
                        let sidebar = editor.sidebar_view.read(cx).render_count - sidebar_before;
                        assert!(
                            turns > 0 && canvas >= turns,
                            "each pan must redraw the canvas"
                        );
                        if targeted {
                            assert_eq!(
                                sidebar, 0,
                                "targeted pan must reuse the cached Layers sidebar"
                            );
                        } else {
                            assert!(
                                sidebar >= turns,
                                "legacy owner notification must invalidate sidebar"
                            );
                        }
                        last_counts.set(Some((turns, canvas, sidebar)));
                    });
                    // The window context owns another App Rc. Release it and
                    // the external editor handle before teardown drops the app,
                    // otherwise input/component timers can keep rearming.
                    drop(editor);
                    drop(window);
                    cx.teardown();
                });
                if let Some((turns, canvas, sidebar)) = last_counts.get() {
                    println!(
                        "{name}: last sample pans={turns} canvas_renders={canvas} sidebar_renders={sidebar}"
                    );
                    eprintln!("Frame report for {name}:");
                    report.print(Some("editor_navigation"));
                }
            }
        }
    }
    group.finish();
}
