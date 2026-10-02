//! Lightroom-style local masking for the agent: subject, sky, background,
//! people (face, eyes, teeth), gradients, brush, colour and luminance ranges,
//! combined with add/subtract/intersect/invert, plus the masking strategies
//! retouchers use for colour control and guiding the eye.
//!
//! Masks live in the RAW recipe's composed local edits, so they render in the
//! same pipeline as the Develop panel, stay editable, and undo in one step.
use crate::{
    exec::Planned,
    server::{ToolDef, ToolResult},
};
use emulsion_core::{
    Document, NodeKind,
    develop_edits::{Component, LocalEdits, Mask, Operation, Shape},
    raw::DevelopParams,
};
use emulsion_io::{develop_edits, photo_develop::PhotoSource};
use serde_json::{Value, json};

fn error(message: impl ToString) -> ToolResult {
    ToolResult::error(message.to_string())
}

fn raw_params(doc: &Document) -> Result<DevelopParams, ToolResult> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| error("No editable RAW source; open a supported camera RAW file first"))?;
    raw.validate().map_err(error)?;
    Ok(raw.params)
}

fn current_edits(params: &DevelopParams) -> Result<LocalEdits, ToolResult> {
    match params.local_edits {
        Some(digest) => develop_edits::load(&digest).map_err(error),
        None => Ok(LocalEdits::default()),
    }
}

// ------------------------------------------------------------ coordinates

/// Map a point in the developed view (after crop and quarter turns) to the
/// uncropped source coordinates masks use. Straighten and perspective are
/// small and ignored, so placement near frame edges is approximate.
pub fn view_to_source(p: &DevelopParams, [x, y]: [f32; 2]) -> [f32; 2] {
    let [cx, cy] = match p.rotation % 4 {
        1 => [y, 1. - x],
        2 => [1. - x, 1. - y],
        3 => [1. - y, x],
        _ => [x, y],
    };
    [
        (p.crop[0] + cx * (p.crop[2] - p.crop[0])).clamp(0., 1.),
        (p.crop[1] + cy * (p.crop[3] - p.crop[1])).clamp(0., 1.),
    ]
}

fn view_radius(p: &DevelopParams, [rx, ry]: [f32; 2]) -> [f32; 2] {
    let [rx, ry] = if p.rotation % 2 == 1 {
        [ry, rx]
    } else {
        [rx, ry]
    };
    [
        (rx * (p.crop[2] - p.crop[0])).clamp(0.001, 1.),
        (ry * (p.crop[3] - p.crop[1])).clamp(0.001, 1.),
    ]
}

fn linear(v: f32) -> f32 {
    let v = v.clamp(0., 1.);
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(v: f32) -> f32 {
    let v = v.clamp(0., 1.);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}

/// Named colours for colour-range masks, as display sRGB.
fn named_colour(name: &str) -> Option<[f32; 3]> {
    Some(match name {
        "red" => [0.8, 0.15, 0.12],
        "orange" => [0.9, 0.5, 0.15],
        "yellow" => [0.88, 0.78, 0.2],
        "green" => [0.3, 0.6, 0.2],
        "aqua" | "teal" | "cyan" => [0.2, 0.65, 0.7],
        "blue" => [0.2, 0.4, 0.8],
        "purple" => [0.5, 0.3, 0.7],
        "magenta" | "pink" => [0.85, 0.3, 0.6],
        "skin" => [0.85, 0.62, 0.5],
        _ => return None,
    })
}

fn colour(value: &Value) -> Result<[f32; 3], ToolResult> {
    let bad = || {
        error(
            "color must be a name (red, orange, yellow, green, aqua, blue, purple, magenta, skin), #rrggbb, or [r,g,b] 0..1",
        )
    };
    match value {
        Value::String(s) if s.starts_with('#') && s.len() == 7 => {
            let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| bad());
            Ok([byte(1)?, byte(3)?, byte(5)?].map(|v| v as f32 / 255.))
        }
        Value::String(s) => named_colour(&s.to_lowercase()).ok_or_else(bad),
        Value::Array(a) if a.len() == 3 => {
            let v: Vec<f32> = a
                .iter()
                .map(|v| {
                    v.as_f64()
                        .map(|v| v as f32)
                        .filter(|v| (0.0..=1.).contains(v))
                })
                .collect::<Option<_>>()
                .ok_or_else(bad)?;
            Ok([v[0], v[1], v[2]])
        }
        _ => Err(bad()),
    }
}

// ------------------------------------------------------------ AI regions

/// Content-aware selections shared by every component in one tool call.
#[derive(Default)]
struct Regions {
    subject: Option<Option<[u8; 32]>>,
    sky: Option<Option<[u8; 32]>>,
    faces: Option<Vec<emulsion_ai::face::Face>>,
    image: Option<emulsion_raster::Raster>,
    size: (f32, f32),
    notes: Vec<String>,
}

