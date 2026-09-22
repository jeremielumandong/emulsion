//! Sensor RAW development. Floating-point scene data survives until the final
//! linear-sRGB raster conversion; every edit can be rendered again from source.

mod develop;

use crate::{IoError, Result};
pub use emulsion_core::raw::DevelopParams;
use emulsion_core::{
    Document, Node,
    raw::{RawDocument, RawMetadata},
};
use emulsion_raster::{Placement, Raster};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

// Decoder/developer float intermediates are substantially larger than a raster.
// Serialize heavy stages across tabs and exports, with a distinct sensor ceiling.
static HEAVY_JOB: Mutex<()> = Mutex::new(());
pub const MAX_RAW_PIXELS: u64 = 128_000_000;
static RESERVED_PIXELS: AtomicU64 = AtomicU64::new(0);

struct PixelReservation(u64);

impl PixelReservation {
    fn acquire(pixels: u64) -> Result<Self> {
        RESERVED_PIXELS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            used.checked_add(pixels).filter(|total| *total <= MAX_RAW_PIXELS)
        }).map_err(|_| IoError::UnsupportedRaw("RAW memory budget is in use; wait for development/export to finish or close another RAW document".into()))?;
        Ok(Self(pixels))
    }
    fn shrink(&mut self, pixels: u64) {
        if pixels < self.0 {
            RESERVED_PIXELS.fetch_sub(self.0 - pixels, Ordering::AcqRel);
            self.0 = pixels;
        }
    }
}

impl Drop for PixelReservation {
    fn drop(&mut self) {
        RESERVED_PIXELS.fetch_sub(self.0, Ordering::AcqRel);
    }
}

fn check_sensor_size(width: u32, height: u32) -> Result<()> {
    crate::import::check_size(width, height)?;
    if u64::from(width) * u64::from(height) > MAX_RAW_PIXELS {
        return Err(IoError::TooLarge(width, height));
    }
    Ok(())
}

pub const RAW_EXTENSIONS: &[&str] = &[
    "arw", "srf", "sr2", "cr2", "cr3", "crw", "nef", "nrw", "dng", "raf", "orf", "rw2", "pef",
    "erf", "mrw", "3fr", "iiq", "mos", "kdc", "dcr", "x3f",
];

/// Extension hint only; format routing also probes file contents.
pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| RAW_EXTENSIONS.contains(&e.as_str()))
}

#[derive(Clone, Debug, Default)]
pub struct RawInfo {
    pub make: String,
    pub model: String,
    pub wb_coeffs: [f32; 4],
    pub width: u32,
    pub height: u32,
}

fn guarded<T>(action: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).unwrap_or_else(|_| {
        Err(IoError::Unsupported(
            "RAW decoder failed: corrupt file or unsupported camera/compression variant".into(),
        ))
    })
}

fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Unsupported("RAW development cancelled".into()))
    } else {
        Ok(())
    }
}

