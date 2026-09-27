//! Page-aware local export. Source projects are never modified or flattened.
use crate::{IoError, Result};
use base64::Engine as _;
use emulsion_core::{
    Document, NodeId, NodeKind,
    project::{PageId, Project},
};
use emulsion_raster::{
    BlendMode, IRect,
    composite::flatten,
    vector::{PathPaint, StrokeAlignment},
};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    io::Write,
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Svg,
    Pdf,
}
impl Format {
    pub const ALL: [Self; 4] = [Self::Png, Self::Jpeg, Self::Svg, Self::Pdf];
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG pages",
            Self::Jpeg => "JPEG pages",
            Self::Svg => "SVG pages",
            Self::Pdf => "Print PDF",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }
}
#[derive(Default, Debug)]
pub struct Report {
    pub pages: usize,
    pub rasterized_pages: Vec<String>,
}

fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}
fn rgba(color: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}
fn matrix(m: glam::DAffine2) -> String {
    let [a, b, c, d, e, f] = m.to_cols_array();
    format!("matrix({a} {b} {c} {d} {e} {f})")
}
fn image(raster: &emulsion_raster::Raster, transform: &str) -> Result<String> {
    let png = crate::export::png16(raster.width(), raster.height(), &raster.to_srgba16())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(format!(
        "<image width=\"{}\" height=\"{}\" transform=\"{transform}\" href=\"data:image/png;base64,{encoded}\"/>",
        raster.width(),
        raster.height()
    ))
}

fn node_svg(doc: &Document, id: NodeId, out: &mut String) -> Result<()> {
    let n = doc.node(id).ok_or_else(|| error("Missing export layer"))?;
    if !n.visible {
        return Ok(());
    }
    if !(n.blend == BlendMode::Normal || (n.is_group() && n.blend == BlendMode::PassThrough))
        || n.blending != Default::default()
        || (n.mask_enabled && n.mask.is_some())
        || (n.effects_enabled && !n.styles.is_empty())
    {
        return Err(error(
            "Layer blending, masks, or effects require a rendered appearance.",
        ));
    }
    write!(out, "<g opacity=\"{}\">", n.opacity).unwrap();
    if let Some(base) = n.clip_to {
        let base = doc
            .node(base)
            .ok_or_else(|| error("Missing clipping base"))?;
        let NodeKind::Path { path, style, .. } = &base.kind else {
            return Err(error("This clipping base requires a rendered appearance."));
        };
        if base.mask.is_some()
            || !base.styles.is_empty()
            || base.opacity != 1.
            || style.fill.is_none_or(|rgba| rgba[3] != 255)
        {
            return Err(error("This frame requires a rendered appearance."));
        }
        write!(out,"<defs><clipPath id=\"clip-{id}\"><path d=\"{}\"/></clipPath></defs><g clip-path=\"url(#clip-{id})\">",path.to_svg()).unwrap();
    }
    match &n.kind {
        NodeKind::Group { .. } => {
            for child in doc.children(Some(id)) {
                node_svg(doc, child, out)?;
            }
        }
        NodeKind::Fill { rgba: color } => write!(
            out,
            "<rect width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"{}\"/>",
            doc.width,
            doc.height,
            rgba(*color),
            color[3] as f32 / 255.
        )
        .unwrap(),
        NodeKind::Path { path, style, .. } => {
            if style.fill_paint != PathPaint::Solid
                || style.stroke_paint != PathPaint::Solid
                || style.alignment != StrokeAlignment::Center
            {
                return Err(error(
                    "This shape paint or stroke requires a rendered appearance.",
                ));
            }
            write!(out,"<path d=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\" stroke-miterlimit=\"{}\"",path.to_svg(),style.fill.map(rgba).unwrap_or_else(||"none".into()),style.stroke.map(rgba).unwrap_or_else(||"none".into()),style.width,
                match style.cap{emulsion_raster::vector::StrokeCap::Butt=>"butt",emulsion_raster::vector::StrokeCap::Round=>"round",emulsion_raster::vector::StrokeCap::Square=>"square"},
                match style.join{emulsion_raster::vector::StrokeJoin::Miter=>"miter",emulsion_raster::vector::StrokeJoin::Round=>"round",emulsion_raster::vector::StrokeJoin::Bevel=>"bevel"},style.miter_limit).unwrap();
            if let Some(c) = style.fill {
                write!(out, " fill-opacity=\"{}\"", c[3] as f32 / 255.).unwrap();
            }
            if let Some(c) = style.stroke {
                write!(out, " stroke-opacity=\"{}\"", c[3] as f32 / 255.).unwrap();
            }
            if style.dash_count > 0 {
                write!(
                    out,
                    " stroke-dasharray=\"{}\" stroke-dashoffset=\"{}\"",
                    style.dash[..style.dash_count as usize]
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                    style.dash_offset
                )
                .unwrap();
            }
            out.push_str("/>");
        }
        NodeKind::Text { spec, .. } => {
            let paths = emulsion_core::text::vector_paths(spec).ok_or_else(|| {
                error("Advanced text effects or color glyphs require a rendered appearance.")
            })?;
            for (path, color) in paths {
                write!(
                    out,
                    "<path d=\"{}\" fill=\"{}\" fill-opacity=\"{}\"/>",
                    path.to_svg(),
                    rgba(color),
                    color[3] as f32 / 255.
                )
                .unwrap();
            }
        }
        NodeKind::Raster { raster, placement } => out.push_str(&image(
            raster,
            &matrix(placement.to_doc(raster.width(), raster.height())),
        )?),
        _ => {
            return Err(error(
                "Adjustment layers or smart filters require a rendered appearance.",
            ));
        }
    }
    if n.clip_to.is_some() {
        out.push_str("</g>");
    }
    out.push_str("</g>");
    Ok(())
}

