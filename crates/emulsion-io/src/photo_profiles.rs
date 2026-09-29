//! Shared camera-profile browser operations. Thumbnails use the real development engine.
use crate::{IoError, Result, photo_develop::PhotoSource, raw::DevelopParams};
use std::{collections::BTreeSet, sync::atomic::AtomicBool};
pub type Digest = [u8; 32];
fn path() -> std::path::PathBuf {
    crate::recent::data_dir().join("camera-profile-favorites.json")
}
pub fn favorites() -> BTreeSet<Digest> {
    let file = path();
    if std::fs::metadata(&file).is_ok_and(|m| m.len() <= 64 * 1024) {
        std::fs::read(file)
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default()
    } else {
        BTreeSet::new()
    }
}
pub fn set_favorite(digest: Digest, on: bool) -> Result<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    crate::camera_profiles::load(&digest)?;
    let mut values = favorites();
    if on {
        values.insert(digest);
    } else {
        values.remove(&digest);
    }
    if values.len() > 512 {
        return Err(IoError::Unsupported(
            "Profile favorites limit reached".into(),
        ));
    }
    let p = path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    let bytes = serde_json::to_vec(&values).map_err(|e| IoError::Manifest(e.to_string()))?;
    crate::write_atomic(&p, |file| {
        use std::io::Write;
        file.write_all(&bytes)?;
        Ok(())
    })
}
pub fn preview(
    source: &PhotoSource,
    params: &DevelopParams,
    digest: Option<Digest>,
    cancel: &AtomicBool,
) -> Result<emulsion_raster::Raster> {
    let params = DevelopParams {
        camera_profile: digest,
        ..*params
    };
    source.validate_settings(&params)?;
    let image = source.develop_preview(&params, cancel)?;
    let factor = (192. / image.width().max(image.height()) as f64).min(1.);
    crate::photo_export::resize(
        &image,
        (image.width() as f64 * factor).round().max(1.) as u32,
        (image.height() as f64 * factor).round().max(1.) as u32,
    )
}