impl Regions {
    /// The photo in mask coordinates: full frame, no crop or geometry.
    fn image(&mut self, doc: &Document) -> Result<&emulsion_raster::Raster, ToolResult> {
        if self.image.is_none() {
            let raw = doc.raw.as_ref().unwrap();
            let mut p = raw.params;
            p.crop = [0., 0., 1., 1.];
            p.straighten = 0.;
            p.perspective = [0.; 2];
            p.distortion = 0.;
            p.lens_profile = None;
            p.aberration = [0.; 2];
            p.masks = Default::default();
            p.local_edits = None;
            p.rotation = 0;
            p.depth_blur = 0.;
            let source =
                PhotoSource::load_verified(&raw.source, &raw.source_sha256).map_err(error)?;
            let image = source.develop_with(&p).map_err(error)?;
            self.size = (image.width() as f32, image.height() as f32);
            self.image = Some(image);
        }
        Ok(self.image.as_ref().unwrap())
    }

    fn subject(&mut self, doc: &Document) -> Result<Option<[u8; 32]>, ToolResult> {
        if self.subject.is_none() {
            let found = if emulsion_ai::matte::available().is_some() {
                let image = self.image(doc)?;
                let job = emulsion_ai::jobs::Job::new();
                let m = emulsion_ai::matte::matte(image, &Default::default(), &job)
                    .map_err(|e| error(e.to_string()))?;
                Some(emulsion_io::photo_develop::save_mask(&m).map_err(error)?)
            } else {
                self.notes.push("No subject model installed (download_model rmbg14 or isnet): used a centred radial approximation".into());
                None
            };
            self.subject = Some(found);
        }
        Ok(self.subject.unwrap())
    }

    fn sky(&mut self, doc: &Document) -> Result<Option<[u8; 32]>, ToolResult> {
        if self.sky.is_none() {
            let found = if emulsion_ai::models::installed_for(emulsion_ai::models::Task::Sky)
                .is_some()
            {
                let image = self.image(doc)?;
                let job = emulsion_ai::jobs::Job::new();
                let m = emulsion_ai::sky::mask(image, &job).map_err(|e| error(e.to_string()))?;
                Some(emulsion_io::photo_develop::save_mask(&m).map_err(error)?)
            } else {
                self.notes.push("No sky model installed (download_model skyseg): used a top gradient intersected with bright tones".into());
                None
            };
            self.sky = Some(found);
        }
        Ok(self.sky.unwrap())
    }

    fn faces(&mut self, doc: &Document) -> Result<&[emulsion_ai::face::Face], ToolResult> {
        if self.faces.is_none() {
            if emulsion_ai::face::detector_available().is_none() {
                return Err(error(
                    "Face, eye and teeth masks need the face detector; install it with download_model, or use radial masks at the eyes/mouth instead",
                ));
            }
            let image = self.image(doc)?;
            let job = emulsion_ai::jobs::Job::new();
            let faces = emulsion_ai::face::detect(image, &job).map_err(|e| error(e.to_string()))?;
            self.faces = Some(faces);
        }
        Ok(self.faces.as_deref().unwrap())
    }
}

fn add(shape: Shape) -> Component {
    Component {
        operation: Operation::Add,
        shape,
    }
}

/// Components for a content-aware region; empty when nothing was found.
/// AI regions are already in source coordinates; the no-model fallbacks are
/// described in view terms (centred, top of the photo) and mapped like drawn shapes.
fn region(
    kind: &str,
    doc: &Document,
    params: &DevelopParams,
    regions: &mut Regions,
) -> Result<Vec<Component>, ToolResult> {
    Ok(match kind {
        "subject" | "background" => {
            let inverted = kind == "background";
            match regions.subject(doc)? {
                Some(digest) => vec![add(Shape::Bitmap { digest, inverted })],
                None => {
                    let radial = Shape::Radial {
                        center: view_to_source(params, [0.5, 0.5]),
                        radius: view_radius(params, [0.32, 0.42]),
                        feather: 0.6,
                    };
                    if inverted {
                        vec![
                            add(Shape::All),
                            Component {
                                operation: Operation::Subtract,
                                shape: radial,
                            },
                        ]
                    } else {
                        vec![add(radial)]
                    }
                }
            }
        }
        "sky" => match regions.sky(doc)? {
            Some(digest) => vec![add(Shape::Bitmap {
                digest,
                inverted: false,
            })],
            None => vec![
                add(Shape::Linear {
                    start: view_to_source(params, [0.5, 0.6]),
                    end: view_to_source(params, [0.5, 0.3]),
                }),
                Component {
                    operation: Operation::Intersect,
                    shape: Shape::Luminance {
                        range: [linear(0.45), 1.],
                        feather: 0.5,
                    },
                },
            ],
        },
        "face" | "eyes" | "teeth" => {
            regions.faces(doc)?;
            let (w, h) = regions.size;
            let faces = regions.faces.as_deref().unwrap();
            let mut parts = Vec::new();
            for face in faces.iter().take(8) {
                let fw = (face.x1 - face.x0) / w;
                let fh = (face.y1 - face.y0) / h;
                let at = |p: [f32; 2]| [(p[0] / w).clamp(0., 1.), (p[1] / h).clamp(0., 1.)];
                match kind {
                    "face" => parts.push(add(Shape::Radial {
                        center: at([(face.x0 + face.x1) / 2., (face.y0 + face.y1) / 2.]),
                        radius: [(fw * 0.55).clamp(0.001, 1.), (fh * 0.6).clamp(0.001, 1.)],
                        feather: 0.5,
                    })),
                    "eyes" => {
                        for eye in &face.landmarks[..2] {
                            parts.push(add(Shape::Radial {
                                center: at(*eye),
                                radius: [
                                    (fw * 0.11).clamp(0.001, 1.),
                                    (fh * 0.07).clamp(0.001, 1.),
                                ],
                                feather: 0.5,
                            }));
                        }
                    }
                    _ => {
                        let [l, r] = [face.landmarks[3], face.landmarks[4]];
                        parts.push(add(Shape::Radial {
                            center: at([(l[0] + r[0]) / 2., (l[1] + r[1]) / 2.]),
                            radius: [
                                ((r[0] - l[0]).abs() / w * 0.5).clamp(0.001, 1.),
                                (fh * 0.06).clamp(0.001, 1.),
                            ],
                            feather: 0.4,
                        }));
                    }
                }
            }
            if kind == "teeth" && !parts.is_empty() {
                // Teeth are the bright, near-neutral part of the mouth.
                parts.push(Component {
                    operation: Operation::Intersect,
                    shape: Shape::Luminance {
                        range: [linear(0.45), 1.],
                        feather: 0.4,
                    },
                });
            }
            if parts.is_empty() {
                regions
                    .notes
                    .push(format!("No faces found for the {kind} mask"));
            }
            parts
        }
        _ => unreachable!(),
    })
}

