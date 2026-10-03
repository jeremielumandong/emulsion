//! PDF's older usvg needs an explicit resolver to retain images inside SVG images.
//!
//! Keep each embedded SVG as a separate tree: flattening XML would change its
//! viewport, filters and ID namespace. The first pass uses our shared fonts and
//! outlines text, including text inside embedded SVGs, before the PDF-only parser.
use crate::{IoError, Result};
use std::{
    borrow::Cow,
    io::{Cursor, Read},
    sync::{Arc, Mutex},
};

const MAX_DEPTH: usize = 16;
const MAX_DECODED_BYTES: usize = 256 << 20;

fn error(message: impl Into<String>) -> IoError {
    IoError::Unsupported(format!("PDF SVG: {}", message.into()))
}

struct Budget {
    max_depth: usize,
    remaining: Mutex<usize>,
    failure: Mutex<Option<String>>,
}

impl Budget {
    fn new(max_depth: usize, bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            max_depth,
            remaining: Mutex::new(bytes),
            failure: Mutex::new(None),
        })
    }

    fn reject(&self, message: impl Into<String>) {
        self.failure.lock().unwrap().get_or_insert(message.into());
    }

    fn check(&self) -> Result<()> {
        match self.failure.lock().unwrap().as_ref() {
            Some(message) => Err(error(message)),
            None => Ok(()),
        }
    }

    fn charge(&self, bytes: usize) -> Result<()> {
        self.check()?;
        let mut remaining = self.remaining.lock().unwrap();
        *remaining = remaining
            .checked_sub(bytes)
            .ok_or_else(|| error("decoded image byte budget exceeded"))?;
        Ok(())
    }

    fn capture<T>(&self, result: Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(err) => {
                self.reject(err.to_string());
                None
            }
        }
    }
}

/// Parse artwork for PDF export without fetching any external image resources.
/// Limits apply independently to the source and outlined passes, counting the
/// top-level SVG, every decoded data image, and decompressed SVGZ bytes.
pub(crate) fn parse(bytes: &[u8]) -> Result<svg2pdf::usvg::Tree> {
    parse_with_limits(bytes, MAX_DEPTH, MAX_DECODED_BYTES)
}

fn parse_with_limits(
    bytes: &[u8],
    max_depth: usize,
    max_bytes: usize,
) -> Result<svg2pdf::usvg::Tree> {
    let budget = Budget::new(max_depth, max_bytes);
    let source = normalize(bytes, &budget, 0)?;
    budget.check()?;
    let outlined = source.to_string(&resvg::usvg::WriteOptions::default());
    let budget = Budget::new(max_depth, max_bytes);
    let tree = pdf_tree(outlined.as_bytes(), &budget, 0)?;
    budget.check()?;
    Ok(tree)
}

fn svg_bytes<'a>(bytes: &'a [u8], budget: &Budget, depth: usize) -> Result<Cow<'a, [u8]>> {
    if depth > budget.max_depth {
        return Err(error("embedded SVG nesting limit exceeded"));
    }
    budget.charge(bytes.len())?;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        // usvg's built-in SVGZ decoder has no output limit. Never pass it gzip.
        let remaining = *budget.remaining.lock().unwrap();
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(remaining.saturating_add(1) as u64)
            .read_to_end(&mut decoded)
            .map_err(|_| error("invalid compressed SVG"))?;
        budget.charge(decoded.len())?;
        Cow::Owned(decoded)
    } else {
        Cow::Borrowed(bytes)
    };
    validate_image_hrefs(&bytes, budget)?;
    Ok(bytes)
}

