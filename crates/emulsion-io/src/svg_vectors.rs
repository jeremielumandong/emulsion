//! SVG normalization to native editable paths. External resources are never fetched.
use crate::{IoError, Result};
use emulsion_core::{Document, Node, NodeId};
use emulsion_raster::vector::{Path, PathStyle, StrokeCap, StrokeJoin};
use resvg::{tiny_skia, usvg};
use std::{
    fmt::Write,
    sync::{Arc, OnceLock},
};
fn error(s: impl Into<String>) -> IoError {
    IoError::Unsupported(s.into())
}

pub(crate) fn options() -> usvg::Options<'static> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    usvg::Options {
        font_family: "Geist".into(),
        fontdb: FONTS
            .get_or_init(|| {
                let mut db = usvg::fontdb::Database::new();
                db.load_system_fonts();
                db.load_font_data(include_bytes!("../../../assets/fonts/Geist.ttf").to_vec());
                db.load_font_data(include_bytes!("../../../assets/fonts/GeistMono.ttf").to_vec());
                db.set_sans_serif_family("Geist");
                db.set_monospace_family("Geist Mono");
                // fontdb does not apply fontconfig aliases. Without a known
                // generic family, SVG fallback can select a symbol face.
                let serif = ["Liberation Serif", "DejaVu Serif", "Georgia", "Geist"]
                    .into_iter()
                    .find(|name| {
                        db.query(&usvg::fontdb::Query {
                            families: &[usvg::fontdb::Family::Name(name)],
                            ..Default::default()
                        })
                        .is_some()
                    })
                    .unwrap_or("Geist");
                db.set_serif_family(serif);
                db.set_cursive_family(serif);
                db.set_fantasy_family("Geist");
                Arc::new(db)
            })
            .clone(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub(crate) fn path(data: &tiny_skia::Path, t: tiny_skia::Transform) -> Result<Path> {
    if data.is_empty() {
        return Ok(Path::default());
    }
    let data = data
        .clone()
        .transform(t)
        .ok_or_else(|| error("Invalid SVG path transform"))?;
    let mut d = String::new();
    for segment in data.segments() {
        use tiny_skia::PathSegment::*;
        match segment {
            MoveTo(p) => write!(d, "M {} {} ", p.x, p.y),
            LineTo(p) => write!(d, "L {} {} ", p.x, p.y),
            QuadTo(a, b) => write!(d, "Q {} {} {} {} ", a.x, a.y, b.x, b.y),
            CubicTo(a, b, c) => write!(d, "C {} {} {} {} {} {} ", a.x, a.y, b.x, b.y, c.x, c.y),
            Close => write!(d, "Z "),
        }
        .unwrap();
    }
    Path::from_svg(&d).map_err(error)
}
fn solid(paint: &usvg::Paint, opacity: f32) -> Result<[u8; 4]> {
    match paint {
        usvg::Paint::Color(c) => Ok([c.red, c.green, c.blue, (opacity * 255.).round() as u8]),
        _ => Err(error("SVG gradient/pattern requires a retained appearance")),
    }
}
/// Append scalable paths under a graph shape, retaining transforms and group opacity.
pub fn append(
    doc: &mut Document,
    parent: NodeId,
    xml: &str,
    bounds: [f64; 4],
) -> Result<Vec<String>> {
    if xml.len() > 32 << 20 {
        return Err(error("SVG exceeds 32 MiB"));
    }
    let tree = usvg::Tree::from_str(xml, &options()).map_err(|e| error(e.to_string()))?;
    let [x, y, w, h] = bounds;
    let t = tiny_skia::Transform::from_row(
        w as f32 / tree.size().width(),
        0.,
        0.,
        h as f32 / tree.size().height(),
        x as f32,
        y as f32,
    );
    let mut nodes = Vec::new();
    let mut next = doc.next_id;
    fn walk(
        g: &usvg::Group,
        parent: NodeId,
        t: tiny_skia::Transform,
        next: &mut u64,
        nodes: &mut Vec<Node>,
        size: (u32, u32),
    ) -> Result<()> {
        if nodes.len() > 16384 {
            return Err(error("SVG exceeds 16,384 editable objects"));
        }
        if g.clip_path().is_some() || g.mask().is_some() || !g.filters().is_empty() {
            return Err(error(
                "SVG clipping, masks or filters require a retained appearance",
            ));
        }
        // Flattened text paths have local (often identity) abs_transform values.
        // Replay group transforms just as resvg does, including the text's own
        // flattened group, instead of consulting each leaf's absolute transform.
        let t = t.pre_concat(g.transform());
        let parent = if g.opacity().get() != 1. {
            let id = *next;
            *next += 1;
            let mut n = Node::group(id, "SVG group");
            n.parent = Some(parent);
            n.opacity = g.opacity().get();
            nodes.push(n);
            id
        } else {
            parent
        };
        for item in g.children() {
            if nodes.len() >= 16384 {
                return Err(error("SVG exceeds 16,384 editable objects"));
            }
            match item {
                usvg::Node::Group(g) => walk(g, parent, t, next, nodes, size)?,
                usvg::Node::Text(text) => walk(text.flattened(), parent, t, next, nodes, size)?,
                usvg::Node::Image(_) => return Err(error("SVG contains an embedded bitmap")),
                usvg::Node::Path(p) => {
                    let transform = t;
                    let scale = (transform.sx * transform.sy - transform.kx * transform.ky)
                        .abs()
                        .sqrt();
                    let mut style = PathStyle {
                        fill: None,
                        stroke: None,
                        width: 1.,
                        cap: StrokeCap::Butt,
                        join: StrokeJoin::Miter,
                        ..Default::default()
                    };
                    if let Some(fill) = p.fill() {
                        style.fill = Some(solid(fill.paint(), fill.opacity().get())?);
                        style.even_odd = fill.rule() == usvg::FillRule::EvenOdd;
                    }
                    if let Some(stroke) = p.stroke() {
                        style.stroke = Some(solid(stroke.paint(), stroke.opacity().get())?);
                        style.width = stroke.width().get() * scale;
                        style.cap = match stroke.linecap() {
                            usvg::LineCap::Butt => StrokeCap::Butt,
                            usvg::LineCap::Round => StrokeCap::Round,
                            usvg::LineCap::Square => StrokeCap::Square,
                        };
                        style.join = match stroke.linejoin() {
                            usvg::LineJoin::Round => StrokeJoin::Round,
                            usvg::LineJoin::Bevel => StrokeJoin::Bevel,
                            _ => StrokeJoin::Miter,
                        };
                        style.miter_limit = stroke.miterlimit().get();
                        if let Some(d) = stroke.dasharray() {
                            if d.len() > 6 {
                                return Err(error("SVG dash pattern exceeds native limit"));
                            }
                            style.dash_count = d.len() as u8;
                            for (i, v) in d.iter().enumerate() {
                                style.dash[i] = v * scale;
                            }
                            style.dash_offset = stroke.dashoffset() * scale;
                        }
                    }
                    let id = *next;
                    *next += 1;
                    let mut n = Node::path(
                        id,
                        if p.id().is_empty() {
                            "SVG path"
                        } else {
                            p.id()
                        },
                        Arc::new(path(p.data(), transform)?),
                        style,
                        size.0,
                        size.1,
                    );
                    n.parent = Some(parent);
                    n.visible = p.is_visible();
                    nodes.push(n);
                }
            }
        }
        Ok(())
    }
    walk(
        tree.root(),
        parent,
        t,
        &mut next,
        &mut nodes,
        (doc.width, doc.height),
    )?;
    let label = doc
        .nodes
        .iter()
        .position(|n| {
            n.parent == Some(parent) && matches!(n.kind, emulsion_core::NodeKind::Text { .. })
        })
        .unwrap_or(doc.nodes.len());
    doc.nodes.splice(label..label, nodes);
    doc.next_id = next;
    Ok(Vec::new())
}

/// Open ordinary SVG artwork with editable paths where possible, preserving the
/// original vector source for features outside native path paint capabilities.
pub fn document(xml: &str) -> Result<Document> {
    use base64::Engine;
    let tree = usvg::Tree::from_str(xml, &options()).map_err(|e| error(e.to_string()))?;
    let (w, h) = (
        tree.size().width().ceil() as u32,
        tree.size().height().ceil() as u32,
    );
    crate::import::check_size(w, h)?;
    let mut doc = Document::new(w, h);
    let root = doc.alloc_id();
    doc.nodes.push(Node::group(root, "SVG artwork"));
    let mut budget = 16_777_216;
    let mut notes = std::collections::BTreeSet::new();
    crate::drawio::images::insert(
        &mut doc,
        root,
        [0., 0., w as f64, h as f64],
        false,
        &format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(xml)
        ),
        &mut budget,
        &mut notes,
    )?;
    doc.normalize();
    doc.validate().map_err(|e| error(e.to_string()))?;
    Ok(doc)
}