// ------------------------------------------------------------ parsing

fn num(v: &Value, key: &str, min: f32, max: f32, default: f32) -> Result<f32, ToolResult> {
    match v.get(key) {
        None => Ok(default),
        Some(x) => x
            .as_f64()
            .map(|x| x as f32)
            .filter(|x| x.is_finite() && (min..=max).contains(x))
            .ok_or_else(|| error(format!("{key} must be a number from {min} to {max}"))),
    }
}

fn pair(v: &Value, key: &str) -> Result<Option<[f32; 2]>, ToolResult> {
    match v.get(key) {
        None => Ok(None),
        Some(Value::Number(n)) => {
            let n = n.as_f64().unwrap_or(-1.) as f32;
            Ok(Some([n, n]))
        }
        Some(Value::Array(a)) if a.len() == 2 => {
            let x = a[0].as_f64();
            let y = a[1].as_f64();
            match (x, y) {
                (Some(x), Some(y)) => Ok(Some([x as f32, y as f32])),
                _ => Err(error(format!("{key} must be two numbers"))),
            }
        }
        _ => Err(error(format!("{key} must be [x, y]"))),
    }
}

fn unit_pair(v: &Value, key: &str) -> Result<Option<[f32; 2]>, ToolResult> {
    let p = pair(v, key)?;
    if p.is_some_and(|p| p.iter().any(|v| !(0.0..=1.).contains(v))) {
        return Err(error(format!("{key} values must be 0..1")));
    }
    Ok(p)
}

