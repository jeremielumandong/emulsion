use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use emulsion_core::{
    Node,
    design_keyframes::{Easing, Keyframe, Property, Track},
};
use emulsion_raster::{
    Raster,
    composite::Placement,
    vector::{
        Anchor, GradientStop, Path as VectorPath, PathPaint, PathStyle, StrokeCap, StrokeJoin,
        SubPath,
    },
};
use glam::{DAffine2, dvec2};
use serde_json::json;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};
fn number(v: &Value, default: f64) -> Result<f64> {
    if v.is_null() {
        return Ok(default);
    }
    let n = v
        .as_f64()
        .ok_or_else(|| error("Expected a finite number"))?;
    if !n.is_finite() || n.abs() > 1e7 {
        return Err(error("Number is outside supported range"));
    }
    Ok(n)
}
fn scalar(v: &Value, default: f64) -> Result<f64> {
    number(if v.is_array() { &v[0] } else { v }, default)
}
fn xy(v: &Value, default: (f64, f64)) -> Result<(f64, f64)> {
    if v.is_null() {
        return Ok(default);
    }
    Ok((number(&v[0], default.0)?, number(&v[1], default.1)?))
}
fn bezier(x: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let curve = |t: f64, a: f64, b: f64| {
        3. * (1. - t).powi(2) * t * a + 3. * (1. - t) * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..28 {
        let m = (lo + hi) / 2.;
        if curve(m, x1, x2) < x { lo = m } else { hi = m }
    }
    curve((lo + hi) / 2., y1, y2)
}
fn component(v: &Value, axis: usize, default: f64) -> f64 {
    v.as_array()
        .and_then(|a| a.get(axis).or(a.first()))
        .unwrap_or(v)
        .as_f64()
        .unwrap_or(default)
}
/// Evaluate temporal cubic easing and optional spatial position tangents.
fn value(prop: &Value, t: f64) -> Result<Value> {
    if prop.is_null() {
        return Ok(Value::Null);
    }
    if prop["a"].as_u64() != Some(1) {
        return Ok(prop.get("k").unwrap_or(prop).clone());
    }
    let keys = prop["k"]
        .as_array()
        .ok_or_else(|| error("Invalid animated property"))?;
    if keys.is_empty() || keys.len() > 600 {
        return Err(error("Property requires 1–600 keyframes"));
    }
    for pair in keys.windows(2) {
        if number(&pair[0]["t"], 0.)? >= number(&pair[1]["t"], 0.)? {
            return Err(error("Keyframes must have strictly increasing times"));
        }
    }
    let i = keys
        .iter()
        .rposition(|k| k["t"].as_f64().unwrap_or(0.) <= t)
        .unwrap_or(0);
    let left = &keys[i];
    let start = left
        .get("s")
        .or_else(|| i.checked_sub(1).and_then(|p| keys[p].get("e")))
        .cloned()
        .unwrap_or(Value::Null);
    let Some(right) = keys.get(i + 1) else {
        return Ok(start);
    };
    if left["h"] == 1 {
        return Ok(start);
    }
    let a = number(&left["t"], 0.)?;
    let b = number(&right["t"], a)?;
    let u = if b > a {
        ((t - a) / (b - a)).clamp(0., 1.)
    } else {
        0.
    };
    let end = left.get("e").or_else(|| right.get("s")).unwrap_or(&start);
    let mix = |a: f64, b: f64, axis: usize| {
        let x1 = component(&left["o"]["x"], axis, 0.333).clamp(0., 1.);
        let y1 = component(&left["o"]["y"], axis, 0.333);
        let x2 = component(&left["i"]["x"], axis, 0.667).clamp(0., 1.);
        let y2 = component(&left["i"]["y"], axis, 0.667);
        let f = bezier(u, x1, y1, x2, y2);
        a + (b - a) * f
    };
    if let Some(values) = start.as_array() {
        let mut out = Vec::new();
        for (axis, v) in values.iter().enumerate() {
            if !v.is_number() {
                return Ok(start.clone());
            }
            let a = number(v, 0.)?;
            let b = number(&end[axis], a)?;
            if left["to"].is_array() && left["ti"].is_array() {
                let f = mix(0., 1., axis);
                let c = a + number(&left["to"][axis], 0.)?;
                let d = b + number(&left["ti"][axis], 0.)?;
                out.push(json!(
                    (1. - f).powi(3) * a
                        + 3. * (1. - f).powi(2) * f * c
                        + 3. * (1. - f) * f * f * d
                        + f.powi(3) * b
                ));
            } else {
                out.push(json!(mix(a, b, axis)));
            }
        }
        Ok(Value::Array(out))
    } else {
        Ok(json!(mix(number(&start, 0.)?, number(end, 0.)?, 0)))
    }
}
fn transform(v: &Value, t: f64) -> Result<DAffine2> {
    let p = if v["p"]["s"].as_bool() == Some(true) {
        (
            scalar(&value(&v["p"]["x"], t)?, 0.)?,
            scalar(&value(&v["p"]["y"], t)?, 0.)?,
        )
    } else {
        xy(&value(&v["p"], t)?, (0., 0.))?
    };
    let a = xy(&value(&v["a"], t)?, (0., 0.))?;
    let s = xy(&value(&v["s"], t)?, (100., 100.))?;
    let r = scalar(&value(&v["r"], t)?, 0.)?;
    if s.0 <= 0. || s.1 <= 0. {
        return Err(error(
            "Zero or reflected scale is not representable by native keyframes",
        ));
    }
    Ok(DAffine2::from_translation(dvec2(p.0, p.1))
        * DAffine2::from_angle(r.to_radians())
        * DAffine2::from_scale(dvec2(s.0 / 100., s.1 / 100.))
        * DAffine2::from_translation(-dvec2(a.0, a.1)))
}
fn color(v: &Value, opacity: f64) -> Result<[u8; 4]> {
    let mut c = [0, 0, 0, 255];
    for i in 0..3 {
        c[i] = (number(&v[i], 0.)?.clamp(0., 1.) * 255.).round() as u8;
    }
    c[3] = (opacity.clamp(0., 1.) * number(&v[3], 1.)?.clamp(0., 1.) * 255.).round() as u8;
    Ok(c)
}
struct Reader<'a> {
    doc: Document,
    report: Report,
    root: &'a Value,
    fps: f64,
    start: f64,
    end: f64,
    visiting: HashSet<String>,
    budget: usize,
    images: HashMap<String, Arc<Raster>>,
    image_pixels: u64,
}
impl Reader<'_> {
    fn add(&mut self, mut node: Node, parent: Option<NodeId>) -> Result<NodeId> {
        if self.doc.nodes.len() >= 2000 {
            return Err(error("At most 2000 editable objects can be imported"));
        }
        node.id = self.doc.alloc_id();
        node.parent = parent;
        let id = node.id;
        self.doc.nodes.push(node);
        Ok(id)
    }
    fn warn(&mut self, label: &str, s: &str) {
        self.report.warn(format!("{label}: {s}"));
    }
    fn shapes(&mut self, items: &[Value], parent: NodeId, m: DAffine2, depth: usize) -> Result<()> {
        if depth > 32 {
            return Err(error("Shape groups nest too deeply"));
        }
        let label = self
            .doc
            .node(parent)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        let mut style = PathStyle {
            fill: None,
            stroke: None,
            ..Default::default()
        };
        let mut local = m;
        for item in items {
            if item["ty"] == "tr" {
                if item.get("sk").is_some() {
                    self.warn(&label, "shape-group skew is not imported");
                }
                local = m * transform(item, self.start)?;
                if contains_animation(item) {
                    self.warn(
                        &label,
                        "animated shape-group transforms use their initial value",
                    );
                }
                self.doc.node_mut(parent).unwrap().opacity *=
                    (scalar(&value(&item["o"], self.start)?, 100.)? / 100.).clamp(0., 1.) as f32;
            }
        }
        for item in items {
            let ty = item["ty"].as_str().unwrap_or("");
            if matches!(ty, "fl" | "st" | "gf" | "gs") {
                if contains_animation(item) {
                    self.warn(&label, "animated paint uses its initial value");
                }
                let o = scalar(&value(&item["o"], self.start)?, 100.)? / 100.;
                let mut c = color(&value(&item["c"], self.start)?, o)?;
                let mut paint = PathPaint::Solid;
                if matches!(ty, "gf" | "gs") {
                    let count = item["g"]["p"].as_u64().unwrap_or(0) as usize;
                    let values = value(&item["g"]["k"], self.start)?;
                    if !(2..=16).contains(&count) {
                        return Err(error("Gradients require 2–16 color stops"));
                    }
                    let mut stops = Vec::new();
                    for i in 0..count {
                        let offset = number(&values[i * 4], 0.)?;
                        let rgba = color(
                            &json!([values[i * 4 + 1], values[i * 4 + 2], values[i * 4 + 3]]),
                            o,
                        )?;
                        stops.push(GradientStop {
                            offset: offset as f32,
                            color: rgba,
                        });
                    }
                    if let Some(extra) = values
                        .as_array()
                        .and_then(|v| v.get(count * 4..))
                        .filter(|v| !v.is_empty())
                    {
                        if extra.len() % 2 != 0 || extra.len() > 32 {
                            return Err(error("Invalid gradient opacity stops"));
                        }
                        let alpha: Vec<_> = extra
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|v| Ok((number(&v[0], 0.)?, number(&v[1], 1.)?)))
                            .collect::<Result<_>>()?;
                        if alpha.windows(2).any(|a| a[0].0 > a[1].0) {
                            return Err(error("Gradient opacity stops must be ordered"));
                        }
                        for stop in &mut stops {
                            let x = f64::from(stop.offset);
                            let i = alpha.iter().rposition(|p| p.0 <= x).unwrap_or(0);
                            let (a, av) = alpha[i];
                            let opacity = if let Some((b, bv)) = alpha.get(i + 1) {
                                let t = if *b > a {
                                    ((x - a) / (*b - a)).clamp(0., 1.)
                                } else {
                                    0.
                                };
                                av + (*bv - av) * t
                            } else {
                                av
                            };
                            stop.color[3] =
                                (f64::from(stop.color[3]) * opacity.clamp(0., 1.)).round() as u8;
                        }
                    }
                    let s = xy(&value(&item["s"], self.start)?, (0., 0.))?;
                    let e = xy(&value(&item["e"], self.start)?, (100., 0.))?;
                    paint = PathPaint::from_stops(
                        &stops,
                        item["t"] == 2,
                        (e.1 - s.1).atan2(e.0 - s.0).to_degrees() as f32,
                    )
                    .map_err(error)?;
                    c = stops[0].color;
                    self.warn(
                        &label,
                        "gradient extent is fitted to the native shape bounds",
                    );
                }
                if matches!(ty, "fl" | "gf") {
                    style.fill = Some(c);
                    style.fill_paint = paint;
                    style.even_odd = item["r"] == 2;
                } else {
                    style.stroke = Some(c);
                    style.stroke_paint = paint;
                    style.width = scalar(&value(&item["w"], self.start)?, 1.)? as f32;
                    style.cap = match item["lc"].as_u64() {
                        Some(1) => StrokeCap::Butt,
                        Some(3) => StrokeCap::Square,
                        _ => StrokeCap::Round,
                    };
                    style.join = match item["lj"].as_u64() {
                        Some(1) => StrokeJoin::Miter,
                        Some(3) => StrokeJoin::Bevel,
                        _ => StrokeJoin::Round,
                    };
                    if item["d"].is_array() {
                        self.warn(&label, "dash operators are not imported");
                    }
                }
            }
        }
        // Paths in one paint scope form one compound native path, preserving holes.
        let mut compound = VectorPath::default();
        let mut compound_name = label.clone();
        let mut shape_slot = None;
        // Lottie shapes are ordered front to back; native siblings are bottom to top.
        for item in items.iter().rev() {
            self.budget += 1;
            if self.budget > 10000 {
                return Err(error("Too many Lottie shape operators"));
            }
            let name = item["nm"].as_str().unwrap_or("Lottie shape");
            if item["hd"].as_bool() == Some(true) {
                continue;
            }
            let mut path = match item["ty"].as_str().unwrap_or("") {
                "gr" => {
                    let group = self.add(Node::group(0, name), Some(parent))?;
                    if let Some(children) = item["it"].as_array() {
                        self.shapes(children, group, local, depth + 1)?;
                    }
                    continue;
                }
                "sh" => {
                    if item["ks"]["a"] == 1 {
                        self.warn(name, "path morph animation uses its initial geometry");
                    }
                    let mut shape = value(&item["ks"], self.start)?;
                    if shape.is_array() {
                        shape = shape[0].clone();
                    }
                    parse_path(&shape)?
                }
                "rc" | "el" => {
                    let p = xy(&value(&item["p"], self.start)?, (0., 0.))?;
                    let s = xy(&value(&item["s"], self.start)?, (0., 0.))?;
                    if contains_animation(item) {
                        self.warn(name, "animated shape geometry uses its initial value");
                    }
                    if item["ty"] == "el" {
                        emulsion_raster::vector_geometry::ellipse(
                            p.0 - s.0 / 2.,
                            p.1 - s.1 / 2.,
                            s.0,
                            s.1,
                        )
                    } else {
                        if scalar(&value(&item["r"], self.start)?, 0.)? > 0. {
                            self.warn(name, "rounded rectangle radius is not imported");
                        }
                        emulsion_raster::vector_geometry::rectangle(
                            p.0 - s.0 / 2.,
                            p.1 - s.1 / 2.,
                            s.0,
                            s.1,
                        )
                    }
                }
                "tr" | "fl" | "st" | "gf" | "gs" => continue,
                ty => {
                    self.warn(name, &format!("unsupported shape operator {ty}"));
                    continue;
                }
            };
            path.transform(local);
            if shape_slot.is_none() {
                shape_slot = Some(self.doc.nodes.len());
                compound_name = name.into();
            }
            compound.subpaths.extend(path.subpaths);
            if compound.anchor_count() > 20000 {
                return Err(error("Compound path exceeds 20000 anchors"));
            }
        }
        if let Some(slot) = shape_slot {
            let mut transformed_style = style;
            transformed_style.width *= local.matrix2.determinant().abs().sqrt() as f32;
            self.add(
                Node::path(
                    0,
                    compound_name,
                    Arc::new(compound),
                    transformed_style,
                    self.doc.width,
                    self.doc.height,
                ),
                Some(parent),
            )?;
            let node = self.doc.nodes.pop().unwrap();
            self.doc.nodes.insert(slot, node);
        }
        Ok(())
    }
    fn layers(
        &mut self,
        layers: &[Value],
        parent: Option<NodeId>,
        prefix: DAffine2,
        depth: usize,
    ) -> Result<()> {
        if depth > 16 || layers.len() > 512 {
            return Err(error("Composition complexity exceeds import limit"));
        }
        for layer in layers.iter().rev() {
            let label = layer["nm"].as_str().unwrap_or("Lottie layer");
            let ty = layer["ty"].as_u64().unwrap_or(99);
            if layer["ks"].get("sk").is_some()
                || layer["ks"].get("rx").is_some()
                || layer["ks"].get("ry").is_some()
            {
                self.warn(label, "skew and 3D rotations are not imported");
            }
            if layer["bm"].as_u64().is_some_and(|mode| mode != 0) {
                self.warn(label, "blend mode is imported as normal");
            }
            if layer["ao"] == 1 {
                self.warn(label, "auto orientation is not imported");
            }
            if layer["ddd"] == 1 {
                self.warn(label, "3D layer skipped");
                continue;
            }
            for key in ["masksProperties", "ef", "tt", "tm"] {
                if layer
                    .get(key)
                    .is_some_and(|v| !v.is_null() && v != &json!(0) && v != &json!([]))
                {
                    self.warn(label, &format!("unsupported {key}"));
                }
            }
            if !matches!(ty, 0..=5) {
                self.warn(label, &format!("unsupported layer type {ty}"));
                continue;
            }
            let group = self.add(Node::group(0, label), parent)?;
            match ty {
                4 => {
                    if let Some(items) = layer["shapes"].as_array() {
                        self.shapes(items, group, DAffine2::IDENTITY, 0)?;
                    }
                }
                1 => {
                    let w = number(&layer["sw"], self.doc.width as f64)?;
                    let h = number(&layer["sh"], self.doc.height as f64)?;
                    let s = layer["sc"].as_str().unwrap_or("#000000");
                    let c = hex_color(s)?;
                    self.add(
                        Node::path(
                            0,
                            label,
                            Arc::new(emulsion_raster::vector_geometry::rectangle(0., 0., w, h)),
                            PathStyle {
                                fill: Some(c),
                                stroke: None,
                                ..Default::default()
                            },
                            self.doc.width,
                            self.doc.height,
                        ),
                        Some(group),
                    )?;
                }
                2 => {
                    let key = layer["refId"]
                        .as_str()
                        .ok_or_else(|| error("Image asset identifier must be a string"))?
                        .to_string();
                    let raster = if let Some(raster) = self.images.get(&key) {
                        Some(raster.clone())
                    } else {
                        let uri = self.asset(layer)?["p"].as_str().unwrap_or("").to_string();
                        if !uri.starts_with("data:image/") {
                            self.warn(
                                label,
                                "external image asset skipped; embed image data before importing",
                            );
                            None
                        } else {
                            let (_, payload) = uri
                                .split_once(";base64,")
                                .ok_or_else(|| error("Invalid embedded image"))?;
                            let bytes = STANDARD
                                .decode(payload)
                                .map_err(|_| error("Invalid embedded image base64"))?;
                            if bytes.len() > 32 << 20 {
                                return Err(error("Embedded image exceeds 32 MiB"));
                            }
                            let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
                                .with_guessed_format()?
                                .into_dimensions()?;
                            crate::import::check_size(w, h)?;
                            let pixels = u64::from(w) * u64::from(h);
                            if pixels > 16_000_000
                                || self.image_pixels.saturating_add(pixels) > 32_000_000
                            {
                                return Err(error(
                                    "Embedded images exceed 16 megapixels per asset or 32 megapixels total",
                                ));
                            }
                            self.image_pixels += pixels;
                            let image = image::ImageReader::new(std::io::Cursor::new(bytes))
                                .with_guessed_format()?
                                .decode()?
                                .to_rgba8();
                            let raster = Arc::new(Raster::from_srgba8(w, h, &image));
                            self.images.insert(key, raster.clone());
                            Some(raster)
                        }
                    };
                    if let Some(raster) = raster {
                        self.add(
                            Node::raster(0, label, raster, Placement::default()),
                            Some(group),
                        )?;
                    }
                }
                5 => {
                    let spec = &layer["t"]["d"]["k"][0]["s"];
                    let text = spec["t"].as_str().unwrap_or("");
                    if text.chars().count() > 20000 {
                        return Err(error("Text exceeds native length limit"));
                    }
                    let font_name = spec["f"].as_str().unwrap_or("");
                    let font = self.root["fonts"]["list"]
                        .as_array()
                        .and_then(|f| f.iter().find(|f| f["fName"] == font_name))
                        .and_then(|f| f["fFamily"].as_str())
                        .unwrap_or(font_name);
                    let size = number(&spec["s"], 24.)? as f32;
                    self.add(
                        Node::text(
                            0,
                            label,
                            emulsion_core::text::TextSpec {
                                text: text.replace('\r', "\n"),
                                font: font.into(),
                                size,
                                color: color(&spec["fc"], 1.)?,
                                x: number(&spec["ps"][0], 0.)? as f32,
                                y: number(&spec["ps"][1], 0.)? as f32 - size,
                                ..Default::default()
                            },
                            self.doc.width,
                            self.doc.height,
                        ),
                        Some(group),
                    )?;
                    self.warn(label,"text uses installed fonts; baseline metrics and advanced text animators may differ");
                }
                0 => {
                    let asset = self.asset(layer)?.clone();
                    let key = layer["refId"].as_str().unwrap_or("").to_string();
                    if !self.visiting.insert(key.clone()) {
                        return Err(error("Cyclic precomposition"));
                    }
                    if let Some(layers) = asset["layers"].as_array() {
                        self.layers(layers, Some(group), DAffine2::IDENTITY, depth + 1)?;
                    }
                    self.visiting.remove(&key);
                    if layer.get("st").is_some_and(|v| v != &json!(0))
                        || layer.get("sr").is_some_and(|v| v != &json!(1))
                    {
                        self.warn(
                            label,
                            "precomposition time stretch/start offset is not imported",
                        );
                    }
                }
                _ => {}
            }
            let mut ancestor = prefix;
            let mut current = layer;
            let mut parents = HashSet::new();
            while let Some(index) = current["parent"].as_i64() {
                if !parents.insert(index) {
                    return Err(error("Cyclic layer parent"));
                }
                let p = layers
                    .iter()
                    .find(|p| p["ind"].as_i64() == Some(index))
                    .ok_or_else(|| error("Missing layer parent"))?;
                if contains_animation(&p["ks"]) {
                    self.warn(label, "animated parent transform uses its initial value");
                }
                ancestor = transform(&p["ks"], self.start)? * ancestor;
                current = p;
            }
            self.doc.normalize();
            let combined = ancestor * transform(&layer["ks"], self.start)?;
            // Nested animated groups cannot compose arbitrary pivots in the current native model.
            if depth > 0 && contains_animation(&layer["ks"]) {
                self.warn(
                    label,
                    "nested composition animation uses its initial transform",
                );
            }
            if contains_animation(&layer["ks"]) && depth == 0 && ancestor == DAffine2::IDENTITY {
                if emulsion_core::geometry::node_bounds(&self.doc, group)?.is_some() {
                    emulsion_core::transform::transform_nodes(
                        &mut self.doc,
                        &[group],
                        combined.to_cols_array(),
                    )
                    .map_err(|e| error(e.to_string()))?;
                }
                self.tracks(group, layer, combined)?;
            } else if emulsion_core::geometry::node_bounds(&self.doc, group)?.is_some() {
                emulsion_core::transform::transform_nodes(
                    &mut self.doc,
                    &[group],
                    combined.to_cols_array(),
                )
                .map_err(|e| error(e.to_string()))?;
                self.doc.node_mut(group).unwrap().opacity *=
                    (scalar(&value(&layer["ks"]["o"], self.start)?, 100.)? / 100.).clamp(0., 1.)
                        as f32;
            }
            self.doc.node_mut(group).unwrap().visible = layer["hd"].as_bool() != Some(true);
            self.timing(group, layer)?;
        }
        Ok(())
    }
    fn asset(&self, layer: &Value) -> Result<&Value> {
        self.root["assets"]
            .as_array()
            .and_then(|a| a.iter().find(|a| a["id"] == layer["refId"]))
            .ok_or_else(|| error("Missing referenced asset"))
    }
    fn tracks(&mut self, id: NodeId, layer: &Value, base: DAffine2) -> Result<()> {
        let Some(bounds) = emulsion_core::geometry::node_bounds(&self.doc, id)? else {
            return Ok(());
        };
        let center = dvec2(
            bounds.x as f64 + bounds.w as f64 / 2.,
            bounds.y as f64 + bounds.h as f64 / 2.,
        );
        let count = ((self.end - self.start).ceil() as usize).clamp(2, 64);
        let mut times = BTreeSet::new();
        for i in 0..count {
            times.insert(
                (self.doc.design.duration_ms as f64 * i as f64 / (count - 1) as f64).round() as u32,
            );
        }
        let mut tracks: Vec<_> = [
            Property::TranslationX,
            Property::TranslationY,
            Property::ScaleX,
            Property::ScaleY,
            Property::Rotation,
            Property::Opacity,
        ]
        .into_iter()
        .map(|property| Track {
            property,
            frames: vec![],
        })
        .collect();
        for ms in times {
            let time = self.start + ms as f64 * self.fps / 1000.;
            let m = transform(&layer["ks"], time)? * base.inverse();
            if m.matrix2
                .x_axis
                .normalize()
                .dot(m.matrix2.y_axis.normalize())
                .abs()
                > 1e-6
            {
                self.warn(layer["nm"].as_str().unwrap_or("Layer"),"animated nonuniform rotated scaling requires shear; animation uses initial transform");
                return Ok(());
            }
            let (scale, angle, _) = m.to_scale_angle_translation();
            let position = m.transform_point2(center) - center;
            let values = [
                position.x,
                position.y,
                scale.x,
                scale.y,
                angle.to_degrees(),
                scalar(&value(&layer["ks"]["o"], time)?, 100.)? / 100.,
            ];
            for (track, value) in tracks.iter_mut().zip(values) {
                track.frames.push(Keyframe {
                    time_ms: ms,
                    value,
                    easing: Easing::Linear,
                });
            }
        }
        // Keep rotation turns continuous instead of snapping at ±180 degrees.
        let mut previous = 0.;
        for (i, frame) in tracks[4].frames.iter_mut().enumerate() {
            if i > 0 {
                while frame.value - previous > 180. {
                    frame.value -= 360.;
                }
                while frame.value - previous < -180. {
                    frame.value += 360.;
                }
            }
            previous = frame.value;
        }
        tracks.retain(|t| {
            t.frames
                .iter()
                .any(|f| (f.value - t.property.initial()).abs() > 1e-8)
        });
        if !tracks.is_empty() {
            self.doc.design.keyframes.insert(id, tracks);
        }
        self.warn(
            layer["nm"].as_str().unwrap_or("Layer"),
            "temporal/spatial easing sampled to up to 64 editable native keys per property",
        );
        Ok(())
    }
    fn timing(&mut self, id: NodeId, layer: &Value) -> Result<()> {
        let ip = number(&layer["ip"], self.start)?;
        let op = number(&layer["op"], self.end)?;
        if ip <= self.start && op >= self.end {
            return Ok(());
        }
        let ms = |t: f64| {
            (((t - self.start) * 1000. / self.fps)
                .round()
                .clamp(0., self.doc.design.duration_ms as f64)) as u32
        };
        let mut frames = vec![Keyframe {
            time_ms: 0,
            value: if ip <= self.start && op > self.start {
                1.
            } else {
                0.
            },
            easing: Easing::Step,
        }];
        if ip > self.start && ip < self.end {
            frames.push(Keyframe {
                time_ms: ms(ip),
                value: 1.,
                easing: Easing::Step,
            });
        }
        if op > self.start && op < self.end {
            frames.push(Keyframe {
                time_ms: ms(op),
                value: 0.,
                easing: Easing::Step,
            });
        }
        frames.sort_by_key(|f| f.time_ms);
        frames.dedup_by_key(|f| f.time_ms);
        self.doc
            .design
            .keyframes
            .entry(id)
            .or_default()
            .push(Track {
                property: Property::Visibility,
                frames,
            });
        Ok(())
    }
}
fn contains_animation(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.get("a") == Some(&json!(1)) || m.values().any(contains_animation),
        Value::Array(a) => a.iter().any(contains_animation),
        _ => false,
    }
}
fn parse_path(v: &Value) -> Result<VectorPath> {
    let points = v["v"]
        .as_array()
        .ok_or_else(|| error("Path vertices missing"))?;
    if points.len() > 20000
        || v["i"].as_array().map(Vec::len) != Some(points.len())
        || v["o"].as_array().map(Vec::len) != Some(points.len())
    {
        return Err(error("Invalid cubic path handles"));
    }
    let anchors = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let p = xy(p, (0., 0.))?;
            let incoming = xy(&v["i"][i], (0., 0.))?;
            let outgoing = xy(&v["o"][i], (0., 0.))?;
            Ok(Anchor {
                p,
                h_in: (p.0 + incoming.0, p.1 + incoming.1),
                h_out: (p.0 + outgoing.0, p.1 + outgoing.1),
                smooth: false,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(VectorPath {
        subpaths: vec![SubPath {
            anchors,
            closed: v["c"].as_bool().unwrap_or(false),
        }],
    })
}
fn hex_color(s: &str) -> Result<[u8; 4]> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 || !s.is_ascii() {
        return Err(error("Invalid solid color"));
    }
    Ok([
        u8::from_str_radix(&s[..2], 16).map_err(|_| error("Invalid color"))?,
        u8::from_str_radix(&s[2..4], 16).map_err(|_| error("Invalid color"))?,
        u8::from_str_radix(&s[4..6], 16).map_err(|_| error("Invalid color"))?,
        255,
    ])
}
pub(super) fn decode(root: &Value) -> Result<(Document, Report)> {
    let width = number(&root["w"], 0.)?;
    let height = number(&root["h"], 0.)?;
    if width.fract() != 0. || height.fract() != 0. || width < 1. || height < 1. {
        return Err(error("Invalid composition dimensions"));
    }
    crate::import::check_size(width as u32, height as u32)?;
    let fps = number(&root["fr"], 0.)?;
    let start = number(&root["ip"], 0.)?;
    let end = number(&root["op"], 0.)?;
    let duration = (end - start) * 1000. / fps;
    if !(1. ..=60.).contains(&fps) || !(100. ..=60000.).contains(&duration) {
        return Err(error("Use 1–60 fps and a 100ms–60s composition"));
    }
    let layers = root["layers"]
        .as_array()
        .ok_or_else(|| error("Missing composition layers"))?;
    let mut reader = Reader {
        doc: Document::new(width as u32, height as u32),
        report: Report::default(),
        root,
        fps,
        start,
        end,
        visiting: HashSet::new(),
        budget: 0,
        images: HashMap::new(),
        image_pixels: 0,
    };
    reader.doc.design.duration_ms = duration.round() as u32;
    reader.doc.design.fps = fps.round() as u32;
    if fps.fract() != 0. {
        reader.report.warn("Fractional frame rate rounded to native whole-number fps; keyframe millisecond timing is preserved.");
    }
    reader.layers(layers, None, DAffine2::IDENTITY, 0)?;
    reader.doc.normalize();
    reader.doc.validate()?;
    reader.report.nodes = reader.doc.nodes.len();
    reader.report.animated_nodes = reader.doc.design.keyframes.len();
    Ok((reader.doc, reader.report))
}