pub fn source_digest(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub struct RawSource {
    raw: rawler::RawImage,
    pub info: RawInfo,
    pub metadata: RawMetadata,
    pub source: PathBuf,
    pub source_sha256: String,
    _reservation: PixelReservation,
}

impl RawSource {
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_checked(path, None)
    }

    /// Refuse a replaced original instead of applying a saved recipe to new pixels.
    pub fn load_verified(path: &Path, expected_sha256: &str) -> Result<Self> {
        Self::load_checked(path, Some(expected_sha256))
    }

    fn load_checked(path: &Path, expected: Option<&str>) -> Result<Self> {
        let _job = HEAVY_JOB
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guarded(|| {
            let hash = source_digest(path)?;
            if expected.is_some_and(|value| value != hash) {
                return Err(IoError::Unsupported("RAW original has changed (SHA-256 mismatch); restore the original file before developing or exporting".into()));
            }
            let mut metadata = crate::raw_probe::metadata(path)?;
            if metadata.width != 0 && metadata.height != 0 {
                check_sensor_size(metadata.width, metadata.height)?;
            }
            // If the metadata API cannot expose dimensions, reserve the entire
            // budget until decoding reveals them. rawler allocations themselves
            // are not controllable; this is not a hard process allocator limit.
            let known_pixels = u64::from(metadata.width) * u64::from(metadata.height);
            let mut reservation = PixelReservation::acquire(if known_pixels == 0 {
                MAX_RAW_PIXELS
            } else {
                known_pixels
            })?;
            let raw = rawler::decode_file(path).map_err(|e| {
                IoError::Unsupported(format!(
                    "RAW {} {} ({}): {e}",
                    metadata.make, metadata.model, metadata.compression
                ))
            })?;
            develop::validate(&raw)?;
            let actual_pixels = raw.width as u64 * raw.height as u64;
            if actual_pixels > reservation.0 {
                drop(reservation);
                reservation = PixelReservation::acquire(actual_pixels)?;
            } else {
                reservation.shrink(actual_pixels);
            }
            metadata.width = raw.width as u32;
            metadata.height = raw.height as u32;
            metadata.bits_per_sample = raw.bps as u32;
            metadata.sensor = match &raw.photometric {
                rawler::rawimage::RawPhotometricInterpretation::Cfa(cfa) => {
                    format!("{:?}", cfa.sensor)
                }
                rawler::rawimage::RawPhotometricInterpretation::LinearRaw => "linear RGB".into(),
                rawler::rawimage::RawPhotometricInterpretation::BlackIsZero => "monochrome".into(),
            };
            if source_digest(path)? != hash {
                return Err(IoError::Unsupported(
                    "RAW original changed while reading; retry when the file is stable".into(),
                ));
            }
            metadata.warnings.push("Working raster is bounded linear sRGB; out-of-gamut colors are clipped at raster conversion. Highlight control is roll-off, not reconstruction of saturated sensor channels.".into());
            let info = RawInfo {
                make: raw.clean_make.clone(),
                model: raw.clean_model.clone(),
                wb_coeffs: raw.wb_coeffs,
                width: raw.width as u32,
                height: raw.height as u32,
            };
            Ok(Self {
                raw,
                info,
                metadata,
                source: path.canonicalize()?,
                source_sha256: hash,
                _reservation: reservation,
            })
        })
    }

    pub fn develop_with(&self, params: &DevelopParams) -> Result<Raster> {
        self.develop_with_cancel(params, &AtomicBool::new(false))
    }

    /// Compute reproducible tonal settings from bounded, evenly spaced sensor
    /// samples. White balance, saturation, and the user's curve are preserved.
    pub fn auto_adjust(&self, params: &DevelopParams) -> Result<DevelopParams> {
        let _job = HEAVY_JOB.lock().unwrap_or_else(|p| p.into_inner());
        guarded(|| develop::auto_adjust(&self.raw, params))
    }

    /// Balance a neutral sample in oriented/cropped source-raster coordinates.
    /// Sampling uses demosaiced camera channels, never the embedded JPEG.
    pub fn neutral_white_balance(
        &self,
        params: &DevelopParams,
        x: u32,
        y: u32,
    ) -> Result<DevelopParams> {
        let _job = HEAVY_JOB.lock().unwrap_or_else(|p| p.into_inner());
        guarded(|| develop::neutral_white_balance(&self.raw, params, x, y))
    }

    /// Cancellation is checked between stages and during normalization. The
    /// external decoder/demosaicer cannot be interrupted inside a stage.
    pub fn develop_with_cancel(
        &self,
        params: &DevelopParams,
        cancel: &AtomicBool,
    ) -> Result<Raster> {
        cancelled(cancel)?;
        let _job = HEAVY_JOB
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guarded(|| develop::render(&self.raw, params, cancel))
    }
}

pub fn develop(path: &Path) -> Result<(Raster, RawInfo)> {
    let src = RawSource::load(path)?;
    Ok((src.develop_with(&DevelopParams::default())?, src.info))
}

pub fn open(path: &Path) -> Result<Document> {
    let src = RawSource::load(path)?;
    let params = DevelopParams::default();
    let raster = src.develop_with(&params)?;
    let mut doc = Document::new(raster.width(), raster.height());
    doc.source_depth = 16;
    doc.info = crate::exif::read(path);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "RAW".into());
    let name = if src.info.model.is_empty() {
        stem
    } else {
        format!("{stem} ({})", src.info.model)
    };
    emulsion_core::Command::AddNode {
        node: Box::new(Node::raster(
            0,
            name,
            Arc::new(raster),
            Placement::default(),
        )),
        slot: emulsion_core::command::Slot::TOP,
    }
    .apply(&mut doc)
    .map_err(|e| IoError::Manifest(e.to_string()))?;
    doc.raw_originals.push(src.source.clone());
    doc.raw = Some(RawDocument {
        schema_version: 1,
        node_id: doc.nodes[0].id,
        source: src.source,
        source_sha256: src.source_sha256,
        params,
        metadata: src.metadata,
    });
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_extensions_are_recognised() {
        for name in ["shot.ARW", "x.dng", "a.RAF", "camera.NRW"] {
            assert!(is_raw(Path::new(name)));
        }
        assert!(!is_raw(Path::new("a.jpg")) && !is_raw(Path::new("a.ora")));
        assert!(open(Path::new("/nonexistent.arw")).is_err());
    }
    #[test]
    fn decoder_panic_is_an_error() {
        assert!(guarded::<()>(|| panic!("bad decoder input")).is_err());
    }
}
