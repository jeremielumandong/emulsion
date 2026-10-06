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
const CASES: [&str; 5] = ["pan", "brush", "vector_edit", "zoom", "rotated_zoom"];
static COMPLETED: AtomicBool = AtomicBool::new(false);

// EditorView is a flex child of Workspace in the shipping app. Preserve that
// sizing contract when hosting it on its own, including after owner redraws.
struct BenchWindow(Entity<EditorView>, bool);
impl Render for BenchWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let host = if self.1 {
            div().size_full()
        } else {
            div().w(px(1000.)).h(px(700.))
        };
        div()
            .size_full()
            .child(host.flex().flex_col().child(self.0.clone()))
    }
}

#[derive(Default)]
pub(super) struct CanvasBenchmark {
    ready: bool,
    diagram: bool,
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
    drag_origin: Option<(f64, f64)>,
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
                    "scenario": if state.diagram { ["pan","object_drag","command_move","zoom","rotated_zoom"][state.case] } else {CASES[state.case]}, "renderer": state.backend,
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
                let diagram=state.diagram;
                editor.update(cx, |editor, cx| {
                    if diagram && matches!(editor.drag, Some(Drag::Move(_))) {
                        let start = Instant::now();
                        editor.drag_end(cx);
                        cx.global_mut::<CanvasBenchmark>().commit_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
                        assert!(!editor.editor.in_transaction(), "drag release commits the move");
                        let moved = editor.editor.doc.clone();
                        editor.undo(cx);
                        assert_ne!(editor.editor.doc, moved, "drag must move artwork");
                        editor.redo(cx);
                        assert_eq!(editor.editor.doc, moved, "redo restores the committed move");
                        return;
                    }
                    if let Some(Drag::Tool(drag)) = editor.drag.take() {
                        let start = Instant::now();
                        editor.tool_up(drag, cx);
                        cx.global_mut::<CanvasBenchmark>().commit_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
                        if diagram { return; }
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
                "editor_logical_px": [f32::from(window.viewport_size().width),f32::from(window.viewport_size().height)], "window_active_at_completion": window.is_window_active(),
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
        if !window.is_window_active() || window.viewport_size().width < px(640.) || window.viewport_size().height < px(480.) {
            window.refresh();
            schedule(editor, window);
            return;
        }
        let (case, step, paint_node, text_node, diagram) = (state.case, state.step, state.paint_node, state.text_node,state.diagram);
        if step == 0 { eprintln!("canvas benchmark scenario {case}: {:?}, active={}", window.viewport_size(), window.is_window_active()); }
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
                1 if diagram => {
                    if step==0 {editor.drag=None;editor.set_tool(Tool::Move,cx);}
                    let b=emulsion_core::geometry::node_bounds(&editor.editor.doc,text_node).ok().flatten().unwrap();
                    let center=(b.x as f64+b.w as f64/2.,b.y as f64+b.h as f64/2.);
                    if step == 0 { cx.global_mut::<CanvasBenchmark>().drag_origin = Some(center); }
                    let origin = cx.global::<CanvasBenchmark>().drag_origin.unwrap();
                    let position=editor.doc_to_window(origin).unwrap();
                    if step==0 {
                        editor.set_layer_selection(vec![text_node], Some(text_node));
                        editor.begin_move(origin, cx);
                        assert!(matches!(editor.drag, Some(Drag::Move(_))), "fixture starts an object move");
                    } else {editor.drag_move(position+point(px(24.+8.*sign as f32),px(0.)),window,cx);}
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
                3 => {
                    editor.drag = None;
                    let bounds = editor.canvas_bounds().unwrap();
                    editor.scroll(&ScrollWheelEvent {
                        position: bounds.center(),
                        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px((sign * 1.08_f64.ln() / 0.004) as f32))),
                        modifiers: Modifiers { control: true, ..Default::default() },
                        touch_phase: gpui_kit::TouchPhase::Moved,
                    }, window, cx);
                }
                4 => {
                    editor.drag = None;
                    editor.view.zoom = 2.0;
                    editor.view.rotation = if sign > 0.0 { 15.0 } else { 18.0 };
                    editor.notify_canvas_navigation(window, cx);
                }
                _ => { editor.execute(Command::TranslateNode { id: text_node, dx: sign * 2.0, dy: 0.0 }, cx); }
            }
        });
        // The production handlers already invalidate the affected views.
        // A full refresh here would discard every retained panel and measure
        // a different render path from real pointer input.
        schedule(editor, window);
    });
}

