//! Linux embedding spike: the engine runs on GPUI's own wgpu device and
//! renders the canvas into a texture that GPUI's renderer composites as an
//! external surface, inside a window of ordinary GPUI chrome. No readback, no
//! `paint_image`.
//!
//! Scripted runs use the same scripts as the standalone window. A frame's
//! time is the interval between successive canvas paints: GPUI's layout,
//! paint, its own draw and present, and the wait for the GPU. Latency runs
//! from an input event to GPU completion of the GPUI frame that showed it.

use crate::bench::{Kind, Report, Script, Series};
use crate::brush::{CpuStroke, GpuStroke, Readback, test_brush};
use crate::compositor::Camera;
use crate::engine::{Engine, FrameTimes, Output};
use crate::gpu::{Gpu, TileFormat};
use crate::vector::VectorSpace;
use anyhow::Context as _;
use emulsion_core::{Document, NodeId};
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Options {
    /// Canvas area in logical pixels; the window adds the chrome around it.
    pub size: (u32, u32),
    pub tiles: Option<TileFormat>,
    pub space: VectorSpace,
    pub vello: bool,
    pub cache: bool,
    pub script: Option<(Kind, Option<usize>)>,
    pub title: String,
}

const SIDEBAR: f32 = 260.0;
const TOOLBAR: f32 = 36.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

enum Painting {
    Idle,
    Cpu(Box<CpuStroke>),
    Gpu(Box<GpuStroke>),
}

struct Target {
    view: wgpu::TextureView,
    size: (u32, u32),
}

/// The frame whose GPU completion the next paint waits for.
struct Pending {
    start: Instant,
    times: FrameTimes,
    scripted: bool,
    events: Vec<Instant>,
}

struct State {
    options: Options,
    doc: Document,
    paint_node: Option<NodeId>,
    engine: Option<Engine>,
    script: Option<Script>,
    target: Option<Target>,
    pending: Option<Pending>,
    frames: VecDeque<f64>,
    last_times: FrameTimes,
    report: Option<Report>,
    error: Option<anyhow::Error>,
    bounds: Option<Bounds<Pixels>>,
    scale: f32,
    painting: Painting,
    gpu_brush: bool,
    stroke_start: Option<Instant>,
    events: Vec<Instant>,
    latency: Series,
    readback: Option<(Readback, usize, bool)>,
    panning: Option<Point<Pixels>>,
    frame: usize,
}

fn device_size(bounds: Bounds<Pixels>, scale: f32) -> (u32, u32) {
    (
        ((f32::from(bounds.size.width) * scale).round() as u32).max(1),
        ((f32::from(bounds.size.height) * scale).round() as u32).max(1),
    )
}

/// The present mode GPUI's Linux backends configure.
fn presentation() -> &'static str {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "Wayland, Mailbox"
    } else {
        "X11, Fifo (vsync)"
    }
}

impl State {
    /// Canvas-local device position → document position.
    fn to_doc(&self, p: Point<Pixels>) -> Option<(f32, f32)> {
        let (engine, bounds) = (self.engine.as_ref()?, self.bounds?);
        let o = engine.camera.origin(engine.screen);
        let x = f32::from(p.x - bounds.origin.x) as f64 * self.scale as f64;
        let y = f32::from(p.y - bounds.origin.y) as f64 * self.scale as f64;
        Some((
            (o[0] + x / engine.camera.zoom) as f32,
            (o[1] + y / engine.camera.zoom) as f32,
        ))
    }