/// Converter SVGs sometimes contain valid geometry but a zero-sized page.
/// Recover the page from visible vector bounds, retaining the vector source.
pub(crate) fn fitted_document(xml: &str) -> Result<Document> {
    use quick_xml::{
        Reader, Writer,
        events::{BytesStart, Event},
    };
    fn root(xml: &str, w: f64, h: f64, view: Option<[f64; 4]>) -> Result<String> {
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event().map_err(|e| error(e.to_string()))? {
                Event::Start(e) if e.local_name().as_ref() == "svg" => {
                    let name = e.name().as_ref().to_string();
                    let mut start = BytesStart::new(name);
                    for attr in e.attributes() {
                        let a = attr.map_err(|e| error(e.to_string()))?;
                        if !matches!(a.key.as_ref(), "width" | "height" | "viewBox") {
                            start.push_attribute(a);
                        }
                    }
                    let width = w.to_string();
                    let height = h.to_string();
                    start.push_attribute(("width", width.as_str()));
                    start.push_attribute(("height", height.as_str()));
                    let view = view.map(|b| format!("{} {} {} {}", b[0], b[1], b[2], b[3]));
                    if let Some(view) = view.as_ref() {
                        start.push_attribute(("viewBox", view.as_str()));
                    }
                    let mut writer = Writer::new(Vec::new());
                    writer.write_event(Event::Start(start))?;
                    let mut out =
                        String::from_utf8(writer.into_inner()).map_err(|e| error(e.to_string()))?;
                    out.push_str(&xml[reader.buffer_position() as usize..]);
                    return Ok(out);
                }
                Event::Eof => return Err(error("SVG has no root")),
                _ => {}
            }
        }
    }
    let provisional = root(xml, 1000., 1000., None)?;
    let tree = usvg::Tree::from_str(&provisional, &options()).map_err(|e| error(e.to_string()))?;
    if tree.root().children().is_empty() {
        return Err(error("SVG has no visible artwork"));
    }
    let b = tree.root().abs_layer_bounding_box();
    let (w, h) = (
        b.width().ceil().max(1.) as f64,
        b.height().ceil().max(1.) as f64,
    );
    let scale = (30000. / w.max(h))
        .min((100_000_000. / (w * h)).sqrt())
        .min(1.);
    let (width, height) = ((w * scale).ceil().max(1.), (h * scale).ceil().max(1.));
    crate::import::check_size(width as u32, height as u32)?;
    document(&root(
        xml,
        width,
        height,
        Some([b.x() as f64, b.y() as f64, w, h]),
    )?)
}

