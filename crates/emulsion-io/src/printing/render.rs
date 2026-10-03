use super::*;
use std::{collections::HashMap, fmt::Write as _, io::Write as _};

#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub ppi: f64,
    pub svg: String,
    pub rasterized: bool,
    /// Immutable edited document retained for real off-page bleed artwork.
    pub document: Option<std::sync::Arc<emulsion_core::Document>>,
    pub original_paths: Vec<std::path::PathBuf>,
}

impl Source {
    pub fn physical_size(&self) -> Result<(f64, f64)> {
        if self.width == 0 || self.height == 0 || !self.ppi.is_finite() || self.ppi <= 0. {
            bail!("Invalid source dimensions or resolution")
        }
        Ok((
            self.width as f64 / self.ppi * 25.4,
            self.height as f64 / self.ppi * 25.4,
        ))
    }
}

pub fn prepare_sources(
    docs: Vec<(String, emulsion_core::Document)>,
    cancel: &AtomicBool,
) -> Result<Vec<Source>> {
    let mut result = Vec::new();
    let mut bytes = 0usize;
    for (name, doc) in docs {
        canceled(cancel)?;
        let doc = crate::export::develop_document(&doc)?;
        let (svg, rasterized) = crate::project_export::pdf_svg(&doc)?;
        bytes = bytes.saturating_add(svg.len());
        if bytes > 512 * 1024 * 1024 {
            bail!("Print sources exceed 512 MiB; select fewer pages")
        }
        result.push(Source {
            name,
            width: doc.width,
            height: doc.height,
            ppi: doc.resolution as f64,
            svg: String::from_utf8(svg)?,
            original_paths: doc
                .raw_originals
                .iter()
                .chain(doc.raw.iter().map(|raw| &raw.source))
                .cloned()
                .collect(),
            document: Some(std::sync::Arc::new(doc.clone())),
            rasterized: rasterized
                || doc.nodes.iter().any(|n| {
                    matches!(
                        n.kind,
                        emulsion_core::NodeKind::Raster { .. }
                            | emulsion_core::NodeKind::Smart { .. }
                    )
                }),
        });
    }
    Ok(result)
}