fn components(
    spec: &Value,
    doc: &Document,
    params: &DevelopParams,
    view: bool,
    regions: &mut Regions,
) -> Result<Vec<Component>, ToolResult> {
    let list = spec
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 16)
        .ok_or_else(|| error("components must be a list of 1–16 parts"))?;
    let point = |p: [f32; 2]| if view { view_to_source(params, p) } else { p };
    let radius = |r: [f32; 2]| if view { view_radius(params, r) } else { r };
    let mut out: Vec<Component> = Vec::new();
    for (index, c) in list.iter().enumerate() {
        let operation = match c.get("operation").and_then(Value::as_str).unwrap_or("add") {
            "add" => Operation::Add,
            "subtract" => Operation::Subtract,
            "intersect" => Operation::Intersect,
            _ => return Err(error("operation must be add, subtract or intersect")),
        };
        let invert = c.get("invert").and_then(Value::as_bool).unwrap_or(false);
        let kind = c
            .get("shape")
            .and_then(Value::as_str)
            .ok_or_else(|| error("each component needs a shape"))?;
        let feather = num(c, "feather", 0., 1., 0.5)?;
        let parts: Vec<Component> = match kind {
            "subject" | "background" | "sky" | "face" | "eyes" | "teeth" => {
                region(kind, doc, params, regions)?
            }
            "all" => vec![add(Shape::All)],
            "radial" => {
                let center = unit_pair(c, "center")?.unwrap_or([0.5, 0.5]);
                let r = unit_pair(c, "radius")?.unwrap_or([0.3, 0.3]);
                if r.iter().any(|v| *v <= 0.) {
                    return Err(error("radius must be above 0"));
                }
                vec![add(Shape::Radial {
                    center: point(center),
                    radius: radius(r),
                    feather,
                })]
            }
            "linear" => {
                let start = unit_pair(c, "start")?.ok_or_else(|| error("linear needs start"))?;
                let end = unit_pair(c, "end")?.ok_or_else(|| error("linear needs end"))?;
                if start == end {
                    return Err(error("linear start and end must differ"));
                }
                vec![add(Shape::Linear {
                    start: point(start),
                    end: point(end),
                })]
            }
            "brush" => {
                let points: Vec<[f32; 2]> = c
                    .get("points")
                    .and_then(Value::as_array)
                    .filter(|a| !a.is_empty() && a.len() <= 4096)
                    .ok_or_else(|| error("brush needs 1–4096 points"))?
                    .iter()
                    .map(|p| {
                        unit_pair(&json!({"p": p}), "p")?
                            .ok_or_else(|| error("brush points must be [x, y]"))
                    })
                    .collect::<Result<_, _>>()?;
                let size = num(c, "size", 0.001, 1., 0.04)?;
                let size = if view {
                    size * (params.crop[2] - params.crop[0]).max(params.crop[3] - params.crop[1])
                } else {
                    size
                };
                vec![add(Shape::Brush {
                    points: points.into_iter().map(point).collect(),
                    radius: size.clamp(0.001, 1.),
                    feather,
                })]
            }
            "luminance" => {
                let range = unit_pair(c, "range")?.ok_or_else(|| error("luminance needs range"))?;
                if range[0] >= range[1] {
                    return Err(error("luminance range must be [low, high] with low < high"));
                }
                vec![add(Shape::Luminance {
                    range: range.map(linear),
                    feather,
                })]
            }
            "color" => {
                let rgb = colour(
                    c.get("color")
                        .ok_or_else(|| error("color mask needs color"))?,
                )?;
                vec![add(Shape::Color {
                    rgb: rgb.map(linear),
                    tolerance: num(c, "tolerance", 0.01, 1., 0.18)?,
                    feather,
                })]
            }
            _ => {
                return Err(error(
                    "shape must be subject, background, sky, face, eyes, teeth, all, radial, linear, brush, luminance or color",
                ));
            }
        };
        if parts.is_empty() {
            continue;
        }
        // A region may itself be several parts; combine them, then apply the
        // requested operation, inverting through the whole-frame shape.
        let single = parts.len() == 1;
        match (invert, single, operation, out.is_empty()) {
            (false, true, op, _) => out.push(Component {
                operation: op,
                shape: parts.into_iter().next().unwrap().shape,
            }),
            (false, false, Operation::Add, true) => out.extend(parts),
            (true, true, Operation::Add, true) => {
                out.push(add(Shape::All));
                out.push(Component {
                    operation: Operation::Subtract,
                    shape: parts.into_iter().next().unwrap().shape,
                });
            }
            (true, true, Operation::Intersect, false) => out.push(Component {
                operation: Operation::Subtract,
                shape: parts.into_iter().next().unwrap().shape,
            }),
            (true, true, Operation::Subtract, false) => out.push(Component {
                operation: Operation::Intersect,
                shape: parts.into_iter().next().unwrap().shape,
            }),
            (_, false, _, _) => {
                return Err(error(format!(
                    "component {} ({kind}) is a multi-part approximation here; use it as the first component, added and not inverted",
                    index + 1
                )));
            }
            (true, true, _, _) => {
                return Err(error(format!(
                    "component {}: invert works on the first component (add) or on later intersect/subtract components",
                    index + 1
                )));
            }
        }
    }
    if out.is_empty() {
        return Err(error(
            "The mask selected nothing (no faces or region found)",
        ));
    }
    Ok(out)
}

const ADJUSTMENTS: [&str; 7] = [
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "saturation",
    "temperature",
    "tint",
];

fn set_adjustments(mask: &mut Mask, v: &Value, scale: f32) -> Result<(), ToolResult> {
    let object = v
        .as_object()
        .ok_or_else(|| error("adjustments must be an object"))?;
    for key in object.keys() {
        if !ADJUSTMENTS.contains(&key.as_str()) {
            return Err(error(format!(
                "Unknown mask adjustment '{key}'; use {}",
                ADJUSTMENTS.join(", ")
            )));
        }
    }
    let get = |key: &str, max: f32, current: f32| -> Result<f32, ToolResult> {
        Ok((num(v, key, -max, max, current / scale.max(1e-6))? * scale).clamp(-max, max))
    };
    mask.exposure = get("exposure", 5., mask.exposure)?;
    mask.contrast = get("contrast", 1., mask.contrast)?;
    mask.highlights = get("highlights", 1., mask.highlights)?;
    mask.shadows = get("shadows", 1., mask.shadows)?;
    mask.saturation = get("saturation", 1., mask.saturation)?;
    mask.temperature = get("temperature", 1., mask.temperature)?;
    mask.tint = get("tint", 1., mask.tint)?;
    Ok(())
}

fn summary(edits: &LocalEdits) -> Vec<Value> {
    edits
        .masks
        .iter()
        .map(|m| {
            json!({"id":m.id,"name":m.name,"enabled":m.enabled,"components":m.components,
                "adjustments":{"exposure":m.exposure,"contrast":m.contrast,"highlights":m.highlights,
                "shadows":m.shadows,"saturation":m.saturation,"temperature":m.temperature,"tint":m.tint}})
        })
        .collect()
}

