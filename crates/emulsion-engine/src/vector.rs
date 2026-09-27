//! The Vello vector layer.
//!
//! Each editable path or text node is encoded once into its own Vello scene
//! fragment, in document coordinates. A frame appends the fragments an R-tree
//! reports as visible, under the view transform, and Vello renders each run
//! of consecutive vector nodes into its own screen-sized target. Editing an
//! object re-encodes that object alone.
//!
//! Vello blends and anti-aliases in the colour space of the values it is
//! given and writes straight-alpha Rgba8Unorm. `VectorSpace::Srgb` passes
//! sRGB-encoded colour (what Vello expects); `Linear` passes linear values so
//! overlaps blend like Emulsion's compositor, at 8-bit linear precision.

use crate::canvas::{Canvas, VectorItem, VectorKind};
use crate::gpu::Gpu;
use emulsion_core::NodeId;
use emulsion_core::text::TextSpec;
use emulsion_raster::color;
use emulsion_raster::vector::{Path, PathStyle, StrokeCap, StrokeJoin};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use vello::kurbo::{Affine, BezPath, Cap, Join, Rect, Shape, Stroke};
use vello::peniko::{Blob, Color, Fill, FontData};
use vello::{AaConfig, Glyph, RenderParams, Renderer, RendererOptions, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorSpace {
    Srgb,
    Linear,
}

impl VectorSpace {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "srgb" => Some(Self::Srgb),
            "linear" => Some(Self::Linear),
            _ => None,
        }
    }

    fn color(self, rgba: [u8; 4]) -> Color {
        match self {
            Self::Srgb => Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]),
            Self::Linear => Color::new([
                color::srgb_to_linear(rgba[0] as f32 / 255.0),
                color::srgb_to_linear(rgba[1] as f32 / 255.0),
                color::srgb_to_linear(rgba[2] as f32 / 255.0),
                rgba[3] as f32 / 255.0,
            ]),
        }
    }
}

/// Glyphs of one font at one size, in layout coordinates.
pub struct GlyphRun {
    pub font: FontData,
    pub size: f32,
    pub glyphs: Vec<Glyph>,
    pub coords: Vec<vello::NormalizedCoord>,
    pub fake_italic: bool,
    pub color: [u8; 4],
}

pub struct Fonts {
    system: cosmic_text::FontSystem,
    data: HashMap<
        (cosmic_text::fontdb::ID, cosmic_text::Weight),
        (FontData, Vec<vello::NormalizedCoord>),
    >,
}

pub struct Shaped {
    pub runs: Vec<GlyphRun>,
    /// Layout-space ink box estimate (x0, y0, x1, y1).
    pub bounds: [f32; 4],
}

impl Fonts {
    pub fn new() -> Self {
        Self {
            system: emulsion_core::text::font_system(),
            data: HashMap::new(),
        }
    }

    fn font(
        &mut self,
        id: cosmic_text::fontdb::ID,
        weight: cosmic_text::Weight,
    ) -> Option<(FontData, Vec<vello::NormalizedCoord>)> {
        if let Some(font) = self.data.get(&(id, weight)) {
            return Some(font.clone());
        }
        let index = self.system.db().face(id)?.index;
        let source = self.system.get_font(id, weight)?;
        let bytes = source.data().to_vec();
        let face = source.as_swash();
        let tag = u32::from_be_bytes(*b"wght");
        let coords = if let Some(axis) = face.variations().find_by_tag(tag) {
            face.variations()
                .normalized_coords([(
                    tag,
                    (weight.0 as f32).clamp(axis.min_value(), axis.max_value()),
                )])
                .collect()
        } else {
            Vec::new()
        };
        let font = FontData::new(Blob::new(Arc::new(bytes)), index);
        self.data
            .insert((id, weight), (font.clone(), coords.clone()));
        Some((font, coords))
    }

