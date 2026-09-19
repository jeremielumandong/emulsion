//! `emulsion-io` — reading and writing documents.
//!
//! * [`open`] reads the native format (OpenRaster plus an Emulsion manifest)
//!   or imports any common image as a one-node document.
//! * [`save`] writes the native format atomically.
//! * [`export`] writes a flattened PNG, JPEG, WebP or TIFF.
//!
//! Every reader validates fully before returning, so a bad file can never
//! replace an open document.

pub mod export;
pub mod import;
pub mod ora;
pub mod recent;
pub mod thumb;

use emulsion_core::Document;
use std::path::Path;

pub use export::{ExportFormat, ExportOptions, export};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("could not decode image: {0}")]
    Image(#[from] image::ImageError),
    #[error("not a valid archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("invalid layer stack: {0}")]
    Xml(String),
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("this file was made by a newer version of Emulsion (format {0}); update Emulsion to open it")]
    TooNew(u32),
    #[error("{0}")]
    Invalid(#[from] emulsion_core::DocumentError),
    #[error("image is {0}×{1}, larger than Emulsion supports")]
    TooLarge(u32, u32),
    #[error("unsupported file type: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

/// Extensions `open` understands, for file dialogs.
pub const OPEN_EXTENSIONS: &[&str] = &["ora", "png", "jpg", "jpeg", "webp", "tif", "tiff", "bmp", "gif"];

pub fn is_native(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ora"))
}

/// Open a native document or import an image.
pub fn open(path: &Path) -> Result<Document> {
    if is_native(path) { ora::read(path) } else { import::import(path) }
}

/// Save in the native format.
pub fn save(doc: &Document, path: &Path) -> Result<()> {
    ora::write(doc, path)
}

/// Write `bytes` to `path` via a temporary file in the same directory, so a
/// crash or full disk never leaves a half-written file in place.
pub(crate) fn write_atomic(path: &Path, write: impl FnOnce(&mut std::fs::File) -> Result<()>) -> Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.emulsion-tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        write(&mut f)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
