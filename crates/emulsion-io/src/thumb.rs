//! Thumbnails for the Home screen and lightweight batch browsing.

use crate::{Result, import};
use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType, metadata::Orientation};
use std::io::{BufRead, Cursor, Seek};
use std::path::Path;

fn decode<R: BufRead + Seek>(reader: ImageReader<R>) -> Result<DynamicImage> {
    let mut decoder = reader.into_decoder()?;
    let (w, h) = decoder.dimensions();
    import::check_size(w, h)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn source(path: &Path, width: u32, height: u32) -> Result<DynamicImage> {
    if crate::is_native(path) {
        let mut z = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path)?))?;
        let embedded = crate::ora::read_entry(&mut z, "Thumbnails/thumbnail.png", 16 << 20)
            .and_then(|bytes| {
                decode(ImageReader::with_format(
                    Cursor::new(bytes),
                    image::ImageFormat::Png,
                ))
            });
        if let Ok(image) = &embedded
            && image.width() >= width
            && image.height() >= height
        {
            return embedded;
        }
        // The standard ORA thumbnail is only 256px. Larger gallery cards need
        // the saved full composite, not an enlargement of that small preview.
        if let Ok(image) =
            crate::ora::read_entry(&mut z, "mergedimage.png", 1 << 30).and_then(|bytes| {
                decode(ImageReader::with_format(
                    Cursor::new(bytes),
                    image::ImageFormat::Png,
                ))
            })
        {
            return Ok(image);
        }
        // Compact saves leave the merged image out: composite the layers
        // themselves, at the mip level that still covers the card.
        drop(z);
        match composite_document(path, width, height) {
            Ok(image) => Ok(image),
            Err(error) => embedded.or(Err(error)),
        }
    } else if crate::raw_probe::is_raw(path)? {
        let original = path.canonicalize()?;
        let sidecar = crate::raw_settings::sidecar_path(&original)?;
        match std::fs::symlink_metadata(&sidecar) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {
                // Embedded JPEGs describe the camera's original rendering, not
                // the saved recipe. Opening validates and develops that recipe.
                return composite(&crate::raw::open(&original)?, width, height);
            }
        }
        if let Ok(Some(preview)) = crate::raw_probe::embedded_preview(path)
            && preview.width() >= width
            && preview.height() >= height
        {
            return Ok(preview);
        }
        let (r, _) = crate::raw::develop(path)?;
        Ok(DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(r.width(), r.height(), r.to_srgba8())
                .ok_or_else(|| crate::IoError::Unsupported("RAW thumbnail".into()))?,
        ))
    } else {
        let reader = image::ImageReader::open(path)?.with_guessed_format()?;
        match decode(reader) {
            Ok(image) => Ok(image),
            // Layered, vector, JPEG XL, and converter-backed formats are
            // handled by the application opener rather than `image` itself.
            // Batch uses the same openable-format predicate, so give every
            // accepted input a real preview path.
            Err(_) if crate::is_openable(path) => composite(&crate::open(path)?, width, height),
            Err(error) => Err(error),
        }
    }
}

/// Open a native document and flatten it at the coarsest mip level whose
/// long side still reaches `width`/`height`, so the card gets real detail
/// without rendering the whole picture at full size.
fn composite_document(path: &Path, width: u32, height: u32) -> Result<DynamicImage> {
    let doc = crate::ora::read(path)?;
    composite(&doc, width, height)
}

fn composite(doc: &emulsion_core::Document, width: u32, height: u32) -> Result<DynamicImage> {
    let long = doc.width.max(doc.height);
    let need = width.max(height).max(1);
    let mut level = 0;
    while level < 8 && (long >> (level + 1)) >= need {
        level += 1;
    }
    let r = emulsion_raster::composite::flatten(&doc.composite_tree(), level);
    image::RgbaImage::from_raw(r.width(), r.height(), r.to_srgba8())
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| crate::IoError::Unsupported("composite thumbnail".into()))
}