/// usvg silently omits malformed data URLs before calling its resolver. Check
/// them first, and reject external references even in hidden/unused definitions.
fn validate_image_hrefs(bytes: &[u8], budget: &Budget) -> Result<()> {
    use quick_xml::{Reader, events::Event};
    let text = std::str::from_utf8(bytes).map_err(|_| error("SVG is not UTF-8"))?;
    let mut reader = Reader::from_str(text);
    let mut version = quick_xml::XmlVersion::Implicit1_0;
    loop {
        match reader.read_event().map_err(|e| error(e.to_string()))? {
            Event::Start(e) | Event::Empty(e)
                if matches!(e.local_name().as_ref(), "image" | "feImage") =>
            {
                for attribute in e.attributes() {
                    let attribute = attribute.map_err(|e| error(e.to_string()))?;
                    if attribute.key.local_name().as_ref() != "href" {
                        continue;
                    }
                    let href = attribute
                        .normalized_value(version)
                        .map_err(|e| error(e.to_string()))?;
                    if e.local_name().as_ref() == "feImage" && href.starts_with('#') {
                        continue;
                    }
                    validate_data_url(&href, budget)?;
                }
            }
            Event::Decl(decl) => {
                version = decl.xml_version().map_err(|e| error(e.to_string()))?;
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn validate_data_url(href: &str, budget: &Budget) -> Result<()> {
    let href = href.trim_matches(|c: char| c <= ' ');
    let (header, body) = href
        .split_once(',')
        .filter(|(header, _)| {
            header
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
        })
        .ok_or_else(|| error("external or invalid image reference is not allowed"))?;
    let base64 = header
        .rsplit_once(';')
        .is_some_and(|(_, encoding)| encoding.trim().eq_ignore_ascii_case("base64"));
    let body = body.split('#').next().unwrap_or_default();
    let mut bytes = body
        .bytes()
        .filter(|b| !matches!(b, b'\t' | b'\r' | b'\n'))
        .peekable();
    let mut count = 0usize;
    let mut padding = 0usize;
    while let Some(mut byte) = bytes.next() {
        if byte == b'%' {
            let mut next = bytes.clone();
            let digits = next.next().zip(next.next()).and_then(|(a, b)| {
                Some((char::from(a).to_digit(16)? * 16 + char::from(b).to_digit(16)?) as u8)
            });
            if let Some(decoded) = digits {
                byte = decoded;
                bytes = next;
            }
        }
        if base64 {
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c') {
                continue;
            }
            if byte == b'=' {
                padding += 1;
            } else if padding != 0 || !(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
            {
                return Err(error("invalid base64 image data"));
            }
        }
        count += 1;
    }
    if base64 && (count % 4 == 1 || padding > 2 || (padding > 0 && !count.is_multiple_of(4))) {
        return Err(error("invalid base64 image data"));
    }
    let decoded = if base64 {
        count / 4 * 3 + count % 4 * 3 / 4 - padding
    } else {
        count
    };
    if decoded > *budget.remaining.lock().unwrap() {
        return Err(error("decoded image byte budget exceeded"));
    }
    Ok(())
}

fn is_svg(mime: &str, bytes: &[u8]) -> bool {
    mime == "image/svg+xml" || (mime == "text/plain" && image::guess_format(bytes).is_err())
}

// Validate the header without allocating another full-resolution pixel buffer.
// Preserve the original encoded bytes; corrupt pixel streams remain a downstream
// raster/PDF decoding error, rather than a guarantee of this resource resolver.
fn validate_raster(mime: &str, bytes: &[u8], budget: &Budget) -> Result<()> {
    budget.charge(bytes.len())?;
    let format = image::guess_format(bytes).map_err(|_| error("invalid embedded raster image"))?;
    let supported = match format {
        image::ImageFormat::Png => mime == "image/png" || mime == "text/plain",
        image::ImageFormat::Jpeg => matches!(mime, "image/jpg" | "image/jpeg" | "text/plain"),
        image::ImageFormat::Gif => mime == "image/gif" || mime == "text/plain",
        image::ImageFormat::WebP => mime == "image/webp" || mime == "text/plain",
        _ => false,
    };
    if !supported {
        return Err(error("unsupported embedded raster image type"));
    }
    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| error("invalid embedded raster image dimensions"))?;
    if width == 0 || height == 0 {
        return Err(error("empty embedded raster image"));
    }
    Ok(())
}

fn normalize(bytes: &[u8], budget: &Arc<Budget>, depth: usize) -> Result<resvg::usvg::Tree> {
    use resvg::usvg;
    let bytes = svg_bytes(bytes, budget, depth)?;
    let mut options = crate::svg_vectors::options();
    let data_budget = budget.clone();
    let string_budget = budget.clone();
    let default_data = usvg::ImageHrefResolver::default_data_resolver();
    options.image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: Box::new(move |mime, data, opts| {
            let result = if is_svg(mime, &data) {
                normalize(&data, &data_budget, depth + 1).map(usvg::ImageKind::SVG)
            } else {
                validate_raster(mime, &data, &data_budget).and_then(|()| {
                    default_data(mime, data, opts)
                        .ok_or_else(|| error("could not resolve embedded image"))
                })
            };
            data_budget.capture(result)
        }),
        resolve_string: Box::new(move |_, _| {
            string_budget.reject("external image references are not allowed");
            None
        }),
    };
    let tree = usvg::Tree::from_data(&bytes, &options).map_err(|e| error(e.to_string()))?;
    budget.check()?;
    Ok(tree)
}

fn pdf_tree(bytes: &[u8], budget: &Arc<Budget>, depth: usize) -> Result<svg2pdf::usvg::Tree> {
    use svg2pdf::usvg;
    let bytes = svg_bytes(bytes, budget, depth)?;
    let data_budget = budget.clone();
    let string_budget = budget.clone();
    let default_data = usvg::ImageHrefResolver::default_data_resolver();
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, opts| {
                let result = if is_svg(mime, &data) {
                    // The default 0.45 loader discards *all* nested images.
                    pdf_tree(&data, &data_budget, depth + 1).map(usvg::ImageKind::SVG)
                } else {
                    validate_raster(mime, &data, &data_budget).and_then(|()| {
                        default_data(mime, data, opts)
                            .ok_or_else(|| error("could not resolve embedded image"))
                    })
                };
                data_budget.capture(result)
            }),
            resolve_string: Box::new(move |_, _| {
                string_budget.reject("external image references are not allowed");
                None
            }),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(&bytes, &options).map_err(|e| error(e.to_string()))?;
    budget.check()?;
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use svg2pdf::usvg::{Group, ImageKind, Node};

    fn svg(content: &str) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="120" height="80">{content}</svg>"#
        )
    }

    fn embed(mime: &str, data: &[u8]) -> String {
        format!(
            r#"<image x="10" y="12" width="90" height="60" xlink:href="data:{mime};base64,{}"/>"#,
            STANDARD.encode(data)
        )
    }

    fn png() -> Vec<u8> {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(3, 2).unwrap();
        // Opaque, distinct colors catch accidental conversion, resizing or loss.
        pixmap.data_mut().copy_from_slice(&[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 0, 255, 255, 255,
            255, 0, 255, 255,
        ]);
        pixmap.encode_png().unwrap()
    }

    fn visit(group: &Group, action: &mut impl FnMut(&Node)) {
        for node in group.children() {
            action(node);
            match node {
                Node::Group(group) => visit(group, action),
                Node::Image(image) => {
                    if let ImageKind::SVG(tree) = image.kind() {
                        visit(tree.root(), action);
                    }
                }
                _ => {}
            }
        }
    }

    fn pdf(tree: &svg2pdf::usvg::Tree) -> Vec<u8> {
        svg2pdf::to_pdf(tree, Default::default(), Default::default()).unwrap()
    }

    #[test]
    fn nested_png_keeps_original_bytes_resolution_and_pdf_image() {
        let original = png();
        let inner = svg(&embed("image/png", &original));
        let outer = svg(&embed("image/svg+xml", inner.as_bytes()));
        let tree = parse(outer.as_bytes()).unwrap();
        let mut images = 0;
        visit(tree.root(), &mut |node| {
            if let Node::Image(image) = node
                && let ImageKind::PNG(data) = image.kind()
            {
                assert_eq!(data.as_slice(), original);
                assert_eq!((image.size().width(), image.size().height()), (3., 2.));
                images += 1;
            }
        });
        assert_eq!(images, 1);
        let pdf = pdf(&tree);
        let text = String::from_utf8_lossy(&pdf);
        assert_eq!(text.matches("/Subtype /Image").count(), 1);
        assert!(text.contains("/Width 3"));
        assert!(text.contains("/Height 2"));
    }

    #[test]
    fn nested_image_viewports_preserve_translation_rotation_and_scale() {
        use svg2pdf::usvg::Transform;
        fn find_image(group: &Group, parent: Transform) -> Option<Transform> {
            let current = parent.pre_concat(group.transform());
            for node in group.children() {
                let found = match node {
                    Node::Group(group) => find_image(group, current),
                    Node::Image(image) => match image.kind() {
                        ImageKind::SVG(tree) => find_image(tree.root(), current),
                        ImageKind::PNG(_) => Some(current),
                        _ => None,
                    },
                    _ => None,
                };
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        let inner = svg(&embed("image/png", &png()));
        let outer = svg(&format!(
            r#"<g transform="translate(17 11) rotate(90) scale(2)">{}</g>"#,
            embed("image/svg+xml", inner.as_bytes())
        ));
        let tree = parse(outer.as_bytes()).unwrap();
        let actual = find_image(tree.root(), Transform::identity()).unwrap();
        // Outer viewport scales by .75; PNG viewport scales by 30. The
        // enclosing group rotates both translated viewports and scales by 2.
        let expected = Transform::from_row(0., 45., -45., 0., -25., 46.);
        for (a, b) in [
            actual.sx, actual.ky, actual.kx, actual.sy, actual.tx, actual.ty,
        ]
        .into_iter()
        .zip([
            expected.sx,
            expected.ky,
            expected.kx,
            expected.sy,
            expected.tx,
            expected.ty,
        ]) {
            assert!(
                (a - b).abs() < 0.001,
                "transform {actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn embedded_svg_filters_and_separate_id_namespaces_survive() {
        let inner = |color: &str| {
            svg(&format!(
                r##"<defs><filter id="effect"><feGaussianBlur stdDeviation="2"/></filter></defs><rect id="shape" x="20" y="20" width="30" height="30" fill="{color}" filter="url(#effect)"/>"##
            ))
        };
        let outer = svg(&format!(
            "{}{}",
            embed("image/svg+xml", inner("red").as_bytes()),
            embed("image/svg+xml", inner("blue").as_bytes())
        ));
        let tree = parse(outer.as_bytes()).unwrap();
        let mut filters = 0;
        let mut colors = Vec::new();
        visit(tree.root(), &mut |node| {
            if let Node::Group(group) = node {
                filters += group.filters().len();
            }
            if let Node::Path(path) = node
                && let Some(fill) = path.fill()
                && let svg2pdf::usvg::Paint::Color(color) = fill.paint()
            {
                colors.push((color.red, color.green, color.blue));
            }
        });
        assert_eq!(filters, 2);
        assert_eq!(colors, [(255, 0, 0), (0, 0, 255)]);
        let output = pdf(&tree);
        assert!(String::from_utf8_lossy(&output).contains("/Subtype /Image"));
    }

    #[test]
    fn embedded_text_is_outlined_with_shared_fonts() {
        let inner = svg(r#"<text x="10" y="40" font-family="Geist" font-size="24">Print</text>"#);
        let outer = svg(&embed("image/svg+xml", inner.as_bytes()));
        let tree = parse(outer.as_bytes()).unwrap();
        let mut paths = 0;
        visit(tree.root(), &mut |node| match node {
            Node::Path(_) => paths += 1,
            Node::Text(_) => panic!("PDF source still has unoutlined text"),
            _ => {}
        });
        assert!(paths > 0, "nested text disappeared");
        assert!(!String::from_utf8_lossy(&pdf(&tree)).contains("/Subtype /Image"));
    }

    #[test]
    fn nested_external_files_and_urls_fail_instead_of_being_omitted() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), png()).unwrap();
        for href in [
            file.path().to_str().unwrap(),
            "file:///not-allowed.png",
            "https://example.invalid/not-allowed.png",
            "relative-image.png",
        ] {
            let inner = svg(&format!(r#"<image width="20" height="20" href="{href}"/>"#));
            let outer = svg(&embed("image/svg+xml", inner.as_bytes()));
            let err = parse(outer.as_bytes()).unwrap_err().to_string();
            assert!(err.contains("external"), "{err}");
        }
    }

    #[test]
    fn malformed_and_unsupported_images_fail_closed() {
        for content in [
            r#"<image width="10" height="10" href="data:image/png;base64,%%%"/>"#.into(),
            embed("image/png", b"not a PNG"),
            embed("image/svg+xml", b"not an SVG"),
            embed("application/octet-stream", &png()),
        ] {
            let inner = svg(&content);
            let outer = svg(&embed("image/svg+xml", inner.as_bytes()));
            assert!(parse(outer.as_bytes()).is_err());
        }
    }

    #[test]
    fn nested_depth_and_cumulative_byte_limits_fail_export() {
        let leaf = svg(&embed("image/png", &png()));
        let middle = svg(&embed("image/svg+xml", leaf.as_bytes()));
        let outer = svg(&embed("image/svg+xml", middle.as_bytes()));
        let err = parse_with_limits(outer.as_bytes(), 1, MAX_DECODED_BYTES)
            .unwrap_err()
            .to_string();
        assert!(err.contains("nesting"), "{err}");
        let err = parse_with_limits(outer.as_bytes(), MAX_DEPTH, outer.len() + middle.len() - 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("byte budget"), "{err}");
        assert!(parse_with_limits(outer.as_bytes(), MAX_DEPTH, outer.len() - 1).is_err());
    }

    #[test]
    fn compressed_svg_expansion_is_bounded() {
        use std::io::Write;
        let xml = svg(&format!("<!--{}-->", "x".repeat(8192)));
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), Default::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let gzip = encoder.finish().unwrap();
        assert!(gzip.len() < 1024);
        let err = parse_with_limits(&gzip, MAX_DEPTH, 1024)
            .unwrap_err()
            .to_string();
        assert!(err.contains("byte budget"), "{err}");
    }

    #[test]
    fn both_passes_latch_resolver_failure() {
        let inner = svg(&embed("image/png", b"not a PNG"));
        let outer = svg(&embed("image/svg+xml", inner.as_bytes()));
        assert!(
            normalize(
                outer.as_bytes(),
                &Budget::new(MAX_DEPTH, MAX_DECODED_BYTES),
                0
            )
            .is_err()
        );
        assert!(
            pdf_tree(
                outer.as_bytes(),
                &Budget::new(MAX_DEPTH, MAX_DECODED_BYTES),
                0
            )
            .is_err()
        );
    }
}