fn commit(
    doc: &Document,
    params: DevelopParams,
    edits: LocalEdits,
    message: Value,
) -> Result<Planned, ToolResult> {
    let mut next = params;
    next.local_edits = if edits.masks.is_empty() && edits.spots.is_empty() {
        None
    } else {
        Some(develop_edits::store(&edits).map_err(error)?)
    };
    let mut planned = crate::raw_tools::develop(doc, next, None)?;
    let mut message = message;
    message["masks"] = json!(summary(&edits));
    message["undo_steps"] = json!(if planned.commands.is_empty() { 0 } else { 1 });
    message["next"] =
        json!("Check placement with list_raw_masks overlay=true and the result with get_view.");
    planned.message = message.to_string();
    Ok(planned)
}

// ------------------------------------------------------------ manual tool

pub fn plan(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("Arguments must be an object"))?;
    for key in object.keys() {
        if ![
            "action",
            "id",
            "name",
            "components",
            "adjustments",
            "enabled",
            "space",
        ]
        .contains(&key.as_str())
        {
            return Err(error(format!("Unknown argument '{key}'")));
        }
    }
    let params = raw_params(doc)?;
    let mut edits = current_edits(&params)?;
    let view = match args.get("space").and_then(Value::as_str).unwrap_or("view") {
        "view" => true,
        "source" => false,
        _ => return Err(error("space must be view or source")),
    };
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| error("action must be add, update, remove or clear"))?;
    let find = |edits: &LocalEdits| -> Result<usize, ToolResult> {
        let id = args
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| error("id is required; see list_raw_masks"))?;
        edits
            .masks
            .iter()
            .position(|m| u64::from(m.id) == id)
            .ok_or_else(|| error(format!("No mask {id}; see list_raw_masks")))
    };
    let mut regions = Regions::default();
    let message = match action {
        "add" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| error("name is required"))?;
            let spec = args
                .get("components")
                .ok_or_else(|| error("components are required"))?;
            let mut mask = Mask {
                id: edits.masks.iter().map(|m| m.id).max().unwrap_or(0) + 1,
                name: name.trim().chars().take(200).collect(),
                enabled: args.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                components: components(spec, doc, &params, view, &mut regions)?,
                exposure: 0.,
                contrast: 0.,
                saturation: 0.,
                temperature: 0.,
                tint: 0.,
                highlights: 0.,
                shadows: 0.,
            };
            if let Some(adjustments) = args.get("adjustments") {
                set_adjustments(&mut mask, adjustments, 1.)?;
            }
            let id = mask.id;
            edits.masks.push(mask);
            json!({"added": id})
        }
        "update" => {
            let index = find(&edits)?;
            if let Some(spec) = args.get("components") {
                edits.masks[index].components = components(spec, doc, &params, view, &mut regions)?;
            }
            if let Some(adjustments) = args.get("adjustments") {
                set_adjustments(&mut edits.masks[index], adjustments, 1.)?;
            }
            if let Some(name) = args.get("name").and_then(Value::as_str) {
                edits.masks[index].name = name.trim().chars().take(200).collect();
            }
            if let Some(enabled) = args.get("enabled") {
                edits.masks[index].enabled = enabled
                    .as_bool()
                    .ok_or_else(|| error("enabled must be a boolean"))?;
            }
            json!({"updated": edits.masks[index].id})
        }
        "remove" => {
            let index = find(&edits)?;
            json!({"removed": edits.masks.remove(index).id})
        }
        "clear" => {
            edits.masks.clear();
            json!({"cleared": true, "healing_spots_kept": edits.spots.len()})
        }
        _ => return Err(error("action must be add, update, remove or clear")),
    };
    edits.validate().map_err(error)?;
    let mut message = message;
    message["notes"] = json!(regions.notes);
    commit(doc, params, edits, message)
}

// ------------------------------------------------------------ strategies

struct Plan {
    name: &'static str,
    why: String,
    components: Value,
    adjustments: Value,
}

const STRATEGIES: [&str; 11] = [
    "subject_pop",
    "background_recede",
    "sky",
    "vignette_focus",
    "directional_light",
    "color_range",
    "color_separation",
    "tonal_balance",
    "eyes",
    "teeth",
    "auto",
];

