//! Scripted, repeatable workloads. The same camera paths and input schedules
//! drive the GPU engine and a CPU baseline of the GPUI canvas's raster work.

use crate::brush::{CpuStroke, GpuStroke, Readback, test_brush, tiles_in};
use crate::compositor::Camera;
use crate::engine::{Engine, FrameTimes};
use emulsion_core::{Document, NodeId, NodeKind};
use emulsion_raster::composite::{render_tile, tile_to_bgra8, tiles_at};
use emulsion_raster::paint::{Ink, Stroke};
use emulsion_raster::{IRect, Raster, TILE, TileCoord};
use rayon::prelude::*;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Navigate,
    BrushA,
    BrushB,
    VectorEdit,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "navigate" => Some(Self::Navigate),
            "brush-a" => Some(Self::BrushA),
            "brush-b" => Some(Self::BrushB),
            "vector-edit" => Some(Self::VectorEdit),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::BrushA => "brush-a",
            Self::BrushB => "brush-b",
            Self::VectorEdit => "vector-edit",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Series(pub Vec<f64>);

impl Series {
    pub fn push(&mut self, v: f64) {
        self.0.push(v);
    }
    pub fn quantile(&self, q: f64) -> f64 {
        if self.0.is_empty() {
            return f64::NAN;
        }
        let mut v = self.0.clone();
        v.sort_by(f64::total_cmp);
        let i = ((v.len() - 1) as f64 * q).round() as usize;
        v[i]
    }
    pub fn mean(&self) -> f64 {
        self.0.iter().sum::<f64>() / self.0.len().max(1) as f64
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn summary(&self) -> String {
        format!("{:.2} / {:.2} ms", self.quantile(0.5), self.quantile(0.99))
    }
}

// ── Scripted camera and input ──

/// Navigation phases: fast pan at 100%, pan at 50%, zoom sweep fit↔400%.
pub const PHASES: [(&str, usize); 3] = [("pan 100%", 240), ("pan 50%", 120), ("zoom sweep", 240)];

pub fn navigate_camera(frame: usize, doc: (u32, u32), screen: (u32, u32)) -> (usize, Camera) {
    let (w, h) = (doc.0 as f64, doc.1 as f64);
    let fit = Camera::fit(doc, screen).zoom;
    let mut f = frame;
    for (phase, (_, n)) in PHASES.iter().enumerate() {
        if f < *n {
            let t = f as f64 / *n as f64;
            let camera = match phase {
                0 | 1 => {
                    let zoom = if phase == 0 { 1.0 } else { 0.5 };
                    let half = screen.0 as f64 / 2.0 / zoom;
                    let span = (w - 2.0 * half).max(1.0);
                    // 24 screen px per frame, bouncing.
                    let travel = (f as f64 * 24.0 / zoom) % (2.0 * span);
                    let x = half
                        + if travel < span {
                            travel
                        } else {
                            2.0 * span - travel
                        };
                    let y = h / 2.0 + (h / 4.0) * (t * std::f64::consts::TAU).sin();
                    Camera {
                        center: [x.round(), y.round()],
                        zoom,
                    }
                }
                _ => {
                    let s = 1.0 - (2.0 * t - 1.0).abs();
                    Camera {
                        center: [w * 0.45, h * 0.55],
                        zoom: fit * (4.0 / fit).powf(s),
                    }
                }
            };
            return (phase, camera);
        }
        f -= n;
    }
    (PHASES.len(), Camera::fit(doc, screen))
}

pub fn navigate_frames() -> usize {
    PHASES.iter().map(|(_, n)| n).sum()
}

/// A 1.5 s S-curve across the screen at 1 kHz, in document pixels.
pub fn stroke_points(camera: &Camera, screen: (u32, u32)) -> Vec<(f32, f32, f64)> {
    let duration = 1500.0;
    let o = camera.origin(screen);
    let (sw, sh) = (screen.0 as f64 / camera.zoom, screen.1 as f64 / camera.zoom);
    (0..=duration as usize)
        .map(|ms| {
            let t = ms as f64 / duration;
            let x = o[0] + sw * (0.1 + 0.8 * t);
            let y = o[1] + sh * (0.5 + 0.3 * (t * 3.0 * std::f64::consts::TAU).sin());
            (x as f32, y as f32, ms as f64)
        })
        .collect()
}

pub const BRUSH_SIZE: f32 = 300.0;

/// Add a transparent layer on top for strokes; returns its id.
pub fn add_paint_layer(doc: &mut Document) -> NodeId {
    emulsion_core::Command::AddNode {
        node: Box::new(emulsion_core::Node::raster(
            0,
            "Paint",
            Arc::new(Raster::transparent(doc.width, doc.height)),
            Default::default(),
        )),
        slot: emulsion_core::command::Slot::TOP,
    }
    .apply(doc)
    .expect("add paint layer")
    .expect("paint id")
}

// ── Runs ──

#[derive(Default)]
pub struct Report {
    pub scenario: String,
    pub mode: String,
    pub frames: Series,
    pub phases: Vec<(String, Series)>,
    pub latency: Series,
    pub upload_bytes: Series,
    /// CPU time to record and submit a frame (excludes waiting on the GPU).
    pub cpu: Series,
    /// CPU time in Vello scene building and `render_to_texture`.
    pub vello: Series,
    /// Named per-frame breakdowns; names carry their unit.
    pub stages: Vec<(String, Series)>,
    pub notes: Vec<String>,
}

impl Report {
    fn stage(&mut self, name: &str, value: f64) {
        match self.stages.iter_mut().find(|(n, _)| n == name) {
            Some((_, s)) => s.push(value),
            None => {
                let mut s = Series::default();
                s.push(value);
                self.stages.push((name.into(), s));
            }
        }
    }
}

impl Report {
    pub fn print(&self) {
        println!(
            "\n### {} ({})\n\n| Measure | p50 / p99 | mean |\n|---|---|---|",
            self.scenario, self.mode
        );
        println!(
            "| frame time ({} frames) | {} | {:.2} ms |",
            self.frames.len(),
            self.frames.summary(),
            self.frames.mean()
        );
        for (name, s) in self.phases.iter().filter(|(_, s)| s.len() > 0) {
            println!("| — {name} | {} | {:.2} ms |", s.summary(), s.mean());
        }
        if self.cpu.len() > 0 {
            println!(
                "| CPU record + submit | {} | {:.2} ms |",
                self.cpu.summary(),
                self.cpu.mean()
            );
        }
        if self.vello.len() > 0 && self.vello.mean() > 0.0 {
            println!(
                "| — of which Vello encode + render call | {} | {:.2} ms |",
                self.vello.summary(),
                self.vello.mean()
            );
        }
        if self.latency.len() > 0 {
            println!(
                "| input-to-pixel ({} events) | {} | {:.2} ms |",
                self.latency.len(),
                self.latency.summary(),
                self.latency.mean()
            );
        }
        for (name, s) in &self.stages {
            println!(
                "| {name} | {:.2} / {:.2} | {:.2} |",
                s.quantile(0.5),
                s.quantile(0.99),
                s.mean()
            );
        }
        if self.upload_bytes.len() > 0 {
            println!(
                "| upload per frame | {:.2} / {:.2} MiB | {:.2} MiB |",
                self.upload_bytes.quantile(0.5) / 1048576.0,
                self.upload_bytes.quantile(0.99) / 1048576.0,
                self.upload_bytes.mean() / 1048576.0
            );
        }
        for n in &self.notes {
            println!("\n- {n}");
        }
    }

