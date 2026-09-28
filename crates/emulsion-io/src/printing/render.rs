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
        let (svg, rasterized) = crate::project_export::svg(&doc)?;
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

fn sheet_svg(sources: &[Source], sheet: &Sheet) -> Result<String> {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>",
        sheet.width, sheet.height, sheet.width, sheet.height
    );
    for (i, item) in sheet.items.iter().enumerate() {
        let s = sources.get(item.source).context("Missing print source")?;
        let b = item.bounds;
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
            b.w / s.width as f64,
            b.h / s.height as f64
        )?;
        // Each source is parsed independently before composition to namespace IDs.
        // Using an SVG image isolates local clip paths and effect IDs between pages.
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&s.svg);
        write!(
            svg,
            "<image width=\"{}\" height=\"{}\" href=\"data:image/svg+xml;base64,{encoded}\"/></g></g>",
            s.width, s.height
        )?;
    }
    svg.push_str("</svg>");
    Ok(svg)
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
                let tree = if grayscale {
                    let image = preview(
                        sources,
                        sheet,
                        true,
                        (sheet.width.max(sheet.height) / 25.4 * 300.).ceil() as u32,
                    )?;
                    let png = crate::export::png8(image.width(), image.height(), image.as_raw())?;
                    use base64::Engine as _;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
                    svg2pdf::usvg::Tree::from_str(
                        &format!(
                            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><image width=\"100%\" height=\"100%\" href=\"data:image/png;base64,{encoded}\"/></svg>",
                            sheet.width, sheet.height
                        ),
                        &Default::default(),
                    )?
                } else {
                    svg2pdf::usvg::Tree::from_str(&sheet_svg(sources, sheet)?, &Default::default())?
                };
                let (chunk, root) = svg2pdf::to_chunk(&tree, Default::default())
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
}