fn strategy(key: &str, a: &crate::raw_looks::Analysis, light_left: bool) -> Vec<Plan> {
    let plan = |name, why: &str, components, adjustments| Plan {
        name,
        why: why.into(),
        components,
        adjustments,
    };
    match key {
        "subject_pop" => vec![plan(
            "Auto · Subject pop",
            "lift and add presence to the subject so the eye lands there first",
            json!([{"shape":"subject"}]),
            json!({"exposure":0.2,"contrast":0.1,"shadows":0.15,"saturation":0.05}),
        )],
        "background_recede" => vec![plan(
            "Auto · Background recede",
            "darker, quieter background separates the subject",
            json!([{"shape":"background"}]),
            json!({"exposure":-0.25,"saturation":-0.2,"contrast":-0.05}),
        )],
        "sky" => {
            let golden = a.has("golden_light");
            vec![plan(
                "Auto · Sky",
                if golden {
                    "recover bright sky and enrich its colour, keeping the warm light"
                } else {
                    "recover bright sky, enrich blue and add depth"
                },
                json!([{"shape":"sky"}]),
                json!({"highlights":-0.35,"exposure":-0.15,"saturation":0.15,"temperature": if golden { 0.05 } else { -0.05 }}),
            )]
        }
        "vignette_focus" => vec![plan(
            "Auto · Vignette focus",
            "darker, desaturated edges draw the eye toward the centre of interest",
            json!([{"shape":"radial","center":[0.5,0.5],"radius":[0.62,0.62],"feather":0.9,"invert":true}]),
            json!({"exposure":-0.3,"saturation":-0.2}),
        )],
        "directional_light" => {
            let (lit, shade) = if light_left {
                ([0.28, 0.4], [0.72, 0.6])
            } else {
                ([0.72, 0.4], [0.28, 0.6])
            };
            vec![
                plan(
                    "Auto · Light side",
                    "warm, brighter side fakes directional light on flat light",
                    json!([{"shape":"radial","center":lit,"radius":[0.55,0.7],"feather":0.9}]),
                    json!({"exposure":0.15,"temperature":0.15}),
                ),
                plan(
                    "Auto · Shadow side",
                    "cooler, darker opposite side completes the light direction",
                    json!([{"shape":"radial","center":shade,"radius":[0.55,0.7],"feather":0.9}]),
                    json!({"exposure":-0.12,"temperature":-0.1}),
                ),
            ]
        }
        "color_range" => {
            let mut out = Vec::new();
            if a.hue_share.get("yellow").copied().unwrap_or(0.)
                + a.hue_share.get("orange").copied().unwrap_or(0.)
                >= 15.
            {
                out.push(plan(
                    "Auto · Warm the yellows",
                    "colour range on yellows/oranges: warmer, richer foliage and light",
                    json!([{"shape":"color","color":"yellow","tolerance":0.2},{"shape":"color","color":"orange","tolerance":0.2}]),
                    json!({"temperature":0.12,"saturation":0.12}),
                ));
            }
            if a.hue_share.get("red").copied().unwrap_or(0.) >= 8. && !a.has("portrait") {
                out.push(plan(
                    "Auto · Deepen the reds",
                    "colour range on reds: deeper, denser reds",
                    json!([{"shape":"color","color":"red","tolerance":0.18}]),
                    json!({"exposure":-0.15,"saturation":0.1}),
                ));
            }
            if a.hue_share.get("blue").copied().unwrap_or(0.)
                + a.hue_share.get("aqua").copied().unwrap_or(0.)
                >= 15.
            {
                out.push(plan(
                    "Auto · Cool the blues",
                    "colour range on blues: cleaner, deeper blues for warm/cool separation",
                    json!([{"shape":"color","color":"blue","tolerance":0.2}]),
                    json!({"temperature":-0.08,"saturation":0.1,"exposure":-0.05}),
                ));
            }
            out
        }
        "color_separation" => vec![
            plan(
                "Auto · Warm foreground",
                "warmer foreground advances toward the viewer",
                json!([{"shape":"linear","start":[0.5,0.55],"end":[0.5,0.95]}]),
                json!({"temperature":0.12,"exposure":0.05}),
            ),
            plan(
                "Auto · Cool background",
                "cooler distance recedes, separating planes by colour",
                json!([{"shape":"linear","start":[0.5,0.5],"end":[0.5,0.1]}]),
                json!({"temperature":-0.1,"saturation":0.05}),
            ),
        ],
        "tonal_balance" => vec![
            plan(
                "Auto · Open shadows",
                "luminance range on dark tones: open shadows without lifting blacks globally",
                json!([{"shape":"luminance","range":[0.0,0.3],"feather":0.6}]),
                json!({"shadows":0.25}),
            ),
            plan(
                "Auto · Hold highlights",
                "luminance range on bright tones: recover highlight detail",
                json!([{"shape":"luminance","range":[0.72,1.0],"feather":0.6}]),
                json!({"highlights":-0.3}),
            ),
        ],
        "eyes" => vec![plan(
            "Auto · Eyes",
            "brighter, crisper eyes",
            json!([{"shape":"eyes"}]),
            json!({"exposure":0.15,"contrast":0.15,"saturation":0.08}),
        )],
        "teeth" => vec![plan(
            "Auto · Teeth",
            "whiter teeth: less yellow, slightly brighter",
            json!([{"shape":"teeth"}]),
            json!({"saturation":-0.35,"exposure":0.1}),
        )],
        _ => Vec::new(),
    }
}

/// Which strategies suit the photo, the way a retoucher would plan masks.
fn auto_strategies(a: &crate::raw_looks::Analysis) -> Vec<&'static str> {
    let mut out = Vec::new();
    if a.has("portrait") {
        out.extend(["subject_pop", "background_recede"]);
    } else if !a.has("landscape") {
        out.push("subject_pop");
    }
    if a.sky_pct >= 8. {
        out.push("sky");
    }
    if a.has("landscape") && a.foliage_pct >= 10. {
        out.push("color_range");
    }
    if a.has("landscape") && a.sky_pct >= 8. {
        out.push("color_separation");
    }
    if a.has("flat") && !a.has("landscape") {
        out.push("directional_light");
    }
    if a.has("contrasty") {
        out.push("tonal_balance");
    }
    out.push("vignette_focus");
    out
}

