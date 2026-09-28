//! Local editable draw.io XML interchange, including bounded raw-DEFLATE pages.
//! Format reference: https://www.drawio.com/docs/reference/diagram-generation/
use crate::{IoError, Result};
use base64::Engine;
#[cfg(test)]
use emulsion_core::Editor;
use emulsion_core::{
    Document, Node, NodeId, NodeKind,
    diagram::{self, Endpoint, Port, Routing, ShapeKind},
    project::{PageMeta, Project, ProjectKind, ProjectPage},
};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    io::{Read, Write},
    path::Path,
    sync::Arc,
};
const MAX_BYTES: usize = 32 << 20;
fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}
#[derive(Default)]
struct Cell {
    attrs: BTreeMap<String, String>,
    geometry: BTreeMap<String, String>,
    points: Vec<(f64, f64)>,
    offset: (f64, f64),
    source_point: Option<(f64, f64)>,
    target_point: Option<(f64, f64)>,
}
struct Page {
    name: String,
    width: u32,
    height: u32,
    cells: Vec<Cell>,
    background: [u8; 4],
}
impl Default for Page {
    fn default() -> Self {
        Self {
            name: "Page 1".into(),
            width: 1600,
            height: 1000,
            cells: Vec::new(),
            background: [255; 4],
        }
    }
}
pub struct Imported {
    pub project: Project,
    pub warnings: Vec<String>,
}
fn attributes(e: &BytesStart) -> Result<BTreeMap<String, String>> {
    e.attributes()
        .map(|a| {
            let a = a.map_err(|e| error(e.to_string()))?;
            let key = a.key.as_ref().to_owned();
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|e| error(e.to_string()))?
                .into_owned();
            Ok((key, value))
        })
        .collect()
}
fn number(map: &BTreeMap<String, String>, key: &str, default: f64) -> Result<f64> {
    let value = map
        .get(key)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>())
        .transpose()
        .map_err(|_| error(format!("Invalid {key}")))?
        .unwrap_or(default);
    if !value.is_finite() || value.abs() > 1e6 {
        return Err(error(format!("Invalid {key}")));
    }
    Ok(value)
}
fn percent_decode(encoded: &str) -> Result<String> {
    let bytes = encoded.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .ok_or_else(|| error("Incomplete URI escape"))?;
            output.push(
                u8::from_str_radix(
                    std::str::from_utf8(hex).map_err(|_| error("Invalid URI escape"))?,
                    16,
                )
                .map_err(|_| error("Invalid URI escape"))?,
            );
            i += 3;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(output).map_err(|e| error(e.to_string()))
}
fn decompress(encoded: &str) -> Result<String> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|e| error(format!("Invalid compressed diagram: {e}")))?;
    let mut output = String::new();
    flate2::read::DeflateDecoder::new(data.as_slice())
        .take(MAX_BYTES as u64 + 1)
        .read_to_string(&mut output)?;
    if output.len() > MAX_BYTES {
        return Err(error("Decompressed diagram exceeds 32 MiB."));
    }
    percent_decode(&output)
}
fn parse_pages(xml: &str, depth: usize, budget: &mut usize) -> Result<Vec<Page>> {
    if xml.len() > *budget || depth > 1 {
        return Err(error("Diagram XML exceeds its decoding limits."));
    }
    *budget -= xml.len();
    let mut reader = Reader::from_str(xml);
    let mut pages = Vec::new();
    let mut page = Page::default();
    let mut current: Option<Cell> = None;
    let mut object = BTreeMap::new();
    let mut compressed = String::new();
    let mut in_diagram = false;
    let mut found_model = false;
    let mut xml_depth = 0usize;
    let mut point_array = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| error(format!("Invalid diagram XML: {e}")))?
        {
            Event::Start(e) | Event::Empty(e) => {
                // Empty tags do not contribute to semantic nesting; the reader
                // already checks matching closing tags.
                xml_depth += 1;
                if xml_depth > 100_000 {
                    return Err(error("Too many XML elements."));
                }
                let attrs = attributes(&e)?;
                match e.name().as_ref() {
                    "diagram" => {
                        if in_diagram {
                            return Err(error("Nested diagram pages are invalid."));
                        }
                        in_diagram = true;
                        page = Page::default();
                        page.name = attrs
                            .get("name")
                            .cloned()
                            .unwrap_or_else(|| format!("Page {}", pages.len() + 1));
                        compressed.clear();
                        found_model = false;
                    }
                    "mxGraphModel" => {
                        found_model = true;
                        if let Some(value) = attrs.get("background").filter(|v| !v.is_empty()) {
                            page.background = color(value)?.unwrap_or([0; 4]);
                        }
                        page.width =
                            number(&attrs, "pageWidth", 1600.)?.ceil().clamp(1., 30000.) as u32;
                        page.height = number(&attrs, "pageHeight", 1000.)?
                            .ceil()
                            .clamp(1., 30000.) as u32;
                    }
                    "object" | "UserObject" => object = attrs,
                    "mxCell" => {
                        if let Some(old) = current.take() {
                            page.cells.push(old);
                        }
                        let mut attrs = attrs;
                        if !object.is_empty() {
                            if let Some(id) = object.get("id") {
                                attrs.insert("id".into(), id.clone());
                            }
                            if let Some(label) = object.get("label") {
                                attrs.insert("value".into(), label.clone());
                            }
                            let data: BTreeMap<_, _> = object
                                .iter()
                                .filter(|(k, _)| !matches!(k.as_str(), "id" | "label"))
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            if !data.is_empty() {
                                attrs.insert(
                                    "emulsionData".into(),
                                    serde_json::to_string(&data).unwrap(),
                                );
                            }
                        }
                        current = Some(Cell {
                            attrs,
                            ..Default::default()
                        });
                        point_array = false;
                        if page.cells.len() > diagram::MAX_SHAPES + diagram::MAX_EDGES + 100 {
                            return Err(error("Too many diagram cells."));
                        }
                    }
                    "mxGeometry" => {
                        if let Some(cell) = &mut current {
                            cell.geometry = attrs;
                        }
                    }
                    "Array" => point_array = attrs.get("as").is_some_and(|s| s == "points"),
                    "mxPoint" if attrs.get("as").is_some_and(|v| v == "offset") => {
                        if let Some(cell) = &mut current {
                            cell.offset = (number(&attrs, "x", 0.)?, number(&attrs, "y", 0.)?);
                        }
                    }
                    "mxPoint"
                        if attrs
                            .get("as")
                            .is_some_and(|v| v == "sourcePoint" || v == "targetPoint") =>
                    {
                        if let Some(cell) = &mut current {
                            let point = (number(&attrs, "x", 0.)?, number(&attrs, "y", 0.)?);
                            if attrs["as"] == "sourcePoint" {
                                cell.source_point = Some(point);
                            } else {
                                cell.target_point = Some(point);
                            }
                        }
                    }
                    "mxPoint" if point_array => {
                        if let Some(cell) = &mut current {
                            if cell.points.len() >= 128 {
                                return Err(error("Too many connector waypoints."));
                            }
                            cell.points
                                .push((number(&attrs, "x", 0.)?, number(&attrs, "y", 0.)?));
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => match e.name().as_ref() {
                "mxCell" => {
                    if let Some(cell) = current.take() {
                        page.cells.push(cell);
                    }
                }
                "object" | "UserObject" => object.clear(),
                "Array" => point_array = false,
                "diagram" => {
                    if let Some(cell) = current.take() {
                        page.cells.push(cell);
                    }
                    if !found_model && !compressed.trim().is_empty() {
                        let content = quick_xml::escape::unescape(compressed.trim())
                            .map_err(|e| error(e.to_string()))?;
                        let text = if content.starts_with('<') {
                            content.into_owned()
                        } else {
                            decompress(&content)?
                        };
                        let mut decoded = parse_pages(&text, depth + 1, budget)?;
                        if decoded.len() != 1 {
                            return Err(error("Compressed page must contain one graph."));
                        }
                        decoded[0].name = page.name.clone();
                        pages.extend(decoded);
                    } else if found_model {
                        pages.push(std::mem::take(&mut page));
                    } else {
                        return Err(error("Diagram page contains no graph."));
                    }
                    in_diagram = false;
                    found_model = false;
                    if pages.len() > emulsion_core::project::MAX_PAGES {
                        return Err(error("Too many diagram pages."));
                    }
                }
                _ => {}
            },
            Event::Text(text) if in_diagram && !found_model => compressed.push_str(text.as_ref()),
            Event::CData(text) if in_diagram && !found_model => compressed.push_str(text.as_ref()),
            Event::GeneralRef(text) if in_diagram && !found_model => {
                compressed.push('&');
                compressed.push_str(text.as_ref());
                compressed.push(';');
            }
            Event::DocType(_) => {
                return Err(error("Document type declarations are not supported."));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if in_diagram {
        return Err(error("Unclosed diagram page."));
    }
    if found_model {
        if let Some(cell) = current {
            page.cells.push(cell);
        }
        pages.push(page);
    }
    if pages.is_empty() {
        return Err(error("No mxGraphModel found."));
    }
    Ok(pages)
}
fn style(cell: &Cell) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut parts = cell
        .attrs
        .get("style")
        .map_or("", String::as_str)
        .split(';')
        .peekable();
    while let Some(part) = parts.next() {
        if part.is_empty() {
            continue;
        }
        let (key, value) = part.split_once('=').unwrap_or((part, "1"));
        let mut value = value.to_string();
        if key == "image"
            && value.starts_with("data:")
            && parts.peek().is_some_and(|s| s.starts_with("base64,"))
        {
            value.push(';');
            value.push_str(parts.next().unwrap());
        }
        result.insert(key.to_string(), value);
    }
    result
}
fn shape_kind(style: &BTreeMap<String, String>, warnings: &mut BTreeSet<String>) -> ShapeKind {
    let shape = style.get("shape").map(String::as_str).unwrap_or("");
    if style.contains_key("swimlane") || shape == "swimlane" {
        return ShapeKind::Swimlane;
    }
    if style.contains_key("group") || style.get("container").is_some_and(|v| v == "1") {
        return ShapeKind::Container;
    }
    if style.contains_key("rhombus") || shape == "rhombus" {
        return ShapeKind::Decision;
    }
    if style.contains_key("ellipse")
        || shape == "ellipse"
        || style.get("rounded").is_some_and(|s| s == "1")
    {
        return ShapeKind::Terminator;
    }
    match shape {
        "" | "rectangle" | "rect" | "image" | "text" | "hexagon" | "triangle" | "line"
        | "doubleEllipse" | "cross" | "partialRectangle" | "actor" | "umlActor" => {
            ShapeKind::Process
        }
        value
            if value.starts_with("stencil(")
                || vendor::contains(value)
                || shapes::supports(value)
                || style.get("resIcon").is_some_and(|v| vendor::contains(v)) =>
        {
            ShapeKind::Process
        }
        "parallelogram" => ShapeKind::Data,
        "cylinder" | "cylinder3" => ShapeKind::Database,
        "document" => ShapeKind::Document,
        "note" => ShapeKind::Note,
        "cloud" => ShapeKind::Cloud,
        "umlClass" => ShapeKind::Class,
        "table" => ShapeKind::Entity,
        other => {
            warnings.insert(format!(
                "Shape {:?} imported as an editable rectangle.",
                other
                    .split('(')
                    .next()
                    .unwrap_or(other)
                    .chars()
                    .take(120)
                    .collect::<String>()
            ));
            ShapeKind::Process
        }
    }
}
fn color(text: &str) -> Result<Option<[u8; 4]>> {
    let text=text.trim();
    let text=text.strip_prefix("light-dark(").and_then(|s|s.strip_suffix(')')).and_then(|s|s.split_once(',')).map_or(text,|(light,_)|light.trim());
    crate::svg::color(text).ok_or_else(|| error(format!("Unsupported color {text:?}")))
}
fn plain_label(text: &str, html: bool, warnings: &mut BTreeSet<String>) -> String {
    if !html {
        return text.into();
    }
    labels::parse(text, &emulsion_core::text::TextSpec::default(), warnings).text
}
fn port(
    style: &BTreeMap<String, String>,
    prefix: &str,
    warnings: &mut BTreeSet<String>,
) -> Result<Port> {
    let x = format!("{prefix}X");
    let y = format!("{prefix}Y");
    if !style.contains_key(&x) && !style.contains_key(&y) {
        return Ok(Port::Auto);
    }
    let (Ok(x), Ok(y)) = (number(style, &x, 0.5), number(style, &y, 0.5)) else {
        warnings.insert(
            "Invalid connector port coordinates were replaced with automatic attachment.".into(),
        );
        return Ok(Port::Auto);
    };
    Ok(match (x, y) {
        (0.5, 0.) => Port::North,
        (1., 0.5) => Port::East,
        (0.5, 1.) => Port::South,
        (0., 0.5) => Port::West,
        _ => Port::Custom { x, y },
    })
}
fn apply_style(
    doc: &mut Document,
    body: NodeId,
    label: NodeId,
    style: &BTreeMap<String, String>,
    warnings: &mut BTreeSet<String>,
) -> Result<()> {
    if let Some(Node {
        kind: NodeKind::Path {
            path, style: old, ..
        },
        ..
    }) = doc.node(body)
    {
        let path = path.clone();
        let mut paint = *old;
        for (key, is_fill) in [("fillColor", true), ("strokeColor", false)] {
            if let Some(value) = style.get(key) {
                match color(value) {
                    Ok(color) => {
                        if is_fill {
                            paint.fill = color;
                        } else {
                            paint.stroke = color;
                        }
                    }
                    Err(_) => {
                        warnings.insert(format!(
                            "Unsupported color {value:?}; retained the default."
                        ));
                    }
                }
            }
        }
        paint.width = number(style, "strokeWidth", paint.width as f64)?.clamp(0., 100.) as f32;
        if let Some(end) = style
            .get("gradientColor")
            .and_then(|v| color(v).ok().flatten())
        {
            paint.fill_paint = emulsion_raster::vector::PathPaint::LinearGradient {
                end,
                angle: match style.get("gradientDirection").map(String::as_str) {
                    Some("north") => 270.,
                    Some("east") => 0.,
                    Some("west") => 180.,
                    _ => 90.,
                },
            };
        }
        for (key, color) in [
            ("fillOpacity", &mut paint.fill),
            ("strokeOpacity", &mut paint.stroke),
        ] {
            if let Some(color) = color {
                color[3] = (number(style, key, 100.)?.clamp(0., 100.) * 2.55).round() as u8;
            }
        }
        if style.get("dashed").is_some_and(|v| v == "1") {
            let values = style
                .get("dashPattern")
                .map_or("3 3", String::as_str)
                .split_whitespace()
                .map(str::parse::<f32>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| error("Invalid dash pattern"))?;
            if values.is_empty()
                || values.len() > 6
                || values
                    .iter()
                    .any(|v| !v.is_finite() || *v < 0. || *v > 10000.)
            {
                return Err(error("Invalid dash pattern"));
            }
            paint.dash_count = values.len() as u8;
            for (i, value) in values.into_iter().enumerate() {
                paint.dash[i] = value * paint.width.max(1.);
            }
        }
        let (w, h) = (doc.width, doc.height);
        doc.node_mut(body).unwrap().kind = NodeKind::Path {
            cache: emulsion_core::vector_cache::VectorRaster::path(path.clone(), paint, w, h),
            path,
            style: paint,
        };
    }
    if let Some(Node {
        kind: NodeKind::Text { spec, .. },
        ..
    }) = doc.node(label)
    {
        let mut spec = (**spec).clone();
        spec.size = number(style, "fontSize", spec.size as f64)?.clamp(1., 1000.) as f32;
        let flags = number(style, "fontStyle", 0.)? as u32;
        spec.bold = flags & 1 != 0;
        spec.italic = flags & 2 != 0;
        spec.align = match style.get("align").map(String::as_str) {
            Some("left") => emulsion_core::text::Align::Left,
            Some("right") => emulsion_core::text::Align::Right,
            _ => emulsion_core::text::Align::Center,
        };
        spec.underline = flags & 4 != 0;
        spec.strikethrough = flags & 8 != 0;
        if let Some(font) = style.get("fontFamily") {
            spec.font = font.clone();
        }
        if let Some(value) = style.get("fontColor")
            && let Ok(Some(color)) = color(value)
        {
            spec.color = color;
        }
        let (w, h) = (doc.width, doc.height);
        let spec = Arc::new(spec.sanitized());
        doc.node_mut(label).unwrap().kind = NodeKind::Text {
            cache: emulsion_core::vector_cache::VectorRaster::text(spec.clone(), w, h),
            spec,
        };
    }
    for key in ["startArrow", "endArrow"] {
        if let Some(arrow) = style
            .get(key)
            .filter(|v| diagram::MarkerKind::from_drawio(v).is_none())
        {
            warnings.insert(format!(
                "Arrow style {arrow:?} uses a triangle; specialized arrowheads need review."
            ));
        }
    }
    for key in ["sketch"] {
        if style.get(key).is_some_and(|v| v != "0" && v != "none") {
            warnings.insert(format!("Style {key} requires manual review after import."));
        }
    }
    Ok(())
}
fn apply_cell(
    doc: &mut Document,
    id: NodeId,
    cell: &Cell,
    style: &BTreeMap<String, String>,
) -> Result<()> {
    let opacity = number(style, "opacity", 100.)?.clamp(0., 100.) as f32 / 100.;
    let node = doc
        .node_mut(id)
        .ok_or_else(|| error("Missing imported cell"))?;
    node.opacity = opacity;
    node.visible = cell.attrs.get("visible").is_none_or(|v| v != "0");
    Ok(())
}
fn label_style(doc: &Document, id: NodeId) -> Result<String> {
    let Some(NodeKind::Text { spec, .. }) = doc.node(id).map(|n| &n.kind) else {
        return Ok(String::new());
    };
    if spec.font.contains(';') {
        return Err(error("A font name contains a draw.io style delimiter."));
    }
    Ok(format!(
        "fontFamily={};fontSize={};fontColor={};fontStyle={};align={};",
        spec.font,
        spec.size,
        hex(Some(spec.color)),
        u32::from(spec.bold) + 2 * u32::from(spec.italic),
        match spec.align {
            emulsion_core::text::Align::Left => "left",
            emulsion_core::text::Align::Right => "right",
            _ => "center",
        }
    ))
}
mod build;
mod dynamic;
pub(crate) mod images;
mod labels;
mod tables;
mod shapes;
mod stencils;
pub mod vendor;
use build::build;
/// SVG exports may embed the complete editable mxfile in the root content attribute.
fn embedded_xml(xml: &str) -> Result<Option<String>> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event().map_err(|e| error(e.to_string()))? {
            Event::Start(e) | Event::Empty(e) => {
                return if e.local_name().as_ref() == "svg" {
                    let attrs = attributes(&e)?;
                    let content = attrs.get("content").ok_or_else(|| {
                        error("SVG has no embedded draw.io model; import it as artwork.")
                    })?;
                    Ok(Some(if content.trim_start().starts_with('%') {
                        percent_decode(content)?
                    } else {
                        content.clone()
                    }))
                } else {
                    Ok(None)
                };
            }
            Event::DocType(ref d)
                if d.as_ref().starts_with("svg PUBLIC ") && !d.as_ref().contains('[') => {}
            Event::DocType(_) => {
                return Err(error("Document type declarations are not supported."));
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}
fn library_pages(xml: &str, budget: &mut usize) -> Result<Option<Vec<Page>>> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event().map_err(|e| error(e.to_string()))? {
            Event::Start(e) if e.name().as_ref() == "mxlibrary" => {
                let json = reader
                    .read_text(e.name())
                    .map_err(|e| error(e.to_string()))?;
                let json = quick_xml::escape::unescape(&json).map_err(|e| error(e.to_string()))?;
                let items: Vec<serde_json::Value> =
                    serde_json::from_str(&json).map_err(|e| error(e.to_string()))?;
                if items.len() > emulsion_core::project::MAX_PAGES {
                    return Err(error("Too many library entries"));
                }
                let mut pages = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let text = item.get("xml").and_then(|v| v.as_str()).ok_or_else(|| {
                        error("Image-only library entry: import its image as artwork.")
                    })?;
                    let text = if text.trim_start().starts_with('<') {
                        text.to_string()
                    } else {
                        decompress(text)?
                    };
                    let mut decoded = parse_pages(&text, 1, budget)?;
                    if decoded.len() != 1 {
                        return Err(error("Library entry must contain one model"));
                    }
                    decoded[0].name = item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map_or_else(|| format!("Stencil {}", i + 1), str::to_string);
                    pages.extend(decoded);
                }
                return Ok(Some(pages));
            }
            Event::Start(_) | Event::Empty(_) | Event::Eof => return Ok(None),
            Event::DocType(_) => {
                return Err(error("Document type declarations are not supported."));
            }
            _ => {}
        }
    }
}
pub fn from_xml(xml: &str) -> Result<Imported> {
    if xml.len() > MAX_BYTES {
        return Err(error("Diagram exceeds 32 MiB."));
    }
    let embedded = embedded_xml(xml)?;
    let xml = embedded.as_deref().unwrap_or(xml);
    if xml.len() > MAX_BYTES {
        return Err(error("Diagram exceeds 32 MiB."));
    }
    let mut budget = MAX_BYTES * 2;
    let pages = match library_pages(xml, &mut budget)? {
        Some(pages) => pages,
        None => parse_pages(xml, 0, &mut budget)?,
    };
    let mut warnings = BTreeSet::new();
    let pages = pages
        .into_iter()
        .enumerate()
        .map(|(i, p)| build(p, i as u64 + 1, &mut warnings))
        .collect::<Result<Vec<_>>>()?;
    let project = Project {
        kind: ProjectKind::Diagram,
        active: 1,
        next_page_id: pages.len() as u64 + 1,
        pages,
    };
    project.validate().map_err(error)?;
    Ok(Imported {
        project,
        warnings: warnings.into_iter().collect(),
    })
}
pub fn is_drawio(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("drawio"))
}
pub fn read(path: &Path) -> Result<Imported> {
    let mut xml = String::new();
    std::fs::File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_string(&mut xml)?;
    if xml.len() > MAX_BYTES {
        return Err(error("Diagram exceeds 32 MiB."));
    }
    from_xml(&xml)
}
fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}
#[cfg(test)]
fn text(doc: &Document, id: NodeId) -> String {
    match doc.node(id).map(|n| &n.kind) {
        Some(NodeKind::Text { spec, .. }) => spec.text.clone(),
        _ => String::new(),
    }
}
fn hex(color: Option<[u8; 4]>) -> String {
    color.map_or("none".into(), |c| {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    })
}
fn paint(doc: &Document, id: NodeId) -> String {
    match doc.node(id).map(|n| &n.kind) {
        Some(NodeKind::Path { style, .. }) => {
            let mut value = format!(
                "fillColor={};strokeColor={};strokeWidth={};fillOpacity={};strokeOpacity={};",
                hex(style.fill),
                hex(style.stroke),
                style.width,
                style.fill.map_or(100., |c| c[3] as f64 / 2.55),
                style.stroke.map_or(100., |c| c[3] as f64 / 2.55)
            );
            if style.dash_count > 0 {
                value.push_str(&format!(
                    "dashed=1;dashPattern={};",
                    style.dash[..style.dash_count as usize]
                        .iter()
                        .map(|d| (d / style.width.max(1.)).to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            value
        }
        _ => String::new(),
    }
}
fn port_style(port: Port, prefix: &str) -> String {
    let point = match port {
        Port::Auto => return String::new(),
        Port::North => (0.5, 0.),
        Port::East => (1., 0.5),
        Port::South => (0.5, 1.),
        Port::West => (0., 0.5),
        Port::Custom { x, y } => (x, y),
    };
    format!("{prefix}X={};{prefix}Y={};", point.0, point.1)
}
fn label_html(doc: &Document, id: NodeId) -> String {
    match doc.node(id).map(|n| &n.kind) {
        Some(NodeKind::Text { spec, .. }) => labels::html(spec),
        _ => String::new(),
    }
}
// Keep compound vector artwork as a scalable image while labels, bounds and
// graph connections remain independent editable draw.io objects.
fn compound_artwork(
    doc: &Document,
    id: NodeId,
    shape: &diagram::Shape,
    model: &diagram::Diagram,
) -> Result<Option<String>> {
    let mut excluded = HashSet::from([shape.label]);
    for child in model.shapes.keys().chain(model.edges.keys()) {
        if *child != id && doc.is_ancestor(id, *child) {
            excluded.extend(doc.subtree(*child));
        }
    }
    let own: HashSet<_> = doc
        .subtree(id)
        .into_iter()
        .filter(|n| !excluded.contains(n))
        .collect();
    let complex_paint = doc.node(shape.body).is_some_and(|n| matches!(&n.kind, NodeKind::Path { style, .. } if style.fill_paint != emulsion_raster::vector::PathPaint::Solid || style.stroke_paint != emulsion_raster::vector::PathPaint::Solid));
    if own.len() <= 2 && !complex_paint && doc.node(id).is_none_or(|n|n.styles.is_empty()) {
        return Ok(None);
    }
    let mut artwork = doc.clone();
    artwork.nodes.retain(|n| own.contains(&n.id));
    artwork.diagram = None;
    let root = artwork.node_mut(id).unwrap();
    root.parent = None;
    root.opacity = 1.;
    root.visible = true;
    let bounds =
        diagram::shape_bounds(doc, shape).ok_or_else(|| error("Missing artwork bounds"))?;
    let svg = crate::project_export::bounded_subtree(&artwork, id, bounds)?;
    Ok(Some(format!(
        "shape=image;image=data:image/svg+xml;base64,{};{}",
        base64::engine::general_purpose::STANDARD.encode(svg),
        if shape.kind.is_container() {
            "container=1;"
        } else {
            ""
        }
    )))
}
pub fn to_xml(project: &Project) -> Result<String> {
    project.validate().map_err(error)?;
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><mxfile host=\"Emulsion\">");
    for page in &project.pages {
        let doc = &page.doc;
        let empty = diagram::Diagram::default();
        let model = doc.diagram.as_deref().unwrap_or(&empty);
        // Additional edge labels have no mxGeometry mapping yet. Never drop
        // them silently while allowing compound artwork owned by shapes.
        for node in &doc.nodes {
            if model.shapes.contains_key(&node.id) {
                continue;
            }
            let mut parent = node.parent;
            while let Some(id) = parent {
                if model.shapes.contains_key(&id) {
                    break;
                }
                if let Some(edge) = model.edges.get(&id) {
                    if ![edge.path, edge.arrow, edge.label].contains(&node.id) && !edge.labels.iter().any(|l|l.node==node.id) && edge.double_path!=Some(node.id) && edge.label_background_path!=Some(node.id) {
                        return Err(error(
                            "Additional connector artwork requires native project or SVG export.",
                        ));
                    }
                    break;
                }
                parent = doc.node(id).and_then(|n| n.parent);
            }
        }
        let layers:HashSet<_>=doc.nodes.iter().filter(|n|n.is_group() && !model.shapes.contains_key(&n.id) && !model.edges.contains_key(&n.id) && doc.subtree(n.id).iter().any(|id|model.shapes.contains_key(id)||model.edges.contains_key(id))).map(|n|n.id).collect();
        let included: HashSet<_> = model
            .shapes
            .keys()
            .chain(model.edges.keys())
            .flat_map(|id| doc.subtree(*id))
            .collect();
        if doc
            .nodes
            .iter()
            .any(|n| !included.contains(&n.id) && !layers.contains(&n.id) && !matches!(n.kind, NodeKind::Fill { .. }))
        {
            return Err(error(
                "This page includes artwork outside the diagram graph. Use SVG/PDF or the native project to retain it.",
            ));
        }
        let background = doc
            .nodes
            .iter()
            .find_map(|n| {
                if let NodeKind::Fill { rgba } = &n.kind {
                    Some(*rgba)
                } else {
                    None
                }
            })
            .unwrap_or([0; 4]);
        xml.push_str(&format!("<diagram id=\"{}\" name=\"{}\"><mxGraphModel pageWidth=\"{}\" pageHeight=\"{}\" background=\"{}\" grid=\"1\" gridSize=\"20\"><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>",page.meta.id,escape(&page.meta.name),doc.width,doc.height,hex((background[3]>0).then_some(background))));
        let mut cells = HashMap::new();
        for layer in doc.nodes.iter().filter(|n|layers.contains(&n.id)) {
            let parent=layer.parent.filter(|id|layers.contains(id)).map_or("0".into(),|id|format!("l{id}"));
            cells.insert(layer.id,format!("<mxCell id=\"l{}\" value=\"{}\" parent=\"{parent}\" visible=\"{}\"/>",layer.id,escape(&layer.name),u8::from(layer.visible)));
        }
        for (id, shape) in &model.shapes {
            let [mut x, mut y, w, h] =
                diagram::shape_bounds(doc, shape).ok_or_else(|| error("Missing shape bounds"))?;
            let parent = shape.container.map(|id|format!("s{id}")).or_else(||doc.node(*id).and_then(|n|n.parent).filter(|id|layers.contains(id)).map(|id|format!("l{id}"))).unwrap_or("1".into());
            if let Some(container) = shape
                .container
                .and_then(|id| model.shapes.get(&id))
                .and_then(|s| diagram::shape_bounds(doc, s))
            {
                x -= container[0];
                y -= container[1];
            }
            if shape.data.contains_key(build::ANCHOR) {
                continue;
            }
            let style = match shape.kind {
                ShapeKind::Process => "rounded=0;",
                ShapeKind::Decision => "rhombus;perimeter=rhombusPerimeter;",
                ShapeKind::Terminator => "rounded=1;arcSize=50;",
                ShapeKind::Data => "shape=parallelogram;",
                ShapeKind::Database => "shape=cylinder3;",
                ShapeKind::Document => "shape=document;",
                ShapeKind::Note => "shape=note;",
                ShapeKind::Class => "shape=umlClass;",
                ShapeKind::Entity => "shape=table;",
                ShapeKind::Container => "container=1;",
                ShapeKind::Swimlane => "swimlane;",
                ShapeKind::Cloud => "shape=cloud;",
            };
            let artwork = compound_artwork(doc, *id, shape, model)?;
            let curved_process = shape.kind == ShapeKind::Process && matches!(&doc.node(shape.body).unwrap().kind, NodeKind::Path {path,..} if path.subpaths.iter().flat_map(|s|&s.anchors).any(|a|a.h_in!=a.p || a.h_out!=a.p));
            let custom = if curved_process || shape.data.contains_key("emulsion_stencil")
                || shape.data.contains_key("drawio_custom_path")
            {
                if let NodeKind::Path { path, .. } = &doc.node(shape.body).unwrap().kind {
                    Some(stencils::encode(
                        path,
                        diagram::shape_bounds(doc, shape).unwrap(),
                    )?)
                } else {
                    None
                }
            } else {
                None
            };
            let node = doc.node(*id).unwrap();
            let label_position = if let NodeKind::Text { spec, .. } =
                &doc.node(shape.label).unwrap().kind
            {
                let [bx, by, _, bh] = diagram::shape_bounds(doc, shape).unwrap();
                format!(
                    "emulsionLabelX={};emulsionLabelY={};emulsionLabelWidth={};emulsionLabelRotation={};{}",
                    spec.x as f64 - bx,
                    spec.y as f64 - by,
                    spec.width.unwrap_or(w as f32),
                    spec.rotation,
                    if spec.y as f64 >= by + bh {
                        "verticalLabelPosition=bottom;verticalAlign=top;"
                    } else {
                        ""
                    }
                )
            } else {
                String::new()
            };
            let style = escape(&format!(
                "{}{}{}opacity={};html=1;whiteSpace=wrap;{label_position}",
                artwork
                    .as_deref()
                    .or(custom.as_deref())
                    .unwrap_or_else(|| shape
                        .data
                        .get("drawio_geometry_style")
                        .map_or(style, String::as_str)),
                paint(doc, shape.body),
                label_style(doc, shape.label)?,
                node.opacity * 100.
            ));
            let link=doc.design.interactions.get(id).and_then(|actions|actions.iter().find_map(|a|if let emulsion_core::design_interactions::Action::Url{url}=a{Some(url)}else{None})).or_else(||shape.data.get("drawio_link")).map_or(String::new(),|url|format!(" link=\"{}\"",escape(url)));
            cells.insert(*id, format!("<mxCell{link} id=\"s{id}\" value=\"{}\" vertex=\"1\" visible=\"{}\" parent=\"{parent}\" style=\"{style}\" emulsionData=\"{}\"><mxGeometry x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" as=\"geometry\"/></mxCell>",escape(&label_html(doc,shape.label)),u8::from(node.visible),escape(&serde_json::to_string(&shape.data).map_err(|e|error(e.to_string()))?)));
        }
        for (id, edge) in &model.edges {
            let mut cell_xml = String::new();
            let routing = if edge.routing == Routing::Orthogonal {
                "edgeStyle=orthogonalEdgeStyle;"
            } else if edge.routing == Routing::Cyclical {
                "edgeStyle=none;curved=1;emulsionRouting=cyclical;"
            } else if edge.routing == Routing::Curved {
                "edgeStyle=none;curved=1;"
            } else {
                "edgeStyle=none;"
            };
            let node = doc.node(*id).unwrap();
            let mut style = escape(&format!(
                "{routing}{}{}{}{}opacity={};endArrow={};startArrow={};html=1;endFill={};startFill={};endSize={};startSize={};jumpStyle={};jumpSize={};rounded={};arcSize={};",
                port_style(edge.source.port, "exit"),
                port_style(edge.target.port, "entry"),
                paint(doc, edge.path),
                label_style(doc, edge.label)?,
                node.opacity * 100.,
                if edge.arrow_end {
                    edge.end_marker.kind.drawio()
                } else {
                    "none"
                },
                if edge.arrow_start {
                    edge.start_marker.kind.drawio()
                } else {
                    "none"
                },
                u8::from(edge.end_marker.filled),
                u8::from(edge.start_marker.filled),
                edge.end_marker.size,
                edge.start_marker.size,
                edge.jump_style.drawio(), edge.jump_size, u8::from(edge.corner_radius > 0.), edge.corner_radius
            ));
            let mut refs = String::new();
            style.push_str(&format!("emulsionDoubleLine={};",u8::from(edge.double_line)));
            if let Some(c)=edge.label_background {style.push_str(&format!("labelBackgroundColor=#{:02x}{:02x}{:02x};emulsionLabelBackgroundAlpha={};",c[0],c[1],c[2],c[3]));}
            let mut endpoints = String::new();
            for (name, endpoint) in [("source", &edge.source), ("target", &edge.target)] {
                if model.edges.contains_key(&endpoint.shape) {
                    refs.push_str(&format!(" {name}=\"e{}\"",endpoint.shape));continue;
                }
                let shape = &model.shapes[&endpoint.shape];
                if shape.data.contains_key(build::ANCHOR) {
                    let bounds = diagram::shape_bounds(doc, shape)
                        .ok_or_else(|| error("Missing endpoint bounds"))?;
                    let ((x, y), _) = endpoint.port.anchor(bounds, (bounds[0], bounds[1]));
                    endpoints.push_str(&format!(
                        "<mxPoint x=\"{x}\" y=\"{y}\" as=\"{name}Point\"/>"
                    ));
                } else {
                    refs.push_str(&format!(" {name}=\"s{}\"", endpoint.shape));
                }
            }
            let edge_parent=node.parent.filter(|id|layers.contains(id)).map_or("1".into(),|id|format!("l{id}"));
            cell_xml.push_str(&format!("<mxCell id=\"e{id}\" value=\"{}\" visible=\"{}\" edge=\"1\" parent=\"{edge_parent}\"{refs} style=\"{style}\"><mxGeometry x=\"{}\" y=\"{}\" relative=\"1\" as=\"geometry\">{endpoints}<mxPoint x=\"{}\" y=\"{}\" as=\"offset\"/><Array as=\"points\">",escape(&label_html(doc,edge.label)),u8::from(node.visible),edge.label_position,edge.label_normal,edge.label_offset.0,edge.label_offset.1));
            for (x, y) in &edge.waypoints {
                cell_xml.push_str(&format!("<mxPoint x=\"{x}\" y=\"{y}\"/>"));
            }
            cell_xml.push_str("</Array></mxGeometry></mxCell>");
            for label in &edge.labels {
                let style=escape(&format!("text;html=1;{}",label_style(doc,label.node)?));
                cell_xml.push_str(&format!("<mxCell id=\"label{}\" value=\"{}\" vertex=\"1\" parent=\"e{id}\" style=\"{style}\"><mxGeometry x=\"{}\" y=\"{}\" relative=\"1\" as=\"geometry\"><mxPoint x=\"{}\" y=\"{}\" as=\"offset\"/></mxGeometry></mxCell>",label.node,escape(&label_html(doc,label.node)),label.position,label.normal,label.offset.0,label.offset.1));
            }
            cells.insert(*id, cell_xml);
        }
        for node in &doc.nodes {
            if let Some(cell) = cells.get(&node.id) {
                xml.push_str(cell);
            }
        }
        xml.push_str("</root></mxGraphModel></diagram>");
    }
    xml.push_str("</mxfile>");
    if xml.len() > MAX_BYTES {
        return Err(error("Diagram exceeds 32 MiB."));
    }
    Ok(xml)
}
pub fn write(project: &Project, path: &Path) -> Result<()> {
    for page in &project.pages {
        crate::ora::ensure_not_raw_original(&page.doc, path)?;
        for commit in page.graph.commits() {
            crate::ora::ensure_not_raw_original(&commit.doc, path)?;
        }
    }
    let xml = to_xml(project)?;
    crate::write_atomic(path, |file| {
        file.write_all(xml.as_bytes())?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const GRAPH: &str = r##"<mxGraphModel pageWidth="800" pageHeight="600"><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="Start &amp; review" vertex="1" parent="1" style="rounded=1;fillColor=#abcdee"><mxGeometry x="40" y="40" width="120" height="60" as="geometry"/></mxCell><mxCell id="b" value="Ready?" vertex="1" parent="1" style="rhombus;"><mxGeometry x="400" y="240" width="140" height="80" as="geometry"/></mxCell><mxCell id="edge" value="Yes" edge="1" source="a" target="b" parent="1" style="edgeStyle=orthogonalEdgeStyle;exitX=1;exitY=0.5;entryX=0.5;entryY=0"><mxGeometry relative="1" as="geometry"><Array as="points"><mxPoint x="250" y="70"/><mxPoint x="250" y="180"/></Array></mxGeometry></mxCell></root></mxGraphModel>"##;
    #[test]
    fn multi_page_drawio_roundtrip_keeps_ports_labels_and_waypoints() {
        let xml = format!(
            "<mxfile><diagram name=\"Flow\">{GRAPH}</diagram><diagram name=\"Copy\">{GRAPH}</diagram></mxfile>"
        );
        let imported = from_xml(&xml).unwrap();
        assert!(imported.warnings.is_empty());
        assert_eq!(imported.project.pages.len(), 2);
        let xml = to_xml(&imported.project).unwrap();
        let round = from_xml(&xml).unwrap();
        assert_eq!(round.project.pages.len(), 2);
        for page in round.project.pages {
            let model = page.doc.diagram.as_ref().unwrap();
            assert_eq!(model.shapes.len(), 2);
            let edge = model.edges.values().next().unwrap();
            assert_eq!(edge.source.port, Port::East);
            assert_eq!(edge.target.port, Port::North);
            assert_eq!(edge.waypoints, vec![(250., 70.), (250., 180.)]);
            assert_eq!(text(&page.doc, edge.label), "Yes");
            assert!(
                model
                    .shapes
                    .values()
                    .any(|s| text(&page.doc, s.label) == "Start & review")
            );
            page.doc.validate().unwrap();
        }
    }
    #[test]
    fn roundtrip_preserves_fonts_visibility_background_and_connector_offsets() {
        let xml = GRAPH.replace("pageHeight=\"600\"", "pageHeight=\"600\" background=\"#123456\"")
            .replace("rounded=1;", "rounded=1;fontFamily=Geist Mono;fontSize=23;fontStyle=3;fontColor=#cc8844;align=left;opacity=65;dashed=1;dashPattern=4 2;")
            .replace("id=\"a\"", "id=\"a\" visible=\"0\"")
            .replace("<Array as=\"points\">", "<mxPoint x=\"13.5\" y=\"-21.25\" as=\"offset\"/><Array as=\"points\">");
        let imported = from_xml(&xml).unwrap();
        let roundtrip = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
        let doc = &roundtrip.project.pages[0].doc;
        assert!(doc.nodes.iter().any(|n| matches!(
            n.kind,
            NodeKind::Fill {
                rgba: [0x12, 0x34, 0x56, 255]
            }
        )));
        let model = doc.diagram.as_ref().unwrap();
        let (id, shape) = model
            .shapes
            .iter()
            .find(|(_, s)| text(doc, s.label).starts_with("Start"))
            .unwrap();
        assert!(!doc.node(*id).unwrap().visible);
        assert!((doc.node(*id).unwrap().opacity - 0.65).abs() < 0.0001);
        let NodeKind::Text { spec, .. } = &doc.node(shape.label).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.font, "Geist Mono");
        assert_eq!(spec.size, 23.);
        assert!(spec.bold && spec.italic);
        assert_eq!(spec.color, [0xcc, 0x88, 0x44, 255]);
        let NodeKind::Path { style, .. } = &doc.node(shape.body).unwrap().kind else {
            panic!()
        };
        assert_eq!(style.dash_count, 2);
        assert_eq!(
            model.edges.values().next().unwrap().label_offset,
            (13.5, -21.25)
        );
    }
    #[test]
    fn compressed_pages_and_nested_containers_are_editable() {
        let encoded = GRAPH
            .bytes()
            .map(|b| format!("%{b:02X}"))
            .collect::<String>();
        let mut compressor =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(encoded.as_bytes()).unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(compressor.finish().unwrap());
        let imported = from_xml(&format!(
            "<mxfile><diagram name=\"Compressed\">{data}</diagram></mxfile>"
        ))
        .unwrap();
        assert_eq!(imported.project.pages[0].meta.name, "Compressed");
        assert_eq!(
            imported.project.pages[0]
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .edges
                .len(),
            1
        );
        let xml = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="child" vertex="1" parent="container" value="Task"><mxGeometry x="20" y="40" width="120" height="60"/></mxCell><mxCell id="container" vertex="1" parent="1" value="Team" style="swimlane;"><mxGeometry x="100" y="100" width="400" height="300"/></mxCell></root></mxGraphModel>"#;
        let imported = from_xml(xml).unwrap();
        let doc = &imported.project.pages[0].doc;
        let model = doc.diagram.as_ref().unwrap();
        let child = model
            .shapes
            .values()
            .find(|s| s.container.is_some())
            .unwrap();
        assert_eq!(
            diagram::shape_bounds(doc, child),
            Some([120., 140., 120., 60.])
        );
        let second = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
        assert_eq!(
            second.project.pages[0]
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .shapes
                .values()
                .filter(|s| s.container.is_some())
                .count(),
            1
        );
    }
    #[test]
    fn invalid_references_do_not_silently_drop_connectors() {
        assert!(from_xml(&GRAPH.replace("target=\"b\"", "target=\"missing\"")).is_err());
        assert!(from_xml(&GRAPH.replace("id=\"b\"", "id=\"a\"")).is_err());
        assert!(
            from_xml(&GRAPH.replace("exitX=1", "exitX=NaN"))
                .unwrap()
                .warnings
                .iter()
                .any(|s| s.contains("port coordinates"))
        );
        assert!(from_xml(&GRAPH.replace("x=\"40\"", "x=\"NaN\"")).is_err());
        assert!(from_xml("<!DOCTYPE mxfile><mxGraphModel/>").is_err());
        assert!(from_xml("<mxfile><diagram>bad base64!</diagram></mxfile>").is_err());
    }
    #[test]
    fn native_project_and_version_roundtrip_keeps_diagram_graph() {
        let mut imported = from_xml(GRAPH).unwrap();
        let page = &mut imported.project.pages[0];
        let mut editor = Editor::new(page.doc.clone(), None);
        editor.create_version("Connected");
        page.graph = editor.graph;
        let expected = page.doc.diagram.clone();
        let file = std::env::temp_dir().join(format!(
            "emulsion-diagram-{}-native.emu",
            std::process::id()
        ));
        crate::project::write(&imported.project, &file).unwrap();
        let reopened = crate::project::read(&file).unwrap();
        std::fs::remove_file(file).unwrap();
        assert_eq!(
            reopened.pages[0].doc.diagram,
            imported.project.pages[0].doc.diagram
        );
        assert!(
            reopened.pages[0]
                .graph
                .commits()
                .any(|c| c.doc.diagram == expected)
        );
    }
}

#[cfg(test)]
mod tests_compat;