/// Where finished gallery crops are kept between launches, keyed by the
/// file's path, size and modification time, adjacent recipe content, and the
/// requested size. Invalid/unreadable sidecars disable cache lookup entirely.
fn cache_path(path: &Path, width: u32, height: u32) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    use std::io::Read;
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "thumb-v3-raw-sidecar".hash(&mut h);
    path.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .hash(&mut h);
    (width, height).hash(&mut h);
    let sidecar = crate::raw_settings::sidecar_path(&path.canonicalize().ok()?).ok()?;
    match std::fs::File::open(&sidecar) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(64 * 1024 + 1).read_to_end(&mut bytes).ok()?;
            if bytes.len() > 64 * 1024 {
                return None;
            }
            Some(bytes).hash(&mut h);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !matches!(std::fs::symlink_metadata(&sidecar), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return None;
            }
            None::<Vec<u8>>.hash(&mut h);
        }
        Err(_) => return None,
    }
    Some(
        crate::recent::data_dir()
            .join("thumbs")
            .join(format!("{:016x}.png", h.finish())),
    )
}

/// Straight-alpha sRGBA8 thumbnail no larger than `max` on either side.
pub fn thumbnail(path: &Path, max: u32) -> Result<(u32, u32, Vec<u8>)> {
    import::check_size(max, max)?;
    let img = source(path, max, max)?;
    let t = img.thumbnail(max, max).into_rgba8();
    Ok((t.width(), t.height(), t.into_raw()))
}

/// A lightweight browsing preview, independent of selection and saved edits.
/// RAW and native documents use only their embedded previews. Other inputs
/// must decode directly within the browsing limits; this never develops RAW,
/// composites a document, or launches an external converter.
pub fn batch_thumbnail(path: &Path, max: u32) -> Result<(u32, u32, Vec<u8>)> {
    import::check_size(max, max)?;
    let img = if crate::is_native(path) {
        let mut archive =
            zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path)?))?;
        let bytes = crate::ora::read_entry(&mut archive, "Thumbnails/thumbnail.png", 16 << 20)?;
        decode_batch(ImageReader::with_format(
            Cursor::new(bytes),
            image::ImageFormat::Png,
        ))?
    } else if crate::raw_probe::is_raw(path)? {
        crate::raw_probe::embedded_preview(path)?
            .ok_or_else(|| crate::IoError::Unsupported("no embedded RAW browsing preview".into()))?
    } else {
        if std::fs::metadata(path)?.len() > 128 << 20 {
            return Err(crate::IoError::Unsupported(
                "source exceeds browsing preview limits".into(),
            ));
        }
        decode_batch(ImageReader::open(path)?.with_guessed_format()?)?
    };
    let preview = if img.width() > max || img.height() > max {
        img.thumbnail(max, max)
    } else {
        img
    }
    .into_rgba8();
    Ok((preview.width(), preview.height(), preview.into_raw()))
}

