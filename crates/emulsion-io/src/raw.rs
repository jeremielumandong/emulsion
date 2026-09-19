//! Camera RAW import through `rawler`: decode, demosaic, white-balance and
//! colour-convert to 16-bit sRGB, honouring the camera's orientation, so a
//! RAW opens as an ordinary 16-bit document that recipes and adjustments
//! then work on non-destructively. The develop is rawler's default
//! pipeline (camera white balance, matrix to sRGB); a proper RAW panel
//! (exposure, WB picker, highlight recovery) builds on top of this.

use crate::{IoError, Result};
use emulsion_core::{Document, Node};
use emulsion_raster::{Placement, Raster};
use rawler::Orientation;
use rawler::imgop::develop::{Intermediate, RawDevelop};
use std::path::Path;
use std::sync::Arc;

/// Extensions handled here (lower case).
pub const RAW_EXTENSIONS: &[&str] = &[
    "arw", "srf", "sr2", // Sony
    "cr2", "cr3", "crw", // Canon
    "nef", "nrw", // Nikon
    "dng", // Adobe / many phones
    "raf", // Fujifilm
    "orf", // Olympus / OM
    "rw2", // Panasonic
    "pef", // Pentax
    "erf", "mrw", "3fr", "iiq", "mos", "kdc", "dcr", "x3f",
];

pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| RAW_EXTENSIONS.contains(&e.as_str()))
}

/// What the file said about itself.
#[derive(Clone, Debug, Default)]
pub struct RawInfo {
    pub make: String,
    pub model: String,
    pub wb_coeffs: [f32; 4],
    pub width: u32,
    pub height: u32,
}

/// How to develop a RAW: the knobs of the RAW panel. Zero everywhere is
/// the camera's own rendering (as-shot white balance, no exposure change).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DevelopParams {
    /// Exposure compensation in EV, −3…+3.
    pub exposure: f32,
    /// White balance shift, −1 (cooler) … +1 (warmer).
    pub temperature: f32,
    /// White balance shift, −1 (greener) … +1 (more magenta).
    pub tint: f32,
    /// Highlight roll-off, 0…1: compress the top of the range so bright
    /// skies and clouds keep their gradation.
    pub highlights: f32,
    /// Shadow lift, −1 (deeper) … +1 (lifted).
    pub shadows: f32,
}

impl Default for DevelopParams {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            highlights: 0.0,
            shadows: 0.0,
        }
    }
}

/// A decoded RAW kept in memory so the panel can re-develop it quickly
/// (decoding is the slow half).
pub struct RawSource {
    raw: rawler::RawImage,
    pub info: RawInfo,
}

impl RawSource {
    pub fn load(path: &Path) -> Result<Self> {
        let raw =
            rawler::decode_file(path).map_err(|e| IoError::Unsupported(format!("RAW: {e}")))?;
        let info = RawInfo {
            make: raw.clean_make.clone(),
            model: raw.clean_model.clone(),
            wb_coeffs: raw.wb_coeffs,
            width: raw.width as u32,
            height: raw.height as u32,
        };
        Ok(Self { raw, info })
    }

    /// Develop with `params` into a 16-bit sRGB raster, upright.
    pub fn develop_with(&self, params: &DevelopParams) -> Result<Raster> {
        let mut raw = self.raw.clone();
        // White balance: shift the camera's coefficients. Warmer = more
        // red, less blue; magenta = less green.
        if !raw.wb_coeffs[0].is_nan() {
            let t = params.temperature.clamp(-1.0, 1.0);
            let g = params.tint.clamp(-1.0, 1.0);
            raw.wb_coeffs[0] *= 2f32.powf(0.7 * t);
            raw.wb_coeffs[2] *= 2f32.powf(-0.7 * t);
            raw.wb_coeffs[1] *= 2f32.powf(-0.4 * g);
            if raw.wb_coeffs[3].is_finite() && raw.wb_coeffs[3] > 0.0 {
                raw.wb_coeffs[3] *= 2f32.powf(-0.4 * g);
            }
        }
        // Everything but the gamma step: we shape the linear values first.
        let mut dev = RawDevelop::default();
        dev.steps
            .retain(|s| *s != rawler::imgop::develop::ProcessingStep::SRgb);
        let developed = dev
            .develop_intermediate(&raw)
            .map_err(|e| IoError::Unsupported(format!("RAW develop: {e}")))?;
        let gain = 2f32.powf(params.exposure.clamp(-3.0, 3.0));
        let hl = params.highlights.clamp(0.0, 1.0);
        let sh = params.shadows.clamp(-1.0, 1.0);
        let shade_gamma = 1.0 - 0.35 * sh;
        let shape = move |v: f32| -> f32 {
            let mut v = (v * gain).max(0.0);
            if hl > 0.0 && v > 0.7 {
                // Soft knee above 70%: the brighter, the gentler the slope.
                let over = v - 0.7;
                v = 0.7 + over / (1.0 + over * hl * 3.0);
            }
            v = v.min(1.0);
            if sh != 0.0 {
                v = v.powf(shade_gamma);
            }
            // sRGB gamma.
            if v <= 0.003_130_8 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        };
        finish(developed, raw.orientation, shape)
    }
}