    pub fn json(&self) -> serde_json::Value {
        let s = |s: &Series| {
            serde_json::json!({
                "n": s.len(),
                "p50": s.quantile(0.5),
                "p99": s.quantile(0.99),
                "mean": s.mean(),
            })
        };
        serde_json::json!({
            "scenario": self.scenario,
            "mode": self.mode,
            "frames": s(&self.frames),
            "phases": self.phases.iter().filter(|(_, v)| v.len() > 0).map(|(n, v)| serde_json::json!({"name": n, "stats": s(v)})).collect::<Vec<_>>(),
            "cpu": s(&self.cpu),
            "vello": s(&self.vello),
            "stages": self.stages.iter().map(|(n, v)| serde_json::json!({"name": n, "stats": s(v)})).collect::<Vec<_>>(),
            "latency": s(&self.latency),
            "upload_bytes": s(&self.upload_bytes),
            "notes": self.notes,
        })
    }
}

/// Per-frame scenario state for the GPU engine, usable from a headless loop
/// or the windowed event loop.
pub struct Script {
    pub kind: Kind,
    frame: usize,
    total: usize,
    points: Vec<(f32, f32, f64)>,
    fed: usize,
    stroke_start: Option<Instant>,
    cpu_stroke: Option<CpuStroke>,
    gpu_stroke: Option<GpuStroke>,
    /// Pending stroke-end readback, its dab count, and whether it is mapped.
    readback: Option<(Readback, usize, bool)>,
    /// Scheduled time (ms since stroke start) of events in the pending frame.
    in_flight: Vec<f64>,
    pub report: Report,
    paint: Option<usize>,
    edit_nodes: Vec<NodeId>,
    finished: bool,
    phase_of_frame: Vec<usize>,
}

impl Script {
    pub fn new(kind: Kind, engine: &mut Engine, frames: Option<usize>) -> Self {
        let doc = (engine.canvas.width, engine.canvas.height);
        let screen = engine.screen;
        let mut points = Vec::new();
        let total = match kind {
            Kind::Navigate => navigate_frames(),
            Kind::BrushA | Kind::BrushB => {
                engine.camera = Camera {
                    center: [doc.0 as f64 / 2.0, doc.1 as f64 / 2.0],
                    zoom: 1.0,
                };
                points = stroke_points(&engine.camera, screen);
                usize::MAX
            }
            Kind::VectorEdit => {
                engine.camera = Camera::fit(doc, screen);
                300
            }
        };
        let edit_nodes = engine.vectors.objects.iter().map(|o| o.node).collect();
        Self {
            kind,
            frame: 0,
            total: frames.unwrap_or(total),
            points,
            fed: 0,
            stroke_start: None,
            cpu_stroke: None,
            gpu_stroke: None,
            readback: None,
            in_flight: Vec::new(),
            report: Report {
                scenario: kind.label().into(),
                phases: PHASES
                    .iter()
                    .map(|(n, _)| (n.to_string(), Series::default()))
                    .collect(),
                ..Default::default()
            },
            paint: engine.canvas.paint,
            edit_nodes,
            finished: false,
            phase_of_frame: Vec::new(),
        }
    }