fn decode_batch<R: BufRead + Seek>(mut reader: ImageReader<R>) -> Result<DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 << 20);
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    import::check_size(width, height)?;
    // Include the eventual RGBA8 conversion even for single-channel images.
    if decoder.total_bytes() > 128 << 20 || u64::from(width) * u64::from(height) > 32 << 20 {
        return Err(crate::IoError::Unsupported(
            "image exceeds browsing preview limits".into(),
        ));
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// A centred gallery crop sized for the card's device pixels. Crop before
/// resizing so tall/wide images do not stretch an undersized fitted thumbnail.
/// Small sources keep their native resolution; no detail is invented.
pub fn thumbnail_cover(path: &Path, width: u32, height: u32) -> Result<(u32, u32, Vec<u8>)> {
    import::check_size(width, height)?;
    let cache = cache_path(path, width, height);
    if let Some(c) = &cache
        && let Ok(bytes) = std::fs::read(c)
        && let Ok(img) = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
    {
        let t = img.into_rgba8();
        return Ok((t.width(), t.height(), t.into_raw()));
    }
    let (w, h, px) = thumbnail_cover_uncached(path, width, height)?;
    // A save racing development must not put the new rendering under the old
    // recipe's key (which could be reused when that recipe is restored later).
    let unchanged = cache == cache_path(path, width, height);
    if let Some(c) = cache
        && unchanged
        && let Some(dir) = c.parent()
        && std::fs::create_dir_all(dir).is_ok()
        && let Ok(bytes) = crate::export::png8(w, h, &px)
    {
        let _ = std::fs::write(c, bytes);
    }
    Ok((w, h, px))
}

fn thumbnail_cover_uncached(path: &Path, width: u32, height: u32) -> Result<(u32, u32, Vec<u8>)> {
    let img = source(path, width, height)?;
    let ratio = width as f64 / height as f64;
    let (cw, ch) = if img.width() as f64 / img.height() as f64 > ratio {
        (
            ((img.height() as f64 * ratio).round() as u32).clamp(1, img.width()),
            img.height(),
        )
    } else {
        (
            img.width(),
            ((img.width() as f64 / ratio).round() as u32).clamp(1, img.height()),
        )
    };
    let crop = img.crop_imm((img.width() - cw) / 2, (img.height() - ch) / 2, cw, ch);
    let preview = if cw >= width && ch >= height {
        crop.resize_exact(width, height, FilterType::Lanczos3)
    } else {
        crop
    }
    .into_rgba8();
    Ok((preview.width(), preview.height(), preview.into_raw()))
}

#[cfg(test)]
#[path = "../tests/common/raw_fixture.rs"]
mod raw_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(extension: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            Self(std::env::temp_dir().join(format!(
                "emulsion-gallery-{}-{}.{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                extension
            )))
        }
        fn archive(merged: bool) -> Self {
            let file = Self::new("ora");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&file.0).unwrap());
            for (name, w, h, color) in [
                ("Thumbnails/thumbnail.png", 256, 192, [255, 0, 0, 255]),
                ("mergedimage.png", 1200, 900, [0, 0, 255, 255]),
            ] {
                if name == "mergedimage.png" && !merged {
                    continue;
                }
                let image = image::RgbaImage::from_pixel(w, h, image::Rgba(color));
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(&crate::export::png8(w, h, image.as_raw()).unwrap())
                    .unwrap();
            }
            zip.finish().unwrap();
            file
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // Remove this fixture's gallery crops before deleting the source:
            // cache keys include the source's size and modification time.
            for (width, height) in [(800, 600), (128, 96)] {
                if let Some(path) = cache_path(&self.0, width, height) {
                    let _ = std::fs::remove_file(path);
                }
            }
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn gallery_uses_full_native_composite_for_retina_cards() {
        let file = Fixture::archive(true);
        let (w, h, pixels) = thumbnail_cover(&file.0, 800, 600).unwrap();
        assert_eq!((w, h), (800, 600));
        assert_eq!(
            &pixels[..4],
            &[0, 0, 255, 255],
            "larger card uses the full composite"
        );
        let (w, h, pixels) = thumbnail_cover(&file.0, 128, 96).unwrap();
        assert_eq!((w, h), (128, 96));
        assert_eq!(
            &pixels[..4],
            &[255, 0, 0, 255],
            "small cards may use embedded preview"
        );
        let (w, h, pixels) = thumbnail(&file.0, 800).unwrap();
        assert_eq!((w, h), (800, 600));
        assert_eq!(&pixels[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn gallery_falls_back_without_enlarging_low_resolution_sources() {
        let file = Fixture::archive(false);
        let (w, h, _) = thumbnail_cover(&file.0, 800, 600).unwrap();
        assert_eq!((w, h), (256, 192));
    }

    #[test]
    fn batch_uses_small_embedded_native_preview_without_compositing() {
        let file = Fixture::archive(true);
        let (width, height, pixels) = batch_thumbnail(&file.0, 800).unwrap();
        assert_eq!((width, height), (256, 192));
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn batch_does_not_develop_raw_without_an_embedded_preview() {
        let file = Fixture::new("dng");
        raw_fixture::write_dng(&file.0);
        assert!(matches!(
            batch_thumbnail(&file.0, 128),
            Err(crate::IoError::Unsupported(_))
        ));
    }

    #[test]
    fn batch_accepts_small_raw_preview_and_ignores_sidecar_edits() {
        let file = Fixture::new("dng");
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[90; 4 * 2 * 3], 4, 2, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut bytes = b"II*\0\x08\0\0\0\x03\0".to_vec();
        for (tag, value) in [(50706u16, 1u32), (513, 50), (514, jpeg.len() as u32)] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(4u16.to_le_bytes());
            bytes.extend(1u32.to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(jpeg);
        std::fs::write(&file.0, bytes).unwrap();
        let sidecar =
            Fixture(crate::raw_settings::sidecar_path(&file.0.canonicalize().unwrap()).unwrap());
        std::fs::write(&sidecar.0, b"invalid saved recipe").unwrap();
        let (width, height, pixels) = batch_thumbnail(&file.0, 128).unwrap();
        assert_eq!((width, height), (4, 2));
        assert_eq!(pixels.len(), 4 * 2 * 4);
    }

    #[test]
    fn batch_does_not_fall_back_to_application_opener() {
        let file = Fixture::new("svg");
        std::fs::write(
            &file.0,
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"/>"#,
        )
        .unwrap();
        assert!(batch_thumbnail(&file.0, 128).is_err());
        let malformed_raw = Fixture::new("nef");
        std::fs::write(&malformed_raw.0, b"invalid raw").unwrap();
        assert!(batch_thumbnail(&malformed_raw.0, 128).is_err());
    }

    #[test]
    fn batch_fits_rasters_and_rejects_excessive_dimensions() {
        let file = Fixture::new("png");
        let img = image::RgbaImage::from_pixel(80, 40, image::Rgba([0, 255, 0, 255]));
        std::fs::write(&file.0, crate::export::png8(80, 40, img.as_raw()).unwrap()).unwrap();
        let (width, height, pixels) = batch_thumbnail(&file.0, 20).unwrap();
        assert_eq!((width, height), (20, 10));
        assert_eq!(pixels.len(), 20 * 10 * 4);

        let img = image::RgbaImage::new(16385, 1);
        std::fs::write(
            &file.0,
            crate::export::png8(16385, 1, img.as_raw()).unwrap(),
        )
        .unwrap();
        assert!(batch_thumbnail(&file.0, 20).is_err());
    }

    #[test]
    fn gallery_crops_tall_sources_before_downsampling() {
        let file = Fixture::new("png");
        let image = image::RgbaImage::from_fn(900, 2400, |_, y| {
            image::Rgba(if (900..1500).contains(&y) {
                [0, 255, 0, 255]
            } else {
                [255, 0, 0, 255]
            })
        });
        std::fs::write(
            &file.0,
            crate::export::png8(900, 2400, image.as_raw()).unwrap(),
        )
        .unwrap();
        let (w, h, pixels) = thumbnail_cover(&file.0, 800, 600).unwrap();
        assert_eq!(
            (w, h),
            (800, 600),
            "use available crop detail, not a fitted 300px-wide source"
        );
        let center = ((h / 2 * w + w / 2) * 4) as usize;
        assert_eq!(&pixels[center..center + 4], &[0, 255, 0, 255]);
    }

    #[test]
    fn thumbnail_falls_back_to_the_application_opener_for_svg() {
        let file = Fixture::new("svg");
        std::fs::write(
            &file.0,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
                    <rect width="40" height="20" fill="#00ff00"/>
                </svg>"##,
        )
        .unwrap();

        let (width, height, pixels) = thumbnail(&file.0, 20).unwrap();
        assert_eq!((width, height), (20, 10));
        assert_eq!(pixels.len(), 20 * 10 * 4);
    }

    #[test]
    fn raw_gallery_tracks_sidecar_saves_external_changes_and_removal() {
        // Retain every historical cache key: later saves change which filename
        // cache_path returns. Clean exactly those files even if an assertion fails.
        struct Cleanup(Vec<PathBuf>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for path in &self.0 {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        let file = Fixture::new("dng");
        raw_fixture::write_dng(&file.0);
        let mut doc = crate::raw::open(&file.0).unwrap();
        let sidecar = crate::raw_settings::suggested_sidecar_path(&doc).unwrap();
        let mut cleanup = Cleanup(vec![sidecar.clone()]);
        cleanup.0.push(cache_path(&file.0, 36, 24).unwrap());
        let original = thumbnail_cover(&file.0, 36, 24).unwrap();
        let source_bytes = std::fs::read(&file.0).unwrap();
        let mut previous = original.clone();
        for exposure in [-1.0, -2.0] {
            doc.raw.as_mut().unwrap().params.exposure = exposure;
            crate::raw_settings::save_sidecar(&doc, &sidecar).unwrap();
            cleanup.0.push(cache_path(&file.0, 36, 24).unwrap());
            let edited = thumbnail_cover(&file.0, 36, 24).unwrap();
            assert_ne!(edited, original);
            assert_ne!(edited, previous);
            assert_eq!(thumbnail_cover(&file.0, 36, 24).unwrap(), edited);
            assert_eq!(thumbnail(&file.0, 36).unwrap(), edited);
            previous = edited;
        }
        // External corruption must not return an older, valid cached image.
        std::fs::write(&sidecar, b"invalid external recipe").unwrap();
        assert!(thumbnail_cover(&file.0, 36, 24).is_err());
        std::fs::remove_file(&sidecar).unwrap();
        assert_eq!(thumbnail_cover(&file.0, 36, 24).unwrap(), original);
        assert_eq!(std::fs::read(&file.0).unwrap(), source_bytes);
    }
}