pub fn plan_auto(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("Arguments must be an object"))?;
    for key in object.keys() {
        if !["strategies", "strength"].contains(&key.as_str()) {
            return Err(error(format!("Unknown argument '{key}'")));
        }
    }
    let strength = num(args, "strength", 0., 1.5, 1.)?;
    let requested: Vec<String> = match args.get("strategies") {
        None => vec!["auto".into()],
        Some(Value::Array(a)) if !a.is_empty() => a
            .iter()
            .map(|v| {
                v.as_str()
                    .filter(|s| STRATEGIES.contains(s))
                    .map(str::to_string)
                    .ok_or_else(|| {
                        error(format!(
                            "strategies must be from: {}",
                            STRATEGIES.join(", ")
                        ))
                    })
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(error("strategies must be a nonempty list")),
    };
    let params = raw_params(doc)?;
    let analysis = crate::raw_looks::analyze_document(doc)?;
    let light_left = brighter_left(doc);
    let mut keys: Vec<&str> = Vec::new();
    for r in &requested {
        if r == "auto" {
            keys.extend(auto_strategies(&analysis));
        } else {
            keys.push(STRATEGIES.iter().find(|s| *s == r).unwrap());
        }
    }
    keys.dedup();
    let mut edits = current_edits(&params)?;
    let mut regions = Regions::default();
    let mut applied = Vec::new();
    let mut skipped = Vec::new();
    for key in keys {
        let plans = strategy(key, &analysis, light_left);
        if plans.is_empty() {
            skipped.push(json!({"strategy": key, "why": "no matching colours in this photo"}));
        }
        for p in plans {
            let parts = match components(&p.components, doc, &params, true, &mut regions) {
                Ok(parts) => parts,
                Err(e) if matches!(key, "eyes" | "teeth") => {
                    skipped.push(json!({"strategy": key, "why": e.content[0]["text"]}));
                    continue;
                }
                Err(e) => return Err(e),
            };
            // Strategies are idempotent: re-running replaces their own masks.
            edits.masks.retain(|m| m.name != p.name);
            let mut mask = Mask {
                id: edits.masks.iter().map(|m| m.id).max().unwrap_or(0) + 1,
                name: p.name.into(),
                enabled: true,
                components: parts,
                exposure: 0.,
                contrast: 0.,
                saturation: 0.,
                temperature: 0.,
                tint: 0.,
                highlights: 0.,
                shadows: 0.,
            };
            set_adjustments(&mut mask, &p.adjustments, strength)?;
            applied.push(json!({"strategy": key, "mask": p.name, "why": p.why}));
            edits.masks.push(mask);
        }
    }
    edits.validate().map_err(error)?;
    commit(
        doc,
        params,
        edits,
        json!({"applied": applied, "skipped": skipped, "strength": strength, "scene": analysis.scene, "notes": regions.notes}),
    )
}

/// Which half of the developed view is brighter: the side light comes from.
fn brighter_left(doc: &Document) -> bool {
    let Some(raw) = doc.raw.as_ref() else {
        return true;
    };
    let Some(NodeKind::Raster { raster, .. }) = doc.node(raw.node_id).map(|n| &n.kind) else {
        return true;
    };
    let (w, h) = (raster.width(), raster.height());
    let step = (w.max(h) / 128).max(1);
    let mut sums = [0f64; 2];
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let p = raster.get(x, y);
            sums[(x >= w / 2) as usize] += (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.;
        }
    }
    sums[0] >= sums[1]
}

// ------------------------------------------------------------ inspection

pub fn list(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("Arguments must be an object"))?;
    for key in object.keys() {
        if !["overlay", "id", "max_size"].contains(&key.as_str()) {
            return Err(error(format!("Unknown argument '{key}'")));
        }
    }
    let params = raw_params(doc)?;
    let edits = current_edits(&params)?;
    let mut result = ToolResult::text(
        json!({"masks": summary(&edits), "healing_spots": edits.spots.len(),
            "coordinates": "mask_raw takes view coordinates (0..1 of the cropped, rotated photo) by default"})
        .to_string(),
    );
    if !args
        .get("overlay")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(result);
    }
    let selected = args.get("id").and_then(Value::as_u64).map(|v| v as u32);
    if selected.is_some_and(|id| !edits.masks.iter().any(|m| m.id == id)) {
        return Err(error("No such mask; see the masks list"));
    }
    let max = num(args, "max_size", 64., 1568., 768.)? as u32;
    let raw = doc.raw.as_ref().unwrap();
    let Some(NodeKind::Raster { raster, .. }) = doc.node(raw.node_id).map(|n| &n.kind) else {
        return Err(error("RAW node has no developed pixels"));
    };
    let bitmaps = develop_edits::bitmaps(&edits).map_err(error)?;
    let (w, h) = (raster.width(), raster.height());
    let scale = (max as f32 / w.max(h) as f32).min(1.);
    let (ow, oh) = (
        ((w as f32 * scale) as u32).max(1),
        ((h as f32 * scale) as u32).max(1),
    );
    let mut image = image::RgbaImage::new(ow, oh);
    for (x, y, out) in image.enumerate_pixels_mut() {
        let sx = ((x as f32 + 0.5) / scale) as u32;
        let sy = ((y as f32 + 0.5) / scale) as u32;
        let p = raster.get(sx.min(w - 1), sy.min(h - 1));
        let lin = [p[0], p[1], p[2]].map(|v| v as f32 / 65535.);
        let xy = view_to_source(
            &params,
            [(x as f32 + 0.5) / ow as f32, (y as f32 + 0.5) / oh as f32],
        );
        let alpha = edits
            .masks
            .iter()
            .filter(|m| selected.is_none_or(|id| id == m.id))
            .map(|m| develop_edits::weight(m, xy, lin, &bitmaps))
            .fold(0., f32::max)
            * 0.55;
        let rgb = lin.map(encode);
        let tint = [1., 0.1, 0.35];
        let [r, g, b] = [0, 1, 2].map(|c| ((rgb[c] * (1. - alpha) + tint[c] * alpha) * 255.) as u8);
        *out = image::Rgba([r, g, b, 255]);
    }
    result.content.push(crate::preview::png_block(&image)?);
    Ok(result)
}