    fn stroke_point(&mut self, p: Point<Pixels>) {
        let Some((x, y)) = self.to_doc(p) else { return };
        let t = self
            .stroke_start
            .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1e3);
        match &mut self.painting {
            Painting::Cpu(s) => s.point(x, y, t),
            Painting::Gpu(s) => s.point(x, y),
            Painting::Idle => return,
        }
        self.events.push(Instant::now());
    }

    /// Finish the previous frame, advance, and render this one. Returns the
    /// texture GPUI should composite.
    fn frame(&mut self, bounds: Bounds<Pixels>, scale: f32) -> anyhow::Result<wgpu::TextureView> {
        self.bounds = Some(bounds);
        self.scale = scale;
        let size = device_size(bounds, scale);

        // The previous GPUI frame, including its present, is complete once
        // everything submitted so far has run.
        if let Some(engine) = &self.engine {
            let _span = tracing::info_span!("gpu_wait").entered();
            engine.gpu.wait();
        }
        // A frame runs from here to the same point of the next paint: its own
        // work, GPUI's layout, draw and present of it, and the GPU finishing.
        let done = Instant::now();
        let start = done;
        if let Some(p) = self.pending.take() {
            let ms = start.duration_since(p.start).as_secs_f64() * 1e3;
            tracing::debug!(
                frame = self.frame,
                ms,
                fills = p.times.cache_fills,
                mips = p.times.mip_tiles,
                "embedded frame"
            );
            self.frames.push_back(ms);
            if self.frames.len() > 120 {
                self.frames.pop_front();
            }
            let engine = self.engine.as_mut().expect("engine");
            if p.scripted
                && let Some(script) = &mut self.script
            {
                script.after_frame(engine, ms, done, &p.times)?;
                if script.done() {
                    let mut report = std::mem::take(&mut script.report);
                    report.mode = format!(
                        "GPUI embedded, {} ({})",
                        engine.gpu.describe(),
                        presentation()
                    );
                    report.notes.push(format!(
                        "canvas {}x{} device px at scale {scale}; GPU textures {:.0} MiB (atlas {} tiles in {} pages, {})",
                        size.0,
                        size.1,
                        engine.texture_bytes() as f64 / 1048576.0,
                        engine.atlas.used(),
                        engine.atlas.pages(),
                        engine.gpu.tile_format.label(),
                    ));
                    self.report = Some(report);
                }
            }
            for e in p.events {
                self.latency
                    .push(done.duration_since(e).as_secs_f64() * 1e3);
            }
            if let Some((readback, _, mapped)) = &mut self.readback {
                if !*mapped {
                    readback.map();
                    *mapped = true;
                }
                let _ = engine.gpu.device.poll(wgpu::PollType::Poll);
                if readback.is_ready()
                    && let Some(source) = engine.canvas.paint
                {
                    let (readback, dabs, _) = self.readback.take().expect("readback");
                    let ms = readback.started.elapsed().as_secs_f64() * 1e3;
                    readback.complete(
                        &engine.gpu,
                        &mut engine.canvas,
                        &mut engine.atlas,
                        source,
                    )?;
                    tracing::info!(ms, dabs, "GPU stroke read back");
                }
            }
        }

        // First paint: GPUI has created its device; build the engine on it.
        let warmup = self.engine.is_none();
        if warmup {
            let shared =
                gpui_wgpu::shared_gpu().context("GPUI has not published its wgpu device")?;
            let gpu = Gpu::from_shared(&shared, self.options.tiles);
            tracing::info!(
                gpu = gpu.describe(),
                tiles = gpu.tile_format.label(),
                "canvas on GPUI's device"
            );
            let mut engine = Engine::new(
                gpu,
                &self.doc,
                self.paint_node,
                self.options.space,
                self.options.vello,
                self.options.cache,
                size,
            )?;
            if let Some((kind, frames)) = self.options.script {
                self.script = Some(Script::new(kind, &mut engine, frames));
            }
            self.engine = Some(engine);
        }
        let engine = self.engine.as_mut().expect("engine");
        engine.screen = size;
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            let texture = engine.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("embedded canvas"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.target = Some(Target {
                view: texture.create_view(&Default::default()),
                size,
            });
        }

        // Scripts start after the warm-up frame, as in the standalone runs.
        let scripted = !warmup && self.script.as_ref().is_some_and(|s| !s.done());
        if scripted {
            self.script.as_mut().expect("script").before_frame(engine)?;
        } else {
            match &mut self.painting {
                Painting::Cpu(s) => {
                    s.render(&mut engine.canvas, &mut engine.atlas, &engine.gpu.queue)?;
                }
                Painting::Gpu(s) => {
                    let (b, c, a, e) = engine.brush_parts();
                    s.render(b, c, a, e)?;
                }
                Painting::Idle => {}
            }
        }
        let view = self.target.as_ref().expect("target").view.clone();
        let times = engine.render(&view, FORMAT, Output::Encoded)?;
        self.last_times = times;
        self.pending = Some(Pending {
            start,
            times,
            scripted,
            events: std::mem::take(&mut self.events),
        });
        self.frame += 1;
        Ok(view)
    }

    fn stats(&self) -> String {
        let mut s = Series::default();
        for t in &self.frames {
            s.push(*t);
        }
        let Some(engine) = &self.engine else {
            return "starting…".into();
        };
        let latency = if self.latency.len() == 0 {
            "–".to_string()
        } else {
            format!(
                "{:.1}/{:.1} ms",
                self.latency.quantile(0.5),
                self.latency.quantile(0.99)
            )
        };
        format!(
            "frame {:.1}/{:.1} ms p50/p99 · cpu {:.2} ms · zoom {:.0}% · brush {} · stroke latency {latency} · {} tiles · {}",
            s.quantile(0.5),
            s.quantile(0.99),
            self.last_times.cpu_ms,
            engine.camera.zoom * 100.0,
            if self.gpu_brush { "B (GPU)" } else { "A (CPU)" },
            engine.gpu.tile_format.label(),
            presentation(),
        )
    }
}

