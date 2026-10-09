//! Flattened export.

#[path = "export_workflow.rs"]
mod workflow;
pub use workflow::{
    ExportColorSpace, ExportScale, ExportWorkflow, export_with_workflow,
    export_with_workflow_report,
};

use crate::{IoError, Result, write_atomic};
use emulsion_core::Document;
use emulsion_raster::composite::flatten;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

fn tag_srgb(encoder: &mut impl ImageEncoder) -> Result<()> {
    let profile = crate::icc::srgb_profile()
        .ok_or_else(|| IoError::Unsupported("could not encode the sRGB output profile".into()))?;
    encoder
        .set_icc_profile(profile)
        .map_err(image::ImageError::Unsupported)?;
    Ok(())
}

/// Develop the linked original using the persisted recipe before compositing.
/// A missing/changed source is an explicit export error; reopening still uses
/// the saved full-resolution pixels so the user can inspect and relink it.
pub fn develop_document(doc: &Document) -> Result<Document> {
    doc.validate()?;
    let mut rendered = doc.clone();
    let Some(raw) = &doc.raw else {
        return Ok(rendered);
    };
    raw.validate().map_err(|e| IoError::Manifest(e.into()))?;
    doc.node(raw.node_id)
        .ok_or_else(|| IoError::Manifest("RAW source layer is missing".into()))?
        .require_affine_capability("RAW export source replacement")?;
    let source = crate::photo_develop::PhotoSource::load_verified(&raw.source, &raw.source_sha256)?;
    let raster = Arc::new(source.develop_with(&raw.params)?);
    let node = rendered
        .node_mut(raw.node_id)
        .ok_or_else(|| IoError::Manifest("RAW source layer is missing".into()))?;
    replace_developed_source(node, raster)?;
    // Development can replace a proxy with a larger source/filter-cache grid.
    // Revalidate before any vector coverage is requested on that derived grid.
    rendered.validate()?;
    Ok(rendered)
}

