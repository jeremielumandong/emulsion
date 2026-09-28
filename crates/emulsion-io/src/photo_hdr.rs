//! Exposure-bracket merging in scene-linear RGB. Originals are read-only.
use crate::{IoError, Result};
use emulsion_raster::Raster;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const MAX_PIXELS: usize = 60_000_000;
const MARKER: &str = "Emulsion HDR 1\n";
static ACTIVE: AtomicBool = AtomicBool::new(false);
fn bad(message: impl Into<String>) -> IoError {
    IoError::Unsupported(message.into())
}
fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(bad("HDR merge cancelled"))
    } else {
        Ok(())
    }
}
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deghost {
    #[default]
    None,
    Low,
    Medium,
    High,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub align: bool,
    pub auto_tone: bool,
    pub deghost: Deghost,
    pub exposure_ev: Option<Vec<f32>>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            align: true,
            auto_tone: true,
            deghost: Deghost::None,
            exposure_ev: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub sha256: String,
    pub exposure_ev: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    #[serde(default)]
    pub panorama: bool,
    pub sources: Vec<Source>,
    pub offsets: Vec<[i32; 2]>,
    #[serde(default)]
    pub homographies: Vec<[f64; 9]>,
    pub reference: usize,
    pub deghosted_pixels: usize,
    pub options: Options,
    pub display_exposure: f32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone)]
