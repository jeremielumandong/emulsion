//! Isolated brush-authoring session. Document pixels and tool settings are never drafts.
use super::brush_library_ui::BrushWorkspace;
use super::*;
use emulsion_io::brush_library::{self as store, Catalog};
use emulsion_raster::paint::{Brush, BrushBlend, DualBlend, GrainKind, GrainMode, RenderingMode};
use emulsion_raster::preview::{self, PreviewMode, StrokeSample};
use gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
    resizable::{ResizableState, h_resizable, resizable_panel},
    slider::{Slider, SliderEvent, SliderState},
};
use std::cell::Cell;
use std::collections::HashSet;

const PAD_WIDTH: u32 = 480;
const PAD_HEIGHT: u32 = 360;
fn fitted_pad_bounds(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let scale = (f32::from(bounds.size.width) / PAD_WIDTH as f32)
        .min(f32::from(bounds.size.height) / PAD_HEIGHT as f32)
        .max(0.);
    let fitted = size(px(PAD_WIDTH as f32 * scale), px(PAD_HEIGHT as f32 * scale));
    Bounds::new(
        point(
            bounds.origin.x + (bounds.size.width - fitted.width) / 2.,
            bounds.origin.y + (bounds.size.height - fitted.height) / 2.,
        ),
        fitted,
    )
}
const GROUPS: &[&str] = &[
    "Stroke Path",
    "Stabilization",
    "Taper",
    "Shape",
    "Grain",
    "Rendering",
    "Wet Mix",
    "Color Dynamics",
    "Dynamics",
    "Stylus",
    "Properties",
    "Preview",
    "About",
];

