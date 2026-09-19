//! Thumbnails for the Home screen.

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
        match crate::ora::read_entry(&mut z, "mergedimage.png", 1 << 30).and_then(|bytes| {
            decode(ImageReader::with_format(
                Cursor::new(bytes),
                image::ImageFormat::Png,
            ))
        }) {
            Ok(image) => Ok(image),
            Err(error) => embedded.or(Err(error)),
        }
    } else if crate::raw::is_raw(path) {
        // Develop small: RAW files carry no cheap preview we read yet.
        let (r, _) = crate::raw::develop(path)?;
        Ok(DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(r.width(), r.height(), r.to_srgba8())
                .ok_or_else(|| crate::IoError::Unsupported("RAW thumbnail".into()))?,
        ))
    } else {
        let reader = image::ImageReader::open(path)?.with_guessed_format()?;
        decode(reader)
    }
}

/// Straight-alpha sRGBA8 thumbnail no larger than `max` on either side.
pub fn thumbnail(path: &Path, max: u32) -> Result<(u32, u32, Vec<u8>)> {
    import::check_size(max, max)?;
    let img = source(path, max, max)?;
    let t = img.thumbnail(max, max).into_rgba8();
    Ok((t.width(), t.height(), t.into_raw()))
}

/// A centred gallery crop sized for the card's device pixels. Crop before
/// resizing so tall/wide images do not stretch an undersized fitted thumbnail.
/// Small sources keep their native resolution; no detail is invented.
pub fn thumbnail_cover(path: &Path, width: u32, height: u32) -> Result<(u32, u32, Vec<u8>)> {
    import::check_size(width, height)?;
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
}
