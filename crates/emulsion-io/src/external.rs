//! Formats Emulsion has no decoder for, opened through a converter the
//! system already has, the way GIMP leans on its plug-ins: HEIC/HEIF and
//! AVIF (libheif's `heif-convert`, libavif's `avifdec`), PDF and PostScript
//! (poppler's `pdftoppm`), and everything ImageMagick reads (PCX, Paint
//! Shop Pro, XPM/XBM, SGI, Sun raster, FITS, DICOM, JPEG 2000, ICNS…). The
//! converter writes a PNG at the source's depth into a private temporary directory, which
//! is then imported like any PNG and removed. No converter installed means
//! a clear error naming what would help.

use crate::import::{Decoded, document_from};
use crate::{IoError, Result};
use emulsion_core::Document;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Extensions a converter may open, lower case, roughly GIMP's list minus
/// what Emulsion decodes itself.
pub const EXTERNAL_EXTENSIONS: &[&str] = &[
    // HEIF family and AVIF
    "heic", "heif", "hif", "avif", // documents: first page
    "pdf", "ps", "eps", "ai", // JPEG 2000
    "jp2", "j2k", "j2c", "jpc", "jpf", "jpx", // old paint formats
    "pcx", "pcc", "psp", "pspimage", "tub", "xpm", "xbm", "sgi", "rgb", "rgba", "bw", "ras", "sun",
    "cel", "fits", "fit", "fts", "dcm", "dicom", "icns", "cur", "wmf", "emf", "xwd", "mng", "pfm",
    "gbr", "gih", "pat", "pix", "als",
];

pub fn is_external(path: &Path) -> bool {
    ext(path).is_some_and(|e| EXTERNAL_EXTENSIONS.contains(&e.as_str()))
}

fn ext(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

fn on_path(tool: &str) -> bool {
    static CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, bool>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(tool).copied()) {
        return v;
    }
    let found = std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|d| {
            let p = d.join(tool);
            p.is_file() || cfg!(windows) && d.join(format!("{tool}.exe")).is_file()
        })
    });
    if let Ok(mut c) = cache.lock() {
        c.insert(tool.to_string(), found);
    }
    found
}

/// One way to turn `input` into a PNG at `out`.
struct Converter {
    tool: &'static str,
    /// Arguments; `{in}` and `{out}` are replaced. `{out_stem}` is `out`
    /// without its `.png`, for tools that add the extension themselves.
    args: &'static [&'static str],
}

const HEIF: Converter = Converter {
    tool: "heif-convert",
    args: &["-q", "100", "{in}", "{out}"],
};
const AVIF: Converter = Converter {
    tool: "avifdec",
    args: &["--depth", "16", "{in}", "{out}"],
};
const PDF: Converter = Converter {
    tool: "pdftoppm",
    args: &[
        "-png",
        "-r",
        "200",
        "-f",
        "1",
        "-l",
        "1",
        "-singlefile",
        "{in}",
        "{out_stem}",
    ],
};
const MAGICK: Converter = Converter {
    tool: "magick",
    args: &["{in}[0]", "-define", "png:color-type=6", "{out}"],
};
const CONVERT: Converter = Converter {
    tool: "convert",
    args: &["{in}[0]", "-define", "png:color-type=6", "{out}"],
};

/// Converters to try for `path`, best first.
fn converters(path: &Path) -> Vec<&'static Converter> {
    let e = ext(path).unwrap_or_default();
    let mut list: Vec<&Converter> = match e.as_str() {
        "heic" | "heif" | "hif" => vec![&HEIF],
        "avif" => vec![&AVIF, &HEIF],
        "pdf" | "ps" | "eps" | "ai" => vec![&PDF],
        _ => vec![],
    };
    list.extend([&MAGICK, &CONVERT]);
    list
}

/// Whether some installed converter can open `path`'s kind.
pub fn can_open(path: &Path) -> bool {
    is_external(path) && converters(path).iter().any(|c| on_path(c.tool))
}

/// External extensions an installed converter handles right now.
pub fn available_extensions() -> Vec<&'static str> {
    EXTERNAL_EXTENSIONS
        .iter()
        .copied()
        .filter(|e| can_open(Path::new(&format!("x.{e}"))))
        .collect()
}

/// What to install for `path`, for the error message.
fn advice(path: &Path) -> String {
    let names: Vec<&str> = converters(path).iter().map(|c| c.tool).collect();
    format!(
        "no decoder for .{}; install one of {} and Emulsion will open it through that",
        ext(path).unwrap_or_default(),
        names.join(", ")
    )
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir() -> Result<TempDir> {
    let d = std::env::temp_dir().join(format!(
        "emulsion-import-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d)?;
    Ok(TempDir(d))
}

/// The converter's output: `out` itself, or else the first PNG it wrote
/// (heif-convert numbers them `page-1.png`, `page-2.png`… for a container
/// holding several images, such as an iPhone burst; the first is the
/// primary picture).
fn first_png(dir: &Path, out: &Path) -> Option<PathBuf> {
    if out.is_file() {
        return Some(out.to_path_buf());
    }
    let mut pngs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .collect();
    pngs.sort();
    pngs.into_iter().next()
}

/// Convert `path` to a PNG with the first converter that succeeds and
/// decode it.
pub fn decode(path: &Path) -> Result<Decoded> {
    let tmp = temp_dir()?;
    let out = tmp.0.join("page.png");
    let out_stem = tmp.0.join("page");
    let mut failures = Vec::new();
    for c in converters(path) {
        if !on_path(c.tool) {
            continue;
        }
        let args: Vec<String> = c
            .args
            .iter()
            .map(|a| {
                a.replace("{in}", &path.to_string_lossy())
                    .replace("{out_stem}", &out_stem.to_string_lossy())
                    .replace("{out}", &out.to_string_lossy())
            })
            .collect();
        for stale in std::fs::read_dir(&tmp.0).into_iter().flatten().flatten() {
            let _ = std::fs::remove_file(stale.path());
        }
        let mut cmd = Command::new(c.tool);
        cmd.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        match cmd.output() {
            Ok(o)
                if o.status.success()
                    && let Some(png) = first_png(&tmp.0, &out) =>
            {
                tracing::info!(tool = c.tool, path = %path.display(), "imported through converter");
                return crate::import::decode(&png);
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                let line = err
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("failed");
                failures.push(format!("{}: {}", c.tool, line.trim()));
            }
            Err(e) => failures.push(format!("{}: {e}", c.tool)),
        }
    }
    if failures.is_empty() {
        return Err(IoError::Unsupported(advice(path)));
    }
    Err(IoError::Unsupported(format!(
        "{} could not be converted ({})",
        path.display(),
        failures.join("; ")
    )))
}

pub fn open(path: &Path) -> Result<Document> {
    document_from(path, decode(path)?)
}