struct Field {
    id: &'static str,
    group: &'static str,
    label: &'static str,
    min: f32,
    max: f32,
    step: f32,
    get: fn(&Brush) -> f32,
    set: fn(&mut Brush, f32),
}
macro_rules! field { ($id:literal,$group:literal,$label:literal,$min:expr,$max:expr,$step:expr,$($path:ident).+) => {
    Field { id:$id,group:$group,label:$label,min:$min,max:$max,step:$step,get:|b|b.$($path).+,set:|b,v|b.$($path).+ = v }
}; }
const FIELDS: &[Field] = &[
    field!("size", "Properties", "Size (pixels)", 1., 1000., 1., size),
    field!(
        "opacity",
        "Rendering",
        "Stroke opacity",
        0.01,
        1.,
        0.01,
        opacity
    ),
    field!("flow", "Rendering", "Dab flow", 0.01, 1., 0.01, flow),
    field!("hardness", "Shape", "Hardness", 0., 1., 0.01, hardness),
    field!("spacing", "Stroke Path", "Spacing", 0.02, 2., 0.01, spacing),
    field!("scatter", "Stroke Path", "Scatter", 0., 1., 0.01, scatter),
    field!(
        "lateral",
        "Stroke Path",
        "Lateral jitter",
        0.,
        1.,
        0.01,
        advanced.path.lateral_jitter
    ),
    field!(
        "linear",
        "Stroke Path",
        "Linear jitter",
        0.,
        1.,
        0.01,
        advanced.path.linear_jitter
    ),
    field!(
        "spacing-jitter",
        "Stroke Path",
        "Spacing jitter",
        0.,
        1.,
        0.01,
        advanced.path.spacing_jitter
    ),
    field!(
        "falloff",
        "Stroke Path",
        "Falloff distance (0 disables)",
        0.,
        4000.,
        1.,
        advanced.path.falloff
    ),
    field!(
        "stabilizer",
        "Stabilization",
        "Smoothing",
        0.,
        1.,
        0.01,
        stabilizer
    ),
    field!(
        "taper-start",
        "Taper",
        "Start length (pixels)",
        0.,
        500.,
        1.,
        taper_start
    ),
    field!(
        "taper-end",
        "Taper",
        "End length (pixels)",
        0.,
        500.,
        1.,
        taper_end
    ),
    field!("roundness", "Shape", "Roundness", 0.05, 1., 0.01, roundness),
    field!("angle", "Shape", "Angle (degrees)", 0., 359., 1., angle),
    field!(
        "rotation-jitter",
        "Shape",
        "Rotation jitter",
        0.,
        1.,
        0.01,
        advanced.shape.rotation_jitter
    ),
    field!(
        "count-jitter",
        "Shape",
        "Count jitter",
        0.,
        1.,
        0.01,
        advanced.shape.count_jitter
    ),
    field!(
        "grain-scale",
        "Grain",
        "Feature size",
        0.1,
        200.,
        0.1,
        grain_scale
    ),
    field!(
        "grain-strength",
        "Grain",
        "Depth",
        0.,
        1.,
        0.01,
        grain_strength
    ),
    field!(
        "grain-multiplier",
        "Grain",
        "Scale multiplier",
        0.01,
        10.,
        0.01,
        advanced.grain.scale
    ),
    field!(
        "grain-rotation",
        "Grain",
        "Rotation (degrees)",
        -180.,
        180.,
        1.,
        advanced.grain.rotation
    ),
    field!(
        "grain-brightness",
        "Grain",
        "Brightness",
        -1.,
        1.,
        0.01,
        advanced.grain.brightness
    ),
    field!(
        "grain-contrast",
        "Grain",
        "Contrast",
        0.,
        4.,
        0.01,
        advanced.grain.contrast
    ),
    field!(
        "grain-offset",
        "Grain",
        "Moving grain phase jitter",
        0.,
        1.,
        0.01,
        advanced.grain.offset_jitter
    ),
    field!(
        "wetness",
        "Wet Mix",
        "Pigment pickup",
        0.,
        1.,
        0.01,
        wetness
    ),
    field!(
        "dilution",
        "Wet Mix",
        "Dilution",
        0.,
        1.,
        0.01,
        advanced.wet.dilution
    ),
    field!(
        "charge",
        "Wet Mix",
        "Pigment charge length (0 unlimited)",
        0.,
        4000.,
        1.,
        advanced.wet.charge
    ),
    field!(
        "pull",
        "Wet Mix",
        "Pigment pull",
        0.,
        1.,
        0.01,
        advanced.wet.pull
    ),
    field!(
        "edge",
        "Rendering",
        "Edge darkening",
        0.,
        1.,
        0.01,
        edge_darken
    ),
    field!("relief", "Rendering", "Relief", 0., 1., 0.01, relief),
    field!(
        "color-jitter",
        "Color Dynamics",
        "Hue and lightness jitter",
        0.,
        1.,
        0.01,
        color_jitter
    ),
    field!(
        "size-jitter",
        "Dynamics",
        "Size jitter",
        0.,
        1.,
        0.01,
        size_jitter
    ),
    field!(
        "opacity-jitter",
        "Dynamics",
        "Opacity jitter",
        0.,
        1.,
        0.01,
        advanced.dynamics.opacity_jitter
    ),
    field!(
        "speed-size",
        "Dynamics",
        "Speed reduces size",
        0.,
        1.,
        0.01,
        advanced.dynamics.speed_size
    ),
    field!(
        "speed-opacity",
        "Dynamics",
        "Speed reduces opacity",
        0.,
        1.,
        0.01,
        advanced.dynamics.speed_opacity
    ),
    field!(
        "mouse-pressure",
        "Dynamics",
        "Mouse speed pressure",
        0.,
        1.,
        0.01,
        speed_thins
    ),
    field!(
        "pressure-size",
        "Stylus",
        "Pressure size",
        0.,
        1.,
        0.01,
        size_pressure
    ),
    field!(
        "pressure-flow",
        "Stylus",
        "Pressure flow",
        0.,
        1.,
        0.01,
        flow_pressure
    ),
    field!(
        "pressure-opacity",
        "Stylus",
        "Pressure opacity",
        0.,
        1.,
        0.01,
        advanced.dynamics.pressure_opacity
    ),
    field!(
        "pressure-exponent",
        "Stylus",
        "Pressure exponent",
        0.1,
        4.,
        0.01,
        pressure_curve
    ),
    field!("tilt", "Stylus", "Tilt shape", 0., 1., 0.01, tilt),
    field!(
        "tilt-opacity",
        "Stylus",
        "Tilt opacity",
        0.,
        1.,
        0.01,
        advanced.dynamics.tilt_opacity
    ),
    field!(
        "opacity-taper",
        "Taper",
        "Opacity taper",
        0.,
        1.,
        0.01,
        advanced.taper.opacity
    ),
    field!(
        "tip-profile",
        "Taper",
        "Tip profile",
        0.1,
        8.,
        0.01,
        advanced.taper.tip_curve
    ),
    field!(
        "stamp-hue",
        "Color Dynamics",
        "Stamp hue",
        0.,
        1.,
        0.01,
        advanced.color.stamp_hue
    ),
    field!(
        "stamp-saturation",
        "Color Dynamics",
        "Stamp saturation",
        0.,
        1.,
        0.01,
        advanced.color.stamp_saturation
    ),
    field!(
        "stamp-lightness",
        "Color Dynamics",
        "Stamp lightness",
        0.,
        1.,
        0.01,
        advanced.color.stamp_lightness
    ),
    field!(
        "stroke-hue",
        "Color Dynamics",
        "Stroke hue",
        0.,
        1.,
        0.01,
        advanced.color.stroke_hue
    ),
    field!(
        "stroke-saturation",
        "Color Dynamics",
        "Stroke saturation",
        0.,
        1.,
        0.01,
        advanced.color.stroke_saturation
    ),
    field!(
        "stroke-lightness",
        "Color Dynamics",
        "Stroke lightness",
        0.,
        1.,
        0.01,
        advanced.color.stroke_lightness
    ),
    field!(
        "pressure-hue",
        "Color Dynamics",
        "Pressure hue",
        -1.,
        1.,
        0.01,
        advanced.color.pressure_hue
    ),
    field!(
        "pressure-saturation",
        "Color Dynamics",
        "Pressure saturation",
        -1.,
        1.,
        0.01,
        advanced.color.pressure_saturation
    ),
    field!(
        "pressure-lightness",
        "Color Dynamics",
        "Pressure lightness",
        -1.,
        1.,
        0.01,
        advanced.color.pressure_lightness
    ),
    field!(
        "min-size",
        "Properties",
        "Minimum size",
        0.,
        4000.,
        1.,
        advanced.properties.min_size
    ),
    field!(
        "max-size",
        "Properties",
        "Maximum size",
        1.,
        4000.,
        1.,
        advanced.properties.max_size
    ),
    field!(
        "min-opacity",
        "Properties",
        "Minimum opacity",
        0.,
        1.,
        0.01,
        advanced.properties.min_opacity
    ),
    field!(
        "max-opacity",
        "Properties",
        "Maximum opacity",
        0.,
        1.,
        0.01,
        advanced.properties.max_opacity
    ),
    field!(
        "position-filter",
        "Stabilization",
        "Position filter",
        0.,
        1.,
        0.01,
        advanced.stabilization.amount
    ),
    field!(
        "pressure-filter",
        "Stabilization",
        "Pressure filter",
        0.,
        1.,
        0.01,
        advanced.stabilization.pressure
    ),
    Field {
        id: "filter-stages",
        group: "Stabilization",
        label: "Filter stages",
        min: 1.,
        max: 16.,
        step: 1.,
        get: |b| f32::from(b.advanced.stabilization.stages),
        set: |b, value| b.advanced.stabilization.stages = value.round() as u8,
    },
];
struct Control {
    slider: Entity<SliderState>,
    input: Entity<InputState>,
}
#[derive(Clone)]
struct PadStroke {
    samples: Vec<StrokeSample>,
    mode: PreviewMode,
}

pub(super) struct BrushStudio {
    parent: WeakEntity<BrushWorkspace>,
    pub(super) draft: Catalog,
    pub(super) id: String,
    brush: Brush,
    editing_secondary: bool,
    is_new: bool,
    focus: FocusHandle,
    group: &'static str,
    name: Entity<InputState>,
    author: Entity<InputState>,
    note: Entity<InputState>,
    controls: Vec<Control>,
    invalid: HashSet<usize>,
    _subscriptions: Vec<Subscription>,
    image: Option<Arc<RenderImage>>,
    strokes: Vec<PadStroke>,
    redo_strokes: Vec<PadStroke>,
    started: Option<Instant>,
    mode: u8,
    ink: [u8; 4],
    dark: bool,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    revision: u64,
    busy: bool,
    saving: bool,
    error: Option<String>,
    source_request: u64,
    source_loading: bool,
    split: Entity<ResizableState>,
}

pub(super) fn raster_image(raster: &Raster) -> Arc<RenderImage> {
    let mut bytes = raster.to_srgba8();
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Arc::new(viewport::bgra_image(raster.width(), raster.height(), bytes))
}

fn decode_source_image(path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    const MAX_BYTES: u64 = 32 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        anyhow::bail!("Image is larger than 32 MiB");
    }
    decode_source_bytes(bytes)
}

fn decode_source_bytes(bytes: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()?
        .into_dimensions()?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        anyhow::bail!("Use a source image no larger than 4096 × 4096");
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(limits);
    let image = reader.decode()?;
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(png.into_inner())
}