#[cfg(test)]
mod fit_tests {
    #[test]
    fn zero_converter_page_uses_vector_ink_bounds() {
        let doc=super::fitted_document(r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"><rect x="20" y="30" width="80" height="40" fill="red"/></svg>"#).unwrap();
        assert_eq!((doc.width, doc.height), (80, 40));
        let pixels = crate::svg_viewport::SvgViewport::new(&doc)
            .unwrap()
            .render((80, 40), [1., 0., 0., 1., 0., 0.])
            .unwrap();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 255, 255])
        );
    }
}

#[cfg(test)]
mod diagram_font_tests {
    use super::*;
    #[test]
    fn diagram_svg_fallback_uses_known_text_faces() {
        let opts = options();
        assert_eq!(opts.font_family, "Geist");
        for family in [
            usvg::fontdb::Family::SansSerif,
            usvg::fontdb::Family::Serif,
            usvg::fontdb::Family::Monospace,
        ] {
            let id = opts
                .fontdb
                .query(&usvg::fontdb::Query {
                    families: &[family],
                    ..Default::default()
                })
                .unwrap();
            let face = opts.fontdb.face(id).unwrap();
            assert!(
                !face
                    .families
                    .iter()
                    .any(|(name, _)| name.to_lowercase().contains("symbol"))
            );
        }
    }
}

#[cfg(test)]
mod text_transform_tests {
    use super::*;

    #[test]
    fn native_outlined_text_keeps_nested_translation_rotation_and_scale() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300" viewBox="-20 -10 400 300"><g transform="translate(170 90) scale(1.5)"><g transform="rotate(25)"><text x="12" y="28" font-family="Geist" font-size="24">Chart label</text><path d="M 0 40 H 100" fill="none" stroke="red"/></g></g></svg>"#;
        let mut doc = Document::new(800, 600);
        let root = doc.alloc_id();
        doc.nodes.push(Node::group(root, "Artwork"));
        append(&mut doc, root, source, [0., 0., 800., 600.]).unwrap();
        doc.normalize();
        doc.validate().unwrap();
        let (exported, rasterized) = crate::project_export::svg(&doc).unwrap();
        assert!(!rasterized);
        let render = |xml: &str, scale: f32| {
            let tree = usvg::Tree::from_str(xml, &options()).unwrap();
            let mut pixmap = tiny_skia::Pixmap::new(800, 600).unwrap();
            resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            pixmap
        };
        let original = render(source, 2.);
        let imported = render(std::str::from_utf8(&exported).unwrap(), 1.);
        let difference: u64 = original
            .data()
            .iter()
            .zip(imported.data())
            .map(|(a, b)| a.abs_diff(*b) as u64)
            .sum();
        // Allow subpixel antialiasing changes when native paths serialize their
        // float coordinates; misplaced glyphs differ by orders of magnitude.
        assert!(
            difference < 25_000,
            "native import moved text or paths: pixel difference {difference}"
        );
    }
}