    pub fn done(&self) -> bool {
        self.finished
    }

    /// Advance the workload for the next frame (camera, input, edits).
    pub fn before_frame(&mut self, engine: &mut Engine) -> anyhow::Result<()> {
        let _span = tracing::info_span!("input").entered();
        match self.kind {
            Kind::Navigate => {
                let (phase, camera) = navigate_camera(
                    self.frame,
                    (engine.canvas.width, engine.canvas.height),
                    engine.screen,
                );
                engine.camera = camera;
                self.phase_of_frame.push(phase);
                if self.frame + 1 >= self.total {
                    self.finished = true;
                }
            }
            Kind::VectorEdit => {
                if !self.edit_nodes.is_empty() {
                    let node = self.edit_nodes[self.frame * 7919 % self.edit_nodes.len()];
                    let d = if self.frame.is_multiple_of(2) {
                        3.0
                    } else {
                        -3.0
                    };
                    engine.vectors.edit(node, |kind| match kind {
                        crate::canvas::VectorKind::Path { path, .. } => {
                            let mut p = (**path).clone();
                            p.translate(d, -d);
                            *path = Arc::new(p);
                        }
                        crate::canvas::VectorKind::Text { spec } => {
                            let mut s = (**spec).clone();
                            s.x += d as f32;
                            *spec = Arc::new(s);
                        }
                    });
                }
                if self.frame + 1 >= self.total {
                    self.finished = true;
                }
            }
            Kind::BrushA | Kind::BrushB => {
                let source = self
                    .paint
                    .ok_or_else(|| anyhow::anyhow!("no paint layer"))?;
                let start = *self.stroke_start.get_or_insert_with(Instant::now);
                let now = start.elapsed().as_secs_f64() * 1e3;
                let brush = test_brush(BRUSH_SIZE);
                if self.kind == Kind::BrushA && self.cpu_stroke.is_none() {
                    self.cpu_stroke = Some(CpuStroke::begin(&engine.canvas, source, brush));
                }
                if self.kind == Kind::BrushB && self.gpu_stroke.is_none() && self.readback.is_none()
                {
                    self.gpu_stroke = Some(GpuStroke::begin(source, brush));
                }
                self.in_flight.clear();
                while self.fed < self.points.len() && self.points[self.fed].2 <= now {
                    let (x, y, t) = self.points[self.fed];
                    if let Some(s) = &mut self.cpu_stroke {
                        s.point(x, y, t);
                    }
                    if let Some(s) = &mut self.gpu_stroke {
                        s.point(x, y);
                    }
                    self.in_flight.push(t);
                    self.fed += 1;
                }
                let last = self.fed == self.points.len();
                if let Some(s) = &mut self.cpu_stroke {
                    if last {
                        s.finish();
                    }
                    let f = s.render(&mut engine.canvas, &mut engine.atlas, &engine.gpu.queue)?;
                    self.report
                        .stage("Stroke::render, CPU stamp + composite (ms)", f.render_ms);
                    self.report
                        .stage("dirty-tile upload, write_texture (ms)", f.upload_ms);
                    self.report.stage("tiles uploaded", f.tiles as f64);
                    if last {
                        self.cpu_stroke = None;
                        self.finished = true;
                    }
                }
                if self.gpu_stroke.is_some() {
                    let (brush_pipes, canvas, atlas, encoder) = engine.brush_parts();
                    let f = self.gpu_stroke.as_mut().unwrap().render(
                        brush_pipes,
                        canvas,
                        atlas,
                        encoder,
                    )?;
                    self.report.stage("dab draws recorded (ms)", f.record_ms);
                    self.report.stage("dabs", f.dabs as f64);
                    self.report.stage("tiles painted", f.tiles as f64);
                    self.report.stage("dab quads", f.quads as f64);
                    if last {
                        let stroke = self.gpu_stroke.take().unwrap();
                        let dabs = stroke.dabs;
                        let (_, _, atlas, encoder) = engine.brush_parts();
                        let readback = stroke.finish(atlas, encoder);
                        self.readback = Some((readback, dabs, false));
                    }
                }
            }
        }
        Ok(())
    }