pub(super) fn sheet_svg(sources: &[Source], sheet: &Sheet) -> Result<String> {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>",
        sheet.width, sheet.height, sheet.width, sheet.height
    );
    // Repeat grids reuse the expanded/encoded source instead of exporting it per cell.
    let mut artwork_cache = HashMap::new();
    for (i, item) in sheet.items.iter().enumerate() {
        let s = sources.get(item.source).context("Missing print source")?;
        let mut b = item.bounds;
        let scale = b.w / s.width as f64;
        let bleed_pixels = item.bleed / scale;
        let key = (item.source, bleed_pixels.to_bits());
        if let std::collections::hash_map::Entry::Vacant(entry) = artwork_cache.entry(key) {
            let (artwork, source_width, source_height, pixels) = if item.bleed > 0.
                && let Some(doc) = &s.document
            {
                // Translate, never enlarge, the original artwork. The expanded
                // canvas reveals authored off-page objects; empty bleed stays clear.
                let source_mm = bleed_pixels / s.ppi * 25.4;
                let (expanded, pixels) = crate::project_export::with_bleed(doc, source_mm)?;
                (
                    crate::project_export::pdf_svg(&expanded)?.0,
                    expanded.width,
                    expanded.height,
                    pixels,
                )
            } else {
                (s.svg.as_bytes().to_vec(), s.width, s.height, 0)
            };
            use base64::Engine as _;
            entry.insert((
                base64::engine::general_purpose::STANDARD.encode(artwork),
                source_width,
                source_height,
                pixels,
            ));
        }
        let (encoded, source_width, source_height, pixels) = &artwork_cache[&key];
        let pad = *pixels as f64 * scale;
        b = Rect {
            x: b.x - pad,
            y: b.y - pad,
            w: b.w + 2. * pad,
            h: b.h + 2. * pad,
        };
        let c = item.clip;
        write!(
            svg,
            "<defs><clipPath id=\"sheet_clip_{i}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs><g clip-path=\"url(#sheet_clip_{i})\"><g transform=\"translate({} {}) scale({} {})\">",
            c.x,
            c.y,
            c.w,
            c.h,
            b.x,
            b.y,
            b.w / *source_width as f64,
            b.h / *source_height as f64
        )?;
        // Each source is parsed independently before composition to namespace IDs.
        // Using an SVG image isolates local clip paths and effect IDs between pages.
        write!(
            svg,
            "<image width=\"{}\" height=\"{}\" href=\"data:image/svg+xml;base64,{encoded}\"/></g></g>",
            source_width, source_height
        )?;
    }
    for item in &sheet.items {
        if !item.crop_marks {
            continue;
        }
        let t = item.trim;
        let gap = item.bleed + 2.;
        // 5 mm marks, with a 2 mm gap outside bleed, entirely inside the cell.
        for x in [t.x, t.x + t.w] {
            for (y1, y2) in [
                (t.y - gap - 5., t.y - gap),
                (t.y + t.h + gap, t.y + t.h + gap + 5.),
            ] {
                write!(
                    svg,
                    "<path d=\"M{x} {y1}V{y2}\" fill=\"none\" stroke=\"black\" stroke-width=\"0.2\"/>"
                )?;
            }
        }
        for y in [t.y, t.y + t.h] {
            for (x1, x2) in [
                (t.x - gap - 5., t.x - gap),
                (t.x + t.w + gap, t.x + t.w + gap + 5.),
            ] {
                write!(
                    svg,
                    "<path d=\"M{x1} {y}H{x2}\" fill=\"none\" stroke=\"black\" stroke-width=\"0.2\"/>"
                )?;
            }
        }
    }
    for (i, item) in sheet.items.iter().enumerate() {
        if let Some(label) = &item.label {
            let text = label
                .text
                .chars()
                .filter(|c| !c.is_control())
                .take(300)
                .collect::<String>();
            let spec = emulsion_core::text::TextSpec {
                text,
                x: 0.,
                y: 4.,
                size: 28.,
                height: Some(50.),
                ..Default::default()
            };
            text_svg(&mut svg, &spec, label.bounds, &format!("label_{i}"))?;
        }
    }
    for (i, mark) in sheet.marks.iter().enumerate() {
        match mark {
            Mark::Text { spec, bounds } => text_svg(&mut svg, spec, *bounds, &format!("mark_{i}"))?,
            Mark::Frame {
                bounds: b,
                stroke_mm,
                color: [r, g, bl],
            } => write!(
                svg,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#{r:02x}{g:02x}{bl:02x}\" stroke-width=\"{stroke_mm}\"/>",
                b.x, b.y, b.w, b.h
            )?,
            Mark::Path {
                points,
                closed,
                filled,
                stroke_mm,
                color: [r, g, bl],
            } => {
                let Some(((x, y), rest)) = points.split_first() else {
                    continue;
                };
                let mut d = format!("M{x} {y}");
                for (x, y) in rest {
                    write!(d, "L{x} {y}")?;
                }
                if *closed || *filled {
                    d.push('Z');
                }
                let ink = format!("#{r:02x}{g:02x}{bl:02x}");
                let fill = if *filled { ink.as_str() } else { "none" };
                write!(
                    svg,
                    "<path d=\"{d}\" fill=\"{fill}\" stroke=\"{ink}\" stroke-width=\"{stroke_mm}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>"
                )?
            }
            Mark::Image {
                data,
                mime,
                bounds: b,
            } => {
                use base64::Engine as _;
                write!(
                    svg,
                    "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:{mime};base64,{}\"/>",
                    b.x,
                    b.y,
                    b.w,
                    b.h,
                    base64::engine::general_purpose::STANDARD.encode(data.as_slice())
                )?
            }
        }
    }
    svg.push_str("</svg>");
    Ok(svg)
}

