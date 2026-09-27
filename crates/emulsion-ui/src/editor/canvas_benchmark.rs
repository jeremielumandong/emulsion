//! Opt-in native-window benchmark of the shipping EditorView and input handlers.
//! Submission includes waiting for current CPU tiles when using the CPU canvas.
//! Frame boundaries are platform callbacks, not physical input-to-photon timing.

use super::tools::ToolDrag;
use super::*;
use emulsion_core::command::Slot;
use emulsion_core::text::TextSpec;
use emulsion_raster::{Placement, Raster, paint::Brush};
use gpui_kit::{Global, component::Root};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const WARMUP: usize = 8;
const SAMPLES: usize = 40;
const CASES: [&str; 3] = ["pan", "brush", "vector_edit"];
static COMPLETED: AtomicBool = AtomicBool::new(false);

// EditorView is a flex child of Workspace in the shipping app. Preserve that
// sizing contract when hosting it on its own, including after owner redraws.
struct BenchWindow(Entity<EditorView>);
impl Render for BenchWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .w(px(1000.))
                .h(px(700.))
                .flex()
                .flex_col()
                .child(self.0.clone()),
        )
    }
}

#[derive(Default)]
pub(super) struct CanvasBenchmark {
    ready: bool,
    started: Option<Instant>,
    submitted: Option<f64>,
    backend: &'static str,
    case: usize,
    step: usize,
    rows: Vec<serde_json::Value>,
    submissions: Vec<f64>,
    boundaries: Vec<f64>,
    paint_node: NodeId,
    text_node: NodeId,
    gpu_brush: bool,
    commit_ms: Option<f64>,
    inactive_samples: usize,
    canvas_size: Option<[f32; 2]>,
}
impl Global for CanvasBenchmark {}