    /// Record a finished frame. `done_ms` is when the GPU finished it,
    /// relative to `frame_start`.
    pub fn after_frame(
        &mut self,
        engine: &mut Engine,
        frame_ms: f64,
        gpu_done: Instant,
        times: &FrameTimes,
    ) -> anyhow::Result<()> {
        self.report.frames.push(frame_ms);
        self.report.upload_bytes.push(times.uploaded_bytes as f64);
        self.report.cpu.push(times.cpu_ms);
        self.report
            .vello
            .push(times.vector_encode_ms + times.vector_render_ms);
        if let Some(&phase) = self.phase_of_frame.last()
            && phase < self.report.phases.len()
        {
            self.report.phases[phase].1.push(frame_ms);
        }
        if let Some(start) = self.stroke_start {
            let done = gpu_done.duration_since(start).as_secs_f64() * 1e3;
            for t in self.in_flight.drain(..) {
                self.report.latency.push(done - t);
            }
        }
        if let Some((readback, _, mapped)) = &mut self.readback
            && !*mapped
        {
            // The copy was submitted with this frame; request the mapping.
            readback.map();
            *mapped = true;
        }
        let _ = engine.gpu.device.poll(wgpu::PollType::Poll);
        if self.readback.as_ref().is_some_and(|(r, _, _)| r.is_ready()) {
            let (readback, dabs, _) = self.readback.take().unwrap();
            let ms = readback.started.elapsed().as_secs_f64() * 1e3;
            let bytes = readback.bytes;
            let source = self.paint.unwrap();
            let raster =
                readback.complete(&engine.gpu, &mut engine.canvas, &mut engine.atlas, source)?;
            self.report.notes.push(format!(
                "stroke end → CPU raster: {ms:.1} ms async readback of {:.1} MiB ({} tiles, {dabs} dabs)",
                bytes as f64 / 1048576.0,
                bytes / crate::atlas::TILE_BYTES,
            ));
            let reference = cpu_reference_stroke(
                &engine.canvas.sources[source].raster,
                &self.points,
                BRUSH_SIZE,
                raster.width(),
                raster.height(),
            );
            self.report.notes.push(stroke_diff(&raster, &reference));
            self.finished = true;
        }
        self.frame += 1;
        Ok(())
    }
}

/// The CPU Stroke's result for the same input on a transparent layer.
fn cpu_reference_stroke(
    _current: &Raster,
    points: &[(f32, f32, f64)],
    size: f32,
    w: u32,
    h: u32,
) -> Raster {
    let base = Arc::new(Raster::transparent(w, h));
    let mut stroke = Stroke::new(
        base.clone(),
        test_brush(size),
        Ink::Color(crate::brush::INK),
        None,
    );
    for &(x, y, t) in points {
        stroke.point_at(x, y, None, Some(t));
    }
    stroke.finish();
    stroke.render(&base).0
}

fn stroke_diff(gpu: &Raster, cpu: &Raster) -> String {
    let (w, h) = (gpu.width(), gpu.height());
    let mut max = 0u16;
    let mut sum = 0u64;
    let mut n = 0u64;
    let mut max_code = 0u8;
    let (tx, ty) = tiles_at(w, h, 0);
    for y in 0..ty {
        for x in 0..tx {
            let c = TileCoord::new(x, y);
            let (a, b) = (gpu.base_tile(c), cpu.base_tile(c));
            if a.is_none() && b.is_none() {
                continue;
            }
            let zero = vec![[0u16; 4]; (TILE * TILE) as usize];
            let a: &[[u16; 4]] = a.map_or(&zero, |t| t);
            let b: &[[u16; 4]] = b.map_or(&zero, |t| t);
            for (p, q) in a.iter().zip(b) {
                for i in 0..4 {
                    let d = p[i].abs_diff(q[i]);
                    max = max.max(d);
                    sum += d as u64;
                }
                let (pf, qf) = (
                    emulsion_raster::color::px_to_f(*p),
                    emulsion_raster::color::px_to_f(*q),
                );
                for i in 0..3 {
                    let e =
                        |v: [f32; 4]| emulsion_raster::color::linear_to_srgb8(v[i] + (1.0 - v[3]));
                    max_code = max_code.max(e(pf).abs_diff(e(qf)));
                }
                n += 4;
            }
        }
    }
    format!(
        "GPU dabs vs CPU Stroke, same input: max {max}/65535, mean {:.3}/65535 per channel, max {max_code} 8-bit code over white",
        sum as f64 / n.max(1) as f64
    )
}

// ── CPU baseline: the GPUI canvas's raster work ──

/// Tiles covering the view at `level`, in level-tile coordinates.
fn visible_tiles(camera: &Camera, doc: (u32, u32), screen: (u32, u32)) -> (u32, Vec<TileCoord>) {
    let level = camera.level();
    let v = camera.visible(screen);
    let span = (TILE << level) as f64;
    let (tx, ty) = tiles_at(doc.0, doc.1, level);
    let x0 = (v[0] / span).floor().max(0.0) as i32;
    let y0 = (v[1] / span).floor().max(0.0) as i32;
    let x1 = ((v[2] / span).ceil() as i32).min(tx);
    let y1 = ((v[3] / span).ceil() as i32).min(ty);
    let mut out = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            out.push(TileCoord::new(x, y));
        }
    }
    (level, out)
}