#[derive(Clone, Copy)]
enum SourceEdit {
    Invert,
    Rotate,
    MirrorTile,
    Diamond,
    Paper,
}

fn mirror_tile(source: image::GrayImage) -> image::GrayImage {
    let largest = source.width().max(source.height());
    let source = if largest > 2048 {
        let scale = 2048.0 / largest as f32;
        image::imageops::resize(
            &source,
            (source.width() as f32 * scale).round().max(1.0) as u32,
            (source.height() as f32 * scale).round().max(1.0) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        source
    };
    let (width, height) = source.dimensions();
    image::GrayImage::from_fn(width * 2, height * 2, |x, y| {
        *source.get_pixel(
            if x < width { x } else { width * 2 - x - 1 },
            if y < height { y } else { height * 2 - y - 1 },
        )
    })
}

fn edited_source(id: u32, edit: SourceEdit) -> anyhow::Result<image::GrayImage> {
    if matches!(edit, SourceEdit::Diamond) {
        return Ok(image::GrayImage::from_fn(128, 128, |x, y| {
            let distance =
                ((x as f32 + 0.5) / 64.0 - 1.0).abs() + ((y as f32 + 0.5) / 64.0 - 1.0).abs();
            image::Luma([((1.0 - distance).clamp(0.0, 1.0) * 255.0).round() as u8])
        }));
    }
    if matches!(edit, SourceEdit::Paper) {
        let noise = image::GrayImage::from_fn(128, 128, |x, y| {
            let mut hash = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ 0xA3B1_5577;
            hash = (hash ^ (hash >> 13)).wrapping_mul(1_274_126_177);
            image::Luma([40 + ((hash ^ (hash >> 16)) % 216) as u8])
        });
        return Ok(mirror_tile(image::imageops::blur(&noise, 0.65)));
    }
    let source = emulsion_raster::paint::textures::get(id)
        .ok_or_else(|| anyhow::anyhow!("Import or generate a source image first."))?;
    let (width, height) = (source.width(), source.height());
    if width > 4096 || height > 4096 {
        anyhow::bail!("Source image exceeds 4096 × 4096");
    }
    let mut pixels = image::GrayImage::from_fn(width, height, |x, y| {
        image::Luma([(source.sample(
            (x as f32 + 0.5) / width as f32,
            (y as f32 + 0.5) / height as f32,
        ) * 255.0)
            .round() as u8])
    });
    match edit {
        SourceEdit::Invert => {
            image::imageops::invert(&mut pixels);
            Ok(pixels)
        }
        SourceEdit::Rotate => Ok(image::imageops::rotate90(&pixels)),
        SourceEdit::MirrorTile => Ok(mirror_tile(pixels)),
        SourceEdit::Diamond | SourceEdit::Paper => unreachable!(),
    }
}

impl BrushStudio {
    pub fn new(
        parent: WeakEntity<BrushWorkspace>,
        draft: Catalog,
        id: String,
        is_new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let definition = draft.brush(&id).expect("existing draft brush");
        let brush = definition.brush;
        let name = cx.new(|cx| InputState::new(window, cx).default_value(definition.name.clone()));
        let author = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(definition.author.name.clone())
                .placeholder("Author")
        });
        let note = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(definition.note.clone())
                .placeholder("Describe this brush")
        });
        let mut controls = Vec::new();
        let mut subscriptions = Vec::new();
        for (index, field) in FIELDS.iter().enumerate() {
            let value = (field.get)(&brush);
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(field.min)
                    .max(field.max)
                    .step(field.step)
                    .default_value(value)
            });
            let input =
                cx.new(|cx| InputState::new(window, cx).default_value(format!("{value:.2}")));
            subscriptions.push(cx.subscribe_in(
                &slider,
                window,
                move |this, _, event: &SliderEvent, window, cx| {
                    if let SliderEvent::Change(value) = event {
                        this.change(index, value.end(), true, window, cx);
                    }
                },
            ));
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        match input.read(cx).value().parse::<f32>() {
                            Ok(value)
                                if value.is_finite()
                                    && value >= FIELDS[index].min
                                    && value <= FIELDS[index].max =>
                            {
                                this.invalid.remove(&index);
                                this.change(index, value, false, window, cx);
                            }
                            _ => {
                                this.invalid.insert(index);
                                cx.notify();
                            }
                        }
                    }
                },
            ));
            controls.push(Control { slider, input });
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let ink = [35, 65, 105, 255];
        let mut studio = Self {
            parent,
            draft,
            id,
            brush,
            editing_secondary: false,
            is_new,
            focus,
            group: "Stroke Path",
            name,
            author,
            note,
            controls,
            invalid: HashSet::new(),
            _subscriptions: subscriptions,
            image: None,
            strokes: vec![PadStroke {
                samples: preview::sample_stroke(PAD_WIDTH, PAD_HEIGHT),
                mode: PreviewMode::Paint(color::srgba8_to_premul(ink)),
            }],
            started: None,
            redo_strokes: Vec::new(),
            mode: 0,
            ink,
            dark: false,
            bounds: Default::default(),
            revision: 0,
            busy: false,
            saving: false,
            error: None,
            source_request: 0,
            source_loading: false,
            split: cx.new(|_| ResizableState::default()),
        };
        studio.render_preview(cx);
        studio
    }
    fn change(
        &mut self,
        index: usize,
        value: f32,
        from_slider: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = &FIELDS[index];
        if ((field.get)(&self.brush) - value).abs() < 0.00001 {
            cx.notify();
            return;
        }
        let previous = self.brush;
        (field.set)(&mut self.brush, value);
        self.brush = self.brush.sanitized();
        self.invalid.remove(&index);
        for (changed_index, field) in FIELDS.iter().enumerate() {
            let resolved = (field.get)(&self.brush);
            if changed_index != index && resolved == (field.get)(&previous) {
                continue;
            }
            self.controls[changed_index]
                .slider
                .update(cx, |s, cx| s.set_value(resolved, window, cx));
            if changed_index != index || from_slider || (resolved - value).abs() > 0.00001 {
                self.controls[changed_index].input.update(cx, |s, cx| {
                    s.set_value(format!("{resolved:.2}"), window, cx)
                });
                self.invalid.remove(&changed_index);
            }
        }
        self.changed(cx);
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        if let Some(definition) = self.draft.brush_mut(&self.id) {
            if self.editing_secondary {
                definition.secondary = Some(self.brush);
            } else {
                definition.brush = self.brush;
            }
        }
        self.revision += 1;
        self.render_preview(cx);
        cx.notify();
    }

    fn invalidate_source(&mut self) {
        self.source_request = self.source_request.wrapping_add(1);
        self.source_loading = false;
    }

    fn select_component(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Self>) {
        if secondary == self.editing_secondary {
            return;
        }
        if !self.invalid.is_empty() {
            self.error = Some("Enter valid values before switching brush components.".into());
            cx.notify();
            return;
        }
        let definition = self.draft.brush(&self.id).expect("draft brush");
        let Some(brush) = (if secondary {
            definition.secondary
        } else {
            Some(definition.brush)
        }) else {
            return;
        };
        self.invalidate_source();
        self.editing_secondary = secondary;
        self.brush = brush;
        self.sync_controls(window, cx);
    }

    fn add_secondary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.invalid.is_empty() {
            return;
        }
        let definition = self.draft.brush_mut(&self.id).expect("draft brush");
        definition.secondary = Some(definition.brush);
        definition.secondary_shape_asset = definition.shape_asset.clone();
        definition.secondary_grain_asset = definition.grain_asset.clone();
        self.select_component(true, window, cx);
    }

    fn remove_secondary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_source();
        let definition = self.draft.brush_mut(&self.id).expect("draft brush");
        definition.secondary = None;
        definition.secondary_shape_asset = None;
        definition.secondary_grain_asset = None;
        definition.combine_mode = DualBlend::Normal;
        self.editing_secondary = false;
        self.brush = definition.brush;
        self.sync_controls(window, cx);
    }
    fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.invalid.clear();
        self.refresh_controls(window, cx);
    }

    fn refresh_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (index, field) in FIELDS.iter().enumerate() {
            if self.invalid.contains(&index) {
                continue;
            }
            let value = (field.get)(&self.brush);
            self.controls[index]
                .slider
                .update(cx, |s, cx| s.set_value(value, window, cx));
            self.controls[index]
                .input
                .update(cx, |s, cx| s.set_value(format!("{value:.2}"), window, cx));
        }
        self.changed(cx);
    }
    fn mode(&self) -> PreviewMode {
        match self.mode {
            1 => PreviewMode::Smudge,
            2 => PreviewMode::Erase,
            _ => PreviewMode::Paint(color::srgba8_to_premul(self.ink)),
        }
    }
    fn render_preview(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let revision = self.revision;
        let definition = self.draft.brush(&self.id).unwrap();
        let brush = definition.brush;
        let secondary = definition.secondary;
        let combine_mode = definition.combine_mode;
        let strokes = self.strokes.clone();
        let dark = self.dark;
        let mode = self.mode;
        cx.spawn(async move |this, cx| {
            let image = cx
                .background_spawn(async move {
                    let mut pixels = vec![0u8; (PAD_WIDTH * PAD_HEIGHT * 4) as usize];
                    for y in 0..PAD_HEIGHT {
                        for x in 0..PAD_WIDTH {
                            let shade = if dark { 40 } else { 245 };
                            let pixel =
                                if mode != 0 && (100..380).contains(&x) && (70..290).contains(&y) {
                                    if x < 240 {
                                        [185, 70, 50, 255]
                                    } else {
                                        [45, 110, 180, 255]
                                    }
                                } else {
                                    [shade, shade, shade, 255]
                                };
                            let i = ((y * PAD_WIDTH + x) * 4) as usize;
                            pixels[i..i + 4].copy_from_slice(&pixel);
                        }
                    }
                    let mut raster = Raster::from_srgba8(PAD_WIDTH, PAD_HEIGHT, &pixels);
                    for (index, stroke) in strokes.iter().enumerate() {
                        raster = if let Some(secondary) = secondary {
                            preview::render_dual_stroke(
                                Arc::new(raster),
                                brush,
                                secondary,
                                combine_mode,
                                stroke.mode,
                                &stroke.samples,
                                index as u64 + 1,
                            )
                        } else {
                            preview::render_stroke(
                                Arc::new(raster),
                                brush,
                                stroke.mode,
                                &stroke.samples,
                                index as u64 + 1,
                            )
                        };
                    }
                    raster_image(&raster)
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                if this.revision == revision {
                    this.image = Some(image);
                } else {
                    this.render_preview(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn point(&self, position: Point<Pixels>) -> Option<StrokeSample> {
        let bounds = self.bounds.get()?;
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        if width <= 0. || height <= 0. {
            return None;
        }
        let pen = crate::tablet::sample();
        Some(StrokeSample {
            x: ((f32::from(position.x - bounds.origin.x) / width) * PAD_WIDTH as f32)
                .clamp(0., PAD_WIDTH as f32),
            y: ((f32::from(position.y - bounds.origin.y) / height) * PAD_HEIGHT as f32)
                .clamp(0., PAD_HEIGHT as f32),
            time_ms: self
                .started
                .map_or(0., |t| t.elapsed().as_secs_f64() * 1000.),
            pressure: pen.map(|p| p.pressure),
            tilt: pen.map(|p| p.tilt),
        })
    }
    fn begin(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.started = None;
        if !self
            .bounds
            .get()
            .is_some_and(|bounds| bounds.contains(&position))
        {
            return;
        }
        crate::tablet::start();
        if let Some(sample) = self.point(position) {
            self.started = Some(Instant::now());
            self.redo_strokes.clear();
            self.strokes.push(PadStroke {
                samples: vec![sample],
                mode: self.mode(),
            });
            self.changed(cx);
        }
    }
    fn extend(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.started.is_none() {
            return;
        }
        if let Some(sample) = self.point(position)
            && let Some(stroke) = self.strokes.last_mut()
            && stroke.samples.len() < 8192
        {
            stroke.samples.push(sample);
            self.changed(cx);
        }
    }
    fn undo_pad(&mut self, cx: &mut Context<Self>) {
        self.started = None;
        if let Some(stroke) = self.strokes.pop() {
            self.redo_strokes.push(stroke);
            self.changed(cx);
        }
    }
    fn redo_pad(&mut self, cx: &mut Context<Self>) {
        self.started = None;
        if let Some(stroke) = self.redo_strokes.pop() {
            self.strokes.push(stroke);
            self.changed(cx);
        }
    }
    fn finish(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        if save {
            if self.source_loading {
                self.error =
                    Some("Wait for the source image to finish loading before saving.".into());
                cx.notify();
                return;
            }
            if !self.invalid.is_empty() {
                self.error = Some("Enter valid values before saving.".into());
                cx.notify();
                return;
            }
            let name = self.name.read(cx).value().trim().to_string();
            if name.is_empty() {
                self.error = Some("Give this brush a name.".into());
                cx.notify();
                return;
            }
            if let Some(definition) = self.draft.brush_mut(&self.id) {
                definition.name = name;
                if self.editing_secondary {
                    definition.secondary = Some(self.brush);
                } else {
                    definition.brush = self.brush;
                }
                definition.author.name = self.author.read(cx).value().to_string();
                definition.note = self.note.read(cx).value().to_string();
            }
        }
        if save
            && self.is_new
            && let Some(definition) = self.draft.brush_mut(&self.id)
        {
            definition.baseline = definition.brush;
            definition.baseline_shape_asset = definition.shape_asset.clone();
            definition.baseline_grain_asset = definition.grain_asset.clone();
            definition.baseline_secondary = definition.secondary;
            definition.baseline_combine_mode = definition.combine_mode;
            definition.baseline_secondary_shape_asset = definition.secondary_shape_asset.clone();
            definition.baseline_secondary_grain_asset = definition.secondary_grain_asset.clone();
        }
        if save {
            self.save_draft(self.draft.clone(), self.id.clone(), window, cx);
            return;
        }
        let result = self
            .parent
            .update(cx, |parent, cx| parent.finish_studio(None, window, cx));
        if !matches!(result, Ok(true)) {
            self.error=Some("Couldn't save the brush. The library may have changed; keep this draft and retry after resolving the library error.".into());
            cx.notify();
        }
    }

    fn save_as_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.source_loading || !self.invalid.is_empty() {
            return;
        }
        let mut definition = self.draft.brush(&self.id).expect("draft brush").clone();
        definition.name = self.name.read(cx).value().trim().to_string();
        definition.author.name = self.author.read(cx).value().to_string();
        definition.note = self.note.read(cx).value().to_string();
        if definition.name.is_empty() {
            self.error = Some("Give this brush a name.".into());
            cx.notify();
            return;
        }
        if self.editing_secondary {
            definition.secondary = Some(self.brush);
        } else {
            definition.brush = self.brush;
        }
        let result = self.parent.update(cx, |parent, cx| {
            let mut fresh = parent.library.read(cx).catalog.clone();
            let target = fresh
                .sets
                .iter()
                .find(|set| set.id == definition.set_id)
                .or_else(|| fresh.sets.iter().find(|set| !set.builtin))
                .or_else(|| fresh.sets.first())
                .map(|set| set.id.clone());
            let target = target?;
            let Ok(id) = fresh.add_brush(&target, &definition.name, definition.brush) else {
                return None;
            };
            definition.id = id.clone();
            definition.set_id = target;
            definition.builtin = false;
            *fresh.brush_mut(&id).expect("new brush") = definition;
            Some((fresh, id))
        });
        if let Ok(Some((draft, id))) = result {
            self.save_draft(draft, id, window, cx);
        } else {
            self.error = Some(
                "Couldn't save a new brush. Your draft is retained; reload the library and retry."
                    .into(),
            );
            cx.notify();
        }
    }

    fn save_draft(
        &mut self,
        mut draft: Catalog,
        saved_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok((library, previous, error, memory)) = self.parent.update(cx, |parent, cx| {
            let state = parent.library.read(cx);
            let memory = parent.owner.upgrade().and_then(|owner| {
                let owner = owner.read(cx);
                Some((
                    owner.active_memory_key()?,
                    owner.presets.current_id.clone()?,
                    owner.tools.brush.sanitized(),
                ))
            });
            (
                parent.library.clone(),
                state.catalog.clone(),
                state.error.clone(),
                memory,
            )
        }) else {
            return;
        };
        if let Some(error) = error {
            self.error = Some(format!("Library could not be loaded: {error}"));
            cx.notify();
            return;
        }
        self.saving = true;
        self.error = None;
        window.focus(&self.focus, cx);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let id = saved_id.clone();
            // Validation, texture hydration and durable writes must not block input/painting.
            let (result, reload) = cx
                .background_spawn(async move {
                    let result = (|| -> store::StoreResult<Catalog> {
                        if let Some((key, previous_id, brush)) = memory
                            && draft.brush(&previous_id).is_some()
                        {
                            draft.remember_tool(key, &previous_id, brush)?;
                        }
                        draft.record_use(&id)?;
                        store::commit(draft.revision, &draft)
                    })();
                    let reload = matches!(result, Err(store::StoreError::Conflict))
                        .then(store::load_with_report);
                    (result, reload)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(saved) => {
                        this.parent
                            .update(cx, |parent, cx| {
                                parent.complete_studio_save(saved, previous, &saved_id, window, cx)
                            })
                            .ok();
                    }
                    Err(error) => {
                        if let Some(reload) = reload {
                            library.update(cx, |state, cx| {
                                match reload {
                                    Ok(report)
                                        if report.catalog.revision >= state.catalog.revision =>
                                    {
                                        state.catalog = report.catalog;
                                        state.warnings = report.warnings;
                                    }
                                    Ok(_) => {}
                                    Err(error) => state.error = Some(error.to_string()),
                                }
                                cx.notify();
                            });
                        }
                        this.error = Some(format!(
                            "Couldn't save brush: {error}. Your draft is retained."
                        ));
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }
    fn reset(&mut self, original: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_source();
        let result = if original {
            self.draft.restore_original(&self.id)
        } else {
            self.draft.reset_brush(&self.id)
        };
        match result {
            Ok(()) => {
                self.editing_secondary = false;
                self.brush = self.draft.brush(&self.id).unwrap().brush;
                self.sync_controls(window, cx);
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn apply_source_result(
        &mut self,
        request: u64,
        secondary: bool,
        grain: bool,
        result: anyhow::Result<(String, u32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.source_request != request || self.editing_secondary != secondary {
            return;
        }
        self.source_loading = false;
        match result {
            Ok((hash, id)) => {
                let definition = self.draft.brush_mut(&self.id).expect("draft brush");
                if grain {
                    self.brush.grain_tex = id;
                    self.brush.grain_strength = 1.0;
                    if secondary {
                        definition.secondary_grain_asset = Some(hash);
                    } else {
                        definition.grain_asset = Some(hash);
                    }
                } else {
                    self.brush.tip = id;
                    if secondary {
                        definition.secondary_shape_asset = Some(hash);
                    } else {
                        definition.shape_asset = Some(hash);
                    }
                }
                self.refresh_controls(window, cx);
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn edit_source(
        &mut self,
        grain: bool,
        edit: SourceEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_source();
        let request = self.source_request;
        let secondary = self.editing_secondary;
        let id = if grain {
            self.brush.grain_tex
        } else {
            self.brush.tip
        };
        self.source_loading = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let source = edited_source(id, edit)?;
                    let mut png = std::io::Cursor::new(Vec::new());
                    image::DynamicImage::ImageLuma8(source)
                        .write_to(&mut png, image::ImageFormat::Png)?;
                    Ok::<_, anyhow::Error>(store::store_texture_asset(png.get_ref())?)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.apply_source_result(request, secondary, grain, result, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn source(&mut self, grain: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_source();
        let request = self.source_request;
        let secondary = self.editing_secondary;
        self.source_loading = true;
        self.error = None;
        cx.notify();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(if grain {
                "Import grain image".into()
            } else {
                "Import shape image".into()
            }),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                this.update(cx, |this, cx| {
                    if this.source_request == request {
                        this.source_loading = false;
                        cx.notify();
                    }
                })
                .ok();
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                this.update(cx, |this, cx| {
                    if this.source_request == request {
                        this.source_loading = false;
                        cx.notify();
                    }
                })
                .ok();
                return;
            };
            let result = cx
                .background_spawn(async move {
                    let png = decode_source_image(&path)?;
                    Ok::<_, anyhow::Error>(store::store_texture_asset(&png)?)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.apply_source_result(request, secondary, grain, result, window, cx);
            })
            .ok();
        })
        .detach();
    }
}

impl Render for BrushStudio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, foreground, border, muted) = (
            theme.background,
            theme.foreground,
            theme.border,
            theme.muted_foreground,
        );
        let mut categories = div()
            .id("studio-categories")
            .w_40()
            .flex_none()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(border);
        for group in GROUPS {
            categories = categories.child(
                Button::new(*group)
                    .ghost()
                    .small()
                    .selected(self.group == *group)
                    .label(*group)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.group = group;
                        cx.notify();
                    })),
            );
        }
        let mut settings = div()
            .id("studio-settings")
            .overflow_y_scroll()
            .flex_1()
            .min_w_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().child(self.group));
        for (index, field) in FIELDS
            .iter()
            .enumerate()
            .filter(|(_, f)| f.group == self.group)
        {
            settings = settings.child(
                div()
                    .id(field.id)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field.label)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(Slider::new(&self.controls[index].slider).flex_1())
                            .child(
                                Input::new(&self.controls[index].input)
                                    .id(SharedString::from(format!("studio-value-{}", field.id)))
                                    .w_20(),
                            ),
                    )
                    .when(self.invalid.contains(&index), |row| {
                        row.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().danger)
                                .child(format!("Enter {} to {}", field.min, field.max)),
                        )
                    }),
            );
        }
        match self.group {
            "Shape" => {
                settings =
                    settings
                        .child(
                            Button::new("shape-source")
                                .label("Import shape…")
                                .disabled(self.source_loading)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.source(false, window, cx)
                                })),
                        )
                        .child(Button::new("shape-round").label("Use round tip").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.brush.tip = 0;
                                this.invalidate_source();
                                let definition = this.draft.brush_mut(&this.id).unwrap();
                                if this.editing_secondary {
                                    definition.secondary_shape_asset = None;
                                } else {
                                    definition.shape_asset = None;
                                }
                                this.changed(cx);
                            }),
                        ))
                        .child(
                            Button::new("follow-path")
                                .selected(self.brush.follow_path)
                                .label("Follow stroke direction")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.brush.follow_path = !this.brush.follow_path;
                                    this.changed(cx);
                                })),
                        )
                        .child(
                            Button::new("flip-x")
                                .selected(self.brush.advanced.shape.flip_x)
                                .label("Flip horizontally")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.brush.advanced.shape.flip_x =
                                        !this.brush.advanced.shape.flip_x;
                                    this.changed(cx);
                                })),
                        )
                        .child(
                            Button::new("flip-y")
                                .selected(self.brush.advanced.shape.flip_y)
                                .label("Flip vertically")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.brush.advanced.shape.flip_y =
                                        !this.brush.advanced.shape.flip_y;
                                    this.changed(cx);
                                })),
                        )
                        .child(
                            Button::new("stamp-count")
                                .label(format!(
                                    "Stamp count: {} (cycle)",
                                    self.brush.advanced.shape.count
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.brush.advanced.shape.count =
                                        this.brush.advanced.shape.count % 16 + 1;
                                    this.changed(cx);
                                })),
                        );
            }
            "Grain" => {
                settings = settings
                    .child(
                        Button::new("grain-source")
                            .label("Import grain…")
                            .disabled(self.source_loading)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.source(true, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("grain-mode")
                            .label(format!("Grain: {:?}", self.brush.advanced.grain.mode))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.brush.advanced.grain.mode =
                                    if this.brush.advanced.grain.mode == GrainMode::Canvas {
                                        GrainMode::Moving
                                    } else {
                                        GrainMode::Canvas
                                    };
                                this.changed(cx);
                            })),
                    );
                for grain in [
                    GrainKind::None,
                    GrainKind::Paper,
                    GrainKind::Canvas,
                    GrainKind::Chalk,
                    GrainKind::Speckle,
                    GrainKind::Bristle,
                    GrainKind::Halftone,
                ] {
                    settings = settings.child(
                        Button::new(SharedString::from(format!("grain-{grain:?}")))
                            .ghost()
                            .selected(self.brush.grain == grain && self.brush.grain_tex == 0)
                            .label(format!("{grain:?}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.brush.grain = grain;
                                this.brush.grain_tex = 0;
                                this.invalidate_source();
                                let definition = this.draft.brush_mut(&this.id).unwrap();
                                if this.editing_secondary {
                                    definition.secondary_grain_asset = None;
                                } else {
                                    definition.grain_asset = None;
                                }
                                this.changed(cx);
                            })),
                    );
                }
            }
            "Rendering" => {
                settings = settings.child(
                    Button::new("render-mode")
                        .label(format!("Rendering: {:?}", self.brush.advanced.rendering))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.brush.advanced.rendering =
                                if this.brush.advanced.rendering == RenderingMode::Glaze {
                                    RenderingMode::Accumulating
                                } else {
                                    RenderingMode::Glaze
                                };
                            this.changed(cx);
                        })),
                );
                for blend in BrushBlend::MENU {
                    settings = settings.child(
                        Button::new(SharedString::from(format!("blend-{blend:?}")))
                            .ghost()
                            .selected(self.brush.blend == *blend)
                            .label(format!("{blend:?}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.brush.blend = *blend;
                                this.changed(cx);
                            })),
                    );
                }
            }
            "Stylus" => {
                settings = settings.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(crate::tablet::status()),
                );
                for index in 0..5 {
                    settings = settings.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(format!(
                                "Pressure {}% → {:.0}%",
                                index * 25,
                                self.brush.advanced.dynamics.pressure.0[index] * 100.
                            ))
                            .child(Button::new(("curve-minus", index)).label("−").on_click(
                                cx.listener(move |this, _, _, cx| {
                                    let value = &mut this.brush.advanced.dynamics.pressure.0[index];
                                    *value = (*value - 0.05).max(0.);
                                    this.changed(cx);
                                }),
                            ))
                            .child(Button::new(("curve-plus", index)).label("+").on_click(
                                cx.listener(move |this, _, _, cx| {
                                    let value = &mut this.brush.advanced.dynamics.pressure.0[index];
                                    *value = (*value + 0.05).min(1.);
                                    this.changed(cx);
                                }),
                            )),
                    )
                }
            }
            "About" => {
                settings = settings
                    .child("Name")
                    .child(Input::new(&self.name))
                    .child("Author")
                    .child(Input::new(&self.author))
                    .child("Description")
                    .child(Input::new(&self.note))
                    .child(
                        Button::new("create-reset-point")
                            .label("Create reset point")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let definition = this.draft.brush_mut(&this.id).unwrap();
                                if this.editing_secondary {
                                    definition.secondary = Some(this.brush);
                                } else {
                                    definition.brush = this.brush;
                                }
                                if let Err(e) = this.draft.create_reset_point(&this.id) {
                                    this.error = Some(e.to_string());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("reset-brush")
                            .label("Reset to saved point")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.reset(false, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("restore-original")
                            .label("Restore original settings")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.reset(true, window, cx)),
                            ),
                    );
            }
            "Preview" => {
                settings=settings.child("The drawing pad uses the canvas brush renderer. Draw to test pressure, tilt, size and texture. Changing settings replays every mark.");
            }
            _ => {}
        }
        if matches!(self.group, "Shape" | "Grain") {
            let grain = self.group == "Grain";
            let has_source = if grain {
                self.brush.grain_tex != 0
            } else {
                self.brush.tip != 0
            };
            let mut source_tools = div().flex().flex_wrap().gap_2();
            for (id, label, edit) in [
                ("invert-source", "Invert source", SourceEdit::Invert),
                ("rotate-source", "Rotate source 90°", SourceEdit::Rotate),
            ] {
                source_tools = source_tools.child(
                    Button::new(id)
                        .small()
                        .label(label)
                        .disabled(!has_source || self.source_loading)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.edit_source(grain, edit, window, cx)
                        })),
                );
            }
            if grain {
                source_tools = source_tools.child(
                    Button::new("seamless-source")
                        .small()
                        .label("Make seamless (mirror)")
                        .disabled(!has_source || self.source_loading)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.edit_source(true, SourceEdit::MirrorTile, window, cx)
                        })),
                );
            }
            source_tools = source_tools.child(
                Button::new("generate-source")
                    .small()
                    .label(if grain {
                        "Generate paper"
                    } else {
                        "Generate diamond"
                    })
                    .disabled(self.source_loading)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.edit_source(
                            grain,
                            if grain {
                                SourceEdit::Paper
                            } else {
                                SourceEdit::Diamond
                            },
                            window,
                            cx,
                        )
                    })),
            );
            settings = settings.child(source_tools);
        }
        let bounds = self.bounds.clone();
        let mut pad = div()
            .id("brush-studio-pad")
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.begin(event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.extend(event.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.started = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.started = None;
                    cx.notify();
                }),
            );
        if let Some(image) = &self.image {
            pad = pad.child(
                img(ImageSource::Render(image.clone()))
                    .object_fit(ObjectFit::Contain)
                    .size_full(),
            );
        }
        pad = pad.child(
            canvas(
                move |b, _, _| {
                    bounds.set(Some(fitted_pad_bounds(b)));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
        let mut pad_tools = div().flex().flex_wrap().gap_2();
        for (mode, label) in [(0, "Paint"), (1, "Smudge"), (2, "Erase")] {
            pad_tools = pad_tools.child(
                Button::new(label)
                    .small()
                    .selected(self.mode == mode)
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.mode = mode;
                        this.strokes.clear();
                        this.redo_strokes.clear();
                        this.started = None;
                        this.changed(cx);
                    })),
            );
        }
        pad_tools = pad_tools
            .child(
                Button::new("undo-pad")
                    .small()
                    .label("Undo stroke")
                    .disabled(self.strokes.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.undo_pad(cx))),
            )
            .child(
                Button::new("redo-pad")
                    .small()
                    .label("Redo stroke")
                    .disabled(self.redo_strokes.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.redo_pad(cx))),
            )
            .child(
                Button::new("clear-pad")
                    .small()
                    .label("Clear pad")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.strokes.clear();
                        this.redo_strokes.clear();
                        this.started = None;
                        this.changed(cx);
                    })),
            )
            .child(
                Button::new("sample-pad")
                    .small()
                    .label("Sample stroke")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.redo_strokes.clear();
                        this.started = None;
                        this.strokes.push(PadStroke {
                            samples: preview::sample_stroke(PAD_WIDTH, PAD_HEIGHT),
                            mode: this.mode(),
                        });
                        this.changed(cx);
                    })),
            )
            .child(
                Button::new("pad-background")
                    .small()
                    .label("Toggle background")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dark = !this.dark;
                        this.changed(cx);
                    })),
            );
        for (index, ink) in [
            [35, 65, 105, 255],
            [180, 55, 40, 255],
            [30, 140, 80, 255],
            [230, 190, 50, 255],
            [245, 245, 245, 255],
            [30, 30, 30, 255],
        ]
        .into_iter()
        .enumerate()
        {
            pad_tools = pad_tools.child(
                Button::new(("pad-ink", index))
                    .small()
                    .label(["Blue", "Red", "Green", "Gold", "White", "Black"][index])
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ink = ink;
                        cx.notify();
                    })),
            );
        }
        let definition = self.draft.brush(&self.id).expect("draft brush");
        let has_secondary = definition.secondary.is_some();
        let combine_mode = definition.combine_mode;
        let mut components = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(border)
            .child("Editing")
            .child(
                Button::new("component-primary")
                    .small()
                    .selected(!self.editing_secondary)
                    .disabled(!self.invalid.is_empty())
                    .label("Primary")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.select_component(false, window, cx)),
                    ),
            )
            .child(
                Button::new("component-secondary")
                    .small()
                    .selected(self.editing_secondary)
                    .disabled(!has_secondary || !self.invalid.is_empty())
                    .label("Secondary")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.select_component(true, window, cx)),
                    ),
            );
        if has_secondary {
            components = components.child("Combine");
            for (mode, label) in [
                (DualBlend::Normal, "Normal"),
                (DualBlend::Multiply, "Multiply"),
                (DualBlend::Screen, "Screen"),
            ] {
                components = components.child(
                    Button::new(SharedString::from(format!("combine-{label}")))
                        .small()
                        .selected(combine_mode == mode)
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.draft
                                .brush_mut(&this.id)
                                .expect("draft brush")
                                .combine_mode = mode;
                            this.changed(cx);
                        })),
                );
            }
            components = components.child(
                Button::new("remove-secondary")
                    .small()
                    .ghost()
                    .label("Remove secondary")
                    .disabled(!self.invalid.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.remove_secondary(window, cx))),
            );
        } else {
            components = components.child(
                Button::new("add-secondary")
                    .small()
                    .label("Add secondary")
                    .disabled(!self.invalid.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.add_secondary(window, cx))),
            );
        }
        div()
            .id("brush-studio")
            .test_support()
            .relative()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .text_color(foreground)
            .on_action(cx.listener(|this, _: &crate::actions::Undo, _, cx| {
                this.undo_pad(cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &crate::actions::Redo, _, cx| {
                this.redo_pad(cx);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.saving {
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" {
                    this.finish(false, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .border_b_1()
                    .border_color(border)
                    .child("Brush Studio")
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("studio-cancel")
                                    .label("Cancel")
                                    .disabled(self.saving)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.finish(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("studio-save-copy")
                                    .label("Save as new brush")
                                    .disabled(
                                        self.saving
                                            || !self.invalid.is_empty()
                                            || self.source_loading,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_as_new(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("studio-done")
                                    .primary()
                                    .disabled(
                                        self.saving
                                            || !self.invalid.is_empty()
                                            || self.source_loading,
                                    )
                                    .label(if self.saving { "Saving…" } else { "Done" })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.finish(true, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(components)
            .when(self.source_loading, |view| {
                view.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_color(muted)
                        .child("Loading source image…"),
                )
            })
            .children(
                self.error
                    .as_ref()
                    .map(|e| div().p_2().text_color(cx.theme().danger).child(e.clone())),
            )
            .child(
                div().flex().flex_1().min_h_0().child(categories).child(
                    div().flex_1().min_w_0().min_h_0().child(
                        h_resizable("studio-settings-pad")
                            .with_state(&self.split)
                            .child(
                                resizable_panel()
                                    .size(window.rem_size() * 22.0)
                                    .size_range(window.rem_size() * 15.0..window.rem_size() * 42.0)
                                    .child(settings.size_full()),
                            )
                            .child(
                                resizable_panel()
                                    .size_range(window.rem_size() * 20.0..Pixels::MAX)
                                    .child(
                                        div()
                                            .size_full()
                                            .flex()
                                            .flex_col()
                                            .min_w_0()
                                            .min_h_0()
                                            .gap_2()
                                            .p_3()
                                            .child("Drawing pad")
                                            .child(pad_tools)
                                            .child(pad),
                                    ),
                            ),
                    ),
                ),
            )
            .when(self.saving, |view| {
                view.child(
                    div()
                        .id("studio-saving")
                        .test_support()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .bg(background.opacity(0.9))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child("Saving brush…"),
                )
            })
    }
}

#[cfg(test)]
mod source_tests {
    use super::{SourceEdit, decode_source_bytes, edited_source, fitted_pad_bounds, mirror_tile};
    use emulsion_raster::paint::textures;
    use gpui_kit as gpui;

    #[test]
    fn pad_coordinates_follow_contained_image() {
        let wide = fitted_pad_bounds(gpui::Bounds::new(
            gpui::point(gpui::px(10.), gpui::px(20.)),
            gpui::size(gpui::px(800.), gpui::px(300.)),
        ));
        assert_eq!(wide.origin, gpui::point(gpui::px(210.), gpui::px(20.)));
        assert_eq!(wide.size, gpui::size(gpui::px(400.), gpui::px(300.)));
        assert!(!wide.contains(&gpui::point(gpui::px(100.), gpui::px(100.))));
        let tall = fitted_pad_bounds(gpui::Bounds::new(
            gpui::point(gpui::px(0.), gpui::px(0.)),
            gpui::size(gpui::px(400.), gpui::px(600.)),
        ));
        assert_eq!(tall.origin, gpui::point(gpui::px(0.), gpui::px(150.)));
        assert_eq!(tall.size, wide.size);
    }

    #[test]
    fn source_transforms_keep_original_and_make_matching_tile_edges() {
        let id = 0xaab2_5511;
        let values = [0, 30, 80, 120, 150, 190, 225, 255];
        textures::register(id, textures::Texture::from_gray8(4, 2, &values).unwrap());
        let inverted = edited_source(id, SourceEdit::Invert).unwrap();
        assert_eq!(inverted.as_raw(), &values.map(|v| 255 - v));
        let rotated = edited_source(id, SourceEdit::Rotate).unwrap();
        assert_eq!(rotated.dimensions(), (2, 4));
        assert_eq!(rotated.get_pixel(1, 0)[0], values[0]);
        assert_eq!(rotated.get_pixel(0, 3)[0], values[7]);
        assert_eq!(textures::get(id).unwrap().sample(0.125, 0.25), 0.0);
        let tiled = edited_source(id, SourceEdit::MirrorTile).unwrap();
        for x in 0..tiled.width() {
            assert_eq!(
                tiled.get_pixel(x, 0),
                tiled.get_pixel(x, tiled.height() - 1)
            );
        }
        for y in 0..tiled.height() {
            assert_eq!(tiled.get_pixel(0, y), tiled.get_pixel(tiled.width() - 1, y));
        }
        let large = mirror_tile(image::GrayImage::new(3000, 2));
        assert_eq!(large.width(), 4096);
    }

    #[test]
    fn generated_sources_are_deterministic_and_portable() {
        let diamond = edited_source(0, SourceEdit::Diamond).unwrap();
        assert!(diamond.get_pixel(64, 64)[0] > diamond.get_pixel(0, 0)[0]);
        let paper = edited_source(0, SourceEdit::Paper).unwrap();
        assert_eq!(paper, edited_source(0, SourceEdit::Paper).unwrap());
        assert!(paper.as_raw().windows(2).any(|pair| pair[0] != pair[1]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(paper.clone())
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let normalized = decode_source_bytes(png.into_inner()).unwrap();
        assert_eq!(
            image::load_from_memory(&normalized).unwrap().to_luma8(),
            paper
        );
    }

    #[test]
    fn source_dimension_headers_are_rejected_before_full_decode() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(image::GrayImage::new(4097, 1))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let result = decode_source_bytes(png.into_inner());
        assert!(result.unwrap_err().to_string().contains("4096"));
    }
}