/// Develop `path` into a 16-bit sRGB raster, upright, as the camera
/// meant it.
pub fn develop(path: &Path) -> Result<(Raster, RawInfo)> {
    let src = RawSource::load(path)?;
    let r = src.develop_with(&DevelopParams::default())?;
    Ok((r, src.info))
}

/// Turn developed linear values into an upright 16-bit raster, passing
/// each channel through `shape` (which also gamma-encodes).
fn finish(
    developed: Intermediate,
    orientation: Orientation,
    shape: impl Fn(f32) -> f32 + Sync,
) -> Result<Raster> {
    let (w, h, rgb): (usize, usize, Vec<[f32; 3]>) = match developed {
        Intermediate::ThreeColor(px) => (px.width, px.height, px.data),
        Intermediate::FourColor(px) => (
            px.width,
            px.height,
            px.data.into_iter().map(|p| [p[0], p[1], p[2]]).collect(),
        ),
        Intermediate::Monochrome(px) => {
            let d = px.dim();
            (d.w, d.h, px.data.into_iter().map(|v| [v, v, v]).collect())
        }
    };
    crate::import::check_size(w as u32, h as u32)?;
    let to16 = |v: f32| (shape(v).clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    type Index = Box<dyn Fn(usize, usize) -> usize>;
    let (ow, oh, mapper): (usize, usize, Index) = match orientation {
        Orientation::Rotate90 => (h, w, Box::new(move |x, y| (h - 1 - x) * w + y)),
        Orientation::Rotate180 => (w, h, Box::new(move |x, y| (h - 1 - y) * w + (w - 1 - x))),
        Orientation::Rotate270 => (h, w, Box::new(move |x, y| x * w + (w - 1 - y))),
        Orientation::HorizontalFlip => (w, h, Box::new(move |x, y| y * w + (w - 1 - x))),
        Orientation::VerticalFlip => (w, h, Box::new(move |x, y| (h - 1 - y) * w + x)),
        _ => (w, h, Box::new(move |x, y| y * w + x)),
    };
    let mut data: Vec<u16> = Vec::with_capacity(ow * oh * 4);
    for y in 0..oh {
        for x in 0..ow {
            let p = rgb[mapper(x, y)];
            data.extend_from_slice(&[to16(p[0]), to16(p[1]), to16(p[2]), u16::MAX]);
        }
    }
    Ok(Raster::from_srgba16(ow as u32, oh as u32, &data))
}

/// Open a RAW file as a one-node 16-bit document.
pub fn open(path: &Path) -> Result<Document> {
    let (raster, info) = develop(path)?;
    let mut doc = Document::new(raster.width(), raster.height());
    doc.source_depth = 16;
    doc.info = crate::exif::read(path);
    let name = if info.model.is_empty() {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "RAW".into())
    } else {
        format!(
            "{} ({})",
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            info.model
        )
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
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_extensions_are_recognised() {
        assert!(
            is_raw(Path::new("shot.ARW"))
                && is_raw(Path::new("x.dng"))
                && is_raw(Path::new("a.RAF"))
        );
        assert!(!is_raw(Path::new("a.jpg")) && !is_raw(Path::new("a.ora")));
        assert!(open(Path::new("/nonexistent.arw")).is_err());
    }
}