/// Native outlined text, so sheets need no external fonts in the PDF. `spec`
/// sizes are in tenths of a millimetre; text wraps to the bounds' width and is
/// clipped to its height.
fn text_svg(
    svg: &mut String,
    spec: &emulsion_core::text::TextSpec,
    bounds: Rect,
    id: &str,
) -> Result<()> {
    let w = (bounds.w * 10.).floor().max(1.) as u32;
    let h = (bounds.h * 10.).floor().max(1.) as u32;
    let mut doc = emulsion_core::Document::new(w, h);
    let mut spec = spec.clone();
    spec.width.get_or_insert(w as f32);
    spec.height.get_or_insert((h as f32 - spec.y).max(1.));
    emulsion_core::Command::AddNode {
        node: Box::new(emulsion_core::Node::text(0, "Print text", spec, w, h)),
        slot: emulsion_core::command::Slot::TOP,
    }
    .apply(&mut doc)?;
    use base64::Engine as _;
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(crate::project_export::pdf_svg(&doc)?.0);
    write!(
        svg,
        "<svg x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {w} {h}\" overflow=\"hidden\"><image id=\"{id}\" width=\"{w}\" height=\"{h}\" href=\"data:image/svg+xml;base64,{encoded}\"/></svg>",
        bounds.x, bounds.y, bounds.w, bounds.h
    )?;
    Ok(())
}

fn tree(sources: &[Source], sheet: &Sheet) -> Result<resvg::usvg::Tree> {
    Ok(resvg::usvg::Tree::from_str(
        &sheet_svg(sources, sheet)?,
        &Default::default(),
    )?)
}

pub fn preview(
    sources: &[Source],
    sheet: &Sheet,
    grayscale: bool,
    max_side: u32,
) -> Result<image::RgbaImage> {
    if !(1..=12000).contains(&max_side) {
        bail!("Invalid print render dimensions")
    }
    let scale = max_side as f64 / sheet.width.max(sheet.height);
    let (w, h) = (
        (sheet.width * scale).round().max(1.) as u32,
        (sheet.height * scale).round().max(1.) as u32,
    );
    if u64::from(w) * u64::from(h) > 80_000_000 {
        bail!("Print sheet exceeds the 80 megapixel render budget")
    }
    let mut pix = resvg::tiny_skia::Pixmap::new(w, h).context("Cannot allocate print preview")?;
    resvg::render(
        &tree(sources, sheet)?,
        resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32),
        &mut pix.as_mut(),
    );
    let mut pixels = pix.take();
    if grayscale {
        for p in pixels.as_chunks_mut::<4>().0 {
            let gray =
                ((p[0] as u32 * 2126 + p[1] as u32 * 7152 + p[2] as u32 * 722) / 10000) as u8;
            p[..3].fill(gray);
        }
    }
    image::RgbaImage::from_raw(w, h, pixels).context("Invalid preview buffer")
}