    /// Use the same paragraph layout and character styles as the CPU renderer.
    pub fn shape(&mut self, spec: &TextSpec) -> Shaped {
        let (buffer, styles) = emulsion_core::text::shaped_buffer(spec, &mut self.system);
        let fallback = spec.base_style();
        let mut runs: Vec<GlyphRun> = Vec::new();
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut placed = Vec::new();
        for run in buffer.layout_runs() {
            bounds[1] = bounds[1].min(run.line_top);
            bounds[3] = bounds[3].max(run.line_top + run.line_height);
            for g in run.glyphs {
                let style = g
                    .metadata
                    .checked_sub(1)
                    .and_then(|i| styles.get(i))
                    .unwrap_or_else(|| styles.first().unwrap_or(&fallback));
                bounds[1] = bounds[1].min(run.line_top - style.baseline);
                bounds[3] = bounds[3].max(run.line_top + run.line_height - style.baseline);
                bounds[0] = bounds[0].min(g.x);
                bounds[2] = bounds[2].max(g.x + g.w);
                placed.push((
                    g.font_id,
                    g.font_size,
                    g.font_weight,
                    g.cache_key_flags
                        .contains(cosmic_text::CacheKeyFlags::FAKE_ITALIC),
                    style.color,
                    Glyph {
                        id: g.glyph_id as u32,
                        x: g.x + g.font_size * g.x_offset,
                        y: run.line_y + g.y - g.font_size * g.y_offset - style.baseline,
                    },
                ));
            }
        }
        for (id, size, weight, fake_italic, color, glyph) in placed {
            let Some((font, coords)) = self.font(id, weight) else {
                continue;
            };
            match runs.last_mut() {
                Some(r)
                    if r.font.data.id() == font.data.id()
                        && r.size == size
                        && r.coords == coords
                        && r.fake_italic == fake_italic
                        && r.color == color =>
                {
                    r.glyphs.push(glyph)
                }
                _ => runs.push(GlyphRun {
                    font,
                    size,
                    glyphs: vec![glyph],
                    coords,
                    fake_italic,
                    color,
                }),
            }
        }
        if bounds[0] > bounds[2] {
            bounds = [0.0; 4];
        }
        Shaped { runs, bounds }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

pub fn bez_path(path: &Path) -> BezPath {
    let mut out = BezPath::new();
    for sp in &path.subpaths {
        let n = sp.anchors.len();
        let Some(first) = sp.anchors.first() else {
            continue;
        };
        if sp.anchors.iter().any(|a| {
            [a.p, a.h_in, a.h_out]
                .iter()
                .any(|p| !p.0.is_finite() || !p.1.is_finite())
        }) {
            continue;
        }
        out.move_to(first.p);
        let segments = match n {
            0 | 1 => 0,
            n if sp.closed => n,
            n => n - 1,
        };
        for i in 0..segments {
            let (a, b) = (&sp.anchors[i], &sp.anchors[(i + 1) % n]);
            if a.h_out == a.p && b.h_in == b.p {
                out.line_to(b.p);
            } else {
                out.curve_to(a.h_out, b.h_in, b.p);
            }
        }
        if sp.closed {
            out.close_path();
        }
    }
    out
}

fn stroke_style(style: &PathStyle) -> Stroke {
    let mut stroke = Stroke::new(style.width as f64)
        .with_caps(match style.cap {
            StrokeCap::Butt => Cap::Butt,
            StrokeCap::Round => Cap::Round,
            StrokeCap::Square => Cap::Square,
        })
        .with_join(match style.join {
            StrokeJoin::Miter => Join::Miter,
            StrokeJoin::Round => Join::Round,
            StrokeJoin::Bevel => Join::Bevel,
        })
        .with_miter_limit(style.miter_limit as f64);
    if style.dash_count > 0 {
        let mut dashes: Vec<f64> = style.dash[..style.dash_count as usize]
            .iter()
            .map(|&d| d as f64)
            .collect();
        if dashes.len() % 2 == 1 {
            dashes.extend_from_within(..);
        }
        stroke = stroke.with_dashes(style.dash_offset as f64, dashes);
    }
    stroke
}

pub struct Object {
    pub node: NodeId,
    pub kind: VectorKind,
    fragment: Scene,
    bounds: [f64; 4],
}

type Entry = GeomWithData<Rectangle<[f64; 2]>, usize>;

struct Run {
    tree: RTree<Entry>,
}

struct Target {
    /// Kept alongside its views for the target's lifetime.
    _texture: wgpu::Texture,
    layers: Vec<wgpu::TextureView>,
    array: wgpu::TextureView,
    size: (u32, u32),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VectorStats {
    pub visible: usize,
    pub encoded: usize,
    pub encode_ms: f64,
    pub render_ms: f64,
}

pub struct VectorLayer {
    gpu: Arc<Gpu>,
    renderer: Renderer,
    pub fonts: Fonts,
    pub objects: Vec<Object>,
    by_node: HashMap<NodeId, Vec<usize>>,
    runs: Vec<Run>,
    pub space: VectorSpace,
    target: Option<Target>,
    scene: Scene,
    pub stats: VectorStats,
    hud: Hud,
}

fn texture_2d(gpu: &Gpu, size: (u32, u32), layers: u32, label: &str) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: layers.max(1),
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn layer_view(texture: &wgpu::Texture, layer: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

impl VectorLayer {
    /// Whether this layer draws nothing, so a caller can skip re-encoding.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.runs.is_empty()
    }

    pub fn new(gpu: Arc<Gpu>, canvas: &Canvas, space: VectorSpace) -> anyhow::Result<Self> {
        let renderer = Renderer::new(
            &gpu.device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .map_err(|e| anyhow::anyhow!("vello renderer: {e}"))?;
        let hud = Hud::new(&gpu);
        let mut layer = Self {
            gpu,
            renderer,
            fonts: Fonts::new(),
            objects: Vec::new(),
            by_node: HashMap::new(),
            runs: Vec::new(),
            space,
            target: None,
            scene: Scene::new(),
            stats: VectorStats::default(),
            hud,
        };
        for run in &canvas.runs {
            let mut entries = Vec::new();
            for VectorItem { node, kind } in run {
                let index = layer.objects.len();
                layer.objects.push(Object {
                    node: *node,
                    kind: kind.clone(),
                    fragment: Scene::new(),
                    bounds: [0.0; 4],
                });
                layer.by_node.entry(*node).or_default().push(index);
                layer.encode(index);
                entries.push(entry(index, &layer.objects[index]));
            }
            layer.runs.push(Run {
                tree: RTree::bulk_load(entries),
            });
        }
        Ok(layer)
    }

    /// Adopt a changed canvas, keeping the Vello renderer, fonts and target.
    ///
    /// `Renderer::new` compiles Vello's pipelines and dominates construction,
    /// so an edit must never go through [`Self::new`]. Objects whose geometry
    /// and style are unchanged keep their encoded fragment, so moving one text
    /// box re-encodes one object rather than the whole document.
    pub fn resync(&mut self, canvas: &Canvas) {
        fn same(a: &VectorKind, b: &VectorKind) -> bool {
            match (a, b) {
                (
                    VectorKind::Path {
                        path: pa,
                        style: sa,
                    },
                    VectorKind::Path {
                        path: pb,
                        style: sb,
                    },
                ) => Arc::ptr_eq(pa, pb) && sa == sb,
                (VectorKind::Text { spec: sa }, VectorKind::Text { spec: sb }) => {
                    Arc::ptr_eq(sa, sb)
                }
                _ => false,
            }
        }
        let mut old: HashMap<NodeId, Object> = std::mem::take(&mut self.objects)
            .into_iter()
            .map(|o| (o.node, o))
            .collect();
        self.by_node.clear();
        self.runs.clear();
        for run in &canvas.runs {
            let mut entries = Vec::new();
            for VectorItem { node, kind } in run {
                let index = self.objects.len();
                let reused = old.remove(node).filter(|o| same(&o.kind, kind));
                let fresh = reused.is_none();
                self.objects.push(reused.unwrap_or_else(|| Object {
                    node: *node,
                    kind: kind.clone(),
                    fragment: Scene::new(),
                    bounds: [0.0; 4],
                }));
                self.by_node.entry(*node).or_default().push(index);
                if fresh {
                    let started = Instant::now();
                    self.encode(index);
                    self.stats.encoded += 1;
                    self.stats.encode_ms += started.elapsed().as_secs_f64() * 1e3;
                }
                entries.push(entry(index, &self.objects[index]));
            }
            self.runs.push(Run {
                tree: RTree::bulk_load(entries),
            });
        }
    }

    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// Re-encode one object's fragment in document coordinates.
    fn encode(&mut self, index: usize) {
        let space = self.space;
        let object = &mut self.objects[index];
        object.fragment.reset();
        match &object.kind {
            VectorKind::Path { path, style } => {
                let shape = bez_path(path);
                let mut bounds = shape.bounding_box();
                if let Some(fill) = style.fill {
                    object.fragment.fill(
                        Fill::NonZero,
                        Affine::IDENTITY,
                        space.color(fill),
                        None,
                        &shape,
                    );
                }
                if let Some(stroke) = style.stroke
                    && style.width > 0.0
                {
                    object.fragment.stroke(
                        &stroke_style(style),
                        Affine::IDENTITY,
                        space.color(stroke),
                        None,
                        &shape,
                    );
                    let pad = style.width as f64 / 2.0
                        * if style.join == StrokeJoin::Miter {
                            style.miter_limit.max(1.0) as f64
                        } else {
                            1.5
                        };
                    bounds = bounds.inflate(pad, pad);
                }
                object.bounds = [
                    bounds.x0 - 1.0,
                    bounds.y0 - 1.0,
                    bounds.x1 + 1.0,
                    bounds.y1 + 1.0,
                ];
            }
            VectorKind::Text { spec } => {
                let shaped = self.fonts.shape(spec);
                let t = spec.transform().to_cols_array();
                let transform = Affine::new(t);
                let frame = spec
                    .width
                    .zip(spec.height)
                    .map(|(width, height)| Rect::new(0., 0., f64::from(width), f64::from(height)));
                if let Some(frame) = frame {
                    // Keep glyphs at display resolution while clipping in text
                    // coordinates, including rotated/scaled chart labels.
                    object
                        .fragment
                        .push_clip_layer(Fill::NonZero, transform, &frame);
                }
                for run in &shaped.runs {
                    object
                        .fragment
                        .draw_glyphs(&run.font)
                        .font_size(run.size)
                        .normalized_coords(&run.coords)
                        .glyph_transform(
                            run.fake_italic.then(|| {
                                Affine::new([1., 0., 14_f64.to_radians().tan(), 1., 0., 0.])
                            }),
                        )
                        .transform(transform)
                        .brush(space.color(run.color))
                        .draw(Fill::NonZero, run.glyphs.iter().copied());
                }
                if frame.is_some() {
                    object.fragment.pop_layer();
                }
                let b = shaped.bounds;
                // A frame is a conservative bound even when clipped glyph ink
                // overhangs its font's advance bounds (italic and raised text).
                let rect = transform.transform_rect_bbox(frame.unwrap_or_else(|| {
                    Rect::new(b[0] as f64, b[1] as f64, b[2] as f64, b[3] as f64)
                }));
                object.bounds = [rect.x0 - 2.0, rect.y0 - 2.0, rect.x1 + 2.0, rect.y1 + 2.0];
            }
        }
    }

    /// Change one node, including any separate clipping-shape instances.
    pub fn edit(&mut self, node: NodeId, change: impl FnOnce(&mut VectorKind)) -> bool {
        let Some(indices) = self.by_node.get(&node).cloned() else {
            return false;
        };
        let mut kind = self.objects[indices[0]].kind.clone();
        change(&mut kind);
        for index in indices {
            let before = entry(index, &self.objects[index]);
            self.objects[index].kind = kind.clone();
            let t = Instant::now();
            self.encode(index);
            self.stats.encoded += 1;
            self.stats.encode_ms += t.elapsed().as_secs_f64() * 1e3;
            let after = entry(index, &self.objects[index]);
            for run in &mut self.runs {
                if run.tree.remove(&before).is_some() {
                    run.tree.insert(after);
                }
            }
        }
        true
    }

    fn ensure_target(&mut self, screen: (u32, u32)) -> anyhow::Result<()> {
        let screen = if self.runs.is_empty() { (1, 1) } else { screen };
        let layers = self.runs.len().max(1) as u32;
        if self
            .target
            .as_ref()
            .is_some_and(|t| t.size == screen && t.layers.len() as u32 == layers)
        {
            return Ok(());
        }
        anyhow::ensure!(
            layers <= self.gpu.device.limits().max_texture_array_layers
                && screen.0 <= self.gpu.device.limits().max_texture_dimension_2d
                && screen.1 <= self.gpu.device.limits().max_texture_dimension_2d
                && u64::from(screen.0) * u64::from(screen.1) * u64::from(layers) * 4
                    <= 128 * 1024 * 1024,
            "vector targets exceed the 128 MiB canvas budget"
        );
        let texture = texture_2d(&self.gpu, screen, layers, "vector runs");
        let views = (0..layers).map(|l| layer_view(&texture, l)).collect();
        let array = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        self.target = Some(Target {
            _texture: texture,
            layers: views,
            array,
            size: screen,
        });
        Ok(())
    }

    /// Render every run for this view. Returns the number of runs drawn.
    pub fn render(
        &mut self,
        affine: [f64; 6],
        visible: [f64; 4],
        screen: (u32, u32),
    ) -> anyhow::Result<usize> {
        let _span = tracing::info_span!("encode").entered();
        self.ensure_target(screen)?;
        let view = Affine::new(affine);
        let envelope = AABB::from_corners([visible[0], visible[1]], [visible[2], visible[3]]);
        let mut drawn = 0;
        let mut visible_total = 0;
        for (r, run) in self.runs.iter().enumerate() {
            let t = Instant::now();
            let mut hits: Vec<usize> = run
                .tree
                .locate_in_envelope_intersecting(&envelope)
                .map(|e| e.data)
                .collect();
            hits.sort_unstable();
            visible_total += hits.len();
            self.scene.reset();
            for &i in &hits {
                self.scene.append(&self.objects[i].fragment, Some(view));
            }
            self.stats.encode_ms += t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            let target = self.target.as_ref().expect("target");
            let _render = tracing::info_span!("vello_render", run = r).entered();
            self.renderer
                .render_to_texture(
                    &self.gpu.device,
                    &self.gpu.queue,
                    &self.scene,
                    &target.layers[r],
                    &RenderParams {
                        base_color: Color::TRANSPARENT,
                        width: screen.0,
                        height: screen.1,
                        antialiasing_method: AaConfig::Area,
                    },
                )
                .map_err(|e| anyhow::anyhow!("vello render: {e}"))?;
            self.stats.render_ms += t.elapsed().as_secs_f64() * 1e3;
            drawn += 1;
        }
        self.stats.visible = visible_total;
        Ok(drawn)
    }

    pub fn take_stats(&mut self) -> VectorStats {
        std::mem::take(&mut self.stats)
    }

    pub fn view(&mut self, screen: (u32, u32)) -> anyhow::Result<&wgpu::TextureView> {
        self.ensure_target(screen)?;
        Ok(&self.target.as_ref().expect("target").array)
    }

    pub fn target_bytes(&self) -> u64 {
        self.target.as_ref().map_or(0, |t| {
            t.size.0 as u64 * t.size.1 as u64 * 4 * t.layers.len() as u64
        })
    }

    /// Draw HUD lines if they changed. Returns the HUD view and its size.
    pub fn hud(&mut self, lines: &[String]) -> anyhow::Result<(&wgpu::TextureView, [f32; 2])> {
        self.hud
            .update(&self.gpu, &mut self.renderer, &mut self.fonts, lines)?;
        Ok((&self.hud.view, self.hud.shown))
    }
}

/// R-tree entry for an object; its data is the object index, which is also
/// its stack order within the run.
fn entry(index: usize, object: &Object) -> Entry {
    let b = object.bounds;
    GeomWithData::new(Rectangle::from_corners([b[0], b[1]], [b[2], b[3]]), index)
}

const HUD_SIZE: (u32, u32) = (560, 140);

struct Hud {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    scene: Scene,
    lines: Vec<String>,
    shown: [f32; 2],
}

impl Hud {
    fn new(gpu: &Gpu) -> Self {
        let texture = texture_2d(gpu, HUD_SIZE, 1, "hud");
        let view = texture.create_view(&Default::default());
        Self {
            texture,
            view,
            scene: Scene::new(),
            lines: Vec::new(),
            shown: [0.0; 2],
        }
    }

    fn update(
        &mut self,
        gpu: &Gpu,
        renderer: &mut Renderer,
        fonts: &mut Fonts,
        lines: &[String],
    ) -> anyhow::Result<()> {
        if self.lines == lines {
            return Ok(());
        }
        self.lines = lines.to_vec();
        if lines.is_empty() {
            self.shown = [0.0; 2];
            return Ok(());
        }
        let line_height = 20.0;
        let height = (lines.len() as f64 * line_height + 12.0).min(HUD_SIZE.1 as f64);
        self.scene.reset();
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0, 0, 0, 170),
            None,
            &Rect::new(0.0, 0.0, HUD_SIZE.0 as f64, height).to_path(0.1),
        );
        for (i, line) in lines.iter().enumerate() {
            let spec = TextSpec {
                text: line.clone(),
                font: "DejaVu Sans Mono".into(),
                size: 14.0,
                ..Default::default()
            };
            let shaped = fonts.shape(&spec);
            for run in &shaped.runs {
                self.scene
                    .draw_glyphs(&run.font)
                    .font_size(run.size)
                    .normalized_coords(&run.coords)
                    .glyph_transform(
                        run.fake_italic
                            .then(|| Affine::new([1., 0., 14_f64.to_radians().tan(), 1., 0., 0.])),
                    )
                    .transform(Affine::translate((8.0, 4.0 + i as f64 * line_height)))
                    .brush(Color::from_rgba8(235, 235, 235, 255))
                    .draw(Fill::NonZero, run.glyphs.iter().copied());
            }
        }
        renderer
            .render_to_texture(
                &gpu.device,
                &gpu.queue,
                &self.scene,
                &self.view,
                &RenderParams {
                    base_color: Color::TRANSPARENT,
                    width: HUD_SIZE.0,
                    height: HUD_SIZE.1,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| anyhow::anyhow!("vello hud: {e}"))?;
        let _ = &self.texture;
        self.shown = [HUD_SIZE.0 as f32, height as f32];
        Ok(())
    }
}

#[cfg(test)]
mod font_tests {
    use super::*;

    fn rich_spec() -> TextSpec {
        let mut spec = TextSpec {
            text: "Sharp color\nRaised café".into(),
            font: "Geist".into(),
            size: 42.,
            x: 55.25,
            y: 45.5,
            color: [255, 30, 30, 255],
            width: Some(390.),
            rotation: 7.,
            scale_x: 1.1,
            ..Default::default()
        };
        spec.apply_style(6..11, |style| {
            style.color = [30, 255, 30, 255];
            style.bold = true;
            style.italic = true;
            style.size = 48.;
        });
        spec.apply_style(12..18, |style| {
            style.color = [30, 30, 255, 255];
            style.baseline = 13.5;
            style.letter_spacing = 1.5;
        });
        spec
    }

    #[test]
    fn rich_text_keeps_colors_font_instances_and_baseline_geometry() {
        let spec = rich_spec();
        assert!(crate::canvas::text_supported(&spec));
        let mut fonts = Fonts::new();
        let shaped = fonts.shape(&spec);
        let colors: Vec<_> = shaped.runs.iter().map(|run| run.color).collect();
        assert!(colors.contains(&[255, 30, 30, 255]));
        assert!(colors.contains(&[30, 255, 30, 255]));
        assert!(colors.contains(&[30, 30, 255, 255]));
        assert!(shaped.runs.iter().any(|r| r.size == 48. && r.fake_italic));

        let mut unraised = spec.clone();
        unraised.apply_style(12..18, |style| style.baseline = 0.);
        let normal = fonts.shape(&unraised);
        let blue = |shaped: &Shaped| {
            shaped
                .runs
                .iter()
                .filter(|r| r.color == [30, 30, 255, 255])
                .flat_map(|r| r.glyphs.iter().map(|g| (g.id, g.x, g.y)))
                .collect::<Vec<_>>()
        };
        let raised_glyphs = blue(&shaped);
        let normal_glyphs = blue(&normal);
        assert!(!raised_glyphs.is_empty());
        assert_eq!(raised_glyphs.len(), normal_glyphs.len());
        for (a, b) in raised_glyphs.iter().zip(&normal_glyphs) {
            assert_eq!((a.0, a.1), (b.0, b.1));
            assert!((b.2 - a.2 - 13.5).abs() < 0.001);
        }

        let mut translucent = spec.clone();
        translucent.apply_style(6..11, |style| style.color[3] = 128);
        assert!(!crate::canvas::text_supported(&translucent));
        let mut clipped = spec;
        clipped.height = Some(40.);
        assert!(crate::canvas::text_supported(&clipped));
        clipped.width = None;
        assert!(!crate::canvas::text_supported(&clipped));
    }

    #[test]
    #[ignore = "requires an offscreen wgpu adapter"]
    fn bounded_chart_text_stays_clipped_and_matches_zoomed_glyph_outlines() {
        use crate::{Engine, Offscreen, Output};
        use emulsion_core::{Command, Document, Node, NodeKind, command::Slot};
        let gpu = Gpu::new(wgpu::Instance::default(), None, None).unwrap();
        let spec = TextSpec {
            text: "Chart label\nOutside the cell".into(),
            font: "Geist".into(),
            size: 24.,
            x: 45.25,
            y: 35.5,
            width: Some(110.),
            height: Some(17.),
            rotation: 19.,
            scale_x: 1.15,
            scale_y: 0.9,
            ..Default::default()
        };
        for zoom in [1., 1.5, 3.] {
            let mut doc = Document::new(240, 160);
            Command::AddNode {
                node: Box::new(Node::text(0, "Chart label", spec.clone(), 240, 160)),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
            let size = ((240. * zoom) as u32, (160. * zoom) as u32);
            let mut engine = Engine::new(
                gpu.clone(),
                &doc,
                None,
                VectorSpace::Srgb,
                true,
                false,
                size,
            )
            .unwrap();
            assert_eq!(engine.canvas.vector_count(), 1);
            assert!(engine.canvas.rasterized.is_empty());
            let NodeKind::Text { cache, .. } = &doc.nodes[0].kind else {
                panic!()
            };
            assert!(
                !cache.is_rendered(),
                "paragraph glyphs must bypass source-resolution rasterization"
            );
            engine.camera = crate::Camera {
                center: [120., 80.],
                zoom,
            };
            let output = Offscreen::new(&gpu, size, wgpu::TextureFormat::Rgba32Float);
            engine
                .render(&output.view, output.format, Output::Raw)
                .unwrap();
            let actual = output.read(&gpu).unwrap();

            // Independent unhinted glyph outlines rendered at output resolution,
            // then clipped in the text's original rotated/scaled local frame.
            let mut scaled = spec.clone();
            scaled.height = None;
            scaled.x *= zoom as f32;
            scaled.y *= zoom as f32;
            scaled.scale_x *= zoom as f32;
            scaled.scale_y *= zoom as f32;
            let mut reference = Document::new(size.0, size.1);
            for (path, color) in emulsion_core::text::vector_paths(&scaled).unwrap() {
                Command::AddNode {
                    node: Box::new(Node::path(
                        0,
                        "Glyph",
                        Arc::new(path),
                        PathStyle {
                            fill: Some(color),
                            stroke: None,
                            ..Default::default()
                        },
                        size.0,
                        size.1,
                    )),
                    slot: Slot::TOP,
                }
                .apply(&mut reference)
                .unwrap();
            }
            let expected = emulsion_raster::composite::flatten(&reference.composite_tree(), 0);
            // Optional native-render evidence for visual review. Compare the
            // actual GPU glyphs against the former document-resolution source
            // enlarged with bilinear sampling; no template HTML is rendered.
            if zoom == 3.
                && let Some(directory) = std::env::var_os("EMULSION_CHART_RENDER_DIR")
            {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let old = emulsion_core::text::rasterize(&spec, 240, 160);
                let header = format!("P6\n{} {}\n255\n", size.0, size.1);
                let mut native = header.as_bytes().to_vec();
                let mut enlarged = header.into_bytes();
                for (i, bytes) in actual.as_chunks::<16>().0.iter().enumerate() {
                    let (x, y) = (i as u32 % size.0, i as u32 / size.0);
                    let alpha = f32::from_le_bytes(bytes[12..16].try_into().unwrap());
                    let gray = ((1. - alpha.clamp(0., 1.)) * 255.).round() as u8;
                    native.extend_from_slice(&[gray; 3]);
                    let (sx, sy) = (
                        (f64::from(x) + 0.5) / zoom - 0.5,
                        (f64::from(y) + 0.5) / zoom - 0.5,
                    );
                    let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
                    let at = |x: i32, y: i32| {
                        if x < 0 || y < 0 || x >= 240 || y >= 160 {
                            0.
                        } else {
                            f64::from(old.get(x as u32, y as u32)[3]) / 65535.
                        }
                    };
                    let (fx, fy) = (sx - sx.floor(), sy - sy.floor());
                    let alpha = (at(ix, iy) * (1. - fx) + at(ix + 1, iy) * fx) * (1. - fy)
                        + (at(ix, iy + 1) * (1. - fx) + at(ix + 1, iy + 1) * fx) * fy;
                    let gray = ((1. - alpha.clamp(0., 1.)) * 255.).round() as u8;
                    enlarged.extend_from_slice(&[gray; 3]);
                }
                std::fs::write(directory.join("native-clipped-glyphs-300pct.ppm"), native).unwrap();
                std::fs::write(
                    directory.join("previous-enlarged-text-300pct.ppm"),
                    enlarged,
                )
                .unwrap();
            }
            let inverse = Affine::new(spec.transform().to_cols_array()).inverse();
            let mut intersection = 0;
            let mut union = 0;
            let mut coverage = 0;
            for (i, bytes) in actual.as_chunks::<16>().0.iter().enumerate() {
                let (x, y) = (i as u32 % size.0, i as u32 / size.0);
                let alpha = f32::from_le_bytes(bytes[12..16].try_into().unwrap());
                let local = inverse
                    * vello::kurbo::Point::new(
                        (f64::from(x) + 0.5) / zoom,
                        (f64::from(y) + 0.5) / zoom,
                    );
                if local.x < -2. || local.x > 112. || local.y < -2. || local.y > 19. {
                    assert!(
                        alpha < 0.01,
                        "glyph ink escaped its frame at zoom {zoom}: {local:?}"
                    );
                }
                let inside = (0. ..110.).contains(&local.x) && (0. ..17.).contains(&local.y);
                let a = alpha > 0.5;
                let b = inside && expected.get(x, y)[3] > 32767;
                coverage += usize::from(a);
                intersection += usize::from(a && b);
                union += usize::from(a || b);
            }
            assert!(coverage > 40, "the cropped chart label must remain visible");
            let iou = intersection as f64 / union.max(1) as f64;
            eprintln!("bounded chart label zoom={zoom}: outlined coverage IoU={iou:.4}");
            assert!(
                iou > 0.88,
                "zoom={zoom}: native clipped glyph coverage IoU={iou}"
            );
            assert!(!cache.is_rendered());
        }
    }

    #[test]
    #[ignore = "requires an offscreen wgpu adapter"]
    fn all_chart_kinds_use_vector_labels_without_raster_sources() {
        use crate::{Engine, Offscreen, Output};
        use emulsion_core::{
            Document, NodeKind,
            design_charts::{self, Chart, Kind},
        };
        let gpu = Gpu::new(wgpu::Instance::default(), None, None).unwrap();
        for kind in Kind::ALL {
            let mut editor = emulsion_core::Editor::new(Document::new(640, 440), None);
            design_charts::apply(&mut editor, None, Chart::example(kind), (20., 20.)).unwrap();
            let mut engine = Engine::new(
                gpu.clone(),
                &editor.doc,
                None,
                VectorSpace::Srgb,
                true,
                false,
                (960, 660),
            )
            .unwrap();
            assert!(
                engine.canvas.sources.is_empty(),
                "{} unnecessarily created bitmap sources",
                kind.label()
            );
            assert!(engine.canvas.rasterized.is_empty());
            engine.camera = crate::Camera {
                center: [320., 220.],
                zoom: 1.5,
            };
            let output = Offscreen::new(&gpu, (960, 660), wgpu::TextureFormat::Rgba32Float);
            engine
                .render(&output.view, output.format, Output::Raw)
                .unwrap();
            for node in &editor.doc.nodes {
                if let NodeKind::Text { cache, .. } = &node.kind {
                    assert!(!cache.is_rendered());
                }
            }
        }
    }

    #[test]
    #[ignore = "requires an offscreen wgpu adapter"]
    fn rich_text_gpu_matches_scalable_export_at_fractional_zoom_and_rotation() {
        use crate::{Engine, Offscreen, Output};
        use emulsion_core::{Command, Document, Node, NodeKind, command::Slot};
        let gpu = Gpu::new(wgpu::Instance::default(), None, None).unwrap();
        for zoom in [1., 1.5, 2.] {
            let mut doc = Document::new(640, 320);
            Command::AddNode {
                node: Box::new(Node::text(0, "Rich text", rich_spec(), 640, 320)),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
            let size = ((640. * zoom) as u32, (320. * zoom) as u32);
            let mut engine = Engine::new(
                gpu.clone(),
                &doc,
                None,
                VectorSpace::Srgb,
                true,
                false,
                size,
            )
            .unwrap();
            assert_eq!(engine.canvas.vector_count(), 1);
            assert!(engine.canvas.rasterized.is_empty());
            let NodeKind::Text { cache, .. } = &doc.nodes[0].kind else {
                panic!()
            };
            assert!(
                !cache.is_rendered(),
                "rich text must not acquire a raster preview as its render source"
            );
            engine.camera = crate::Camera {
                center: [320., 160.],
                zoom,
            };
            let output = Offscreen::new(&gpu, size, wgpu::TextureFormat::Rgba32Float);
            engine
                .render(&output.view, output.format, Output::Raw)
                .unwrap();
            let actual = output.read(&gpu).unwrap();
            let mut reference = rich_spec();
            reference.x *= zoom as f32;
            reference.y *= zoom as f32;
            reference.scale_x *= zoom as f32;
            reference.scale_y *= zoom as f32;
            // Rasterize the scalable export outlines at the target resolution.
            // The ordinary CPU text path enlarges hinted source pixels, which
            // intentionally is not the quality target for zoomed GPU text.
            let mut export = Document::new(size.0, size.1);
            for (path, color) in emulsion_core::text::vector_paths(&reference).unwrap() {
                Command::AddNode {
                    node: Box::new(Node::path(
                        0,
                        "Glyph",
                        Arc::new(path),
                        PathStyle {
                            fill: Some(color),
                            stroke: None,
                            ..Default::default()
                        },
                        size.0,
                        size.1,
                    )),
                    slot: Slot::TOP,
                }
                .apply(&mut export)
                .unwrap();
            }
            let expected = emulsion_raster::composite::flatten(&export.composite_tree(), 0);
            // Compare each color independently so losing a style cannot be hidden
            // by a similar total glyph silhouette. Ignore AA intensity differences.
            let mut intersection = [0_usize; 3];
            let mut union = [0_usize; 3];
            for (i, bytes) in actual.as_chunks::<16>().0.iter().enumerate() {
                let pixel: [f32; 4] = std::array::from_fn(|c| {
                    f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap())
                });
                let reference = expected.get(i as u32 % size.0, i as u32 / size.0);
                for c in 0..3 {
                    let a = pixel[3] > 0.5
                        && pixel[c] > pixel[(c + 1) % 3] * 2.
                        && pixel[c] > pixel[(c + 2) % 3] * 2.;
                    let b = reference[3] > 32767
                        && u32::from(reference[c]) > u32::from(reference[(c + 1) % 3]) * 2
                        && u32::from(reference[c]) > u32::from(reference[(c + 2) % 3]) * 2;
                    intersection[c] += usize::from(a && b);
                    union[c] += usize::from(a || b);
                }
            }
            for c in 0..3 {
                let iou = intersection[c] as f64 / union[c].max(1) as f64;
                eprintln!("rich text zoom={zoom} channel={c}: IoU={iou:.4}");
                assert!(
                    iou > 0.90,
                    "zoom={zoom}, color={c}, outline coverage IoU={iou}"
                );
            }
        }
    }
    #[test]
    fn bundled_variable_weights_and_synthetic_italic_reach_vello() {
        let mut fonts = Fonts::new();
        let spec = TextSpec {
            text: "Crisp text".into(),
            font: "Geist".into(),
            size: 48.,
            ..Default::default()
        };
        let regular = fonts.shape(&spec);
        let bold = fonts.shape(&TextSpec {
            bold: true,
            ..spec.clone()
        });
        assert!(!regular.runs.is_empty());
        assert!(!bold.runs.is_empty());
        assert!(!regular.runs[0].coords.is_empty());
        assert_ne!(
            regular.runs[0].coords, bold.runs[0].coords,
            "Vello must draw the same font instance that shaped the text"
        );
        let italic = fonts.shape(&TextSpec {
            italic: true,
            ..spec
        });
        assert!(italic.runs.iter().any(|run| run.fake_italic));
    }

    #[test]
    #[ignore = "requires an offscreen wgpu adapter"]
    fn vello_variable_bold_italic_matches_cpu_glyph_coverage() {
        use crate::{Engine, Offscreen, Output};
        use emulsion_core::{Command, Document, Node, NodeKind, command::Slot};
        let gpu = Gpu::new(wgpu::Instance::default(), None, None).unwrap();
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut doc = Document::new(512, 180);
            Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Background",
                    NodeKind::Fill {
                        rgba: [0, 0, 0, 255],
                    },
                )),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
            let spec = TextSpec {
                text: "Crisp text".into(),
                font: "Geist".into(),
                size: 60.,
                x: 30.,
                y: 40.,
                color: [255; 4],
                bold,
                italic,
                ..Default::default()
            };
            Command::AddNode {
                node: Box::new(Node::text(0, "Text", spec, 512, 180)),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
            let mut engine = Engine::new(
                gpu.clone(),
                &doc,
                None,
                VectorSpace::Srgb,
                true,
                false,
                (512, 180),
            )
            .unwrap();
            engine.camera = crate::Camera {
                center: [256., 90.],
                zoom: 1.,
            };
            let output = Offscreen::new(&gpu, (512, 180), wgpu::TextureFormat::Rgba32Float);
            engine
                .render(&output.view, output.format, Output::Raw)
                .unwrap();
            let actual = output.read(&gpu).unwrap();
            let expected = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
            let mut intersection = 0;
            let mut union = 0;
            for (i, pixel) in actual.as_chunks::<16>().0.iter().enumerate() {
                let a = f32::from_le_bytes(pixel[..4].try_into().unwrap()) > 0.5;
                let b = expected.get(i as u32 % 512, i as u32 / 512)[0] > 32767;
                intersection += usize::from(a && b);
                union += usize::from(a || b);
            }
            let iou = intersection as f64 / union.max(1) as f64;
            eprintln!("Geist bold={bold} italic={italic}: glyph coverage IoU={iou:.4}");
            assert!(
                iou > 0.78,
                "font-instance or italic mismatch: bold={bold}, italic={italic}, IoU={iou}"
            );
        }
    }
}
