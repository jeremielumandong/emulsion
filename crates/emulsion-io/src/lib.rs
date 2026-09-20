//! `emulsion-io` — reading and writing documents.
//!
//! * [`open`] reads the native format (OpenRaster plus an Emulsion manifest)
//!   or imports any common image as a one-node document.
//! * [`save`] writes the native format atomically.
//! * [`export`] writes a flattened PNG, JPEG, WebP or TIFF.
//!
//! Every reader validates fully before returning, so a bad file can never
//! replace an open document.

pub mod brushset;
pub mod exif;
pub mod export;
pub mod external;
pub mod history;
pub mod icc;
pub mod import;
pub mod jxl;
pub mod lensfun;
pub mod ora;
mod path_data;
pub mod psd;
pub mod raw;
pub mod recent;
pub mod settings;
pub mod svg;
pub mod thumb;
pub mod xcf;

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
    #[error(
        "this file was made by a newer version of Emulsion (format {0}); update Emulsion to open it"
    )]
    TooNew(u32),
    #[error("{0}")]
    Invalid(#[from] emulsion_core::DocumentError),
    #[error("image is {0}×{1}, larger than Emulsion supports")]
    TooLarge(u32, u32),
    #[error("unsupported file type: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

/// Extensions `open` decodes by itself, lower case: the native project,
/// Photoshop and GIMP documents, the common web and print formats, the
/// `image` crate's wider set (Targa, PNM, icons, Radiance HDR, OpenEXR,
/// DDS, QOI, farbfeld), JPEG XL, SVG and camera RAW.
pub const OPEN_EXTENSIONS: &[&str] = &[
    "ora", "psd", "psb", "xcf", "png", "jpg", "jpeg", "jpe", "jfif", "webp", "tif", "tiff", "bmp",
    "dib", "gif", "svg", "svgz", "jxl", "tga", "icb", "vda", "vst", "pbm", "pgm", "ppm", "pam",
    "pnm", "ico", "hdr", "rgbe", "exr", "dds", "qoi", "ff", "arw", "srf", "sr2", "cr2", "cr3",
    "crw", "nef", "nrw", "dng", "raf", "orf", "rw2", "pef", "erf", "mrw", "3fr", "iiq", "mos",
    "kdc", "dcr", "x3f",
];

/// Everything that opens on this machine right now: `OPEN_EXTENSIONS` plus
/// the formats an installed converter handles (`external`).
pub fn openable_extensions() -> Vec<&'static str> {
    let mut v: Vec<&str> = OPEN_EXTENSIONS.to_vec();
    v.extend(external::available_extensions());
    v
}

/// Whether `path` looks like something `open` can take, by extension.
pub fn is_openable(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| OPEN_EXTENSIONS.contains(&e.as_str()) || external::can_open(path))
}

pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

/// SVG text from `path`, inflating `.svgz`.
fn read_svg(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut text = String::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut text)?;
        return Ok(text);
    }
    String::from_utf8(bytes).map_err(|_| IoError::Unsupported("SVG is not UTF-8 text".into()))
}

pub fn is_native(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ora"))
}

/// Open a native document or import an image.
pub fn open(path: &Path) -> Result<Document> {
    Ok(open_full(path)?.doc)
}

/// Import anything that is not the native format as a fresh document.
fn import_any(path: &Path) -> Result<Document> {
    if psd::is_psd(path) {
        psd::read(path)
    } else if xcf::is_xcf(path) {
        match xcf::read(path) {
            Ok(doc) => Ok(doc),
            // Precisions and modes the XCF crate lacks: flattened through a
            // converter when one is installed, else the crate's error.
            Err(e) if external::can_open(path) => {
                tracing::info!(path = %path.display(), error = %e, "XCF: falling back to converter");
                external::open(path)
            }
            Err(e) => Err(e),
        }
    } else if is_svg(path) {
        open_svg(path)
    } else if raw::is_raw(path) {
        raw::open(path)
    } else if jxl::is_jxl(path) {
        jxl::open(path)
    } else if external::is_external(path) {
        external::open(path)
    } else {
        match import::import(path) {
            // Unknown to `image` but perhaps to a converter on this machine.
            Err(IoError::Unsupported(_)) | Err(IoError::Image(_)) if !is_known(path) => {
                external::open(path)
            }
            r => r,
        }
    }
}

/// An SVG made only of plain paths and shapes opens as editable path
/// layers; anything richer (gradients, filters, text, images, masks) is
/// rendered whole into one pixel layer so it looks as drawn.
fn open_svg(path: &Path) -> Result<Document> {
    let text = read_svg(path)?;
    let paths = svg::import(&text);
    if let Ok(imp) = &paths
        && imp.skipped.is_empty()
        && !imp.doc.nodes.is_empty()
    {
        return Ok(imp.doc.clone());
    }
    match svg::rasterize(&text) {
        Ok(raster) => {
            let mut doc = import::document_from(path, import::Decoded { raster, depth: 8 })?;
            if let Ok(imp) = &paths
                && !imp.skipped.is_empty()
            {
                tracing::info!(
                    path = %path.display(),
                    skipped = imp.skipped.len(),
                    "SVG rendered to pixels: some elements have no editable path form"
                );
            }
            doc.info = Default::default();
            Ok(doc)
        }
        Err(render_err) => match paths {
            Ok(imp) if !imp.doc.nodes.is_empty() => Ok(imp.doc),
            Ok(_) => Err(render_err),
            Err(e) => Err(e),
        },
    }
}

fn is_known(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| OPEN_EXTENSIONS.contains(&e.as_str()))
}

/// Save in the native format.
pub fn save(doc: &Document, path: &Path) -> Result<()> {
    ora::write(doc, path)
}

pub use ora::Opened;

/// Open a document with its history graph when it is a native file.
pub fn open_full(path: &Path) -> Result<Opened> {
    if is_native(path) {
        ora::read_full(path)
    } else {
        Ok(Opened {
            doc: import_any(path)?,
            graph: None,
            history_error: None,
        })
    }
}

/// Save in the native format with the history graph.
pub fn save_full(doc: &Document, graph: &emulsion_core::graph::Graph, path: &Path) -> Result<()> {
    ora::write_full(doc, Some(graph), path)
}

/// Write `bytes` to `path` via a temporary file in the same directory, so a
/// crash or full disk never leaves a half-written file in place.
pub(crate) fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<()>,
) -> Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
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
