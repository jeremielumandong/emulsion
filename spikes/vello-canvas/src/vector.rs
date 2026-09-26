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
use emulsion_core::text::{Align, TextSpec};
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
}

pub struct Fonts {
    system: cosmic_text::FontSystem,
    data: HashMap<cosmic_text::fontdb::ID, FontData>,
}

pub struct Shaped {
    pub runs: Vec<GlyphRun>,
    /// Layout-space ink box estimate (x0, y0, x1, y1).
    pub bounds: [f32; 4],
}

impl Fonts {
    pub fn new() -> Self {
        Self {
            system: cosmic_text::FontSystem::new(),
            data: HashMap::new(),
        }
    }

    fn font(&mut self, id: cosmic_text::fontdb::ID) -> Option<FontData> {
        if let Some(font) = self.data.get(&id) {
            return Some(font.clone());
        }
        let index = self.system.db().face(id)?.index;
        let bytes = self
            .system
            .get_font(id, cosmic_text::Weight::NORMAL)?
            .data()
            .to_vec();
        let font = FontData::new(Blob::new(Arc::new(bytes)), index);
        self.data.insert(id, font.clone());
        Some(font)
    }

    /// Shape a single-style text spec the way `emulsion-core` does.
    pub fn shape(&mut self, spec: &TextSpec) -> Shaped {
        use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight, Wrap};
        let metrics = Metrics::new(spec.size, spec.size * spec.line_height);
        let mut buffer = Buffer::new(&mut self.system, metrics);
        {
            let mut b = buffer.borrow_with(&mut self.system);
            b.set_size(spec.width, None);
            b.set_wrap(if spec.width.is_some() {
                Wrap::WordOrGlyph
            } else {
                Wrap::None
            });
            let font = spec.font.trim();
            let mut attrs = Attrs::new()
                .family(if font.is_empty() {
                    Family::SansSerif
                } else {
                    Family::Name(font)
                })
                .metrics(metrics);
            if spec.bold {
                attrs = attrs.weight(Weight::BOLD);
            }
            if spec.italic {
                attrs = attrs.style(Style::Italic);
            }
            if spec.letter_spacing != 0.0 {
                attrs = attrs.letter_spacing(spec.letter_spacing);
            }
            b.set_rich_text(
                [(spec.text.as_str(), attrs.clone())],
                &attrs,
                Shaping::Advanced,
                Some(match spec.align {
                    Align::Left => cosmic_text::Align::Left,
                    Align::Center => cosmic_text::Align::Center,
                    Align::Right => cosmic_text::Align::Right,
                    Align::Justify => cosmic_text::Align::Justified,
                }),
            );
            b.shape_until_scroll(true);
        }
        let mut runs: Vec<GlyphRun> = Vec::new();
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut placed = Vec::new();
        for run in buffer.layout_runs() {
            bounds[1] = bounds[1].min(run.line_top);
            bounds[3] = bounds[3].max(run.line_top + run.line_height);
            for g in run.glyphs {
                bounds[0] = bounds[0].min(g.x);
                bounds[2] = bounds[2].max(g.x + g.w);
                placed.push((
                    g.font_id,
                    g.font_size,
                    Glyph {
                        id: g.glyph_id as u32,
                        x: g.x + g.font_size * g.x_offset,
                        y: run.line_y + g.y - g.font_size * g.y_offset,
                    },
                ));
            }
        }
        for (id, size, glyph) in placed {
            let Some(font) = self.font(id) else { continue };
            match runs.last_mut() {
                Some(r) if r.font.data.id() == font.data.id() && r.size == size => {
                    r.glyphs.push(glyph)
                }
                _ => runs.push(GlyphRun {
                    font,
                    size,
                    glyphs: vec![glyph],
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
    by_node: HashMap<NodeId, usize>,
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
                layer.by_node.insert(*node, index);
                layer.encode(index);
                entries.push(entry(index, &layer.objects[index]));
            }
            layer.runs.push(Run {
                tree: RTree::bulk_load(entries),
            });
        }
        Ok(layer)
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
                let color = space.color(spec.color);
                for run in &shaped.runs {
                    object
                        .fragment
                        .draw_glyphs(&run.font)
                        .font_size(run.size)
                        .transform(transform)
                        .brush(color)
                        .draw(Fill::NonZero, run.glyphs.iter().copied());
                }
                let b = shaped.bounds;
                let rect = transform.transform_rect_bbox(Rect::new(
                    b[0] as f64,
                    b[1] as f64,
                    b[2] as f64,
                    b[3] as f64,
                ));
                object.bounds = [rect.x0 - 2.0, rect.y0 - 2.0, rect.x1 + 2.0, rect.y1 + 2.0];
            }
        }
    }

    /// Change one object and re-encode it alone.
    pub fn edit(&mut self, node: NodeId, change: impl FnOnce(&mut VectorKind)) -> bool {
        let Some(&index) = self.by_node.get(&node) else {
            return false;
        };
        let before = entry(index, &self.objects[index]);
        change(&mut self.objects[index].kind);
        let t = Instant::now();
        self.encode(index);
        self.stats.encoded += 1;
        self.stats.encode_ms += t.elapsed().as_secs_f64() * 1e3;
        let after = entry(index, &self.objects[index]);
        for run in &mut self.runs {
            if run.tree.remove(&before).is_some() {
                run.tree.insert(after);
                break;
            }
        }
        true
    }

    fn ensure_target(&mut self, screen: (u32, u32)) {
        let layers = self.runs.len().max(1) as u32;
        if self
            .target
            .as_ref()
            .is_some_and(|t| t.size == screen && t.layers.len() as u32 == layers)
        {
            return;
        }
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
    }

    /// Render every run for this view. Returns the number of runs drawn.
    pub fn render(
        &mut self,
        affine: [f64; 6],
        visible: [f64; 4],
        screen: (u32, u32),
    ) -> anyhow::Result<usize> {
        let _span = tracing::info_span!("encode").entered();
        self.ensure_target(screen);
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

    pub fn view(&mut self, screen: (u32, u32)) -> &wgpu::TextureView {
        self.ensure_target(screen);
        &self.target.as_ref().expect("target").array
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
