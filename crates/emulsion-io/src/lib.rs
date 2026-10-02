//! `emulsion-io` — reading and writing documents.
//!
//! * [`open`] reads the native format (OpenRaster plus an Emulsion manifest)
//!   or imports any common image as a one-node document.
//! * [`save`] writes the native format atomically.
//! * [`export`] writes any [`export::ExportFormat`]: a flattened raster or
//!   converter-backed format, or a layered PSD/XCF.
//!
//! Every reader validates fully before returning, so a bad file can never
//! replace an open document.

pub mod abr;
pub mod audio;
pub mod brush_library;
pub mod brushset;
pub mod camera_profiles;
pub mod cloud;
pub mod creative_library;
pub mod design_bulk;
pub mod design_charts;
pub mod design_html;
#[cfg(test)]
mod design_layout_tests;
pub mod design_media;
#[cfg(test)]
mod design_responsive_tests;
#[cfg(test)]
mod design_styles_tests;
pub mod develop_edits;
pub mod develop_presets;
pub mod diagram_data;
pub mod diagram_import;
pub mod drawio;
pub mod exif;
pub mod export;
pub mod external;
pub mod ffmpeg;
pub mod film_library;
pub mod frame_export;
pub mod history;
pub mod icc;
pub mod import;
pub mod jxl;
pub mod lensfun;
pub mod lightroom_catalog;
pub mod lightroom_presets;
pub mod lottie;
pub mod ora;
mod path_data;
pub mod pdf_import;
pub mod photo_catalog;
pub mod photo_color;
pub mod photo_develop;
pub mod photo_export;
pub mod photo_index;
pub mod photo_proxy;
pub mod pptx;
pub mod printing;
pub mod project;
pub mod project_animation;
pub mod project_export;
pub mod psd;
pub mod raw;
#[cfg(test)]
#[path = "../tests/common/raw_fixture.rs"]
mod raw_fixture;
pub mod raw_probe;
pub mod raw_settings;
pub mod recent;
pub mod script;
pub mod selection_export;
pub mod settings;
pub mod spell;
pub mod storyboard_export;
pub mod storyboard_library;
pub mod svg;
pub mod svg_viewport;
pub mod template_pack;
pub mod thumb;
pub mod update;
pub mod video_export;
mod viewport_shadow;
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
    #[error("unsupported RAW camera or encoding: {0}")]
    UnsupportedRaw(String),
    #[error(
        "RAW memory budget is in use; wait for development/export to finish or close another RAW document"
    )]
    RawMemoryBudget,
    #[error("malformed RAW file: {0}")]
    MalformedRaw(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

/// Extensions `open` decodes by itself, lower case: the native project,
/// Photoshop and GIMP documents, the common web and print formats, the
/// `image` crate's wider set (Targa, PNM, icons, Radiance HDR, OpenEXR,
/// DDS, QOI, farbfeld), JPEG XL, SVG and camera RAW.
pub const OPEN_EXTENSIONS: &[&str] = &[
    "emuphoto",
    "ora",
    "emu",
    "drawio",
    "xml",
    "vsdx",
    "vsdm",
    "vstx",
    "vssx",
    "vssm",
    "vstm",
    "vss",
    "vst",
    "vdx",
    "vsx",
    "lucid",
    "lucidjson",
    "mmd",
    "mermaid",
    "d2",
    "dot",
    "gv",
    "md",
    "markdown",
    "glyphtide",
    "csv",
    "sql",
    "txt",
    "emutemplate",
    "emustencil",
    "psd",
    "psb",
    "xcf",
    "png",
    "jpg",
    "jpeg",
    "jpe",
    "jfif",
    "webp",
    "tif",
    "tiff",
    "bmp",
    "dib",
    "gif",
    "svg",
    "svgz",
    "jxl",
    "tga",
    "icb",
    "vda",
    "vst",
    "pbm",
    "pgm",
    "ppm",
    "pam",
    "pnm",
    "ico",
    "hdr",
    "rgbe",
    "exr",
    "dds",
    "qoi",
    "ff",
    "arw",
    "srf",
    "sr2",
    "cr2",
    "cr3",
    "crw",
    "nef",
    "nrw",
    "dng",
    "raf",
    "orf",
    "rw2",
    "pef",
    "erf",
    "mrw",
    "3fr",
    "iiq",
    "mos",
    "kdc",
    "dcr",
    "x3f",
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
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        && diagram_import::is_diagram(path)
    {
        return true;
    }
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| OPEN_EXTENSIONS.contains(&e.as_str()) || external::can_open(path))
}

pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

/// Largest SVG text an `.svgz` may inflate to.
const MAX_SVG_BYTES: u64 = 64 << 20;

/// SVG text from `path`, inflating `.svgz`.
fn read_svg(path: &Path) -> Result<String> {
    svg_text(std::fs::read(path)?, MAX_SVG_BYTES)
}

/// SVG text from raw file bytes, inflating gzip up to `limit` bytes.
fn svg_text(bytes: Vec<u8>, limit: u64) -> Result<String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut text = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .take(limit + 1)
            .read_to_string(&mut text)?;
        if text.len() as u64 > limit {
            return Err(IoError::Unsupported(
                "compressed SVG inflates past the size limit".into(),
            ));
        }
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
    if photo_develop::is_virtual(path) {
        return photo_develop::open_virtual(path);
    }
    Ok(open_full(path)?.doc)
}

