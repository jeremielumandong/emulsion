//! SVG import as editable Path nodes.
//!
//! Paths, rectangles, circles, ellipses, lines, polylines and polygons
//! become Path nodes with their fill and stroke; groups and `transform`
//! attributes (translate, scale, rotate, matrix) are applied. The canvas
//! takes the SVG's width and height, or its viewBox. Text, gradients,
//! filters and clip paths are skipped, and the importer says how many.

use crate::{IoError, Result};
use emulsion_core::{Document, Node};
use emulsion_raster::Raster;
use emulsion_raster::vector::{Path, PathStyle, StrokeCap, StrokeJoin};
use glam::{DAffine2, dvec2};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::sync::Arc;

pub const MAX_NODES: usize = 2000;

/// What an import produced, with a note on anything skipped.
pub struct Imported {
    pub doc: Document,
    pub skipped: Vec<String>,
}

fn attrs(e: &BytesStart) -> HashMap<String, String> {
    e.attributes()
        .flatten()
        .filter_map(|a| {
            let k = a.key.as_ref().to_string();
            let v = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()?
                .to_string();
            Some((k, v))
        })
        .collect()
}

fn num(s: &str) -> Option<f64> {
    s.trim().trim_end_matches("px").parse::<f64>().ok()
}

/// Parse `transform="translate(…) scale(…) rotate(…) matrix(…)"`.
fn transform(s: &str) -> DAffine2 {
    let mut m = DAffine2::IDENTITY;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args: Vec<f64> = rest[open + 1..open + close]
            .split([',', ' '])
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        let t = match (name, args.as_slice()) {
            ("translate", [x]) => DAffine2::from_translation(dvec2(*x, 0.0)),
            ("translate", [x, y, ..]) => DAffine2::from_translation(dvec2(*x, *y)),
            ("scale", [k]) => DAffine2::from_scale(dvec2(*k, *k)),
            ("scale", [x, y, ..]) => DAffine2::from_scale(dvec2(*x, *y)),
            ("rotate", [a]) => DAffine2::from_angle(a.to_radians()),
            ("rotate", [a, cx, cy, ..]) => {
                DAffine2::from_translation(dvec2(*cx, *cy))
                    * DAffine2::from_angle(a.to_radians())
                    * DAffine2::from_translation(dvec2(-cx, -cy))
            }
            ("matrix", [a, b, c, d, e, f, ..]) => {
                DAffine2::from_cols_array(&[*a, *b, *c, *d, *e, *f])
            }
            ("skewX", [a]) => {
                DAffine2::from_cols_array(&[1.0, 0.0, a.to_radians().tan(), 1.0, 0.0, 0.0])
            }
            ("skewY", [a]) => {
                DAffine2::from_cols_array(&[1.0, a.to_radians().tan(), 0.0, 1.0, 0.0, 0.0])
            }
            _ => DAffine2::IDENTITY,
        };
        m *= t;
        rest = &rest[open + close + 1..];
    }
    m
}

/// `#rgb`, `#rrggbb`, `rgb(r,g,b)`, a few names, or `none`.
fn color(s: &str) -> Option<Option<[u8; 4]>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("transparent") {
        return Some(None);
    }
    if let Some(h) = s.strip_prefix('#') {
        let v = u32::from_str_radix(h, 16).ok()?;
        return Some(Some(match h.len() {
            3 => [
                ((v >> 8) & 0xF) as u8 * 17,
                ((v >> 4) & 0xF) as u8 * 17,
                (v & 0xF) as u8 * 17,
                255,
            ],
            6 => [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255],
            8 => [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8],
            _ => return None,
        }));
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let v: Vec<u8> = inner
            .split(',')
            .filter_map(|c| c.trim().trim_end_matches('%').parse::<f32>().ok())
            .map(|c| c.round().clamp(0.0, 255.0) as u8)
            .collect();
        if v.len() == 3 {
            return Some(Some([v[0], v[1], v[2], 255]));
        }
    }
    let named = match s.to_lowercase().as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "gray" | "grey" => [128, 128, 128],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "currentcolor" => [0, 0, 0],
        _ => return None,
    };
    Some(Some([named[0], named[1], named[2], 255]))
}