/// Glyphs are outlined using the editor's shaping and bundled fonts, so SVG
/// readers need no matching font installation. Unsupported effects keep pixels.
pub fn svg(doc: &Document) -> Result<(Vec<u8>, bool)> {
    doc.validate().map_err(|e| error(e.to_string()))?;
    let mut content = String::new();
    let mut fallback = false;
    for id in doc.children(None) {
        if node_svg(doc, id, &mut content).is_err() {
            content = image(&flatten(&doc.composite_tree(), 0), "matrix(1 0 0 1 0 0)")?;
            fallback = true;
            break;
        }
    }
    Ok((format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">{content}</svg>",doc.width,doc.height,doc.width,doc.height).into_bytes(),fallback))
}

fn with_bleed(doc: &Document, bleed_mm: f64) -> Result<(Document, u32)> {
    let mut doc = crate::export::develop_document(doc)?;
    let bleed = (bleed_mm * doc.resolution as f64 / 25.4).round() as u32;
    if bleed > 0 {
        let (w, h) = (doc.width + bleed * 2, doc.height + bleed * 2);
        crate::import::check_size(w, h)?;
        emulsion_core::geometry::crop(
            &mut doc,
            IRect::new(-(bleed as i32), -(bleed as i32), w as i32, h as i32),
            0.,
        );
    }
    Ok((doc, bleed))
}

