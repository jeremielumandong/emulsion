//! The windowed render loop: winit + one wgpu surface, rendering once per
//! frame regardless of input rate. Runs a scripted benchmark or an
//! interactive viewer with painting, pan/zoom and a HUD.

use crate::bench::{Report, Script, Series};
use crate::brush::{CpuStroke, GpuStroke, Readback, test_brush};
use crate::compositor::Camera;
use crate::engine::{Engine, FrameTimes, Output};
use crate::gpu::{Gpu, TileFormat};
use crate::vector::VectorSpace;
use emulsion_core::{Document, NodeId};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

pub struct Options {
    pub size: (u32, u32),
    pub vsync: bool,
    pub tiles: Option<TileFormat>,
    pub space: VectorSpace,
    pub vello: bool,
    pub cache: bool,
    pub script: Option<(crate::bench::Kind, Option<usize>)>,
    pub title: String,
}

enum Painting {
    Idle,
    Cpu(Box<CpuStroke>),
    Gpu(Box<GpuStroke>),
}

struct Presenter {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    output: Output,
}

struct App {
    options: Options,
    doc: Document,
    paint_node: Option<NodeId>,
    gpu: Option<Arc<Gpu>>,
    win: Option<Presenter>,
    engine: Option<Engine>,
    script: Option<Script>,
    error: Option<anyhow::Error>,
    report: Option<Report>,
    // Interactive state.
    frame_times: VecDeque<f64>,
    last_frame: Option<Instant>,
    cursor: (f64, f64),
    panning: bool,
    painting: Painting,
    gpu_brush: bool,
    stroke_start: Option<Instant>,
    pending_events: Vec<Instant>,
    latency: Series,
    readback: Option<(Readback, usize, bool)>,
    show_hud: bool,
    animate_edits: bool,
    frame: usize,
    last_times: FrameTimes,
    upload: VecDeque<u64>,
    hud_at: Option<Instant>,
}

pub fn run(
    options: Options,
    doc: Document,
    paint_node: Option<NodeId>,
) -> anyhow::Result<Option<Report>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        options,
        doc,
        paint_node,
        gpu: None,
        win: None,
        engine: None,
        script: None,
        error: None,
        report: None,
        frame_times: VecDeque::new(),
        last_frame: None,
        cursor: (0.0, 0.0),
        panning: false,
        painting: Painting::Idle,
        gpu_brush: false,
        stroke_start: None,
        pending_events: Vec::new(),
        latency: Series::default(),
        readback: None,
        show_hud: true,
        animate_edits: false,
        frame: 0,
        last_times: FrameTimes::default(),
        upload: VecDeque::new(),
        hud_at: None,
    };
    event_loop.run_app(&mut app)?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok(app.report)
}

