//! Compare the same CPU rendering workloads with cold and retained Taffy nodes.
//!
//! Release measurement:
//! cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse
//! Faster local build (development profile; do not compare to release numbers):
//! cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse --profile dev
//!
//! This synthetic benchmark measures full GPUI CPU drawing, not only Taffy, and
//! deliberately excludes GPU submission. Text uses the native platform system.

use std::{cell::Cell, rc::Rc, time::Duration};

use criterion::{BenchmarkId, Criterion};
use gpui_bench::{
    AppContext as _, BenchAppContext, BenchReport, Context, Entity, IntoElement,
    ParentElement as _, Render, SharedString, StyleRefinement, Styled as _, Window, div, px, rgb,
};
use gpui_kit::platform;

const ROWS: usize = 256;

#[derive(Clone, Copy)]
enum Workload {
    Fixed,
    Text,
    Churn,
    StableText,
    ChangingNoWrapText,
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Fixed => "fixed_geometry",
            Self::Text => "changing_text",
            Self::Churn => "structural_churn",
            Self::StableText => "stable_text",
            Self::ChangingNoWrapText => "changing_nowrap_text",
        }
    }
}

struct LayoutFixture {
    workload: Workload,
    frame: usize,
    labels: Vec<SharedString>,
}

impl Render for LayoutFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let color = if self.frame.is_multiple_of(2) {
            rgb(0x303840)
        } else {
            rgb(0x384048)
        };
        let count = if matches!(self.workload, Workload::Churn) {
            ROWS - self.frame % 8
        } else {
            ROWS
        };
        let mut rows = Vec::with_capacity(count);
        for index in 0..count {
            let mut row = div()
                .flex()
                .flex_none()
                .h(px(26.))
                .gap(px(4.))
                .px(px(6.))
                .items_center()
                .bg(color)
                .child(div().size(px(16.)).flex_none().bg(rgb(0x80a0c0)));
            match self.workload {
                Workload::Text => {
                    // Both natural text width and its opaque measurement closure
                    // change, including for offscreen rows participating in layout.
                    row = row.child(format!(
                        "Layer {index}: {}",
                        "measured text ".repeat(1 + (self.frame + index) % 5)
                    ));
                }
                Workload::StableText => {
                    row = row.whitespace_nowrap().child(self.labels[index].clone());
                }
                Workload::ChangingNoWrapText => {
                    row = row.whitespace_nowrap().child(format!(
                        "Layer {index}: {}",
                        "measured text ".repeat(1 + (self.frame + index) % 5)
                    ));
                }
                Workload::Fixed => {
                    row = row.child(
                        div()
                            .flex()
                            .gap(px(3.))
                            .child(div().w(px(80.)).h(px(8.)).bg(rgb(0x607080)))
                            .child(div().w(px(48.)).h(px(8.)).bg(rgb(0x90a0b0))),
                    );
                }
                Workload::Churn => {
                    // Rotate different widths through the tree and insert/remove
                    // a nested child while the total row count also changes.
                    let width = 40. + ((index + self.frame) % 7) as f32 * 12.;
                    row = row.child(div().w(px(width)).h(px(10.)).bg(rgb(0x90a0b0)));
                    if (index + self.frame).is_multiple_of(3) {
                        row = row.child(div().size(px(12.)).bg(rgb(0xb08060)));
                    }
                }
            }
            rows.push(row);
        }
        div()
            .w(px(1040.))
            .h(px(800.))
            .overflow_hidden()
            .font_family("Arial")
            .text_size(px(14.))
            .child(div().flex().flex_col().gap(px(2.)).p(px(8.)).children(rows))
    }
}

// Mirror the editor's notification ownership without document/raster work.
// The root stays uncached; only the canvas is notified during measurement.
struct SidebarFixture {
    labels: Vec<SharedString>,
    renders: Rc<Cell<usize>>,
}

