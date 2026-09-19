//! Thumbnails for the Home screen.

use crate::{Result, import};
use std::io::Read;
use std::path::Path;

/// Straight-alpha sRGBA8 thumbnail no larger than `max` on either side.
pub fn thumbnail(path: &Path, max: u32) -> Result<(u32, u32, Vec<u8>)> {
    let img = if crate::is_native(path) {
        let mut z = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path)?))?;
        let mut bytes = Vec::new();
        z.by_name("Thumbnails/thumbnail.png")?
            .take(16 << 20)
            .read_to_end(&mut bytes)?;
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?
    } else if crate::raw::is_raw(path) {
        // Develop small: RAW files carry no cheap preview we read yet.
        let (r, _) = crate::raw::develop(path)?;
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(r.width(), r.height(), r.to_srgba8())
                .ok_or_else(|| crate::IoError::Unsupported("RAW thumbnail".into()))?,
        )
    } else {
        let reader = image::ImageReader::open(path)?.with_guessed_format()?;
        let img = reader.decode()?;
        import::check_size(img.width(), img.height())?;
        img
    };
    let t = img.thumbnail(max, max).into_rgba8();
    Ok((t.width(), t.height(), t.into_raw()))
}