/// Substitute RAW development pixels without changing any independent mask's
/// document mapping. Kept separate so the proxy/full-resolution boundary can
/// be tested without a camera decoder or an external original file.
fn replace_developed_source(
    node: &mut emulsion_core::Node,
    raster: Arc<emulsion_raster::Raster>,
) -> Result<()> {
    node.require_affine_capability("RAW export source replacement")?;
    let previous_size = match &node.kind {
        emulsion_core::NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
        emulsion_core::NodeKind::Smart { source, .. } => (source.width(), source.height()),
        _ => (0, 0),
    };
    let source_size_changed = previous_size != (raster.width(), raster.height());
    let mask_world = emulsion_core::transform::mask_to_document(node)?
        .require_affine("RAW export source replacement")?;
    let vector_world = emulsion_core::transform::vector_mask_to_document(node)?;
    let filter_world = emulsion_core::smart_filter_mask::to_document(node)?
        .map(|world| world.require_affine("RAW export source replacement"))
        .transpose()?;
    match &mut node.kind {
        emulsion_core::NodeKind::Raster { raster: pixels, .. } => *pixels = raster,
        emulsion_core::NodeKind::Smart {
            editable: None,
            source,
            original_image,
            filters,
            filter_styles,
            filters_enabled,
            cache,
            offset,
            ..
        } => {
            let (next, next_offset) = emulsion_core::smart::render_stack(
                &raster,
                filters,
                filter_styles,
                *filters_enabled,
            );
            *original_image = None;
            *source = raster;
            *cache = next;
            *offset = next_offset;
        }
        _ => {
            return Err(IoError::Manifest(
                "RAW source layer is not a pixel layer".into(),
            ));
        }
    }
    // Source coordinates may change when development replaces a proxy. Keep
    // independent mask world mappings; a same-size refresh retains descriptors
    // exactly rather than accumulating affine roundoff.
    if source_size_changed {
        let inverse = emulsion_core::transform::local_to_document(node)?
            .require_affine("RAW export source replacement")?
            .inverse();
        if node.mask.is_some() {
            node.mask_transform = emulsion_core::Mapping2::Affine(inverse * mask_world);
        }
        if let (Some(world), Some(mask)) = (vector_world, &mut node.vector_mask) {
            mask.transform = (inverse * world).to_cols_array();
        }
        if let (
            Some(world),
            emulsion_core::NodeKind::Smart {
                filter_mask: Some(mask),
                ..
            },
        ) = (filter_world, &mut node.kind)
        {
            mask.transform = emulsion_core::Mapping2::Affine(inverse * world);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Webp,
    Tiff,
    /// Layered PSD document.
    Psd,
    /// Layered XCF document (8-bit).
    Xcf,
    Bmp,
    Gif,
    Tga,
    /// PPM without alpha; PAM keeps it.
    Pnm,
    /// Windows icon, at most 256 px a side.
    Ico,
    /// Radiance HDR, float RGB without alpha.
    Hdr,
    /// OpenEXR, float RGBA.
    Exr,
    Qoi,
    Farbfeld,
    /// Written by a converter on this machine (AVIF, HEIC, JPEG XL, PDF…):
    /// see `external::encoders`.
    External(&'static str),
}

impl ExportFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "webp" => Self::Webp,
            "tif" | "tiff" => Self::Tiff,
            "psd" | "psb" => Self::Psd,
            "xcf" => Self::Xcf,
            "bmp" => Self::Bmp,
            "gif" => Self::Gif,
            "tga" => Self::Tga,
            "ppm" | "pnm" | "pam" => Self::Pnm,
            "ico" => Self::Ico,
            "hdr" => Self::Hdr,
            "exr" => Self::Exr,
            "qoi" => Self::Qoi,
            "ff" => Self::Farbfeld,
            e => Self::External(
                crate::external::EXPORT_EXTENSIONS
                    .iter()
                    .copied()
                    .find(|x| *x == e)?,
            ),
        })
    }

    pub fn supports_16bit(self) -> bool {
        matches!(self, Self::Png | Self::Tiff | Self::Exr | Self::Farbfeld)
    }

    /// Whether the format keeps transparency.
    pub fn keeps_alpha(self) -> bool {
        !matches!(self, Self::Jpeg | Self::Hdr | Self::External("pdf"))
    }

    /// Whether this machine can write the format right now.
    pub fn available(self) -> bool {
        match self {
            Self::External("pdf") => true,
            Self::External(ext) => crate::external::can_encode(ext),
            _ => true,
        }
    }

    /// Extensions that export, in-process first, then the converter-backed
    /// ones a tool on this machine handles.
    pub fn exportable_extensions() -> Vec<&'static str> {
        let mut v: Vec<&str> = vec![
            "png", "jpg", "webp", "tif", "psd", "xcf", "bmp", "gif", "tga", "ppm", "ico", "hdr",
            "exr", "qoi", "ff", "pdf",
        ];
        v.extend(
            crate::external::EXPORT_EXTENSIONS
                .iter()
                .copied()
                .filter(|e| *e != "pdf" && crate::external::can_encode(e)),
        );
        v
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    /// 8 or 16. Formats without 16-bit support write 8.
    pub depth: u8,
    /// 1–100, JPEG only.
    pub jpeg_quality: u8,
}

impl ExportOptions {
    pub fn for_doc(doc: &Document) -> Self {
        Self {
            depth: doc.source_depth,
            jpeg_quality: 92,
        }
    }
}

/// Encode a straight-alpha sRGBA8 buffer as PNG bytes.
pub fn png8(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder =
        PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive);
    tag_srgb(&mut encoder)?;
    encoder.write_image(rgba, w, h, ExtendedColorType::Rgba8)?;
    Ok(out)
}

/// Encode a straight-alpha sRGBA16 buffer as PNG bytes.
pub fn png16(w: u32, h: u32, rgba: &[u16]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    // Encoders take 16-bit samples as native-endian bytes.
    let bytes: Vec<u8> = rgba.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let mut encoder =
        PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive);
    tag_srgb(&mut encoder)?;
    encoder.write_image(&bytes, w, h, ExtendedColorType::Rgba16)?;
    Ok(out)
}