pub struct FloatImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 3]>,
}
pub struct Frame {
    pub image: FloatImage,
    pub signal: Vec<f32>,
}
pub struct Merge {
    pub image: FloatImage,
    pub ghosts: Vec<u8>,
    pub report: Report,
}
fn validate_image(image: &FloatImage) -> Result<()> {
    let n = image.width as usize * image.height as usize;
    if image.width == 0
        || image.height == 0
        || n > MAX_PIXELS
        || image.pixels.len() != n
        || image
            .pixels
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e12)
    {
        return Err(bad("Invalid or oversized HDR pixels (60 megapixel limit)"));
    }
    Ok(())
}
fn lum(p: [f32; 3]) -> f32 {
    (p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722).max(0.)
}
impl FloatImage {
    pub fn resized(&self, max: u32) -> Self {
        let scale = (max as f32 / self.width.max(self.height) as f32).min(1.);
        let w = (self.width as f32 * scale).round().max(1.) as u32;
        let h = (self.height as f32 * scale).round().max(1.) as u32;
        let pixels = (0..h)
            .flat_map(|y| {
                (0..w).map(move |x| {
                    self.pixels[((y as u64 * self.height as u64 / h as u64) * self.width as u64
                        + x as u64 * self.width as u64 / w as u64)
                        as usize]
                })
            })
            .collect();
        Self {
            width: w,
            height: h,
            pixels,
        }
    }
    pub fn develop_display(
        &self,
        params: &crate::raw::DevelopParams,
        display_exposure: f32,
        panorama: bool,
        cancel: &AtomicBool,
    ) -> Result<Raster> {
        if panorama {
            validate_image(self)?;
            check(cancel)?;
            crate::raw::develop_linear_rgb(
                self.width,
                self.height,
                self.pixels.clone(),
                params,
                cancel,
            )
        } else {
            self.develop(params, display_exposure, cancel)
        }
    }
    pub fn develop(
        &self,
        params: &crate::raw::DevelopParams,
        display_exposure: f32,
        cancel: &AtomicBool,
    ) -> Result<Raster> {
        validate_image(self)?;
        check(cancel)?;
        if params.camera_profile.is_some()
            || params.wide_gamut
            || params.sensor_noise_reduction > 0.
            || params.wb_override.is_some()
        {
            return Err(bad("Merged HDR is linear RGB, not a camera mosaic"));
        }
        let mut p = *params;
        p.exposure = 0.;
        // Apply exposure before the display rolloff, retaining highlight detail in the file.
        let gain = 2f32.powf(params.exposure + display_exposure);
        let pixels = self
            .pixels
            .iter()
            .map(|rgb| {
                let y = lum(*rgb) * gain;
                rgb.map(|v| v.max(0.) * gain / (1. + y))
            })
            .collect();
        crate::raw::develop_linear_rgb(self.width, self.height, pixels, &p, cancel)
    }
}
fn exposures(paths: &[PathBuf], options: &Options) -> Result<Vec<f32>> {
    let mut ev = if let Some(values) = &options.exposure_ev {
        if values.len() != paths.len() {
            return Err(bad("Provide one exposure EV per bracket photo"));
        }
        values.clone()
    } else {
        paths
            .iter()
            .map(|path| {
                let info = crate::exif::read(path).ok_or_else(|| {
                    bad(format!(
                        "Missing exposure metadata for {}; enter relative EV values",
                        path.display()
                    ))
                })?;
                if info.exposure_s <= 0. || info.f_number <= 0. || info.iso == 0 {
                    return Err(bad(
                        "Incomplete exposure metadata; enter relative EV values",
                    ));
                }
                Ok((info.exposure_s * info.iso as f32 / info.f_number.powi(2)).log2())
            })
            .collect::<Result<Vec<_>>>()?
    };
    if ev.iter().any(|v| !v.is_finite() || v.abs() > 40.) {
        return Err(bad("Exposure EV must be finite and between -40 and 40"));
    }
    let min = ev.iter().copied().fold(f32::INFINITY, f32::min);
    let max = ev.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if max - min < 0.25 || max - min > 20. {
        return Err(bad("Select an exposure bracket spanning 0.25 to 20 stops"));
    }
    let mut sorted = ev.clone();
    sorted.sort_by(f32::total_cmp);
    let anchor = sorted[sorted.len() / 2];
    for value in &mut ev {
        *value -= anchor;
    }
    Ok(ev)
}
fn frame(
    path: &Path,
    wb: &mut Option<[f32; 4]>,
    camera: &mut Option<(String, String)>,
    cancel: &AtomicBool,
) -> Result<Frame> {
    check(cancel)?;
    if crate::raw::is_raw(path) {
        let source = crate::raw::RawSource::load(path)?;
        let identity = (source.metadata.make.clone(), source.metadata.model.clone());
        if camera.as_ref().is_some_and(|v| v != &identity) {
            return Err(bad("RAW HDR frames must use the same camera model"));
        }
        *camera = Some(identity);
        let balance = *wb.get_or_insert_with(|| {
            source
                .info
                .wb_coeffs
                .map(|v| if v.is_finite() && v > 0. { v } else { 1. })
        });
        source.linear_hdr(balance, cancel)
    } else {
        if !crate::photo_develop::supported(path) || crate::photo_develop::is_virtual(path) {
            return Err(bad("HDR brackets require online RAW or RGB photos"));
        }
        let raster = crate::import::decode(path)?.raster;
        if raster.width() as usize * raster.height() as usize > MAX_PIXELS {
            return Err(bad("HDR input exceeds 24 megapixels"));
        }
        let mut signal = Vec::new();
        let mut pixels = Vec::new();
        for p in raster.to_pixels() {
            if p[3] != 65535 {
                return Err(bad("HDR brackets must be opaque"));
            }
            let rgb = [p[0], p[1], p[2]].map(|v| v as f32 / 65535.);
            signal.push(rgb.into_iter().fold(0., f32::max));
            pixels.push(rgb);
        }
        Ok(Frame {
            image: FloatImage {
                width: raster.width(),
                height: raster.height(),
                pixels,
            },
            signal,
        })
    }
}
fn small_luma(image: &FloatImage) -> (usize, usize, Vec<f32>) {
    let image = image.resized(256);
    (
        image.width as usize,
        image.height as usize,
        image.pixels.into_iter().map(lum).collect(),
    )
}
// Exposure-normalized log-luminance search. Translation only, deliberately bounded.
fn align(
    reference: &FloatImage,
    other: &FloatImage,
    ratio: f32,
    cancel: &AtomicBool,
) -> Result<[i32; 2]> {
    let (w, h, a) = small_luma(reference);
    let (_, _, b) = small_luma(other);
    let radius = (w.min(h) / 8).clamp(1, 16) as i32;
    let mut best = (f64::INFINITY, [0, 0]);
    for dy in -radius..=radius {
        check(cancel)?;
        for dx in -radius..=radius {
            let mut error = 0.;
            let mut n = 0;
            for y in (radius as usize..h.saturating_sub(radius as usize)).step_by(2) {
                for x in (radius as usize..w.saturating_sub(radius as usize)).step_by(2) {
                    let av = a[y * w + x];
                    let bv = b[(y as i32 + dy) as usize * w + (x as i32 + dx) as usize] / ratio;
                    if av > 0.01 && bv > 0.01 && av < 0.8 && bv < 0.8 {
                        error += (av.ln() - bv.ln()).abs().min(1.) as f64;
                        n += 1;
                    }
                }
            }
            if n > 32 {
                let score = error / n as f64 + ((dx * dx + dy * dy) as f64) * 1e-8;
                if score < best.0 {
                    best = (score, [dx, dy]);
                }
            }
        }
    }
    if !best.0.is_finite() {
        return Err(bad(
            "Insufficient unclipped texture for auto alignment; disable Auto Align for tripod brackets",
        ));
    }
    Ok([
        (best.1[0] as f32 * reference.width as f32 / w as f32).round() as i32,
        (best.1[1] as f32 * reference.height as f32 / h as f32).round() as i32,
    ])
}
fn sample_index(w: u32, h: u32, x: u32, y: u32, offset: [i32; 2]) -> Option<usize> {
    let sx = x as i32 + offset[0];
    let sy = y as i32 + offset[1];
    (sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32)
        .then_some((sy.max(0) as usize * w as usize) + sx.max(0) as usize)
}
fn weight(signal: f32) -> f32 {
    if !(0.002..0.995).contains(&signal) {
        0.
    } else {
        signal.min(1. - signal).max(0.)
    }
}