/// A tile cache like the GPUI canvas's: missing tiles are composited on the
/// CPU and encoded to BGRA8. Presentation and atlas upload are not included.
struct TileCache {
    entries: HashSet<(u32, TileCoord)>,
    order: VecDeque<(u32, TileCoord)>,
    cap: usize,
}

impl TileCache {
    fn new() -> Self {
        Self {
            entries: HashSet::new(),
            order: VecDeque::new(),
            cap: 480,
        }
    }

    fn fill(
        &mut self,
        tree: &emulsion_raster::CompositeTree,
        level: u32,
        tiles: &[TileCoord],
        force: &HashSet<TileCoord>,
    ) -> usize {
        let missing: Vec<TileCoord> = tiles
            .iter()
            .copied()
            .filter(|c| force.contains(c) || !self.entries.contains(&(level, *c)))
            .collect();
        let (lw, lh) = emulsion_raster::composite::level_size(tree.width, tree.height, level);
        missing.par_iter().for_each(|c| {
            let tile = render_tile(tree, level, *c);
            let bgra = tile_to_bgra8(
                &tile,
                (c.x as i64 * TILE as i64, c.y as i64 * TILE as i64),
                (lw, lh),
                8,
                205,
                155,
            );
            std::hint::black_box(bgra);
        });
        for c in &missing {
            if self.entries.insert((level, *c)) {
                self.order.push_back((level, *c));
            }
        }
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
        missing.len()
    }
}