impl Render for SidebarFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .whitespace_nowrap()
            .children(
                self.labels
                    .iter()
                    .cloned()
                    .map(|label| div().h(px(26.)).flex_none().px(px(6.)).child(label)),
            )
    }
}

struct CanvasFixture {
    frame: usize,
    renders: Rc<Cell<usize>>,
}

impl Render for CanvasFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .w(px(720.))
            .h(px(800.))
            .relative()
            .bg(rgb(0x202830))
            .child(
                div()
                    .absolute()
                    .left(px((self.frame % 680) as f32))
                    .top(px(100.))
                    .size(px(24.))
                    .bg(rgb(0xb08060)),
            )
    }
}

struct EditorFixture {
    sidebar: Entity<SidebarFixture>,
    canvas: Entity<CanvasFixture>,
}

impl Render for EditorFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w(px(1040.))
            .h(px(800.))
            .overflow_hidden()
            .font_family("Arial")
            .text_size(px(14.))
            .child(
                self.sidebar.clone().cached(
                    StyleRefinement::default()
                        .w(px(320.))
                        .h(px(800.))
                        .flex_none(),
                ),
            )
            .child(self.canvas.clone())
    }
}

fn benchmark(criterion: &mut Criterion) {
    let retained_first =
        std::env::var("EMULSION_LAYOUT_BENCH_RETAINED_FIRST").is_ok_and(|value| value == "1");
    let modes = if retained_first {
        [
            ("retained", true, true),
            ("retained_geometry_only", true, false),
            ("cold", false, false),
        ]
    } else {
        [
            ("cold", false, false),
            ("retained_geometry_only", true, false),
            ("retained", true, true),
        ]
    };
    let mode_order = if retained_first {
        "retained,retained_geometry_only,cold"
    } else {
        "cold,retained_geometry_only,retained"
    };
    println!(
        "layout reuse: {} {} debug_assertions={} rows={} GPU submission=excluded; native text shaping; mode_order={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(debug_assertions),
        ROWS,
        mode_order,
    );
    let native = platform::current_platform(true);
    let mut group = criterion.benchmark_group("layout_reuse");
    for workload in [
        Workload::Fixed,
        Workload::Text,
        Workload::Churn,
        Workload::StableText,
        Workload::ChangingNoWrapText,
    ] {
        for (mode, retained, intrinsic) in modes {
            let report = BenchReport::default();
            let last_stats = Rc::new(Cell::new(None));
            group.bench_function(BenchmarkId::new(workload.name(), mode), |bencher| {
                let mut cx = BenchAppContext::new_with_platform_and_report(
                    gpui_bench::bench_platform(None, native.text_system()),
                    Some(workload.name()),
                    bencher,
                    report.clone(),
                );
                let mut window = cx.add_empty_window();
                let view = window.update(|window, cx| {
                    window.set_layout_reuse_enabled(retained);
                    window.set_intrinsic_text_layout_reuse(intrinsic);
                    window.replace_root(cx, |_, _| LayoutFixture {
                        workload,
                        frame: 0,
                        labels: (0..ROWS)
                            .map(|index| format!("Layer {index}: measured text").into())
                            .collect(),
                    })
                });
                cx.bench_renderer(view, |view, _, cx| {
                    view.frame = view.frame.wrapping_add(1);
                    cx.notify();
                });
                window.update(|window, _| {
                    let stats = window.layout_reuse_stats();
                    last_stats.set(Some([
                        stats.nodes_created as u64,
                        stats.nodes_reused as u64,
                        stats.style_changes as u64,
                        stats.children_changes as u64,
                        stats.measured_nodes as u64,
                        stats.retained_nodes as u64,
                        stats.intrinsic_nodes as u64,
                        stats.intrinsic_reused as u64,
                        stats.measurement_calls as u64,
                    ]));
                });
                cx.teardown();
            });
            let Some(
                [
                    created,
                    reused,
                    styles,
                    children,
                    measured,
                    retained_nodes,
                    intrinsic_nodes,
                    intrinsic_reused,
                    measurement_calls,
                ],
            ) = last_stats.get()
            else {
                continue;
            };
            println!(
                "{}/{} last-frame layout: created={} reused={} style_changes={} children_changes={} measured={} retained_nodes={} intrinsic_nodes={} intrinsic_reused={} measurement_calls={}",
                workload.name(),
                mode,
                created,
                reused,
                styles,
                children,
                measured,
                retained_nodes,
                intrinsic_nodes,
                intrinsic_reused,
                measurement_calls,
            );
            // Separate each mode's report; these include Criterion calibration and
            // warmup, whereas Criterion's estimates use its measurement samples.
            eprintln!("Frame report for {}/{}:", workload.name(), mode);
            report.print(Some(workload.name()));
        }
    }
    for (mode, retained, intrinsic) in modes {
        let report = BenchReport::default();
        let last_stats = Rc::new(Cell::new(None));
        group.bench_function(BenchmarkId::new("cached_sidebar_canvas", mode), |bencher| {
            let mut cx = BenchAppContext::new_with_platform_and_report(
                gpui_bench::bench_platform(None, native.text_system()),
                Some("cached_sidebar_canvas"),
                bencher,
                report.clone(),
            );
            let sidebar_renders = Rc::new(Cell::new(0));
            let canvas_renders = Rc::new(Cell::new(0));
            let mut window = cx.add_empty_window();
            let canvas = window.update(|window, cx| {
                window.set_layout_reuse_enabled(retained);
                window.set_intrinsic_text_layout_reuse(intrinsic);
                let sidebar = cx.new(|_| SidebarFixture {
                    labels: (0..ROWS)
                        .map(|index| format!("Layer {index}: measured text").into())
                        .collect(),
                    renders: sidebar_renders.clone(),
                });
                let canvas = cx.new(|_| CanvasFixture {
                    frame: 0,
                    renders: canvas_renders.clone(),
                });
                window.replace_root(cx, |_, _| EditorFixture {
                    sidebar,
                    canvas: canvas.clone(),
                });
                canvas
            });
            // Settle the initial sidebar draw before capturing its render count.
            cx.settle();
            let sidebar_before = sidebar_renders.get();
            let canvas_before = canvas_renders.get();
            assert!(
                sidebar_before > 0,
                "sidebar fixture must render during setup"
            );
            cx.bench_renderer(canvas, |canvas, _, cx| {
                canvas.frame = canvas.frame.wrapping_add(1);
                cx.notify();
            });
            assert_eq!(
                sidebar_renders.get(),
                sidebar_before,
                "canvas-only notifications must reuse the sidebar"
            );
            assert!(
                canvas_renders.get() > canvas_before,
                "canvas fixture must render during measurement"
            );
            window.update(|window, _| {
                let stats = window.layout_reuse_stats();
                last_stats.set(Some((
                    stats.nodes_created,
                    stats.nodes_reused,
                    stats.retained_nodes,
                    canvas_renders.get() - canvas_before,
                    stats.intrinsic_nodes,
                    stats.intrinsic_reused,
                    stats.measurement_calls,
                )));
            });
            cx.teardown();
        });
        if let Some((
            created,
            reused,
            retained_nodes,
            canvas_renders,
            intrinsic_nodes,
            intrinsic_reused,
            measurement_calls,
        )) = last_stats.get()
        {
            println!(
                "cached_sidebar_canvas/{mode} last-frame layout: created={created} reused={reused} retained_nodes={retained_nodes} intrinsic_nodes={intrinsic_nodes} intrinsic_reused={intrinsic_reused} measurement_calls={measurement_calls}; measured canvas renders={canvas_renders} sidebar renders=0"
            );
            eprintln!("Frame report for cached_sidebar_canvas/{mode}:");
            report.print(Some("cached_sidebar_canvas"));
        }
    }
    group.finish();
}

fn main() {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    benchmark(&mut criterion);
    criterion.final_summary();
}