type Shared = Rc<RefCell<State>>;

struct CanvasView {
    state: Shared,
}

impl Render for CanvasView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        div()
            .id("canvas")
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    let mut s = this.state.borrow_mut();
                    if s.script.is_some() || s.readback.is_some() {
                        return;
                    }
                    let Some(source) = s.engine.as_ref().and_then(|e| e.canvas.paint) else {
                        return;
                    };
                    let brush = test_brush(crate::bench::BRUSH_SIZE);
                    s.painting = if s.gpu_brush {
                        Painting::Gpu(Box::new(GpuStroke::begin(source, brush)))
                    } else {
                        let canvas = &s.engine.as_ref().expect("engine").canvas;
                        Painting::Cpu(Box::new(CpuStroke::begin(canvas, source, brush)))
                    };
                    s.stroke_start = Some(Instant::now());
                    s.latency = Series::default();
                    s.stroke_point(e.position);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    let mut s = this.state.borrow_mut();
                    let s = &mut *s;
                    let Some(engine) = s.engine.as_mut() else {
                        return;
                    };
                    let result = match std::mem::replace(&mut s.painting, Painting::Idle) {
                        Painting::Cpu(mut stroke) => {
                            stroke.finish();
                            stroke
                                .render(&mut engine.canvas, &mut engine.atlas, &engine.gpu.queue)
                                .map(|_| ())
                        }
                        Painting::Gpu(mut stroke) => {
                            let (b, c, a, e) = engine.brush_parts();
                            let r = stroke.render(b, c, a, e).map(|_| ());
                            let dabs = stroke.dabs;
                            let (_, _, atlas, encoder) = engine.brush_parts();
                            s.readback = Some((stroke.finish(atlas, encoder), dabs, false));
                            r
                        }
                        Painting::Idle => Ok(()),
                    };
                    if let Err(e) = result {
                        s.error = Some(e);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    this.state.borrow_mut().panning = Some(e.position);
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.state.borrow_mut().panning = None;
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, _| {
                let mut s = this.state.borrow_mut();
                if let Some(last) = s.panning {
                    let (scale, delta) = (s.scale as f64, e.position - last);
                    if let Some(engine) = &mut s.engine {
                        let k = scale / engine.camera.zoom;
                        engine.camera.center[0] -= f32::from(delta.x) as f64 * k;
                        engine.camera.center[1] -= f32::from(delta.y) as f64 * k;
                    }
                    s.panning = Some(e.position);
                }
                s.stroke_point(e.position);
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, _| {
                let mut s = this.state.borrow_mut();
                let steps = -f32::from(e.delta.pixel_delta(px(20.)).y) as f64 / 20.0;
                let Some(before) = s.to_doc(e.position) else {
                    return;
                };
                if let Some(engine) = &mut s.engine {
                    engine.camera.zoom =
                        (engine.camera.zoom * 1.15f64.powf(steps)).clamp(0.02, 64.0);
                }
                let Some(after) = s.to_doc(e.position) else {
                    return;
                };
                if let Some(engine) = &mut s.engine {
                    engine.camera.center[0] += (before.0 - after.0) as f64;
                    engine.camera.center[1] += (before.1 - after.1) as f64;
                }
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        let result = state.borrow_mut().frame(bounds, window.scale_factor());
                        match result {
                            Ok(view) => window
                                .paint_external_texture(bounds, ExternalTexture(Arc::new(view))),
                            Err(e) => state.borrow_mut().error = Some(e),
                        }
                        let s = state.borrow();
                        if s.report.is_some() || s.error.is_some() {
                            cx.defer(|cx| cx.quit());
                        } else {
                            window.request_animation_frame();
                        }
                    },
                )
                .size_full(),
            )
    }
}