/// Encode an 8-bit grey buffer as PNG bytes.
pub fn png_gray(w: u32, h: u32, px: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive)
        .write_image(px, w, h, ExtendedColorType::L8)?;
    Ok(out)
}

/// Write the flattened document to `path`.
pub fn export(doc: &Document, path: &Path, opts: ExportOptions) -> Result<()> {
    export_with_exif(doc, path, opts, None)
}
pub fn export_with_exif(
    doc: &Document,
    path: &Path,
    opts: ExportOptions,
    exif: Option<&[u8]>,
) -> Result<()> {
    let format = ExportFormat::from_path(path)
        .ok_or_else(|| IoError::Unsupported(path.display().to_string()))?;
    crate::ora::ensure_not_raw_original(doc, path)?;
    let developed = if doc.raw.is_some() {
        Some(develop_document(doc)?)
    } else {
        None
    };
    let doc = developed.as_ref().unwrap_or(doc);
    doc.validate()?;
    if format == ExportFormat::Psd {
        return crate::psd::write(doc, path);
    }
    if format == ExportFormat::Xcf {
        return crate::xcf::write(doc, path);
    }
    if format == ExportFormat::External("pdf") {
        let project = emulsion_core::project::ProjectEditor::new_project(
            if doc.diagram.is_some() {
                emulsion_core::project::ProjectKind::Diagram
            } else {
                emulsion_core::project::ProjectKind::Design
            },
            doc.clone(),
        )
        .map_err(|e| IoError::Unsupported(e.to_string()))?
        .snapshot()
        .ok_or_else(|| IoError::Unsupported("Could not prepare diagram PDF".into()))?;
        return crate::project_export::write(
            &project,
            &[project.pages[0].meta.id],
            crate::project_export::Format::Pdf,
            false,
            path,
        )
        .map(|_| ());
    }
    let flat = flatten(&doc.try_composite_tree()?, 0);
    let (w, h) = (doc.width, doc.height);
    let wide = opts.depth == 16 && format.supports_16bit();
    if let ExportFormat::External(ext) = format {
        // A PNG at the source depth, handed to the converter.
        let png = if wide {
            png16(w, h, &flat.to_srgba16())?
        } else {
            png8(w, h, &flat.to_srgba8())?
        };
        return crate::external::encode(&png, ext, path, opts.jpeg_quality);
    }
    // Formats without alpha composite over white.
    let over_white = |rgba: &[u8]| -> Vec<u8> {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = p[3] as u32;
                [0, 1, 2].map(|i| ((p[i] as u32 * a + 255 * (255 - a) + 127) / 255) as u8)
            })
            .collect()
    };
    write_atomic(path, |f| {
        let mut out = BufWriter::new(f);
        match format {
            ExportFormat::Psd | ExportFormat::Xcf | ExportFormat::External(_) => {
                unreachable!("handled above")
            }
            ExportFormat::Bmp | ExportFormat::Gif | ExportFormat::Tga | ExportFormat::Qoi => {
                let img = image::RgbaImage::from_raw(w, h, flat.to_srgba8())
                    .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let fmt = match format {
                    ExportFormat::Bmp => image::ImageFormat::Bmp,
                    ExportFormat::Gif => image::ImageFormat::Gif,
                    ExportFormat::Tga => image::ImageFormat::Tga,
                    _ => image::ImageFormat::Qoi,
                };
                let mut buf = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(img).write_to(&mut buf, fmt)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Pnm => {
                // PAM keeps alpha, but few readers open it; PPM travels.
                let rgb = over_white(&flat.to_srgba8());
                let img = image::RgbImage::from_raw(w, h, rgb)
                    .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let mut buf = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgb8(img).write_to(&mut buf, image::ImageFormat::Pnm)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Ico => {
                let img = image::RgbaImage::from_raw(w, h, flat.to_srgba8())
                    .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let mut img = image::DynamicImage::ImageRgba8(img);
                if w > 256 || h > 256 {
                    img = img.resize(256, 256, image::imageops::FilterType::Lanczos3);
                }
                let mut buf = std::io::Cursor::new(Vec::new());
                img.write_to(&mut buf, image::ImageFormat::Ico)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Farbfeld => {
                let img =
                    image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(w, h, flat.to_srgba16())
                        .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let mut buf = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba16(img)
                    .write_to(&mut buf, image::ImageFormat::Farbfeld)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Exr | ExportFormat::Hdr => {
                // Float formats take the linear values straight from the raster.
                let px = flat.to_srgba16();
                let lin = |v: u16| emulsion_raster::color::srgb_to_linear(v as f32 / 65535.0);
                let mut buf = std::io::Cursor::new(Vec::new());
                if format == ExportFormat::Exr {
                    let data: Vec<f32> = px
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| [lin(p[0]), lin(p[1]), lin(p[2]), p[3] as f32 / 65535.0])
                        .collect();
                    let img = image::ImageBuffer::<image::Rgba<f32>, _>::from_raw(w, h, data)
                        .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                    image::DynamicImage::ImageRgba32F(img)
                        .write_to(&mut buf, image::ImageFormat::OpenExr)?;
                } else {
                    let data: Vec<f32> = px
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| {
                            let a = p[3] as f32 / 65535.0;
                            [0, 1, 2].map(|i| lin(p[i]) * a + (1.0 - a))
                        })
                        .collect();
                    let img = image::ImageBuffer::<image::Rgb<f32>, _>::from_raw(w, h, data)
                        .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                    image::DynamicImage::ImageRgb32F(img)
                        .write_to(&mut buf, image::ImageFormat::Hdr)?;
                }
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Png => {
                let mut encoder = PngEncoder::new_with_quality(
                    &mut out,
                    CompressionType::Fast,
                    FilterType::Adaptive,
                );
                tag_srgb(&mut encoder)?;
                if let Some(exif) = exif {
                    encoder
                        .set_exif_metadata(exif.to_vec())
                        .map_err(image::ImageError::Unsupported)?;
                }
                if wide {
                    let bytes: Vec<u8> = flat
                        .to_srgba16()
                        .iter()
                        .flat_map(|v| v.to_ne_bytes())
                        .collect();
                    encoder.write_image(&bytes, w, h, ExtendedColorType::Rgba16)?;
                } else {
                    encoder.write_image(&flat.to_srgba8(), w, h, ExtendedColorType::Rgba8)?;
                }
            }
            ExportFormat::Jpeg => {
                // JPEG has no alpha: composite over white.
                let rgb = over_white(&flat.to_srgba8());
                let mut encoder =
                    JpegEncoder::new_with_quality(&mut out, opts.jpeg_quality.clamp(1, 100));
                tag_srgb(&mut encoder)?;
                if let Some(exif) = exif {
                    encoder
                        .set_exif_metadata(exif.to_vec())
                        .map_err(image::ImageError::Unsupported)?;
                }
                encoder.write_image(&rgb, w, h, ExtendedColorType::Rgb8)?;
            }
            ExportFormat::Webp => {
                let mut encoder = WebPEncoder::new_lossless(&mut out);
                tag_srgb(&mut encoder)?;
                if let Some(exif) = exif {
                    encoder
                        .set_exif_metadata(exif.to_vec())
                        .map_err(image::ImageError::Unsupported)?;
                }
                encoder.write_image(&flat.to_srgba8(), w, h, ExtendedColorType::Rgba8)?;
            }
            ExportFormat::Tiff => {
                let mut buf = std::io::Cursor::new(Vec::new());
                let mut encoder = TiffEncoder::new(&mut buf);
                tag_srgb(&mut encoder)?;
                if wide {
                    let px = flat.to_srgba16();
                    let bytes: Vec<u8> = px.iter().flat_map(|v| v.to_ne_bytes()).collect();
                    encoder.write_image(&bytes, w, h, ExtendedColorType::Rgba16)?;
                } else {
                    encoder.write_image(&flat.to_srgba8(), w, h, ExtendedColorType::Rgba8)?;
                }
                let bytes = if let Some(exif) = exif {
                    crate::photo_metadata::tiff(buf.into_inner(), exif)?
                } else {
                    buf.into_inner()
                };
                out.write_all(&bytes)?;
            }
        }
        out.flush()?;
        Ok(())
    })
}

#[cfg(test)]
mod raw_export_tests {
    use super::*;
    use emulsion_core::{
        Node,
        raw::{DevelopParams, RawDocument, RawMetadata},
    };
    use emulsion_raster::Raster;
    use image::ImageDecoder;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "emulsion-raw-export-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn photo() -> Document {
        let mut doc = Document::new(16, 12);
        doc.source_depth = 16;
        doc.nodes.push(Node::raster(
            1,
            "Photo",
            Arc::new(Raster::solid(16, 12, [0.25, 0.4, 0.6, 1.0])),
            Default::default(),
        ));
        doc.next_id = 2;
        doc
    }

    #[test]
    fn raw_source_substitution_preserves_filter_and_layer_mask_world_mappings() {
        use emulsion_core::{MaskProperties, NodeKind, SmartFilterMask, VectorMask};
        use emulsion_raster::{Mask, Placement};
        for enabled in [false, true] {
            for linked in [false, true] {
                for dimensions in [(8, 6), (20, 9)] {
                    let mut node = Node::smart(
                        1,
                        "RAW Smart proxy",
                        Arc::new(Raster::solid(8, 6, [0.2, 0.4, 0.6, 0.75])),
                        vec![emulsion_filters::Filter::GaussianBlur { radius: 1.0 }],
                        Placement {
                            x: 3.25,
                            y: -1.5,
                            scale_x: 1.5,
                            scale_y: 0.75,
                            rotation: 37.0,
                            flip_x: true,
                            ..Default::default()
                        },
                    );
                    let pixels = Arc::new(Mask::from_fn(7, 5, 127, |x, y| (x * 17 + y * 23) as u8));
                    let transform = [1.0, 0.25, -0.1, 0.9, -2.0, 3.0];
                    node.mask = Some(pixels.clone());
                    node.mask_transform = emulsion_core::Mapping2::Affine(
                        glam::DAffine2::from_cols_array(&transform),
                    );
                    node.mask_enabled = enabled;
                    node.mask_linked = linked;
                    node.vector_mask = Some(VectorMask {
                        enabled,
                        linked,
                        transform,
                        ..Default::default()
                    });
                    let NodeKind::Smart { filter_mask, .. } = &mut node.kind else {
                        unreachable!()
                    };
                    *filter_mask = Some(SmartFilterMask {
                        pixels: pixels.clone(),
                        enabled,
                        linked,
                        transform: emulsion_core::Mapping2::Affine(
                            glam::DAffine2::from_cols_array(&transform),
                        ),
                        properties: MaskProperties {
                            density: 0.75,
                            feather: 2.0,
                        },
                    });
                    let before = node.clone();
                    let worlds = |node: &Node| {
                        [
                            emulsion_core::transform::mask_to_document(node)
                                .unwrap()
                                .affine()
                                .unwrap(),
                            emulsion_core::transform::vector_mask_to_document(node)
                                .unwrap()
                                .unwrap(),
                            emulsion_core::smart_filter_mask::to_document(node)
                                .unwrap()
                                .unwrap()
                                .affine()
                                .unwrap(),
                        ]
                    };
                    replace_developed_source(
                        &mut node,
                        Arc::new(Raster::transparent(dimensions.0, dimensions.1)),
                    )
                    .unwrap();
                    for (old, new) in worlds(&before).into_iter().zip(worlds(&node)) {
                        assert!(
                            old.to_cols_array()
                                .into_iter()
                                .zip(new.to_cols_array())
                                .all(|(a, b)| (a - b).abs() < 1e-8)
                        );
                    }
                    let mask = emulsion_core::smart_filter_mask::descriptor(&node).unwrap();
                    assert!(Arc::ptr_eq(&mask.pixels, &pixels));
                    assert!(Arc::ptr_eq(node.mask.as_ref().unwrap(), &pixels));
                    assert_eq!(mask.enabled, enabled);
                    assert_eq!(mask.linked, linked);
                    assert_eq!(
                        mask.properties,
                        emulsion_core::smart_filter_mask::descriptor(&before)
                            .unwrap()
                            .properties
                    );
                    if dimensions == (8, 6) {
                        assert_eq!(mask.transform.affine().unwrap().to_cols_array(), transform);
                        assert_eq!(
                            node.mask_transform.affine().unwrap().to_cols_array(),
                            transform
                        );
                        assert_eq!(node.vector_mask.as_ref().unwrap().transform, transform);
                    }
                }
            }
        }
    }

    #[test]
    fn photograph_exports_have_srgb_profile_and_requested_depth() {
        let scratch = Scratch::new();
        let doc = photo();
        for ext in ["png", "tif", "jpg", "webp"] {
            let path = scratch.0.join(format!("photo.{ext}"));
            export(&doc, &path, ExportOptions::for_doc(&doc)).unwrap();
            // image 0.25.10's ImageReader sets TIFF's tag allocation limit
            // to the pixel-buffer size. For this tiny image, expanding an
            // ICC tag into tiff 0.11's Value array exceeds that limit, and
            // icc_profile silently returns None. The direct TIFF decoder
            // retains its default metadata budget and checks the actual tag.
            let mut decoder: Box<dyn ImageDecoder> = if ext == "tif" {
                Box::new(
                    image::codecs::tiff::TiffDecoder::new(std::io::BufReader::new(
                        std::fs::File::open(&path).unwrap(),
                    ))
                    .unwrap(),
                )
            } else {
                Box::new(
                    image::ImageReader::open(&path)
                        .unwrap()
                        .with_guessed_format()
                        .unwrap()
                        .into_decoder()
                        .unwrap(),
                )
            };
            assert_eq!(decoder.dimensions(), (16, 12));
            let icc = decoder
                .icc_profile()
                .unwrap()
                .unwrap_or_else(|| panic!("{ext}: missing output ICC profile"));
            assert!(moxcms::ColorProfile::new_from_slice(&icc).is_ok());
            if ["png", "tif"].contains(&ext) {
                assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
            }
        }
    }

    #[test]
    fn missing_or_changed_original_does_not_export_stale_proxy_or_replace_output() {
        let scratch = Scratch::new();
        let mut doc = photo();
        let source = scratch.0.join("original.dng");
        doc.raw = Some(RawDocument {
            schema_version: 1,
            node_id: 1,
            source: source.clone(),
            source_sha256: "0".repeat(64),
            params: DevelopParams::default(),
            metadata: RawMetadata::default(),
        });
        let destination = scratch.0.join("existing.png");
        std::fs::write(&destination, b"keep existing output").unwrap();
        assert!(export(&doc, &destination, ExportOptions::for_doc(&doc)).is_err());
        std::fs::write(&source, b"different original").unwrap();
        let err = export(&doc, &destination, ExportOptions::for_doc(&doc)).unwrap_err();
        assert!(err.to_string().contains("SHA-256"));
        assert_eq!(std::fs::read(destination).unwrap(), b"keep existing output");
    }

    #[test]
    fn raw_substitution_rejects_latent_projective_metadata_without_mutation() {
        let mut node = Node::smart(
            1,
            "RAW proxy with dormant mapping",
            Arc::new(Raster::solid(4, 4, [1., 0., 0., 1.])),
            vec![],
            Default::default(),
        );
        node.mask_transform =
            emulsion_core::Mapping2::Projective(emulsion_raster::projective::Projective2::IDENTITY);
        let before = node.clone();
        assert!(matches!(
            replace_developed_source(&mut node, Arc::new(Raster::transparent(8, 8))),
            Err(IoError::Geometry(_))
        ));
        assert_eq!(node, before);
    }
}