pub fn definitions() -> Vec<ToolDef> {
    let def = |name: &str, description: &str, properties: Value, required: Value| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    };
    let component = json!({"type":"object","required":["shape"],"properties":{
        "shape":{"type":"string","enum":["subject","background","sky","face","eyes","teeth","all","radial","linear","brush","luminance","color"]},
        "operation":{"type":"string","enum":["add","subtract","intersect"],"default":"add"},
        "invert":{"type":"boolean","default":false,"description":"Invert this part: allowed on the first (add) part or on later subtract/intersect parts"},
        "center":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
        "radius":{"type":["number","array"],"description":"radial radius, fraction of the frame; number or [rx, ry]"},
        "start":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2,"description":"linear: effect is zero here"},
        "end":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2,"description":"linear: effect is full here and beyond"},
        "points":{"type":"array","items":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}},
        "size":{"type":"number","description":"brush radius, fraction of the frame"},
        "range":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2,"description":"luminance: display brightness [low, high] 0..1"},
        "color":{"description":"color range: name (red, orange, yellow, green, aqua, blue, purple, magenta, skin), #rrggbb or [r,g,b]"},
        "tolerance":{"type":"number","minimum":0.01,"maximum":1},
        "feather":{"type":"number","minimum":0,"maximum":1}}});
    let adjustments = json!({"type":"object","additionalProperties":false,"properties":{
        "exposure":{"type":"number","minimum":-5,"maximum":5},"contrast":{"type":"number","minimum":-1,"maximum":1},
        "highlights":{"type":"number","minimum":-1,"maximum":1},"shadows":{"type":"number","minimum":-1,"maximum":1},
        "saturation":{"type":"number","minimum":-1,"maximum":1},"temperature":{"type":"number","minimum":-1,"maximum":1},
        "tint":{"type":"number","minimum":-1,"maximum":1}}});
    vec![
        def(
            "mask_raw",
            "Add, update, remove or clear a local RAW mask, like Lightroom masking, in one undo step. A mask is a list of components combined in order: subject, background, sky, face, eyes, teeth (AI, with labelled approximations when a model is missing), all, radial, linear, brush, luminance range or color range, with add/subtract/intersect and invert. Each mask adjusts exposure, contrast, highlights, shadows, saturation, temperature and tint inside it. Coordinates are 0..1 of the cropped, rotated photo as get_view shows it (space=view), or of the uncropped original (space=source).",
            json!({"action":{"type":"string","enum":["add","update","remove","clear"]},"id":{"type":"integer","minimum":1},
                "name":{"type":"string","minLength":1,"maxLength":200},"components":{"type":"array","minItems":1,"maxItems":16,"items":component},
                "adjustments":adjustments,"enabled":{"type":"boolean"},"space":{"type":"string","enum":["view","source"],"default":"view"}}),
            json!(["action"]),
        ),
        def(
            "auto_mask_raw",
            "Apply proven masking strategies in one undo step: subject_pop (brighter subject), background_recede (darker, quieter background), sky (recover and enrich sky), vignette_focus (darker, desaturated edges), directional_light (warm light side, cool shadow side for flat light), color_range (warm yellows, deepen reds, cool blues), color_separation (warm foreground, cool distance), tonal_balance (open shadows, hold highlights by luminance range), eyes, teeth, or auto (chosen from photo analysis). Re-running replaces a strategy's own masks; other masks are kept. strength 0–1.5 scales the adjustments.",
            json!({"strategies":{"type":"array","minItems":1,"items":{"type":"string","enum":STRATEGIES}},"strength":{"type":"number","minimum":0,"maximum":1.5,"default":1}}),
            json!([]),
        ),
        def(
            "list_raw_masks",
            "List the RAW photo's local masks (id, name, components, adjustments). overlay=true adds a PNG of the photo with the masks (or mask id) tinted, to check placement and edges before refining. Read-only.",
            json!({"overlay":{"type":"boolean","default":false},"id":{"type":"integer","minimum":1},"max_size":{"type":"integer","minimum":64,"maximum":1568,"default":768}}),
            json!([]),
        ),
    ]
}

#[cfg(test)]
#[path = "raw_masks_tests.rs"]
mod tests;