struct Toolbar {
    state: Shared,
    _refresh: Task<()>,
}

impl Toolbar {
    fn new(state: Shared, cx: &mut Context<Self>) -> Self {
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            state,
            _refresh: refresh,
        }
    }
}

fn button(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(0x3a3a3a))
        .hover(|s| s.bg(rgb(0x4a4a4a)))
        .cursor_pointer()
        .child(label.into())
}

impl Render for Toolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.state.borrow();
        let brush = if s.gpu_brush {
            "Brush B: GPU"
        } else {
            "Brush A: CPU"
        };
        let stats = s.stats();
        drop(s);
        div()
            .h(px(TOOLBAR))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .bg(rgb(0x2b2b2b))
            .text_sm()
            .child(button("fit", "Fit").on_click(cx.listener(|this, _, _, _| {
                let mut s = this.state.borrow_mut();
                if let Some(e) = &mut s.engine {
                    e.camera = Camera::fit((e.canvas.width, e.canvas.height), e.screen);
                }
            })))
            .child(
                button("actual", "100%").on_click(cx.listener(|this, _, _, _| {
                    if let Some(e) = &mut this.state.borrow_mut().engine {
                        e.camera.zoom = 1.0;
                    }
                })),
            )
            .child(
                button("brush", brush).on_click(cx.listener(|this, _, _, cx| {
                    let mut s = this.state.borrow_mut();
                    s.gpu_brush = !s.gpu_brush;
                    cx.notify();
                })),
            )
            .child(div().text_color(rgb(0xa0a0a0)).child(stats))
    }
}

struct Host {
    toolbar: Entity<Toolbar>,
    canvas: Entity<CanvasView>,
    layers: Vec<SharedString>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x232323))
            .text_color(rgb(0xdedede))
            .child(self.toolbar.clone())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(SIDEBAR))
                            .flex()
                            .flex_col()
                            .flex_none()
                            .overflow_hidden()
                            .bg(rgb(0x2b2b2b))
                            .text_sm()
                            .children(self.layers.iter().map(|name| {
                                div()
                                    .h(px(24.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_b_1()
                                    .border_color(rgb(0x333333))
                                    .child(name.clone())
                            })),
                    )
                    .child(div().flex_1().h_full().child(self.canvas.clone())),
            )
    }
}

/// Open the GPUI window and run until the script finishes (or the window
/// closes). Returns the scripted report, if any.
pub fn run(
    options: Options,
    doc: Document,
    paint_node: Option<NodeId>,
) -> anyhow::Result<Option<Report>> {
    let (w, h) = options.size;
    let title = options.title.clone();
    // Top of the stack first, as a layers panel shows it.
    let layers: Vec<SharedString> = doc
        .nodes
        .iter()
        .rev()
        .take(80)
        .map(|n| SharedString::from(n.name.clone()))
        .collect();
    let state: Shared = Rc::new(RefCell::new(State {
        options,
        doc,
        paint_node,
        engine: None,
        script: None,
        target: None,
        pending: None,
        frames: VecDeque::new(),
        last_times: FrameTimes::default(),
        report: None,
        error: None,
        bounds: None,
        scale: 1.0,
        painting: Painting::Idle,
        gpu_brush: false,
        stroke_start: None,
        events: Vec::new(),
        latency: Series::default(),
        readback: None,
        panning: None,
        frame: 0,
    }));
    let app_state = state.clone();
    gpui_kit::application().run(move |cx| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(
            None,
            size(px(w as f32 + SIDEBAR), px(h as f32 + TOOLBAR)),
            cx,
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = cx.open_window(options, |_window, cx| {
            let canvas = cx.new(|_| CanvasView {
                state: app_state.clone(),
            });
            let toolbar = cx.new(|cx| Toolbar::new(app_state.clone(), cx));
            cx.new(|_| Host {
                toolbar,
                canvas,
                layers,
            })
        });
        if let Err(e) = opened {
            app_state.borrow_mut().error = Some(e);
            cx.quit();
        }
    });
    let mut s = state.borrow_mut();
    if let Some(e) = s.error.take() {
        return Err(e);
    }
    Ok(s.report.take())
}