/// Opens the real editor, runs five bounded workloads, and prints JSON to stdout.
/// The caller must isolate XDG_DATA_HOME before threads or libraries initialize.
pub fn run(path: Option<&std::path::Path>) -> anyhow::Result<()> {
    let diagram_count = std::env::var("EMULSION_BENCH_DIAGRAM")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.clamp(2, 10_000));
    let project = path
        .filter(|path| emulsion_io::project::is_project(path))
        .map(emulsion_io::project::read)
        .transpose()?;
    let project_kind = project.as_ref().map(|project| project.kind);
    let mut doc = if let Some(count) = diagram_count {
        use emulsion_core::diagram::{Builder, Endpoint, Port, Routing, ShapeKind};
        let cols = (count as f64).sqrt().ceil() as usize;
        let mut b = Builder::new(
            (cols * 230 + 40) as u32,
            (count.div_ceil(cols) * 130 + 40) as u32,
        )
        .map_err(anyhow::Error::msg)?;
        let mut ids = Vec::new();
        for i in 0..count {
            ids.push(
                b.add_shape(
                    ShapeKind::Process,
                    [
                        20. + (i % cols) as f64 * 230.,
                        20. + (i / cols) as f64 * 130.,
                        200.,
                        90.,
                    ],
                    &format!("Service {i}\nRequest processing"),
                )
                .map_err(anyhow::Error::msg)?,
            );
        }
        for pair in ids.windows(2) {
            b.connect(
                Endpoint {
                    shape: pair[0],
                    port: Port::East,
                },
                Endpoint {
                    shape: pair[1],
                    port: Port::West,
                },
                "",
                Routing::Straight,
            )
            .map_err(anyhow::Error::msg)?;
        }
        b.finish().map_err(anyhow::Error::msg)?
    } else {
        match path {
            Some(path) if emulsion_io::diagram_import::is_diagram(path) => {
                emulsion_io::diagram_import::read(path)?
                    .project
                    .pages
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Diagram contains no page"))?
                    .doc
            }
            Some(path) if emulsion_io::project::is_project(path) => {
                let project = project.as_ref().expect("loaded project");
                project
                    .pages
                    .iter()
                    .find(|page| page.meta.id == project.active)
                    .ok_or_else(|| anyhow::anyhow!("Project contains no active page"))?
                    .doc
                    .clone()
            }
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
        }
    };
    let is_diagram = doc.diagram.as_ref().is_some_and(|d| !d.shapes.is_empty());
    let artwork = is_diagram || project_kind.is_some();
    if !artwork {
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
    }
    let text_node = if is_diagram {
        let model = doc.diagram.as_ref().unwrap();
        let ids = model
            .shapes
            .iter()
            .filter(|(id, s)| {
                !s.kind.is_container()
                    && !s.data.contains_key("emulsion_drawio_endpoint")
                    && doc.node(**id).is_some_and(|n| n.visible)
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        *ids.get(ids.len() / 2)
            .ok_or_else(|| anyhow::anyhow!("No visible shape to benchmark"))?
    } else if artwork {
        doc.nodes
            .iter()
            .rev()
            .find(|node| {
                node.visible
                    && !node.locked
                    && matches!(node.kind, NodeKind::Path { .. } | NodeKind::Text { .. })
            })
            .ok_or_else(|| anyhow::anyhow!("No editable vector object to benchmark"))?
            .id
    } else {
        doc.nodes.last().unwrap().id
    };
    if !artwork {
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
    }
    let paint_node = doc.nodes.last().unwrap().id;
    let prepared = EditorView::prepare(doc.clone(), None, None)?;
    let project = if artwork {
        Some(
            emulsion_core::project::ProjectEditor::new_project(
                project_kind.unwrap_or(emulsion_core::project::ProjectKind::Diagram),
                doc,
            )
            .map_err(anyhow::Error::msg)?,
        )
    } else {
        None
    };
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
                diagram: artwork,
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
                    // Own only this disposable window. Fullscreen avoids a tiling
                    // compositor shrinking it below the fixed 1000×700 editor,
                    // and gives CPU/GPU runs identical available surface bounds.
                    window_bounds: Some(WindowBounds::Fullscreen(bounds)),
                    window_min_size: Some(size(px(1000.), px(700.))),
                    ..Default::default()
                },
                move |window, cx| {
                    eprintln!("editor benchmark adapter: {:?}", window.gpu_specs());
                    let editor = cx.new(|cx| {
                        let mut view = EditorView::from_prepared(
                            prepared,
                            None,
                            "Disposable canvas benchmark".into(),
                            cx,
                        );
                        if let Some(project) = project {
                            view.editor = project;
                        }
                        view
                    });
                    editor.update(cx, |editor, _| {
                        if is_diagram {
                            let b =
                                emulsion_core::geometry::node_bounds(&editor.editor.doc, text_node)
                                    .ok()
                                    .flatten()
                                    .unwrap();
                            editor.view.zoom = 1.;
                            editor.view.center =
                                (b.x as f64 + b.w as f64 / 2., b.y as f64 + b.h as f64 / 2.);
                            editor.fit_pending = false;
                        }
                        editor.sidebar_tab = SidebarTab::History;
                        editor.dock_tab = DockTab::Layers;
                    });
                    schedule(editor.clone(), window);
                    let shell = cx.new(|_| BenchWindow(editor, artwork));
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