/// Import anything that is not the native format as a fresh document.
fn import_any(path: &Path) -> Result<Document> {
    if photo_develop::is_virtual(path) {
        return photo_develop::open_virtual(path);
    }
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
    } else if raw_probe::is_raw(path)? {
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

/// Open SVG paths and outlined text as editable vectors. Richer appearances
/// retain their original scalable SVG source alongside a preview.
fn open_svg(path: &Path) -> Result<Document> {
    svg_vectors::document(&read_svg(path)?)
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
    if project::is_project(path) || diagram_import::is_diagram(path) || template_pack::is_pack(path)
    {
        return Err(IoError::Unsupported(
            "This is a multi-page project. Open it as a project to preserve every page.".into(),
        ));
    }
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

/// Parse a JSON config file's `bytes`. When they do not parse, keep them
/// at `<name>.bak` (owner-only) and log a warning, so falling back to
/// defaults and saving later cannot silently destroy the user's settings.
pub(crate) fn parse_config<T: serde::de::DeserializeOwned>(path: &Path, bytes: &[u8]) -> Option<T> {
    let error = match serde_json::from_slice(bytes) {
        Ok(value) => return Some(value),
        Err(e) => e,
    };
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    let backup = path.with_file_name(name);
    let kept = write_atomic_mode(&backup, Some(0o600), |f| {
        use std::io::Write;
        f.write_all(bytes)?;
        Ok(())
    });
    match kept {
        Ok(()) => tracing::warn!(
            path = %path.display(),
            backup = %backup.display(),
            %error,
            "unreadable config file kept as backup; using defaults"
        ),
        Err(backup_error) => tracing::warn!(
            path = %path.display(),
            %error,
            %backup_error,
            "unreadable config file could not be backed up; using defaults"
        ),
    }
    None
}

/// Write `value` as pretty JSON to `path` atomically, owner-only on Unix.
pub(crate) fn save_config<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    write_atomic_mode(path, Some(0o600), |f| {
        serde_json::to_writer_pretty(f, value).map_err(std::io::Error::other)?;
        Ok(())
    })
    .map_err(|e| match e {
        IoError::Io(e) => e,
        e => std::io::Error::other(e),
    })
}

/// Write `bytes` to `path` via a temporary file in the same directory, so a
/// crash or full disk never leaves a half-written file in place.
pub(crate) fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<()>,
) -> Result<()> {
    write_atomic_mode(path, None, write)
}

/// `write_atomic`, creating the file with Unix permission bits `mode`
/// (ignored elsewhere) instead of the process default.
pub(crate) fn write_atomic_mode(
    path: &Path,
    mode: Option<u32>,
    write: impl FnOnce(&mut std::fs::File) -> Result<()>,
) -> Result<()> {
    // Unique per call, so concurrent writes to one path never share a file.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.emulsion-tmp-{}-{seq}", std::process::id()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        let mut f = options.open(&tmp)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_atomic_writes_never_mix_payloads() {
        let dir = std::env::temp_dir().join(format!("emulsion-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.ora");
        let payloads = [vec![b'a'; 256 << 10], vec![b'b'; 256 << 10]];
        for _ in 0..20 {
            std::thread::scope(|s| {
                for payload in &payloads {
                    let path = &path;
                    s.spawn(move || {
                        for _ in 0..5 {
                            write_atomic(path, |f| {
                                use std::io::Write;
                                for chunk in payload.chunks(4096) {
                                    f.write_all(chunk)?;
                                }
                                Ok(())
                            })
                            .unwrap();
                        }
                    });
                }
            });
            let written = std::fs::read(&path).unwrap();
            assert!(payloads.contains(&written), "file mixes both writes");
        }
        let leftovers = std::fs::read_dir(&dir).unwrap().count();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(leftovers, 1, "no temporary files remain");
    }

    #[test]
    fn svgz_inflation_is_capped() {
        use std::io::Write;
        let text = format!("<svg>{}</svg>", " ".repeat(4096));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(text.as_bytes()).unwrap();
        let bytes = gz.finish().unwrap();
        assert_eq!(svg_text(bytes.clone(), 1 << 20).unwrap(), text);
        assert!(matches!(
            svg_text(bytes, 1024),
            Err(IoError::Unsupported(_))
        ));
    }
}

#[cfg(test)]
mod design_components_tests;

#[cfg(test)]
mod design_variable_tests;

pub mod svg_vectors;

pub mod diagram_packs;
pub mod document_stencils;

mod creative_brand;

#[cfg(test)]
mod design_font_tests;

mod font_data;
mod media_data;

#[cfg(test)]
mod design_vector_tests;

pub mod design_motion_export;

pub mod smart_source;
mod smart_source_data;

mod photo_files;

pub mod photo_metadata;

pub mod photo_backup;

pub mod photo_depth;
pub mod photo_geometry;
pub mod photo_panorama;
pub mod photo_registration;
pub mod photo_wide;

#[cfg(test)]
mod photo_workflow_tests;

pub mod photo_publish;

pub mod lightroom_bridge;

pub mod photo_hdr;
pub mod photo_profiles;