pub fn write_pdf(
    sources: &[Source],
    layout: &JobLayout,
    grayscale: bool,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
    if layout.sheets.is_empty() {
        bail!("No sheets to print")
    }
    let destination = std::fs::canonicalize(path).ok();
    for original in sources.iter().flat_map(|source| &source.original_paths) {
        if original == path
            || destination
                .as_ref()
                .is_some_and(|p| std::fs::canonicalize(original).ok().as_ref() == Some(p))
        {
            bail!("Printing to PDF cannot overwrite an original RAW. Choose another file.")
        }
    }
    crate::write_atomic(path, |file| {
        let run = || -> Result<Vec<u8>> {
            let mut pdf = Pdf::new();
            let mut alloc = Ref::new(1);
            let catalog = alloc.bump();
            let pages = alloc.bump();
            let mut ids = Vec::new();
            for sheet in &layout.sheets {
                canceled(cancel)?;
                let svg = if grayscale {
                    let image = preview(
                        sources,
                        sheet,
                        true,
                        (sheet.width.max(sheet.height) / 25.4 * 300.).ceil() as u32,
                    )?;
                    let png = crate::export::png8(image.width(), image.height(), image.as_raw())?;
                    use base64::Engine as _;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
                    format!(
                        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><image width=\"100%\" height=\"100%\" href=\"data:image/png;base64,{encoded}\"/></svg>",
                        sheet.width, sheet.height
                    )
                } else {
                    sheet_svg(sources, sheet)?
                };
                let tree = crate::pdf_svg::parse(svg.as_bytes())?;
                // Sheet SVG units are millimeters. The converter's preflight
                // also accounts for each nested source's placement transform.
                let (options, _) = crate::pdf_effects::options(&tree, 25.4)?;
                let (chunk, root) = svg2pdf::to_chunk(&tree, options)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let mut map = HashMap::new();
                let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(|| alloc.bump()));
                let root = map[&root];
                let page = alloc.bump();
                let content = alloc.bump();
                ids.push(page);
                let w = (sheet.width / 25.4 * 72.) as f32;
                let h = (sheet.height / 25.4 * 72.) as f32;
                let mut out = pdf.page(page);
                out.parent(pages)
                    .media_box(Rect::new(0., 0., w, h))
                    .contents(content);
                if let [item] = sheet.items.as_slice()
                    && (item.bleed > 0. || item.crop_marks)
                {
                    let pdf_rect = |r: super::Rect| {
                        let k = (72. / 25.4) as f32;
                        Rect::new(
                            r.x as f32 * k,
                            (sheet.height - r.y - r.h) as f32 * k,
                            (r.x + r.w) as f32 * k,
                            (sheet.height - r.y) as f32 * k,
                        )
                    };
                    out.trim_box(pdf_rect(item.trim))
                        .bleed_box(pdf_rect(item.clip));
                }
                out.resources().x_objects().pair(Name(b"Sheet"), root);
                out.finish();
                let mut commands = Content::new();
                commands
                    .transform([w, 0., 0., h, 0., 0.])
                    .x_object(Name(b"Sheet"));
                pdf.stream(content, &commands.finish());
                pdf.extend(&chunk);
                if pdf.len() > 512 * 1024 * 1024 {
                    bail!("Print PDF exceeds 512 MiB; print fewer pages")
                }
            }
            pdf.catalog(catalog).pages(pages);
            pdf.pages(pages)
                .kids(ids.iter().copied())
                .count(ids.len() as i32);
            canceled(cancel)?;
            Ok(pdf.finish())
        };
        let bytes = run().map_err(|e| crate::IoError::Manifest(e.to_string()))?;
        file.write_all(&bytes)?;
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(color: &str) -> Source {
        Source {
            name: "Square".into(),
            width: 100,
            height: 100,
            ppi: 25.4,
            rasterized: false,
            original_paths: vec![],
            document: None,
            svg: format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><defs><clipPath id=\"clip\"><rect width=\"100\" height=\"100\"/></clipPath></defs><rect width=\"100\" height=\"100\" fill=\"{color}\" clip-path=\"url(#clip)\"/></svg>"
            ),
        }
    }
    #[test]
    fn preview_composes_distinct_sources_and_grayscale() {
        let sources = [source("#ff0000"), source("#0000ff")];
        let settings = Settings {
            layout: Layout::Contact,
            ..Default::default()
        };
        let layout = super::super::layout(&sources, &[0, 1], &settings).unwrap();
        let image = preview(&sources, &layout.sheets[0], false, 594).unwrap();
        // 2 px/mm, first row's two centered squares have independent local IDs.
        assert_eq!(*image.get_pixel(100, 100), image::Rgba([255, 0, 0, 255]));
        assert_eq!(*image.get_pixel(300, 100), image::Rgba([0, 0, 255, 255]));
        let image = preview(&sources, &layout.sheets[0], true, 594).unwrap();
        let p = image.get_pixel(100, 100);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
    }
    #[test]
    fn canceled_pdf_keeps_existing_destination_and_pdf_uses_physical_points() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proof.pdf");
        let sources = [source("#ff0000")];
        let settings = Settings::default();
        let layout = super::super::layout(&sources, &[0], &settings).unwrap();
        std::fs::write(&path, b"existing").unwrap();
        assert!(write_pdf(&sources, &layout, false, &path, &AtomicBool::new(true)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        write_pdf(&sources, &layout, false, &path, &AtomicBool::new(false)).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("/MediaBox [0 0 595.2756 841.8898]"), "{text}");
    }
    #[test]
    fn document_pdf_media_boxes_keep_business_card_and_invitation_sizes() {
        let mut card = source("#ff0000");
        card.width = 1050;
        card.height = 600;
        card.ppi = 300.;
        let mut invite = source("#0000ff");
        invite.width = 1500;
        invite.height = 2100;
        invite.ppi = 300.;
        let sources = [card, invite];
        let settings = Settings {
            layout: Layout::Document,
            ..Default::default()
        };
        let job = super::super::layout(&sources, &[0, 1], &settings).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mixed.pdf");
        write_pdf(&sources, &job, false, &path, &AtomicBool::new(false)).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/MediaBox [0 0 252 144]"), "{pdf}");
        assert!(pdf.contains("/MediaBox [0 0 360 504]"), "{pdf}");
        assert!(pdf.contains("/Count 2"));
    }
    #[test]
    fn bleed_reveals_off_page_vectors_and_pdf_records_trim_and_bleed_boxes() {
        let mut doc = emulsion_core::Document::new(100, 100);
        doc.resolution = 25.4;
        emulsion_core::Command::AddNode {
            node: Box::new(emulsion_core::Node::path(
                0,
                "Beyond trim",
                std::sync::Arc::new(emulsion_raster::vector_geometry::rectangle(
                    -5., -5., 110., 110.,
                )),
                emulsion_raster::vector::PathStyle {
                    fill: Some([255, 0, 0, 255]),
                    stroke: None,
                    ..Default::default()
                },
                100,
                100,
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let cancel = AtomicBool::new(false);
        let sources = prepare_sources(vec![("Bleed proof".into(), doc)], &cancel).unwrap();
        let mut settings = Settings {
            layout: Layout::Document,
            ..Default::default()
        };
        settings.creative.bleed_mm = 3.;
        settings.creative.crop_marks = true;
        let job = super::super::layout(&sources, &[0], &settings).unwrap();
        let sheet = &job.sheets[0];
        let preview = preview(&sources, sheet, false, 1200).unwrap();
        // At 10 px/mm: trim starts at 10 mm, authored bleed at 7 mm.
        assert_eq!(*preview.get_pixel(80, 600), image::Rgba([255, 0, 0, 255]));
        assert_eq!(
            *preview.get_pixel(60, 600),
            image::Rgba([255, 255, 255, 255])
        );
        assert!(
            preview.get_pixel(100, 30)[0] < 20,
            "crop mark outside bleed"
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bleed.pdf");
        write_pdf(&sources, &job, false, &path, &cancel).unwrap();
        let bytes = std::fs::read(path).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        let pdf_box = |name: &str| -> Vec<f64> {
            pdf.split(&format!("/{name} ["))
                .nth(1)
                .unwrap()
                .split(']')
                .next()
                .unwrap()
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect()
        };
        for (actual, expected) in pdf_box("TrimBox").iter().zip([10., 10., 110., 110.]) {
            assert!((actual - expected / 25.4 * 72.).abs() < 0.0001);
        }
        for (actual, expected) in pdf_box("BleedBox").iter().zip([7., 7., 113., 113.]) {
            assert!((actual - expected / 25.4 * 72.).abs() < 0.0001);
        }
        assert!(pdf.contains("/BleedBox"));
        assert!(
            !pdf.contains("/Subtype /Image"),
            "native vectors must remain sharp"
        );
        assert_eq!(
            sources[0].document.as_ref().unwrap().width,
            100,
            "printing cannot resize the original snapshot"
        );
    }
    #[test]
    fn crop_position_changes_pixels_without_changing_the_artwork_box() {
        let mut source = source("red");
        source.width = 200;
        source.svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="100" height="100" fill="red"/><rect x="100" width="100" height="100" fill="blue"/></svg>"#.into();
        let sources = [source];
        let mut settings = Settings {
            placement: Placement::Fill,
            ..Default::default()
        };
        settings.creative.artwork_mm = Some([100., 100.]);
        for (x, color) in [(0., [255, 0, 0, 255]), (1., [0, 0, 255, 255])] {
            settings.creative.crop[0] = x;
            let job = super::super::layout(&sources, &[0], &settings).unwrap();
            let image = preview(&sources, &job.sheets[0], false, 594).unwrap();
            assert_eq!(image.get_pixel(210, 297).0, color);
        }
    }
}