pub(super) fn painted(backend: &'static str, cx: &mut App) {
    if cx.has_global::<CanvasBenchmark>() {
        let state = cx.global_mut::<CanvasBenchmark>();
        state.ready = true;
        state.backend = backend;
        if let Some(start) = state.started {
            state
                .submitted
                .get_or_insert(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

fn percentiles(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    serde_json::json!({"p50": values[values.len() / 2], "p95": values[(values.len() * 95 / 100).min(values.len() - 1)]})
}

fn schedule(editor: Entity<EditorView>, window: &mut Window) {
    window.on_next_frame(move |window, cx| {
        let active = window.is_window_active();
        let bounds = editor.read(cx).canvas_bounds();
        let state = cx.global_mut::<CanvasBenchmark>();
        if !state.ready || (state.started.is_some() && state.submitted.is_none()) {
            window.refresh();
            schedule(editor, window);
            return;
        }
        if let Some(started) = state.started.take() {
            let submitted = state.submitted.take().unwrap();
            if state.step > WARMUP {
                if !active { state.inactive_samples += 1; }
                let bounds = bounds.expect("laid-out benchmark canvas");
                let size = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
                if let Some(expected) = state.canvas_size { assert_eq!(size, expected, "canvas changed size during measurement"); }
                else { state.canvas_size = Some(size); }
                state.submissions.push(submitted);
                state.boundaries.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            if state.step == WARMUP + SAMPLES {
                state.rows.push(serde_json::json!({
                    "scenario": CASES[state.case], "renderer": state.backend,
                    "gpu_brush": state.gpu_brush && state.case == 1, "samples": SAMPLES,
                    "inactive_window_samples": state.inactive_samples,
                    "input_to_canvas_submission_ms": percentiles(&mut state.submissions),
                    "input_to_next_frame_boundary_ms": percentiles(&mut state.boundaries),
                }));
                state.case += 1;
                state.step = 0;
                state.submissions.clear();
                state.boundaries.clear();
                state.inactive_samples = 0;
                editor.update(cx, |editor, cx| {
                    if let Some(Drag::Tool(drag)) = editor.drag.take() {
                        let start = Instant::now();
                        editor.tool_up(drag, cx);
                        cx.global_mut::<CanvasBenchmark>().commit_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
                        let id = cx.global::<CanvasBenchmark>().paint_node;
                        let sample = |editor: &EditorView| {
                            let NodeKind::Raster { raster, .. } = &editor.editor.doc.node(id).unwrap().kind else { panic!("paint layer missing") };
                            raster.get((editor.editor.doc.width as f64 * 0.4) as u32, (editor.editor.doc.height as f64 * 0.55) as u32)
                        };
                        assert!(sample(editor)[3] > 0, "committed stroke must contain pixels");
                        let painted = sample(editor);
                        editor.undo(cx);
                        assert_eq!(sample(editor), [0; 4], "one undo removes the whole stroke");
                        editor.redo(cx);
                        assert_eq!(sample(editor), painted, "redo restores committed GPU pixels");
                    }
                });
            }
        }
        let state = cx.global_mut::<CanvasBenchmark>();
        if state.case == CASES.len() {
            let rows = state.rows.clone();
            let commit_ms = state.commit_ms;
            let memory = editor.read(cx).gpu_canvas.borrow().texture_bytes();
            let view = editor.read(cx);
            let bounds = view.canvas_bounds().unwrap();
            assert!(bounds.size.width >= px(400.) && bounds.size.height >= px(300.),
                "benchmark canvas is too small; use a larger desktop/window");
            println!("{}", serde_json::json!({
                "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
                "debug_assertions": cfg!(debug_assertions), "gpu_texture_bytes": memory,
                "document_px": [view.editor.doc.width, view.editor.doc.height],
                "nodes": view.editor.doc.nodes.len(), "display_scale": window.scale_factor(),
                "editor_logical_px": [1000, 700], "window_active_at_completion": window.is_window_active(),
                "canvas_device_px": [f32::from(bounds.size.width) * window.scale_factor(), f32::from(bounds.size.height) * window.scale_factor()],
                "adapter": format!("{:?}", window.gpu_specs()), "stroke_commit_ms": commit_ms,
                "measurement": "scripted editor input to canvas submission and following platform frame callback; excludes physical display latency",
                "results": rows,
            }));
            COMPLETED.store(true, Ordering::SeqCst);
            cx.quit();
            return;
        }
        // Compositors may initially tile below the requested minimum or place
        // the window on another workspace. Wait for a usable, active surface;
        // starting there measures desktop throttling instead of the editor.
        if !window.is_window_active() || window.viewport_size().width < px(1000.) || window.viewport_size().height < px(700.) {
            window.refresh();
            schedule(editor, window);
            return;
        }
        let (case, step, paint_node, text_node) = (state.case, state.step, state.paint_node, state.text_node);
        state.step += 1;
        state.started = Some(Instant::now());
        editor.update(cx, |editor, cx| {
            let sign = if step.is_multiple_of(2) { 1.0 } else { -1.0 };
            match case {
                0 => {
                    let center = editor.canvas_bounds().unwrap().center();
                    editor.drag = Some(Drag::Pan { last: center });
                    editor.drag_move(center + point(px(4.0 * sign as f32), px(0.0)), window, cx);
                }
                1 => {
                    let w = editor.editor.doc.width as f64;
                    let h = editor.editor.doc.height as f64;
                    let position = editor.doc_to_window((w * (0.25 + step as f64 / 100.0), h * 0.55)).unwrap();
                    if step == 0 {
                        editor.drag = None;
                        editor.selected = Some(paint_node);
                        editor.set_paint(PaintKind::Brush, cx);
                        editor.tools.brush = Brush { size: 48.0, ..Brush::default() };
                        editor.tool_down(&MouseDownEvent { position, button: MouseButton::Left, ..Default::default() }, window, cx);
                        let active = matches!(editor.drag, Some(Drag::Tool(ToolDrag::Stroke { gpu_points: Some(_), .. })));
                        if cx.global::<CanvasBenchmark>().backend == "gpu" {
                            assert!(active, "compatible brush must use the GPU in this fixture");
                        }
                        cx.global_mut::<CanvasBenchmark>().gpu_brush = active;
                    } else {
                        editor.tool_move(position, cx);
                    }
                }
                _ => { editor.execute(Command::TranslateNode { id: text_node, dx: sign * 2.0, dy: 0.0 }, cx); }
            }
        });
        window.refresh();
        schedule(editor, window);
    });
}

/// Opens the real editor, runs three bounded workloads, and prints JSON to stdout.
/// The caller must isolate XDG_DATA_HOME before threads or libraries initialize.
pub fn run(path: Option<&std::path::Path>) -> anyhow::Result<()> {
    let mut doc = match path {
        Some(path) => emulsion_io::open(path)?,
        None => {
            let mut doc = Document::new(3840, 2160);
            for layer in 0..24 {
                let color = if layer == 0 {
                    [0.1, 0.15, 0.2, 1.0]
                } else {
                    [0.01, 0.02, 0.03, 0.04]
                };
                Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        format!("Layer {layer}"),
                        Arc::new(Raster::solid(3840, 2160, color)),
                        Placement::default(),
                    )),
                    slot: Slot::TOP,
                }
                .apply(&mut doc)?;
            }
            doc
        }
    };
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Benchmark text",
            TextSpec {
                text: "Crisp editable Vello text".into(),
                size: 72.0,
                x: doc.width as f32 * 0.25,
                y: doc.height as f32 * 0.3,
                ..Default::default()
            },
            doc.width,
            doc.height,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)?;
    let text_node = doc.nodes.last().unwrap().id;
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Benchmark paint",
            Arc::new(Raster::empty(doc.width, doc.height, [0; 4])),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)?;
    let paint_node = doc.nodes.last().unwrap().id;
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        eprintln!("native editor benchmark timed out");
        std::process::exit(1);
    });
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            crate::actions::bind(cx);
            cx.set_global(crate::app_state::AppSettings(Default::default()));
            cx.set_global(crate::app_state::Capabilities {
                cli: crate::app_state::CliStatus::Missing,
            });
            cx.set_global(CanvasBenchmark {
                paint_node,
                text_node,
                ..Default::default()
            });
            let bounds = Bounds::centered(None, size(px(1600.), px(1000.)), cx);
            cx.open_window(
                WindowOptions {
                    app_id: Some("dev.emulsion.canvas-benchmark".into()),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Emulsion canvas benchmark".into()),
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1000.), px(700.))),
                    ..Default::default()
                },
                move |window, cx| {
                    eprintln!("editor benchmark adapter: {:?}", window.gpu_specs());
                    let editor = cx.new(|cx| {
                        EditorView::new(
                            doc,
                            None,
                            None,
                            None,
                            "Disposable canvas benchmark".into(),
                            cx,
                        )
                    });
                    editor.update(cx, |editor, _| {
                        editor.sidebar_tab = SidebarTab::History;
                        editor.dock_tab = DockTab::Layers;
                    });
                    schedule(editor.clone(), window);
                    let shell = cx.new(|_| BenchWindow(editor));
                    cx.new(|cx| Root::new(shell, window, cx))
                },
            )
            .expect("open benchmark window");
            cx.activate(true);
        });
    anyhow::ensure!(
        COMPLETED.load(Ordering::SeqCst),
        "benchmark window closed before completion"
    );
    Ok(())
}
