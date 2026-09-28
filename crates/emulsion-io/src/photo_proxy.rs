//! Verified, bounded offline edit proxies. These are never full-quality exports.
use crate::{IoError, Result};
use emulsion_core::raw::RawMetadata;
use emulsion_raster::Raster;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proxy {
    pub version: u32,
    pub source: PathBuf,
    pub source_sha256: String,
    pub pixel_sha256: String,
    pub width: u32,
    pub height: u32,
    pub metadata: RawMetadata,
}
fn bad(s: impl ToString) -> IoError {
    IoError::Manifest(s.to_string())
}
pub fn directory() -> PathBuf {
    crate::recent::data_dir().join("photo-proxies")
}
fn path(source: &Path) -> PathBuf {
    let digest = Sha256::digest(source.as_os_str().as_encoded_bytes());
    directory().join(format!(
        "{}.json",
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}
pub fn exists(source: &Path) -> bool {
    path(source).is_file()
}
pub fn create(source: &Path) -> Result<()> {
    let source = source.canonicalize()?;
    let photo = crate::photo_develop::PhotoSource::load(&source)?;
    let raster = photo.develop_preview(
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    // Fit proxies are bounded even for rendered originals.
    let image = image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8())
        .ok_or_else(|| bad("Invalid proxy pixels"))?;
    let image = image::DynamicImage::ImageRgba8(image)
        .thumbnail(1600, 1600)
        .into_rgba8();
    let (width, height) = image.dimensions();
    let bytes = crate::export::png8(width, height, image.as_raw())?;
    let pixel_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    std::fs::create_dir_all(directory())?;
    let pixels = directory().join(format!("{pixel_sha256}.png"));
    crate::write_atomic(&pixels, |f| {
        use std::io::Write;
        f.write_all(&bytes)?;
        Ok(())
    })?;
    if crate::raw::source_digest(&source)? != photo.source_sha256 {
        return Err(bad("Original changed while making proxy"));
    }
    let proxy = Proxy {
        version: 1,
        source: source.clone(),
        source_sha256: photo.source_sha256.clone(),
        pixel_sha256,
        width,
        height,
        metadata: photo.metadata,
    };
    crate::raw_settings::make_managed(&source, &proxy.source_sha256)?;
    crate::write_atomic(&path(&source), |f| {
        serde_json::to_writer(f, &proxy).map_err(bad)
    })
}
pub fn load(source: &Path) -> Result<(Proxy, Raster)> {
    let mut bytes = vec![];
    std::fs::File::open(path(source))?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(bad("Oversized proxy manifest"));
    }
    let proxy: Proxy = serde_json::from_slice(&bytes).map_err(bad)?;
    if proxy.version != 1
        || proxy.source != source
        || proxy.width == 0
        || proxy.height == 0
        || proxy.width > 1600
        || proxy.height > 1600
        || proxy.source_sha256.len() != 64
        || !proxy.source_sha256.bytes().all(|c| c.is_ascii_hexdigit())
        || proxy.pixel_sha256.len() != 64
        || !proxy.pixel_sha256.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(bad("Invalid proxy identity"));
    }
    let file = directory().join(format!("{}.png", proxy.pixel_sha256));
    if crate::raw::source_digest(&file)? != proxy.pixel_sha256 {
        return Err(bad("Offline proxy pixels changed"));
    }
    let (w, h) = image::ImageReader::open(&file)?
        .with_guessed_format()?
        .into_dimensions()?;
    if (w, h) != (proxy.width, proxy.height) {
        return Err(bad("Offline proxy dimensions changed"));
    }
    let image = image::open(file)?.into_rgba8();
    let raster = Raster::from_srgba8(w, h, image.as_raw());
    Ok((proxy, raster))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_edits_require_verified_original_for_export() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("photo.png");
        let bytes = crate::export::png8(2, 2, &[100, 80, 60, 255].repeat(4)).unwrap();
        std::fs::write(&original, &bytes).unwrap();
        create(&original).unwrap();
        let (proxy, _) = load(&original).unwrap();
        let removed = dir.path().join("disconnected.png");
        std::fs::rename(&original, &removed).unwrap();
        let source = crate::photo_develop::PhotoSource::load(&original).unwrap();
        assert!(source.is_proxy());
        let params = emulsion_core::raw::DevelopParams {
            exposure: 0.5,
            ..Default::default()
        };
        crate::raw_settings::save_photo_settings(&original, &proxy.source_sha256, params).unwrap();
        assert!(
            source
                .develop_preview(&params, &std::sync::atomic::AtomicBool::new(false))
                .is_ok()
        );
        assert!(source.develop_with(&params).is_err());
        assert!(crate::thumb::batch_thumbnail(&original, 128).is_ok());
        std::fs::rename(&removed, &original).unwrap();
        let reconnected = crate::photo_develop::PhotoSource::load(&original).unwrap();
        assert!(!reconnected.is_proxy());
        assert_eq!(
            crate::raw_settings::adjacent_settings(&original, &reconnected.source_sha256).unwrap(),
            params
        );
        assert_eq!(std::fs::read(&original).unwrap(), bytes);
        std::fs::write(
            &original,
            crate::export::png8(2, 2, &[0, 0, 0, 255].repeat(4)).unwrap(),
        )
        .unwrap();
        let changed = crate::photo_develop::PhotoSource::load(&original).unwrap();
        assert!(crate::raw_settings::adjacent_settings(&original, &changed.source_sha256).is_err());
        let _ = std::fs::remove_file(path(&original));
        let _ = std::fs::remove_file(crate::raw_settings::sidecar_path(&original).unwrap());
    }
}