fn warp_frame(source: Frame, matrix: &[f64; 9], cancel: &AtomicBool) -> Result<Frame> {
    let (w, h) = (source.image.width, source.image.height);
    let mut pixels = vec![[0.; 3]; source.image.pixels.len()];
    let mut signal = vec![0.; pixels.len()];
    for y in 0..h {
        check(cancel)?;
        for x in 0..w {
            let Some([sx, sy]) = crate::photo_registration::project(
                matrix,
                x as f64 / w as f64,
                y as f64 / h as f64,
            ) else {
                continue;
            };
            let (sx, sy) = ((sx * w as f64) as f32, (sy * h as f64) as f32);
            if sx < 0. || sy < 0. || sx > (w - 1) as f32 || sy > (h - 1) as f32 {
                continue;
            }
            let (ix, iy) = (sx as u32, sy as u32);
            let (fx, fy) = (sx - ix as f32, sy - iy as f32);
            let dest = (y * w + x) as usize;
            for (xx, yy, a) in [
                (ix, iy, (1. - fx) * (1. - fy)),
                ((ix + 1).min(w - 1), iy, fx * (1. - fy)),
                (ix, (iy + 1).min(h - 1), (1. - fx) * fy),
                ((ix + 1).min(w - 1), (iy + 1).min(h - 1), fx * fy),
            ] {
                let i = (yy * w + xx) as usize;
                for (dst, src) in pixels[dest].iter_mut().zip(source.image.pixels[i]) {
                    *dst += src * a;
                }
                signal[dest] += source.signal[i] * a;
            }
        }
    }
    Ok(Frame {
        image: FloatImage {
            width: w,
            height: h,
            pixels,
        },
        signal,
    })
}
#[expect(
    clippy::too_many_arguments,
    reason = "Keeps existing explicit workflow inputs together"
)]
fn accumulate(
    reference: &Frame,
    other: &Frame,
    ratio: f32,
    offset: [i32; 2],
    deghost: Deghost,
    sum: &mut [[f32; 4]],
    fallback: &mut [[f32; 4]],
    ghosts: &mut [u8],
    cancel: &AtomicBool,
) -> Result<()> {
    let (w, h) = (reference.image.width, reference.image.height);
    let threshold = match deghost {
        Deghost::None => f32::INFINITY,
        Deghost::Low => 1.,
        Deghost::Medium => 0.5,
        Deghost::High => 0.25,
    };
    for y in 0..h {
        check(cancel)?;
        for x in 0..w {
            let i = (y * w + x) as usize;
            let Some(j) = sample_index(w, h, x, y, offset) else {
                continue;
            };
            let signal = other.signal[j];
            let rgb = other.image.pixels[j].map(|v| v / ratio);
            let wgt = weight(signal);
            let reference_y = lum(reference.image.pixels[i]);
            let yy = lum(rgb);
            let moving = weight(reference.signal[i]) > 0.02
                && wgt > 0.02
                && (((yy + 0.001) / (reference_y + 0.001)).log2().abs() > threshold
                    || (0..3).any(|c| {
                        ((rgb[c] + 0.001) / (reference.image.pixels[i][c] + 0.001))
                            .log2()
                            .abs()
                            > threshold * 1.5
                    }));
            if moving {
                ghosts[i] = 255;
                continue;
            }
            // Prefer the least clipped input even if every exposure lies outside the weighting range.
            let score = (signal - 0.5).abs();
            if score < fallback[i][3] {
                fallback[i] = [rgb[0], rgb[1], rgb[2], score];
            }
            for c in 0..3 {
                sum[i][c] += rgb[c] * wgt;
            }
            sum[i][3] += wgt;
        }
    }
    Ok(())
}
/// Preview uses the same algorithm at reduced resolution; alignment/deghost details may differ.
pub fn merge(
    paths: &[PathBuf],
    options: &Options,
    preview: bool,
    cancel: &AtomicBool,
) -> Result<Merge> {
    if ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return Err(bad("Another HDR merge is running"));
    }
    let _guard = Guard;
    if !(2..=9).contains(&paths.len()) {
        return Err(bad("Select 2–9 exposure-bracketed photos"));
    }
    let paths = paths
        .iter()
        .map(|p| p.canonicalize().map_err(IoError::from))
        .collect::<Result<Vec<_>>>()?;
    let unique = paths.iter().collect::<std::collections::HashSet<_>>();
    if unique.len() != paths.len() {
        return Err(bad("HDR bracket contains duplicate originals"));
    }
    let raw = crate::raw::is_raw(&paths[0]);
    if paths.iter().any(|p| crate::raw::is_raw(p) != raw) {
        return Err(bad("Do not mix RAW and rendered RGB bracket files"));
    }
    let ev = exposures(&paths, options)?;
    let reference_index = ev.iter().position(|v| v.abs() < 1e-5).unwrap();
    let mut sources = Vec::new();
    for (path, exposure_ev) in paths.iter().zip(&ev) {
        check(cancel)?;
        sources.push(Source {
            path: path.clone(),
            sha256: crate::raw::source_digest(path)?,
            exposure_ev: *exposure_ev,
        });
    }
    let mut wb = None;
    let mut camera = None;
    let mut reference = frame(&paths[reference_index], &mut wb, &mut camera, cancel)?;
    validate_image(&reference.image)?;
    let dimensions = (reference.image.width, reference.image.height);
    let reduce = |frame: Frame| -> Frame {
        if !preview || frame.image.width.max(frame.image.height) <= 1400 {
            return frame;
        }
        let small = frame.image.resized(1400);
        let signal = (0..small.height)
            .flat_map(|y| {
                (0..small.width).map({
                    let frame = &frame;
                    move |x| {
                        frame.signal[((y as u64 * frame.image.height as u64 / small.height as u64)
                            * frame.image.width as u64
                            + x as u64 * frame.image.width as u64 / small.width as u64)
                            as usize]
                    }
                })
            })
            .collect();
        Frame {
            image: small,
            signal,
        }
    };
    reference = reduce(reference);
    let n = reference.image.pixels.len();
    let mut sum = vec![[0.; 4]; n];
    let mut fallback = vec![[0., 0., 0., f32::INFINITY]; n];
    let mut ghosts = vec![0; n];
    let mut offsets = vec![[0, 0]; paths.len()];
    let mut homographies = vec![crate::photo_registration::IDENTITY; paths.len()];
    accumulate(
        &reference,
        &reference,
        1.,
        [0, 0],
        Deghost::None,
        &mut sum,
        &mut fallback,
        &mut ghosts,
        cancel,
    )?;
    for (i, path) in paths.iter().enumerate() {
        if i == reference_index {
            continue;
        }
        check(cancel)?;
        let image = frame(path, &mut wb, &mut camera, cancel)?;
        if (image.image.width, image.image.height) != dimensions {
            return Err(bad("HDR bracket dimensions/orientation do not match"));
        }
        let mut image = reduce(image);
        let ratio = 2f32.powf(ev[i]);
        let offset = if options.align {
            if let Ok(matrix) =
                crate::photo_registration::register(&reference.image, &image.image, cancel)
            {
                homographies[i] = matrix;
                image = warp_frame(image, &matrix, cancel)?;
                [0, 0]
            } else {
                align(&reference.image, &image.image, ratio, cancel)?
            }
        } else {
            [0, 0]
        };
        offsets[i] = offset;
        accumulate(
            &reference,
            &image,
            ratio,
            offset,
            options.deghost,
            &mut sum,
            &mut fallback,
            &mut ghosts,
            cancel,
        )?;
    }
    // Cover motion boundaries as well as their centers, avoiding colored edge ghosts.
    let detected = ghosts.clone();
    for y in 0..reference.image.height as usize {
        for x in 0..reference.image.width as usize {
            let w = reference.image.width as usize;
            if detected[y * w + x] == 0
                && (y.saturating_sub(2)..=(y + 2).min(reference.image.height as usize - 1)).any(
                    |yy| {
                        (x.saturating_sub(2)..=(x + 2).min(w - 1))
                            .any(|xx| detected[yy * w + xx] > 0)
                    },
                )
            {
                ghosts[y * w + x] = 255;
            }
        }
    }
    let mut pixels = Vec::with_capacity(n);
    for i in 0..n {
        let rgb = if ghosts[i] > 0 && weight(reference.signal[i]) > 0.02 {
            reference.image.pixels[i]
        } else if sum[i][3] > 1e-8 {
            [0, 1, 2].map(|c| sum[i][c] / sum[i][3])
        } else {
            [fallback[i][0], fallback[i][1], fallback[i][2]]
        };
        pixels.push(rgb);
    }
    let image = FloatImage {
        width: reference.image.width,
        height: reference.image.height,
        pixels,
    };
    validate_image(&image)?;
    let log_mean = (image
        .pixels
        .iter()
        .map(|p| (lum(*p) + 1e-6).ln() as f64)
        .sum::<f64>()
        / n as f64)
        .exp() as f32;
    let display_exposure = if options.auto_tone {
        (0.22 / log_mean.max(1e-6)).log2().clamp(-10., 10.)
    } else {
        0.
    };
    for source in &sources {
        check(cancel)?;
        if crate::raw::source_digest(&source.path)? != source.sha256 {
            return Err(bad("A bracket original changed while merging"));
        }
    }
    let report = Report {
        panorama: false,
        sources,
        offsets,
        homographies,
        reference: reference_index,
        deghosted_pixels: ghosts.iter().filter(|v| **v > 0).count(),
        options: options.clone(),
        display_exposure,
        width: image.width,
        height: image.height,
    };
    Ok(Merge {
        image,
        ghosts,
        report,
    })
}
impl Merge {
    pub fn preview(&self, overlay: bool, cancel: &AtomicBool) -> Result<Raster> {
        let small = self.image.resized(1400);
        let raster = small.develop_display(
            &Default::default(),
            self.report.display_exposure,
            self.report.panorama,
            cancel,
        )?;
        if !overlay {
            return Ok(raster);
        }
        let mut pixels = raster.to_pixels();
        for (i, p) in pixels.iter_mut().enumerate() {
            let x = i as u64 % small.width as u64;
            let y = i as u64 / small.width as u64;
            let source = (y * self.image.height as u64 / small.height as u64)
                * self.image.width as u64
                + x * self.image.width as u64 / small.width as u64;
            if self.ghosts[source as usize] > 0 {
                p[0] = 45000;
                p[1] /= 2;
                p[2] /= 2;
            }
        }
        Ok(Raster::from_pixels(
            small.width,
            small.height,
            [0; 4],
            &pixels,
        ))
    }
    pub fn save(&self, path: &Path, cancel: &AtomicBool) -> Result<()> {
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff"))
        {
            return Err(bad("HDR output must be a new .tif or .tiff file"));
        }
        validate_image(&self.image)?;
        check(cancel)?;
        if path.exists() {
            return Err(bad("HDR output already exists; choose a new filename"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        {
            let mut encoder = tiff::encoder::TiffEncoder::new(temp.as_file_mut())
                .map_err(|e| bad(e.to_string()))?;
            let mut image = encoder
                .new_image::<tiff::encoder::colortype::RGB32Float>(
                    self.image.width,
                    self.image.height,
                )
                .map_err(|e| bad(e.to_string()))?;
            let description = format!(
                "{MARKER}{}",
                serde_json::to_string(&self.report).map_err(|e| bad(e.to_string()))?
            );
            image
                .encoder()
                .write_tag(tiff::tags::Tag::ImageDescription, description.as_str())
                .map_err(|e| bad(e.to_string()))?;
            image
                .encoder()
                .write_tag(tiff::tags::Tag::Software, "Emulsion HDR")
                .map_err(|e| bad(e.to_string()))?;
            let mut profile = moxcms::ColorProfile::new_srgb();
            profile.red_trc = Some(moxcms::ToneReprCurve::Parametric(vec![1.]));
            profile.green_trc = profile.red_trc.clone();
            profile.blue_trc = profile.red_trc.clone();
            let icc = profile.encode().map_err(|e| bad(e.to_string()))?;
            image
                .encoder()
                .write_tag(tiff::tags::Tag::IccProfile, icc.as_slice())
                .map_err(|e| bad(e.to_string()))?;
            image
                .write_data(self.image.pixels.as_flattened())
                .map_err(|e| bad(e.to_string()))?;
        }
        check(cancel)?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(path)
            .map_err(|e| bad(e.to_string()))?;
        Ok(())
    }
}
/// Only our tagged float TIFFs opt into the HDR source path. Other TIFFs keep their importer.
pub fn load(path: &Path) -> Result<Option<(FloatImage, Report)>> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff"))
    {
        return Ok(None);
    }
    let mut decoder =
        match tiff::decoder::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?)) {
            Ok(d) => d,
            Err(_) => return Ok(None),
        };
    let Ok(description) = decoder.get_tag_ascii_string(tiff::tags::Tag::ImageDescription) else {
        return Ok(None);
    };
    let Some(json) = description.strip_prefix(MARKER) else {
        return Ok(None);
    };
    if json.len() > 65536 {
        return Err(bad("HDR manifest too large"));
    }
    let report: Report = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
    let (width, height) = decoder.dimensions().map_err(|e| bad(e.to_string()))?;
    if width == 0
        || height == 0
        || width as usize * height as usize > MAX_PIXELS
        || report.width != width
        || report.height != height
        || !report.display_exposure.is_finite()
        || report.display_exposure.abs() > 10.
    {
        return Err(bad("Invalid HDR dimensions or display exposure"));
    }
    if decoder.colortype().map_err(|e| bad(e.to_string()))? != tiff::ColorType::RGB(32) {
        return Err(bad("HDR TIFF must contain RGB32 float samples"));
    }
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = MAX_PIXELS * 12;
    decoder = decoder.with_limits(limits);
    let data = decoder.read_image().map_err(|e| bad(e.to_string()))?;
    let tiff::decoder::DecodingResult::F32(data) = data else {
        return Err(bad("HDR TIFF samples are not floating point"));
    };
    if data.len() != width as usize * height as usize * 3 {
        return Err(bad("Invalid HDR sample count"));
    }
    let image = FloatImage {
        width,
        height,
        pixels: data.as_chunks::<3>().0.to_vec(),
    };
    validate_image(&image)?;
    Ok(Some((image, report)))
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn bracket(dir: &Path) -> Vec<PathBuf> {
        [-2f32, 0., 2.]
            .into_iter()
            .enumerate()
            .map(|(i, ev)| {
                let path = dir.join(format!("bracket-{i}.png"));
                let pixels: Vec<_> = (0..64 * 48)
                    .map(|p| {
                        let v = (0.01 + (p % 64) as f32 / 63. * 3.) * 2f32.powf(ev);
                        let v = (v.clamp(0., 1.) * 65535.).round() as u16;
                        [v, v, v, 65535]
                    })
                    .collect();
                let r = Raster::from_pixels(64, 48, [0; 4], &pixels);
                image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(64, 48, r.to_srgba16())
                    .unwrap()
                    .save(&path)
                    .unwrap();
                path
            })
            .collect()
    }
    #[test]
    fn hdr_recovers_highlights_roundtrips_and_never_overwrites_originals() {
        let _guard = TEST.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = bracket(dir.path());
        let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let cancel = AtomicBool::new(false);
        let options = Options {
            align: false,
            auto_tone: false,
            exposure_ev: Some(vec![-2., 0., 2.]),
            ..Default::default()
        };
        let merged = merge(&paths, &options, false, &cancel).unwrap();
        assert!(merged.image.pixels[63][0] > 2.9);
        let output = dir.path().join("merged.tif");
        merged.save(&output, &cancel).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        assert!(merged.save(&output, &cancel).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
        let (decoded, report) = load(&output).unwrap().unwrap();
        assert_eq!(decoded.pixels, merged.image.pixels);
        assert_eq!(report.sources.len(), 3);
        let source = crate::photo_develop::PhotoSource::load(&output).unwrap();
        assert_eq!(source.metadata.bits_per_sample, 32);
        let neutral = source.develop_with(&Default::default()).unwrap();
        let dim = source
            .develop_with(&crate::raw::DevelopParams {
                exposure: -2.,
                ..Default::default()
            })
            .unwrap();
        assert_ne!(neutral.get(63, 0), dim.get(63, 0));
        assert_eq!(
            neutral.to_srgba8(),
            merged.preview(false, &cancel).unwrap().to_srgba8()
        );
        assert!(crate::thumb::batch_thumbnail(&output, 64).is_ok());
        for (p, bytes) in paths.iter().zip(before) {
            assert_eq!(std::fs::read(p).unwrap(), bytes);
        }
    }
    #[test]
    fn translation_alignment_and_motion_rejection() {
        let image = FloatImage {
            width: 96,
            height: 96,
            pixels: (0..96 * 96)
                .map(|i| {
                    let v = 0.08 + ((i * 17 + i / 96 * 31) % 67) as f32 / 100.;
                    [v; 3]
                })
                .collect(),
        };
        let mut shifted = FloatImage {
            width: 96,
            height: 96,
            pixels: vec![[0.; 3]; 96 * 96],
        };
        for y in 0..96 {
            for x in 0..96 {
                if let Some(j) = sample_index(96, 96, x, y, [3, -2]) {
                    shifted.pixels[j] = image.pixels[(y * 96 + x) as usize].map(|v| v * 0.5);
                }
            }
        }
        assert_eq!(
            align(&image, &shifted, 0.5, &AtomicBool::new(false)).unwrap(),
            [3, -2]
        );
        let reference = Frame {
            image: FloatImage {
                width: 2,
                height: 1,
                pixels: vec![[0.2; 3]; 2],
            },
            signal: vec![0.2; 2],
        };
        let other = Frame {
            image: FloatImage {
                width: 2,
                height: 1,
                pixels: vec![[0.6; 3], [0.2; 3]],
            },
            signal: vec![0.6, 0.2],
        };
        let mut sum = vec![[0.; 4]; 2];
        let mut fallback = vec![[0., 0., 0., f32::INFINITY]; 2];
        let mut ghosts = vec![0; 2];
        accumulate(
            &reference,
            &other,
            1.,
            [0, 0],
            Deghost::Medium,
            &mut sum,
            &mut fallback,
            &mut ghosts,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(ghosts, [255, 0]);
        assert_eq!(sum[0][3], 0.);
        assert!(sum[1][3] > 0.);
    }
    #[test]
    fn invalid_brackets_and_cancel_do_not_publish() {
        let _guard = TEST.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = bracket(dir.path());
        let cancel = AtomicBool::new(false);
        let options = Options {
            align: false,
            exposure_ev: Some(vec![0.; 3]),
            ..Default::default()
        };
        assert!(merge(&paths, &options, false, &cancel).is_err());
        let options = Options {
            align: false,
            exposure_ev: Some(vec![-2., 0., 2.]),
            ..Default::default()
        };
        assert!(
            merge(
                &[paths[0].clone(), paths[0].clone()],
                &options,
                false,
                &cancel
            )
            .is_err()
        );
        let merged = merge(&paths, &options, true, &cancel).unwrap();
        cancel.store(true, Ordering::Relaxed);
        let path = dir.path().join("cancelled.tif");
        assert!(merged.save(&path, &cancel).is_err());
        assert!(!path.exists());
        assert!(merge(&paths, &options, false, &cancel).is_err());
    }
}

#[cfg(test)]
mod large_input_tests {
    use super::*;
    #[test]
    #[ignore = "Full-resolution memory and throughput acceptance check"]
    fn merges_more_than_twenty_four_megapixels() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<_> = (0..2)
            .map(|i| dir.path().join(format!("{i}.png")))
            .collect();
        for (i, path) in paths.iter().enumerate() {
            image::RgbImage::from_pixel(6000, 4200, image::Rgb([80 + i as u8 * 30; 3]))
                .save(path)
                .unwrap();
        }
        let started = std::time::Instant::now();
        let merged = merge(
            &paths,
            &Options {
                align: false,
                auto_tone: false,
                exposure_ev: Some(vec![0., 1.]),
                ..Default::default()
            },
            false,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!((merged.image.width, merged.image.height), (6000, 4200));
        assert!(merged.image.pixels.iter().flatten().all(|v| v.is_finite()));
        eprintln!("25.2 MP HDR: {:?}", started.elapsed());
    }
}