/// Images/SVG are a single ZIP, PDF is one multi-page file. The atomic writer
/// leaves an existing destination intact if any page fails to export.
pub fn write(
    project: &Project,
    selected: &[PageId],
    format: Format,
    include_bleed: bool,
    path: &Path,
) -> Result<Report> {
    project.validate().map_err(error)?;
    let ids: HashSet<_> = selected.iter().copied().collect();
    if ids.is_empty()
        || ids.len() != selected.len()
        || ids
            .iter()
            .any(|id| !project.pages.iter().any(|p| p.meta.id == *id))
    {
        return Err(error("Select existing pages to export."));
    }
    for page in &project.pages {
        crate::ora::ensure_not_raw_original(&page.doc, path)?;
        for commit in page.graph.commits() {
            crate::ora::ensure_not_raw_original(&commit.doc, path)?;
        }
    }
    let mut report = Report::default();
    crate::write_atomic(path, |file| {
        use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
        let mut pdf = Pdf::new();
        let mut alloc = Ref::new(1);
        let catalog = alloc.bump();
        let tree = alloc.bump();
        let mut pdf_pages = Vec::new();
        let (mut zip, mut pdf_sink) = if format == Format::Pdf {
            (None, Some(file))
        } else {
            (
                Some(zip::ZipWriter::new(std::io::BufWriter::new(file))),
                None,
            )
        };
        let mut total = 0usize;
        for (index, page) in project
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| ids.contains(&p.meta.id))
        {
            let (doc, bleed) = with_bleed(
                &page.doc,
                if include_bleed {
                    page.meta.bleed_mm
                } else {
                    0.
                },
            )?;
            let bytes = if matches!(format, Format::Svg | Format::Pdf) {
                let (bytes, fallback) = svg(&doc)?;
                if fallback {
                    report.rasterized_pages.push(page.meta.name.clone());
                }
                bytes
            } else {
                let raster = flatten(&doc.composite_tree(), 0);
                if format == Format::Png {
                    crate::export::png16(raster.width(), raster.height(), &raster.to_srgba16())?
                } else {
                    let rgba = raster.to_srgba8();
                    let rgb = rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| {
                            [0, 1, 2].map(|i| {
                                ((p[i] as u32 * p[3] as u32 + 255 * (255 - p[3] as u32) + 127)
                                    / 255) as u8
                            })
                        })
                        .collect::<Vec<_>>();
                    let mut bytes = Vec::new();
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 92).encode(
                        &rgb,
                        doc.width,
                        doc.height,
                        image::ExtendedColorType::Rgb8,
                    )?;
                    bytes
                }
            };
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| error("Export size overflow"))?;
            if total > 2 << 30 {
                return Err(error(
                    "Export exceeds the 2 GiB budget. Export fewer pages at a time.",
                ));
            }
            if format == Format::Pdf {
                let svg = svg2pdf::usvg::Tree::from_data(&bytes, &Default::default())
                    .map_err(|e| error(e.to_string()))?;
                let (chunk, root) = svg2pdf::to_chunk(&svg, Default::default())
                    .map_err(|e| error(e.to_string()))?;
                let mut map = HashMap::new();
                let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(|| alloc.bump()));
                let root = map[&root];
                let page_id = alloc.bump();
                let content_id = alloc.bump();
                pdf_pages.push(page_id);
                let scale = 72. / doc.resolution;
                let w = doc.width as f32 * scale;
                let h = doc.height as f32 * scale;
                let b = bleed as f32 * scale;
                let mut output = pdf.page(page_id);
                output
                    .parent(tree)
                    .media_box(Rect::new(0., 0., w, h))
                    .bleed_box(Rect::new(0., 0., w, h))
                    .trim_box(Rect::new(b, b, w - b, h - b))
                    .contents(content_id);
                output.resources().x_objects().pair(Name(b"Artwork"), root);
                output.finish();
                let mut content = Content::new();
                content
                    .transform([w, 0., 0., h, 0., 0.])
                    .x_object(Name(b"Artwork"));
                pdf.stream(content_id, &content.finish());
                pdf.extend(&chunk);
            } else {
                let zip = zip.as_mut().unwrap();
                zip.start_file(
                    format!(
                        "page-{:03}-{}.{}",
                        index + 1,
                        page.meta.id,
                        format.extension()
                    ),
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )?;
                zip.write_all(&bytes)?;
            }
            report.pages += 1;
        }
        if format == Format::Pdf {
            pdf.catalog(catalog).pages(tree);
            pdf.pages(tree)
                .kids(pdf_pages.iter().copied())
                .count(pdf_pages.len() as i32);
            let writer = pdf_sink.as_mut().unwrap();
            writer.write_all(&pdf.finish())?;
            writer.flush()?;
        } else {
            zip.unwrap().finish()?.flush()?;
        }
        Ok(())
    })?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{
        creation::{CanvasKind, CanvasSpec},
        design::Template,
    };
    fn path(ext: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "emulsion-project-export-{}-{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            ext
        ))
    }
    #[test]
    fn all_supplied_starters_export_without_flattening_text_or_placeholder_frames() {
        for template in Template::catalog() {
            let (w, h) = template.native_size();
            let scale = 320. / w.max(h) as f64;
            let doc = template
                .create(
                    (w as f64 * scale).round() as u32,
                    (h as f64 * scale).round() as u32,
                )
                .unwrap();
            let (bytes, flat) = svg(&doc).unwrap();
            assert!(!flat, "{} must retain vector artwork", template.label());
            let source = String::from_utf8(bytes).unwrap();
            assert!(
                !source.contains("<image"),
                "{} contains a bitmap",
                template.label()
            );
            resvg::usvg::Tree::from_str(&source, &Default::default()).unwrap();
        }
    }
    #[test]
    fn native_templates_export_outlined_svg_and_multi_page_pdf_with_bleed() {
        let mut session = CanvasSpec {
            kind: CanvasKind::Design,
            width: 320.,
            height: 240.,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        let page = session
            .add_page(
                Template::Announcement.create(320, 240).unwrap(),
                "Announcement".into(),
                3.,
            )
            .unwrap();
        let (svg, flat) = svg(&session.doc).unwrap();
        assert!(!flat, "template text and paths must remain vectors");
        let svg = String::from_utf8(svg).unwrap();
        assert!(svg.contains("<path"));
        assert!(!svg.contains("<image"));
        let parsed = resvg::usvg::Tree::from_str(&svg, &Default::default()).unwrap();
        assert_eq!(parsed.size().width(), 320.);
        let project = session.snapshot().unwrap();
        let file = path("pdf");
        let report = write(&project, &[1, page], Format::Pdf, true, &file).unwrap();
        assert_eq!(report.pages, 2);
        assert!(report.rasterized_pages.is_empty());
        let bytes = std::fs::read(&file).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/TrimBox"));
        assert!(text.contains("/BleedBox"));
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn selected_pages_zip_respects_order_bleed_and_failed_export_keeps_destination() {
        let session = CanvasSpec {
            kind: CanvasKind::Design,
            width: 32.,
            height: 24.,
            pages: 3,
            bleed_mm: 2.54,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        let project = session.snapshot().unwrap();
        let file = path("zip");
        write(&project, &[3, 1], Format::Png, true, &file).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
        assert_eq!(zip.len(), 2);
        assert_eq!(zip.by_index(0).unwrap().name(), "page-001-1.png");
        let bytes = crate::ora::read_entry(&mut zip, "page-003-3.png", 1 << 20).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (46, 38));
        drop(zip);
        let before = std::fs::read(&file).unwrap();
        assert!(write(&project, &[999], Format::Png, false, &file).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn unsupported_effects_report_raster_appearance_instead_of_dropping_them() {
        let mut doc = Template::Quote.create(160, 120).unwrap();
        emulsion_core::Command::AddNode {
            node: Box::new(emulsion_core::Node::new(
                0,
                "Effect",
                NodeKind::Adjust(emulsion_raster::Adjustment::Exposure {
                    exposure: 0.5,
                    offset: 0.,
                    gamma: 1.,
                }),
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let (bytes, flat) = svg(&doc).unwrap();
        assert!(flat);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("data:image/png;base64,")
        );
    }
}