/// Inherited presentation attributes.
#[derive(Clone)]
struct Style {
    fill: Option<[u8; 4]>,
    stroke: Option<[u8; 4]>,
    width: f32,
    opacity: f32,
    stroke_options: PathStyle,
}

impl Style {
    fn apply(&self, a: &HashMap<String, String>) -> Style {
        let mut s = self.clone();
        let mut props: Vec<(String, String)> =
            a.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some(css) = a.get("style") {
            for decl in css.split(';') {
                if let Some((k, v)) = decl.split_once(':') {
                    props.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
        }
        for (k, v) in props {
            match k.as_str() {
                "fill" => {
                    if let Some(c) = color(&v) {
                        s.fill = c;
                    }
                }
                "stroke" => {
                    if let Some(c) = color(&v) {
                        s.stroke = c;
                    }
                }
                "stroke-width" => {
                    if let Some(w) = num(&v) {
                        s.width = w as f32;
                    }
                }
                "opacity" => {
                    if let Some(o) = num(&v) {
                        s.opacity = o.clamp(0.0, 1.0) as f32;
                    }
                }
                "stroke-linecap" => {
                    s.stroke_options.cap = match v.as_str() {
                        "round" => StrokeCap::Round,
                        "square" => StrokeCap::Square,
                        "butt" => StrokeCap::Butt,
                        _ => s.stroke_options.cap,
                    };
                }
                "stroke-linejoin" => {
                    s.stroke_options.join = match v.as_str() {
                        "round" => StrokeJoin::Round,
                        "bevel" => StrokeJoin::Bevel,
                        "miter" => StrokeJoin::Miter,
                        _ => s.stroke_options.join,
                    };
                }
                "stroke-miterlimit" => {
                    if let Some(limit) = num(&v) {
                        s.stroke_options.miter_limit = limit as f32;
                    }
                }
                "stroke-dashoffset" => {
                    if let Some(offset) = num(&v) {
                        s.stroke_options.dash_offset = offset as f32;
                    }
                }
                "stroke-dasharray" => {
                    if v == "none" {
                        s.stroke_options.dash_count = 0;
                    } else {
                        let values: Option<Vec<f32>> = v
                            .split(|c: char| c == ',' || c.is_ascii_whitespace())
                            .filter(|part| !part.is_empty())
                            .map(|part| num(part).map(|n| n as f32))
                            .collect();
                        if let Some(values) = values.filter(|values| {
                            !values.is_empty()
                                && values.len() <= 6
                                && values.iter().all(|n| n.is_finite() && *n >= 0.0)
                        }) {
                            s.stroke_options.dash = [0.0; 6];
                            s.stroke_options.dash[..values.len()].copy_from_slice(&values);
                            s.stroke_options.dash_count = values.len() as u8;
                        }
                    }
                }
                _ => {}
            }
        }
        s
    }

    fn to_path_style(&self, scale: f64) -> PathStyle {
        let a = |c: [u8; 4]| [c[0], c[1], c[2], (c[3] as f32 * self.opacity).round() as u8];
        let mut options = self.stroke_options;
        options.dash.iter_mut().for_each(|n| *n *= scale as f32);
        options.dash_offset *= scale as f32;
        PathStyle {
            stroke: self.stroke.map(a),
            width: (self.width as f64 * scale) as f32,
            fill: self.fill.map(a),
            ..options
        }
        .sanitized()
    }
}

fn shape_path(name: &str, a: &HashMap<String, String>) -> Option<Path> {
    let g = |k: &str| a.get(k).and_then(|v| num(v));
    let d = match name {
        "path" => a.get("d")?.clone(),
        "rect" => {
            let (x, y, w, h) = (
                g("x").unwrap_or(0.0),
                g("y").unwrap_or(0.0),
                g("width")?,
                g("height")?,
            );
            let rx = g("rx").or_else(|| g("ry")).unwrap_or(0.0).min(w / 2.0);
            let ry = g("ry").or_else(|| g("rx")).unwrap_or(0.0).min(h / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                format!(
                    "M {x} {y} L {} {y} L {} {} L {x} {} Z",
                    x + w,
                    x + w,
                    y + h,
                    y + h
                )
            } else {
                let k = 0.5523;
                format!(
                    "M {} {y} L {} {y} C {} {y} {} {} {} {} L {} {} C {} {} {} {} {} {} L {} {} C {} {} {} {} {x} {} L {x} {} C {x} {} {} {y} {} {y} Z",
                    x + rx,
                    x + w - rx,
                    x + w - rx + rx * k,
                    x + w,
                    y + ry - ry * k,
                    x + w,
                    y + ry,
                    x + w,
                    y + h - ry,
                    x + w,
                    y + h - ry + ry * k,
                    x + w - rx + rx * k,
                    y + h,
                    x + w - rx,
                    y + h,
                    x + rx,
                    y + h,
                    x + rx - rx * k,
                    y + h,
                    x,
                    y + h - ry + ry * k,
                    y + h - ry,
                    y + ry,
                    y + ry - ry * k,
                    x + rx - rx * k,
                    x + rx
                )
            }
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (g("cx").unwrap_or(0.0), g("cy").unwrap_or(0.0));
            let (rx, ry) = if name == "circle" {
                let r = g("r")?;
                (r, r)
            } else {
                (g("rx")?, g("ry")?)
            };
            let k = 0.5523;
            format!(
                "M {} {cy} C {} {} {} {} {cx} {} C {} {} {} {} {} {cy} C {} {} {} {} {cx} {} C {} {} {} {} {} {cy} Z",
                cx + rx,
                cx + rx,
                cy + ry * k,
                cx + rx * k,
                cy + ry,
                cy + ry,
                cx - rx * k,
                cy + ry,
                cx - rx,
                cy + ry * k,
                cx - rx,
                cx - rx,
                cy - ry * k,
                cx - rx * k,
                cy - ry,
                cy - ry,
                cx + rx * k,
                cy - ry,
                cx + rx,
                cy - ry * k,
                cx + rx
            )
        }
        "line" => format!(
            "M {} {} L {} {}",
            g("x1").unwrap_or(0.0),
            g("y1").unwrap_or(0.0),
            g("x2").unwrap_or(0.0),
            g("y2").unwrap_or(0.0)
        ),
        "polyline" | "polygon" => {
            let pts: Vec<f64> = a
                .get("points")?
                .split([',', ' ', '\n', '\t'])
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            if pts.len() < 4 {
                return None;
            }
            let mut d = format!("M {} {}", pts[0], pts[1]);
            for [px, py] in pts[2..].as_chunks::<2>().0 {
                d.push_str(&format!(" L {px} {py}"));
            }
            if name == "polygon" {
                d.push_str(" Z");
            }
            d
        }
        _ => return None,
    };
    Path::from_svg(&d).ok()
}

/// Long side a small SVG is scaled up to when rasterized: vectors are free
/// to render large, and a 24 px icon is no use as a 24 px picture.
const RASTER_MIN_SIDE: f32 = 1024.0;
const RASTER_MAX_SIDE: f32 = 8192.0;

/// Render the whole SVG, gradients, filters, text and embedded images
/// included, through resvg into one pixel layer, the way GIMP opens SVGs.
/// The document takes the SVG's own size, scaled so its long side is at
/// least `RASTER_MIN_SIDE` and at most `RASTER_MAX_SIDE`.
pub fn rasterize(text: &str) -> Result<Raster> {
    use resvg::usvg;
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    let opt = usvg::Options {
        fontdb: Arc::new(fonts),
        ..Default::default()
    };
    let tree =
        usvg::Tree::from_str(text, &opt).map_err(|e| IoError::Unsupported(format!("SVG: {e}")))?;
    let size = tree.size();
    let (sw, sh) = (size.width().max(1.0), size.height().max(1.0));
    let long = sw.max(sh);
    let scale = (RASTER_MIN_SIDE / long)
        .max(1.0)
        .min(RASTER_MAX_SIDE / long);
    let (w, h) = ((sw * scale).round() as u32, (sh * scale).round() as u32);
    crate::import::check_size(w, h)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| IoError::Unsupported("SVG: could not allocate the picture".into()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied RGBA; the raster wants straight alpha.
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(Raster::from_srgba8(w, h, &rgba))
}

/// Import an SVG file as a document of Path nodes.
pub fn import(text: &str) -> Result<Imported> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<(DAffine2, Style)> = vec![(
        DAffine2::IDENTITY,
        Style {
            fill: Some([0, 0, 0, 255]),
            stroke: None,
            width: 1.0,
            opacity: 1.0,
            stroke_options: PathStyle {
                cap: StrokeCap::Butt,
                join: StrokeJoin::Miter,
                ..Default::default()
            },
        },
    )];
    let mut doc: Option<Document> = None;
    let mut shapes: Vec<(String, Path, PathStyle)> = Vec::new();
    let mut skipped: HashMap<String, usize> = HashMap::new();
    let mut skip_depth = 0usize;
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| IoError::Xml(e.to_string()))?;
        match ev {
            Event::Eof => break,
            Event::Start(_) | Event::Empty(_) => {
                let (e, is_empty) = match &ev {
                    Event::Start(e) => (e, false),
                    Event::Empty(e) => (e, true),
                    _ => unreachable!(),
                };
                let name = e.local_name().as_ref().to_string();
                let a = attrs(e);
                if skip_depth > 0 {
                    if !is_empty {
                        skip_depth += 1;
                    }
                    continue;
                }
                let (parent_t, parent_s) = stack.last().cloned().expect("root");
                let t = parent_t
                    * a.get("transform")
                        .map(|s| transform(s))
                        .unwrap_or(DAffine2::IDENTITY);
                let s = parent_s.apply(&a);
                match name.as_str() {
                    "svg" if doc.is_none() => {
                        let vb: Vec<f64> = a
                            .get("viewBox")
                            .map(|v| {
                                v.split([',', ' '])
                                    .filter_map(|x| x.trim().parse().ok())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let (vw, vh) = if vb.len() == 4 {
                            (vb[2], vb[3])
                        } else {
                            (
                                a.get("width").and_then(|v| num(v)).unwrap_or(1024.0),
                                a.get("height").and_then(|v| num(v)).unwrap_or(768.0),
                            )
                        };
                        let w = a
                            .get("width")
                            .and_then(|v| num(v))
                            .filter(|v| *v > 0.0)
                            .unwrap_or(vw);
                        let h = a
                            .get("height")
                            .and_then(|v| num(v))
                            .filter(|v| *v > 0.0)
                            .unwrap_or(vh);
                        let scale = (w / vw.max(1e-6)).min(h / vh.max(1e-6));
                        let (dw, dh) = (
                            (vw * scale).round().clamp(1.0, 30000.0) as u32,
                            (vh * scale).round().clamp(1.0, 30000.0) as u32,
                        );
                        crate::import::check_size(dw, dh)?;
                        doc = Some(Document::new(dw, dh));
                        let origin = if vb.len() == 4 {
                            dvec2(-vb[0], -vb[1])
                        } else {
                            dvec2(0.0, 0.0)
                        };
                        stack.push((
                            DAffine2::from_scale(dvec2(scale, scale))
                                * DAffine2::from_translation(origin)
                                * t,
                            s,
                        ));
                        continue;
                    }
                    "g" | "a" | "switch" => {}
                    "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                        if let Some(mut p) = shape_path(&name, &a) {
                            p.transform(t);
                            let sc = t.matrix2.determinant().abs().sqrt();
                            let label = a.get("id").cloned().unwrap_or_else(|| name.clone());
                            shapes.push((label, p, s.to_path_style(sc)));
                            if shapes.len() > MAX_NODES {
                                return Err(IoError::Manifest(format!(
                                    "more than {MAX_NODES} shapes"
                                )));
                            }
                        }
                    }
                    "defs" | "clipPath" | "mask" | "linearGradient" | "radialGradient"
                    | "filter" | "symbol" | "style" | "metadata" | "title" | "desc" => {
                        if !is_empty {
                            skip_depth = 1;
                        }
                        *skipped.entry(name.clone()).or_default() += 1;
                        continue;
                    }
                    other => {
                        *skipped.entry(other.to_string()).or_default() += 1;
                    }
                }
                if !is_empty {
                    stack.push((t, s));
                }
            }
            Event::End(_) => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                } else if stack.len() > 1 {
                    stack.pop();
                }
            }
            _ => {}
        }
    }
    let mut doc = doc.ok_or_else(|| IoError::Unsupported("not an SVG document".into()))?;
    let (w, h) = (doc.width, doc.height);
    for (label, path, style) in shapes {
        let node = Node::path(0, label, Arc::new(path), style, w, h);
        emulsion_core::Command::AddNode {
            node: Box::new(node),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .map_err(|e| IoError::Manifest(e.to_string()))?;
    }
    let skipped: Vec<String> = skipped
        .into_iter()
        .map(|(k, n)| if n == 1 { k } else { format!("{k} ×{n}") })
        .collect();
    Ok(Imported { doc, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::NodeKind;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 100 50">
  <defs><linearGradient id="g"/></defs>
  <rect id="box" x="10" y="10" width="30" height="20" fill="#ff0000" stroke="black" stroke-width="2"/>
  <g transform="translate(50 0)" fill="none" stroke="#00f">
    <circle cx="20" cy="25" r="10"/>
    <path d="M 0 40 L 40 40" style="stroke-width:3"/>
  </g>
  <text x="5" y="45">hello</text>
</svg>"##;

    #[test]
    fn inherited_stroke_geometry_and_dash_lengths_scale_with_viewbox() {
        let imported = import(
            r##"<svg width="100" height="100" viewBox="0 0 50 50">
            <g fill="none" stroke="#123456" stroke-linecap="square" stroke-linejoin="bevel"
               stroke-miterlimit="7" stroke-dasharray="3, 2" stroke-dashoffset="-1">
                <path d="M 5 5 L 30 30"/>
                <path d="M 5 10 L 30 35" style="stroke-linecap:round;stroke-dasharray:none"/>
            </g></svg>"##,
        )
        .unwrap();
        let NodeKind::Path { style, .. } = &imported.doc.nodes[0].kind else {
            panic!("editable path expected")
        };
        assert_eq!(style.cap, StrokeCap::Square);
        assert_eq!(style.join, StrokeJoin::Bevel);
        assert_eq!(style.miter_limit, 7.0);
        assert_eq!(style.dash_count, 2);
        assert_eq!(&style.dash[..2], &[6.0, 4.0]);
        assert_eq!(style.dash_offset, -2.0);
        let NodeKind::Path { style, .. } = &imported.doc.nodes[1].kind else {
            panic!("editable path expected")
        };
        assert_eq!(style.cap, StrokeCap::Round);
        assert_eq!(style.dash_count, 0);
    }

    #[test]
    fn shapes_become_path_nodes_with_scale_and_style() {
        let imp = import(SVG).unwrap();
        let d = imp.doc;
        assert_eq!(
            (d.width, d.height),
            (200, 100),
            "viewBox scaled to width/height"
        );
        let names: Vec<&str> = d.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["box", "circle", "path"]);
        let NodeKind::Path { path, style, cache } = &d.nodes[0].kind else {
            panic!()
        };
        assert_eq!(style.fill, Some([255, 0, 0, 255]));
        assert_eq!(style.width, 4.0, "stroke width scales with the viewBox");
        let b = path.bounds(style);
        assert!(b.x <= 20 && b.right() >= 80, "{b:?}");
        assert!(cache.get(50, 40)[0] > 60000, "red fill rendered");
        let NodeKind::Path { style, path, .. } = &d.nodes[1].kind else {
            panic!()
        };
        assert_eq!(style.fill, None);
        assert_eq!(style.stroke, Some([0, 0, 255, 255]));
        let b = path.bounds(style);
        assert!(
            (b.x as f64 - 118.0).abs() < 6.0,
            "translated then scaled: {b:?}"
        );
        let NodeKind::Path { style, .. } = &d.nodes[2].kind else {
            panic!()
        };
        assert_eq!(style.width, 6.0);
        assert!(imp.skipped.iter().any(|s| s.starts_with("text")));
        assert!(imp.skipped.iter().any(|s| s.starts_with("defs")));
    }
}