impl App {
    fn setup(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(self.options.title.clone())
                    .with_inner_size(PhysicalSize::new(self.options.size.0, self.options.size.1)),
            )?,
        );
        let instance = crate::gpu::instance();
        let surface = instance.create_surface(window.clone())?;
        let gpu = Gpu::new(instance, Some(&surface), self.options.tiles)?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let present_mode = if self.options.vsync {
            wgpu::PresentMode::Fifo
        } else if caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&gpu.device, &config);
        tracing::info!(?format, ?present_mode, "surface");
        let mut engine = Engine::new(
            gpu.clone(),
            &self.doc,
            self.paint_node,
            self.options.space,
            self.options.vello,
            self.options.cache,
            (config.width, config.height),
        )?;
        if let Some((kind, frames)) = self.options.script {
            self.script = Some(Script::new(kind, &mut engine, frames));
        }
        self.win = Some(Presenter {
            window,
            surface,
            output: if format.is_srgb() {
                Output::HardwareSrgb
            } else {
                Output::Encoded
            },
            config,
        });
        self.engine = Some(engine);
        self.gpu = Some(gpu);
        Ok(())
    }

    fn hud_lines(&self, engine: &Engine) -> Vec<String> {
        let mut s = Series::default();
        for t in &self.frame_times {
            s.push(*t);
        }
        let upload: f64 =
            self.upload.iter().sum::<u64>() as f64 / self.upload.len().max(1) as f64 / 1024.0;
        let t = &self.last_times;
        let alloc = self
            .gpu
            .as_ref()
            .and_then(|g| g.allocated_bytes())
            .map_or("n/a".into(), |b| format!("{:.0} MiB", b as f64 / 1048576.0));
        vec![
            format!(
                "frame p50 {:.2} ms  p99 {:.2} ms  ({:.0} fps)  zoom {:.0}% L{}",
                s.quantile(0.5),
                s.quantile(0.99),
                1000.0 / s.mean().max(1e-3),
                engine.camera.zoom * 100.0,
                engine.camera.level()
            ),
            format!(
                "cpu {:.2} ms: mips {:.2} ({} tiles)  vello enc {:.2} render {:.2}  comp {:.2}",
                t.cpu_ms,
                t.mips_ms,
                t.mip_tiles,
                t.vector_encode_ms,
                t.vector_render_ms,
                t.composite_ms
            ),
            format!(
                "upload {upload:.0} KiB/frame  atlas {}/{} tiles  textures {:.0} MiB  alloc {alloc}",
                engine.atlas.used(),
                engine.atlas.capacity(),
                engine.texture_bytes() as f64 / 1048576.0
            ),
            format!(
                "brush {} (b)  vectors {} visible, {} runs ({})  edits {} (e)",
                if self.gpu_brush {
                    "B: GPU dabs"
                } else {
                    "A: CPU stamp + dirty tiles"
                },
                t.visible_vectors,
                t.vector_runs,
                match engine.vectors.space {
                    VectorSpace::Srgb => "sRGB",
                    VectorSpace::Linear => "linear",
                },
                if self.animate_edits { "on" } else { "off" }
            ),
            format!(
                "last stroke input→GPU p50 {:.1} ms p99 {:.1} ms  ({} events)",
                self.latency.quantile(0.5),
                self.latency.quantile(0.99),
                self.latency.len()
            ),
            "drag: paint  right/middle drag: pan  wheel: zoom  1: 100%  0: fit  h: HUD".into(),
        ]
    }

    fn stroke_point(&mut self) {
        let Some(engine) = &self.engine else { return };
        let (x, y) = to_doc(engine, self.cursor);
        let t = self
            .stroke_start
            .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1e3);
        match &mut self.painting {
            Painting::Cpu(s) => s.point(x, y, t),
            Painting::Gpu(s) => s.point(x, y),
            Painting::Idle => return,
        }
        self.pending_events.push(Instant::now());
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let now = Instant::now();
        if let Some(last) = self.last_frame.replace(now) {
            let ms = now.duration_since(last).as_secs_f64() * 1e3;
            self.frame_times.push_back(ms);
            if self.frame_times.len() > 120 {
                self.frame_times.pop_front();
            }
        }
        let mut engine = self.engine.take().expect("engine");
        let result = self.frame_inner(event_loop, &mut engine, now);
        self.engine = Some(engine);
        result
    }

    fn frame_inner(
        &mut self,
        event_loop: &ActiveEventLoop,
        engine: &mut Engine,
        start: Instant,
    ) -> anyhow::Result<()> {
        // The first frame uploads the document and compiles pipelines; keep
        // it out of scripted measurements, as the headless runner does.
        let warmup = self.frame == 0;
        if let Some(script) = &mut self.script {
            if !warmup {
                script.before_frame(engine)?;
            }
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
            if self.animate_edits && !engine.vectors.objects.is_empty() {
                let n = engine.vectors.objects.len();
                let node = engine.vectors.objects[self.frame * 7919 % n].node;
                let d = if self.frame.is_multiple_of(2) {
                    2.0
                } else {
                    -2.0
                };
                engine.vectors.edit(node, |k| {
                    if let crate::canvas::VectorKind::Path { path, .. } = k {
                        let mut p = (**path).clone();
                        p.translate(d, 0.0);
                        *path = Arc::new(p);
                    }
                });
            }
        }
        // Scripted runs measure the canvas alone. Interactively the HUD text
        // refreshes at 4 Hz so Vello re-renders it rarely.
        if self.script.is_some() || !self.show_hud {
            engine.hud.clear();
        } else if self
            .hud_at
            .is_none_or(|t| t.elapsed().as_secs_f64() >= 0.25)
        {
            engine.hud = self.hud_lines(engine);
            self.hud_at = Some(Instant::now());
        }
        let win = self.win.as_mut().expect("window");
        let texture = match win.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                win.surface.configure(&engine.gpu.device, &win.config);
                return Ok(());
            }
            _ => return Ok(()),
        };
        let view = texture.texture.create_view(&Default::default());
        let times = engine.render(&view, win.config.format, win.output)?;
        {
            let _span = tracing::info_span!("present").entered();
            texture.present();
        }
        // Serialise on the GPU so input-to-pixel latency is attributable.
        engine.gpu.wait();
        let done = Instant::now();
        self.last_times = times;
        self.upload.push_back(times.uploaded_bytes);
        if self.upload.len() > 60 {
            self.upload.pop_front();
        }
        for e in self.pending_events.drain(..) {
            self.latency
                .push(done.duration_since(e).as_secs_f64() * 1e3);
        }
        if warmup {
            // Nothing to record.
        } else if let Some(script) = &mut self.script {
            let ms = done.duration_since(start).as_secs_f64() * 1e3;
            script.after_frame(engine, ms, done, &times)?;
            if script.done() {
                let mut report = std::mem::take(&mut script.report);
                report.mode = format!(
                    "windowed, {} ({})",
                    engine.gpu.describe(),
                    if self.options.vsync {
                        "vsync"
                    } else {
                        "vsync off"
                    }
                );
                self.report = Some(report);
                event_loop.exit();
            }
        } else if let Some((readback, _, mapped)) = &mut self.readback {
            if !*mapped {
                readback.map();
                *mapped = true;
            }
            let _ = engine.gpu.device.poll(wgpu::PollType::Poll);
            if readback.is_ready() {
                let (readback, dabs, _) = self.readback.take().unwrap();
                let ms = readback.started.elapsed().as_secs_f64() * 1e3;
                if let Some(source) = engine.canvas.paint {
                    readback.complete(
                        &engine.gpu,
                        &mut engine.canvas,
                        &mut engine.atlas,
                        source,
                    )?;
                }
                tracing::info!(ms, dabs, "GPU stroke read back");
            }
        }
        self.frame += 1;
        Ok(())
    }

    fn handle(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) -> anyhow::Result<()> {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(win), Some(engine)) = (&mut self.win, &mut self.engine) {
                    win.config.width = size.width.max(1);
                    win.config.height = size.height.max(1);
                    win.surface.configure(&engine.gpu.device, &win.config);
                    engine.screen = (win.config.width, win.config.height);
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop)?,
            _ if self.script.is_some() => {}
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let engine = self.engine.as_mut().expect("engine");
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Character("1") => engine.camera.zoom = 1.0,
                    Key::Character("0") => {
                        engine.camera =
                            Camera::fit((engine.canvas.width, engine.canvas.height), engine.screen)
                    }
                    Key::Character("b") => self.gpu_brush = !self.gpu_brush,
                    Key::Character("h") => self.show_hud = !self.show_hud,
                    Key::Character("e") => self.animate_edits = !self.animate_edits,
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (dx, dy) = (position.x - self.cursor.0, position.y - self.cursor.1);
                self.cursor = (position.x, position.y);
                if self.panning
                    && let Some(engine) = &mut self.engine
                {
                    engine.camera.center[0] -= dx / engine.camera.zoom;
                    engine.camera.center[1] -= dy / engine.camera.zoom;
                }
                self.stroke_point();
            }
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => {
                    let engine = self.engine.as_mut().expect("engine");
                    if let Some(source) = engine.canvas.paint
                        && self.readback.is_none()
                    {
                        let brush = test_brush(crate::bench::BRUSH_SIZE);
                        self.painting = if self.gpu_brush {
                            Painting::Gpu(Box::new(GpuStroke::begin(source, brush)))
                        } else {
                            Painting::Cpu(Box::new(CpuStroke::begin(&engine.canvas, source, brush)))
                        };
                        self.stroke_start = Some(Instant::now());
                        self.latency = Series::default();
                        self.stroke_point();
                    }
                }
                (MouseButton::Left, ElementState::Released) => {
                    let engine = self.engine.as_mut().expect("engine");
                    match std::mem::replace(&mut self.painting, Painting::Idle) {
                        Painting::Cpu(mut s) => {
                            s.finish();
                            s.render(&mut engine.canvas, &mut engine.atlas, &engine.gpu.queue)?;
                        }
                        Painting::Gpu(mut s) => {
                            let (b, c, a, e) = engine.brush_parts();
                            s.render(b, c, a, e)?;
                            let dabs = s.dabs;
                            let (_, _, atlas, encoder) = engine.brush_parts();
                            self.readback = Some((s.finish(atlas, encoder), dabs, false));
                        }
                        Painting::Idle => {}
                    }
                }
                (MouseButton::Right | MouseButton::Middle, s) => {
                    self.panning = s == ElementState::Pressed
                }
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let engine = self.engine.as_mut().expect("engine");
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 60.0,
                };
                let before = to_doc(engine, self.cursor);
                engine.camera.zoom = (engine.camera.zoom * 1.15f64.powf(steps)).clamp(0.02, 64.0);
                let after = to_doc(engine, self.cursor);
                engine.camera.center[0] += (before.0 - after.0) as f64;
                engine.camera.center[1] += (before.1 - after.1) as f64;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Screen position → document position.
fn to_doc(engine: &Engine, p: (f64, f64)) -> (f32, f32) {
    let o = engine.camera.origin(engine.screen);
    (
        (o[0] + p.0 / engine.camera.zoom) as f32,
        (o[1] + p.1 / engine.camera.zoom) as f32,
    )
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        if let Err(e) = self.setup(event_loop) {
            self.error = Some(e);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Err(e) = self.handle(event_loop, event) {
            self.error = Some(e);
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(win) = &self.win {
            win.window.request_redraw();
        }
    }
}
