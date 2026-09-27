//! Local editable draw.io XML interchange, including bounded raw-DEFLATE pages.
//! Format reference: https://www.drawio.com/docs/reference/diagram-generation/
use crate::{IoError, Result};
use base64::Engine;
use emulsion_core::{
    Command, Document, Editor, Node, NodeId, NodeKind,
    command::Slot,
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
                        if let Some(value) = attrs.get("background") {
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
                        let text = decompress(&compressed)?;
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
    cell.attrs
        .get("style")
        .into_iter()
        .flat_map(|s| s.split(';'))
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.split_once('=')
                .map_or((s.to_owned(), "1".into()), |(k, v)| {
                    (k.to_owned(), v.to_owned())
                })
        })
        .collect()
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
        "" | "rectangle" | "rect" => ShapeKind::Process,
        "parallelogram" => ShapeKind::Data,
        "cylinder" | "cylinder3" => ShapeKind::Database,
        "document" => ShapeKind::Document,
        "note" => ShapeKind::Note,
        "cloud" => ShapeKind::Cloud,
        "umlClass" => ShapeKind::Class,
        "table" => ShapeKind::Entity,
        other => {
            warnings.insert(format!(
                "Shape {other:?} imported as an editable rectangle."
            ));
            ShapeKind::Process
        }
    }
}
fn color(text: &str) -> Result<Option<[u8; 4]>> {
    if text == "none" {
        return Ok(None);
    }
    let hex = text
        .strip_prefix('#')
        .ok_or_else(|| error("Expected a hexadecimal color"))?;
    if hex.len() != 6 {
        return Err(error("Expected a six-digit color"));
    }
    let n = u32::from_str_radix(hex, 16).map_err(|_| error("Invalid color"))?;
    Ok(Some([(n >> 16) as u8, (n >> 8) as u8, n as u8, 255]))
}
fn plain_label(text: &str, html: bool, warnings: &mut BTreeSet<String>) -> String {
    if !html {
        return text.into();
    }
    warnings.insert("HTML label formatting was converted to editable plain text.".into());
    let text = text
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</div>", "\n")
        .replace("</p>", "\n");
    let mut out = String::new();
    let mut tag = false;
    for c in text.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    quick_xml::escape::unescape(&out)
        .map(|s| s.trim_end().replace("&nbsp;", " "))
        .unwrap_or(out)
}
fn port(style: &BTreeMap<String, String>, prefix: &str) -> Result<Port> {
    let x = format!("{prefix}X");
    let y = format!("{prefix}Y");
    if !style.contains_key(&x) && !style.contains_key(&y) {
        return Ok(Port::Auto);
    }
    let x = number(style, &x, 0.5)?;
    let y = number(style, &y, 0.5)?;
    Ok(match (x, y) {
        (0.5, 0.) => Port::North,
        (1., 0.5) => Port::East,
        (0.5, 1.) => Port::South,
        (0., 0.5) => Port::West,
        _ => Port::Custom { x, y },
    })
}
fn apply_style(
    editor: &mut Editor,
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
    }) = editor.doc.node(body)
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
        editor
            .execute(Command::SetPath {
                id: body,
                path,
                style: paint,
            })
            .map_err(|e| error(e.to_string()))?;
    }
    if let Some(Node {
        kind: NodeKind::Text { spec, .. },
        ..
    }) = editor.doc.node(label)
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
        if flags & !3 != 0 {
            warnings.insert("Underline/strikethrough labels require manual review.".into());
        }
        if let Some(font) = style.get("fontFamily") {
            spec.font = font.clone();
        }
        if let Some(value) = style.get("fontColor")
            && let Ok(Some(color)) = color(value)
        {
            spec.color = color;
        }
        editor
            .execute(Command::SetText {
                id: label,
                spec: Box::new(spec),
            })
            .map_err(|e| error(e.to_string()))?;
    }
    for key in [
        "rotation",
        "gradientColor",
        "image",
        "shadow",
        "sketch",
        "curved",
    ] {
        if style.get(key).is_some_and(|v| v != "0" && v != "none") {
            warnings.insert(format!("Style {key} requires manual review after import."));
        }
    }
    Ok(())
}
fn apply_cell(
    editor: &mut Editor,
    id: NodeId,
    cell: &Cell,
    style: &BTreeMap<String, String>,
) -> Result<()> {
    let opacity = number(style, "opacity", 100.)?.clamp(0., 100.) as f32 / 100.;
    editor
        .execute(Command::SetOpacity { id, opacity })
        .map_err(|e| error(e.to_string()))?;
    editor
        .execute(Command::SetVisible {
            id,
            visible: cell.attrs.get("visible").is_none_or(|v| v != "0"),
        })
        .map_err(|e| error(e.to_string()))?;
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
fn build(page: Page, id: u64, warnings: &mut BTreeSet<String>) -> Result<ProjectPage> {
    let mut seen = HashSet::new();
    let mut cells = BTreeMap::new();
    for cell in page.cells {
        let key = cell
            .attrs
            .get("id")
            .ok_or_else(|| error("Cell is missing an ID"))?
            .clone();
        if !seen.insert(key.clone()) {
            return Err(error("Duplicate diagram cell ID"));
        }
        cells.insert(key, cell);
    }
    let mut editor = Editor::new(Document::new(page.width, page.height), None);
    editor
        .execute(Command::AddNode {
            node: Box::new(Node::new(
                0,
                "Background",
                NodeKind::Fill {
                    rgba: page.background,
                },
            )),
            slot: Slot::TOP,
        })
        .map_err(|e| error(e.to_string()))?;
    let mut map = HashMap::new();
    let mut positions = HashMap::new();
    let mut waiting = cells
        .iter()
        .filter(|(_, c)| c.attrs.get("vertex").is_some_and(|v| v == "1"))
        .collect::<Vec<_>>();
    while !waiting.is_empty() {
        let count = waiting.len();
        let mut next = Vec::new();
        for (key, cell) in waiting {
            let parent = cell.attrs.get("parent").filter(|p| {
                cells
                    .get(*p)
                    .is_some_and(|c| c.attrs.get("vertex").is_some_and(|v| v == "1"))
            });
            if parent.is_some_and(|p| !map.contains_key(p)) {
                next.push((key, cell));
                continue;
            }
            let offset = parent
                .and_then(|p| positions.get(p))
                .copied()
                .unwrap_or((0., 0.));
            let bounds = [
                number(&cell.geometry, "x", 0.)? + offset.0,
                number(&cell.geometry, "y", 0.)? + offset.1,
                number(&cell.geometry, "width", 120.)?,
                number(&cell.geometry, "height", 60.)?,
            ];
            let style = style(cell);
            let mut kind = shape_kind(&style, warnings);
            if !kind.is_container()
                && cells.values().any(|c| {
                    c.attrs.get("parent") == Some(key)
                        && c.attrs.get("vertex").is_some_and(|v| v == "1")
                })
            {
                kind = ShapeKind::Container;
            }
            let label = plain_label(
                cell.attrs.get("value").map(String::as_str).unwrap_or(""),
                style.get("html").is_some_and(|v| v == "1"),
                warnings,
            );
            let node = diagram::add_shape(&mut editor, kind, bounds, &label).map_err(error)?;
            let mut model = editor.doc.diagram.as_deref().unwrap().clone();
            let shape = model.shapes.get_mut(&node).unwrap();
            let (body, text) = (shape.body, shape.label);
            if let Some(data) = cell.attrs.get("emulsionData") {
                shape.data = serde_json::from_str(data)
                    .map_err(|e| error(format!("Invalid shape data: {e}")))?;
            }
            editor
                .execute(Command::SetDiagram {
                    diagram: Some(Arc::new(model)),
                })
                .map_err(|e| error(e.to_string()))?;
            if let Some(parent) = parent {
                editor
                    .execute(Command::MoveNode {
                        id: node,
                        slot: Slot::top_of(Some(map[parent])),
                    })
                    .map_err(|e| error(e.to_string()))?;
            }
            apply_style(&mut editor, body, text, &style, warnings)?;
            apply_cell(&mut editor, node, cell, &style)?;
            map.insert(key.clone(), node);
            positions.insert(key.clone(), (bounds[0], bounds[1]));
        }
        if next.len() == count {
            return Err(error("Diagram contains cyclic or missing containers."));
        }
        waiting = next;
    }
    for (key, cell) in &cells {
        if !cell.attrs.get("edge").is_some_and(|v| v == "1") {
            continue;
        }
        let source = cell.attrs.get("source").and_then(|s| map.get(s));
        let target = cell.attrs.get("target").and_then(|s| map.get(s));
        let (Some(source), Some(target)) = (source, target) else {
            return Err(error(format!(
                "Connector {key} has an unbound or unsupported endpoint. No pages were imported."
            )));
        };
        let style = style(cell);
        let routing = if style
            .get("edgeStyle")
            .is_some_and(|s| s == "orthogonalEdgeStyle" || s == "elbowEdgeStyle")
        {
            Routing::Orthogonal
        } else {
            Routing::Straight
        };
        let label = plain_label(
            cell.attrs.get("value").map(String::as_str).unwrap_or(""),
            style.get("html").is_some_and(|v| v == "1"),
            warnings,
        );
        let edge = diagram::connect(
            &mut editor,
            Endpoint {
                shape: *source,
                port: port(&style, "exit")?,
            },
            Endpoint {
                shape: *target,
                port: port(&style, "entry")?,
            },
            &label,
            routing,
        )
        .map_err(error)?;
        let mut model = editor.doc.diagram.as_deref().unwrap().clone();
        let e = model.edges.get_mut(&edge).unwrap();
        e.waypoints = cell.points.clone();
        e.label_offset = cell.offset;
        e.arrow_end = style.get("endArrow").is_none_or(|v| v != "none");
        e.arrow_start = style.get("startArrow").is_some_and(|v| v != "none");
        let (path, label) = (e.path, e.label);
        editor
            .execute(Command::SetDiagram {
                diagram: Some(Arc::new(model)),
            })
            .map_err(|e| error(e.to_string()))?;
        apply_style(&mut editor, path, label, &style, warnings)?;
        apply_cell(&mut editor, edge, cell, &style)?;
    }
    editor.doc.validate().map_err(|e| error(e.to_string()))?;
    let graph = Editor::new(editor.doc.clone(), None).graph;
    Ok(ProjectPage {
        meta: PageMeta {
            id,
            name: page.name,
            bleed_mm: 0.,
        },
        doc: editor.doc,
        graph,
    })
}
pub fn from_xml(xml: &str) -> Result<Imported> {
    let pages = parse_pages(xml, 0, &mut (MAX_BYTES * 2))?;
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
pub fn to_xml(project: &Project) -> Result<String> {
    project.validate().map_err(error)?;
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><mxfile host=\"Emulsion\">");
    for page in &project.pages {
        let doc = &page.doc;
        let empty = diagram::Diagram::default();
        let model = doc.diagram.as_deref().unwrap_or(&empty);
        let included: HashSet<_> = model
            .shapes
            .keys()
            .chain(model.edges.keys())
            .flat_map(|id| doc.subtree(*id))
            .collect();
        if doc
            .nodes
            .iter()
            .any(|n| !included.contains(&n.id) && !matches!(n.kind, NodeKind::Fill { .. }))
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
        for (id, shape) in &model.shapes {
            let [mut x, mut y, w, h] =
                diagram::shape_bounds(doc, shape).ok_or_else(|| error("Missing shape bounds"))?;
            let parent = shape.container.map_or("1".into(), |id| format!("s{id}"));
            if let Some(container) = shape
                .container
                .and_then(|id| model.shapes.get(&id))
                .and_then(|s| diagram::shape_bounds(doc, s))
            {
                x -= container[0];
                y -= container[1];
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
            let node = doc.node(*id).unwrap();
            let style = escape(&format!(
                "{style}{}{}opacity={};html=0;whiteSpace=wrap;",
                paint(doc, shape.body),
                label_style(doc, shape.label)?,
                node.opacity * 100.
            ));
            xml.push_str(&format!("<mxCell id=\"s{id}\" value=\"{}\" vertex=\"1\" visible=\"{}\" parent=\"{parent}\" style=\"{style}\" emulsionData=\"{}\"><mxGeometry x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" as=\"geometry\"/></mxCell>",escape(&text(doc,shape.label)),u8::from(node.visible),escape(&serde_json::to_string(&shape.data).map_err(|e|error(e.to_string()))?)));
        }
        for (id, edge) in &model.edges {
            let routing = if edge.routing == Routing::Orthogonal {
                "edgeStyle=orthogonalEdgeStyle;"
            } else {
                "edgeStyle=none;"
            };
            let node = doc.node(*id).unwrap();
            let style = escape(&format!(
                "{routing}{}{}{}{}opacity={};endArrow={};startArrow={};html=0;",
                port_style(edge.source.port, "exit"),
                port_style(edge.target.port, "entry"),
                paint(doc, edge.path),
                label_style(doc, edge.label)?,
                node.opacity * 100.,
                if edge.arrow_end { "block" } else { "none" },
                if edge.arrow_start { "block" } else { "none" }
            ));
            xml.push_str(&format!("<mxCell id=\"e{id}\" value=\"{}\" visible=\"{}\" edge=\"1\" parent=\"1\" source=\"s{}\" target=\"s{}\" style=\"{style}\"><mxGeometry relative=\"1\" as=\"geometry\"><mxPoint x=\"{}\" y=\"{}\" as=\"offset\"/><Array as=\"points\">",escape(&text(doc,edge.label)),u8::from(node.visible),edge.source.shape,edge.target.shape,edge.label_offset.0,edge.label_offset.1));
            for (x, y) in &edge.waypoints {
                xml.push_str(&format!("<mxPoint x=\"{x}\" y=\"{y}\"/>"));
            }
            xml.push_str("</Array></mxGeometry></mxCell>");
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
        assert!(from_xml(&GRAPH.replace("exitX=1", "exitX=NaN")).is_err());
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