pub fn baseline(
    kind: Kind,
    doc: &mut Document,
    paint: Option<NodeId>,
    screen: (u32, u32),
) -> anyhow::Result<Report> {
    let size = (doc.width, doc.height);
    let mut report = Report {
        scenario: kind.label().into(),
        mode: "CPU baseline (GPUI raster work, no present)".into(),
        phases: PHASES
            .iter()
            .map(|(n, _)| (n.to_string(), Series::default()))
            .collect(),
        ..Default::default()
    };
    let mut cache = TileCache::new();
    let mut tiles_rendered = Series::default();
    match kind {
        Kind::Navigate => {
            let tree = doc.composite_tree();
            for frame in 0..navigate_frames() {
                let (phase, camera) = navigate_camera(frame, size, screen);
                let t = Instant::now();
                let (level, tiles) = visible_tiles(&camera, size, screen);
                let n = cache.fill(&tree, level, &tiles, &HashSet::new());
                let ms = t.elapsed().as_secs_f64() * 1e3;
                report.frames.push(ms);
                report.phases[phase].1.push(ms);
                tiles_rendered.push(n as f64);
            }
        }
        Kind::BrushA | Kind::BrushB => {
            let paint = paint.ok_or_else(|| anyhow::anyhow!("no paint layer"))?;
            let camera = Camera {
                center: [size.0 as f64 / 2.0, size.1 as f64 / 2.0],
                zoom: 1.0,
            };
            let points = stroke_points(&camera, screen);
            let base = match &doc.node(paint).unwrap().kind {
                NodeKind::Raster { raster, .. } => raster.clone(),
                _ => anyhow::bail!("paint node is not raster"),
            };
            let (level, view_tiles) = visible_tiles(&camera, size, screen);
            let tree = doc.composite_tree();
            cache.fill(&tree, level, &view_tiles, &HashSet::new());
            let mut stroke = Stroke::new(
                base.clone(),
                test_brush(BRUSH_SIZE),
                Ink::Color(crate::brush::INK),
                None,
            );
            let mut current = base;
            let start = Instant::now();
            let mut fed = 0;
            while fed < points.len() {
                // Idle until the next input event, as an event loop would,
                // so only frames with work are recorded.
                let wait = points[fed].2 - start.elapsed().as_secs_f64() * 1e3;
                if wait > 0.0 {
                    std::thread::sleep(std::time::Duration::from_secs_f64(wait / 1e3));
                }
                let t = Instant::now();
                let now = start.elapsed().as_secs_f64() * 1e3;
                let mut events = Vec::new();
                while fed < points.len() && points[fed].2 <= now {
                    stroke.point_at(points[fed].0, points[fed].1, None, Some(points[fed].2));
                    events.push(points[fed].2);
                    fed += 1;
                }
                if fed == points.len() {
                    stroke.finish();
                }
                let (next, dirty) = stroke.render(&current);
                current = Arc::new(next);
                if let Some(NodeKind::Raster { raster, .. }) =
                    doc.node_mut(paint).map(|n| &mut n.kind)
                {
                    *raster = current.clone();
                }
                let tree = doc.composite_tree();
                let force: HashSet<TileCoord> = tiles_in(dirty).into_iter().collect();
                let n = cache.fill(&tree, level, &view_tiles, &force);
                tiles_rendered.push(n as f64);
                report.frames.push(t.elapsed().as_secs_f64() * 1e3);
                let done = start.elapsed().as_secs_f64() * 1e3;
                for e in events {
                    report.latency.push(done - e);
                }
            }
        }
        Kind::VectorEdit => {
            let camera = Camera::fit(size, screen);
            let (level, view_tiles) = visible_tiles(&camera, size, screen);
            let tree = doc.composite_tree();
            cache.fill(&tree, level, &view_tiles, &HashSet::new());
            let ids: Vec<NodeId> = doc
                .nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
                .map(|n| n.id)
                .collect();
            for frame in 0..300 {
                let id = ids[frame * 7919 % ids.len()];
                let d = if frame.is_multiple_of(2) { 3.0 } else { -3.0 };
                let t = Instant::now();
                let (w, h) = size;
                let node = doc.node_mut(id).unwrap();
                let mut dirty = IRect::default();
                if let NodeKind::Path { path, style, cache } = &mut node.kind {
                    dirty = path.bounds(style);
                    let mut p = (**path).clone();
                    p.translate(d, -d);
                    dirty = dirty.union(&p.bounds(style));
                    *cache = Arc::new(p.rasterize(style, w, h));
                    *path = Arc::new(p);
                }
                let tree = doc.composite_tree();
                let span = (TILE << level) as i32;
                let force: HashSet<TileCoord> = tiles_in(IRect::new(
                    dirty.x.div_euclid(span) * TILE as i32,
                    dirty.y.div_euclid(span) * TILE as i32,
                    (dirty.w / span + 2) * TILE as i32,
                    (dirty.h / span + 2) * TILE as i32,
                ))
                .into_iter()
                .filter(|c| view_tiles.contains(c))
                .collect();
                let n = cache.fill(&tree, level, &view_tiles, &force);
                tiles_rendered.push(n as f64);
                report.frames.push(t.elapsed().as_secs_f64() * 1e3);
            }
        }
    }
    report.phases.retain(|(_, s)| s.len() > 0);
    report.notes.push(format!(
        "tiles composited per frame: p50 {:.0}, p99 {:.0}, mean {:.1}",
        tiles_rendered.quantile(0.5),
        tiles_rendered.quantile(0.99),
        tiles_rendered.mean()
    ));
    Ok(report)
}
