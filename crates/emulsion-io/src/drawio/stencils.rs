//! Bounded mxGraph inline stencil geometry and native-path interchange.
use super::*;
use emulsion_raster::vector::Path as VectorPath;
use std::fmt::Write as _;

pub(super) fn decode(
    encoded: &str,
    bounds: [f64; 4],
    warnings: &mut BTreeSet<String>,
) -> Result<VectorPath> {
    let xml = decompress(encoded)?;
    if xml.len() > 2 << 20 {
        return Err(error("Inline stencil exceeds 2 MiB"));
    }
    let mut reader = Reader::from_str(&xml);
    let mut svg = String::new();
    let mut width = 100.;
    let mut height = 100.;
    let mut elements = 0;
    loop {
        match reader.read_event().map_err(|e| error(e.to_string()))? {
            Event::Start(e) | Event::Empty(e) => {
                elements += 1;
                if elements > 16384 {
                    return Err(error("Too many stencil elements"));
                }
                let attrs = attributes(&e)?;
                let n = |key, default| number(&attrs, key, default);
                match e.name().as_ref() {
                    "shape" => {
                        width = n("w", 100.)?;
                        height = n("h", 100.)?;
                        if width <= 0. || height <= 0. {
                            return Err(error("Invalid stencil dimensions"));
                        }
                    }
                    "move" => write!(svg, "M {} {} ", n("x", 0.)?, n("y", 0.)?).unwrap(),
                    "line" => write!(svg, "L {} {} ", n("x", 0.)?, n("y", 0.)?).unwrap(),
                    "curve" => write!(
                        svg,
                        "C {} {} {} {} {} {} ",
                        n("x1", 0.)?,
                        n("y1", 0.)?,
                        n("x2", 0.)?,
                        n("y2", 0.)?,
                        n("x3", 0.)?,
                        n("y3", 0.)?
                    )
                    .unwrap(),
                    "quad" => write!(
                        svg,
                        "Q {} {} {} {} ",
                        n("x1", 0.)?,
                        n("y1", 0.)?,
                        n("x2", 0.)?,
                        n("y2", 0.)?
                    )
                    .unwrap(),
                    "arc" => write!(
                        svg,
                        "A {} {} {} {} {} {} {} ",
                        n("rx", 0.)?,
                        n("ry", 0.)?,
                        n("x-axis-rotation", 0.)?,
                        u8::from(n("large-arc-flag", 0.)? != 0.),
                        u8::from(n("sweep-flag", 0.)? != 0.),
                        n("x", 0.)?,
                        n("y", 0.)?
                    )
                    .unwrap(),
                    "close" => svg.push_str("Z "),
                    "rect" | "roundrect" => {
                        let x = n("x", 0.)?;
                        let y = n("y", 0.)?;
                        let w = n("w", 0.)?;
                        let h = n("h", 0.)?;
                        write!(svg, "M {x} {y}h {w}v {h}h {} Z ", -w).unwrap();
                        if e.name().as_ref() == "roundrect" {
                            warnings.insert(
                                "Inline stencil rounded rectangles currently use square corners."
                                    .into(),
                            );
                        }
                    }
                    "ellipse" => {
                        let x = n("x", 0.)?;
                        let y = n("y", 0.)?;
                        let w = n("w", 0.)?;
                        let h = n("h", 0.)?;
                        write!(
                            svg,
                            "M {} {} A {} {} 0 1 1 {} {} A {} {} 0 1 1 {} {} Z ",
                            x + w,
                            y + h / 2.,
                            w / 2.,
                            h / 2.,
                            x,
                            y + h / 2.,
                            w / 2.,
                            h / 2.,
                            x + w,
                            y + h / 2.
                        )
                        .unwrap();
                    }
                    "foreground" | "background" | "path" | "fillstroke" | "fill" | "stroke"
                    | "connections" | "constraint" => {}
                    other => {
                        warnings.insert(format!("Inline stencil instruction {other:?} uses the cell's default appearance."));
                    }
                }
            }
            Event::DocType(_) => {
                return Err(error("Stencil entity declarations are not supported"));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    // usvg normalizes SVG arcs and quadratic segments to native cubic geometry.
    let source = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><path d="{svg}" stroke="black"/></svg>"#
    );
    let tree = resvg::usvg::Tree::from_str(&source, &resvg::usvg::Options::default())
        .map_err(|e| error(e.to_string()))?;
    let mut normalized = String::new();
    for node in tree.root().children() {
        if let resvg::usvg::Node::Path(path) = node {
            for segment in path.data().segments() {
                use resvg::tiny_skia::PathSegment;
                match segment {
                    PathSegment::MoveTo(p) => write!(normalized, "M {} {} ", p.x, p.y),
                    PathSegment::LineTo(p) => write!(normalized, "L {} {} ", p.x, p.y),
                    PathSegment::QuadTo(a, b) => {
                        write!(normalized, "Q {} {} {} {} ", a.x, a.y, b.x, b.y)
                    }
                    PathSegment::CubicTo(a, b, c) => write!(
                        normalized,
                        "C {} {} {} {} {} {} ",
                        a.x, a.y, b.x, b.y, c.x, c.y
                    ),
                    PathSegment::Close => write!(normalized, "Z "),
                }
                .unwrap();
            }
        }
    }
    let mut path = VectorPath::from_svg(&normalized).map_err(|e| error(e.to_string()))?;
    if path.anchor_count() > emulsion_raster::vector::MAX_ANCHORS || path.subpaths.is_empty() {
        return Err(error("Empty or oversized stencil geometry"));
    }
    let [x, y, w, h] = bounds;
    path.transform(glam::DAffine2::from_cols_array(&[
        w / width,
        0.,
        0.,
        h / height,
        x,
        y,
    ]));
    Ok(path)
}

pub(super) fn encode(path: &VectorPath, [x, y, w, h]: [f64; 4]) -> Result<String> {
    let mut path = path.clone();
    path.transform(glam::DAffine2::from_cols_array(&[
        100. / w,
        0.,
        0.,
        100. / h,
        -x * 100. / w,
        -y * 100. / h,
    ]));
    let mut xml = String::from("<shape w=\"100\" h=\"100\"><foreground><path>");
    for line in &path.subpaths {
        let Some(first) = line.anchors.first() else {
            continue;
        };
        write!(xml, "<move x=\"{}\" y=\"{}\"/>", first.p.0, first.p.1).unwrap();
        let count = line.anchors.len();
        for i in 1..count + usize::from(line.closed) {
            let previous = &line.anchors[(i - 1) % count];
            let next = &line.anchors[i % count];
            if previous.h_out == previous.p && next.h_in == next.p {
                if i < count {
                    write!(xml, "<line x=\"{}\" y=\"{}\"/>", next.p.0, next.p.1).unwrap();
                }
            } else {
                write!(
                    xml,
                    "<curve x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" x3=\"{}\" y3=\"{}\"/>",
                    previous.h_out.0,
                    previous.h_out.1,
                    next.h_in.0,
                    next.h_in.1,
                    next.p.0,
                    next.p.1
                )
                .unwrap();
            }
        }
        if line.closed {
            xml.push_str("<close/>");
        }
    }
    xml.push_str("</path><fillstroke/></foreground></shape>");
    let mut compressor =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    compressor.write_all(xml.as_bytes())?;
    Ok(format!(
        "shape=stencil({});",
        base64::engine::general_purpose::STANDARD.encode(compressor.finish()?)
    ))
}
