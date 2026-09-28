//! One development source for camera RAW and rendered photographs.
use crate::{
    IoError, Result,
    raw::{self, DevelopParams, RawInfo, RawSource},
    raw_settings,
};
use emulsion_core::raw::RawMetadata;
use emulsion_raster::Raster;
use std::path::{Path, PathBuf};

enum Pixels {
    Raw(Box<RawSource>),
    Rgb(Raster),
    Hdr(crate::photo_hdr::FloatImage, f32, bool),
    Proxy(Raster),
}
pub struct PhotoSource {
    pub source: PathBuf,
    pub source_sha256: String,
    pub metadata: RawMetadata,
    pub info: RawInfo,
    pixels: Pixels,
    rgb_preview: std::sync::OnceLock<Raster>,
    wide_rgb: std::sync::OnceLock<Raster>,
    inspection: std::sync::Mutex<Option<Inspection>>,
}
static INSPECTION_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const INSPECTION_BUDGET: usize = 256 * 1024 * 1024;
struct Inspection {
    params: DevelopParams,
    raster: Raster,
    bytes: usize,
}
impl Drop for Inspection {
    fn drop(&mut self) {
        INSPECTION_BYTES.fetch_sub(self.bytes, std::sync::atomic::Ordering::Relaxed);
    }
}
pub fn supported(path: &Path) -> bool {
    is_virtual(path)
        || raw::is_raw(path)
        || path.extension().is_some_and(|e| {
            ["jpg", "jpeg", "tif", "tiff", "png", "webp"]
                .iter()
                .any(|s| e.eq_ignore_ascii_case(s))
        })
}
impl PhotoSource {
    pub fn supports_wide_gamut(&self) -> bool {
        matches!(self.pixels, Pixels::Raw(_) | Pixels::Rgb(_))
    }
    pub fn is_proxy(&self) -> bool {
        matches!(self.pixels, Pixels::Proxy(_))
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() && crate::photo_proxy::exists(path) {
            let (proxy, raster) = crate::photo_proxy::load(path)?;
            return Ok(Self {
                rgb_preview: Default::default(),
                wide_rgb: Default::default(),
                inspection: Default::default(),
                source: path.to_path_buf(),
                source_sha256: proxy.source_sha256,
                info: RawInfo {
                    width: proxy.width,
                    height: proxy.height,
                    ..Default::default()
                },
                metadata: proxy.metadata,
                pixels: Pixels::Proxy(raster),
            });
        }
        if is_virtual(path) {
            let copy = reference(path)?;
            let mut source = Self::load_verified(&copy.source, &copy.source_sha256)?;
            source.source = path.canonicalize()?;
            source.source_sha256 = raw::source_digest(path)?;
            return Ok(source);
        }
        if raw::is_raw(path) {
            let source = RawSource::load(path)?;
            return Ok(Self {
                rgb_preview: Default::default(),
                wide_rgb: Default::default(),
                inspection: Default::default(),
                source: source.source.clone(),
                source_sha256: source.source_sha256.clone(),
                metadata: source.metadata.clone(),
                info: source.info.clone(),
                pixels: Pixels::Raw(Box::new(source)),
            });
        }
        if !supported(path) {
            return Err(IoError::Unsupported(
                "Develop supports camera RAW, JPEG, TIFF, PNG and WebP".into(),
            ));
        }
        let source = path.canonicalize()?;
        let source_sha256 = raw::source_digest(&source)?;
        if let Some((image, report)) = crate::photo_hdr::load(&source)? {
            let (width, height) = (image.width, image.height);
            return Ok(Self {
                rgb_preview: Default::default(),
                wide_rgb: Default::default(),
                inspection: Default::default(),
                source,
                source_sha256,
                metadata: RawMetadata {
                    width,
                    height,
                    bits_per_sample: 32,
                    format: "HDR TIFF".into(),
                    ..Default::default()
                },
                info: RawInfo {
                    width,
                    height,
                    ..Default::default()
                },
                pixels: Pixels::Hdr(image, report.display_exposure, report.panorama),
            });
        }
        let decoded = crate::import::decode(&source)?;
        let (width, height) = (decoded.raster.width(), decoded.raster.height());
        Ok(Self {
            rgb_preview: Default::default(),
            wide_rgb: Default::default(),
            inspection: Default::default(),
            source,
            source_sha256,
            metadata: RawMetadata {
                width,
                height,
                bits_per_sample: decoded.depth as u32,
                ..Default::default()
            },
            info: RawInfo {
                width,
                height,
                ..Default::default()
            },
            pixels: Pixels::Rgb(decoded.raster),
        })
    }
    pub fn load_verified(path: &Path, digest: &str) -> Result<Self> {
        if !raw::source_digest(path)?.eq_ignore_ascii_case(digest) {
            return Err(IoError::Manifest(
                "Photo original SHA-256 changed; reload before editing".into(),
            ));
        }
        let source = Self::load(path)?;
        if !source.source_sha256.eq_ignore_ascii_case(digest) {
            return Err(IoError::Manifest(
                "Photo original SHA-256 changed; reload before editing".into(),
            ));
        }
        Ok(source)
    }
    pub fn validate_settings(&self, params: &DevelopParams) -> Result<()> {
        params.validate().map_err(|e| IoError::Manifest(e.into()))?;
        if matches!(self.pixels, Pixels::Rgb(_) | Pixels::Hdr(_, _, _))
            && (params.camera_profile.is_some()
                || params.sensor_noise_reduction > 0.
                || params.sensor_ai_denoise
                || params.highlight_reconstruction > 0.
                || params.wb_override.is_some())
        {
            return Err(IoError::Unsupported("Camera profile, sensor denoise, camera-channel white balance and highlight reconstruction require a RAW original".into()));
        }
        if let Some(digest) = params.camera_profile {
            let profile = crate::camera_profiles::load(&digest)?;
            if !profile.compatible(&self.metadata.make, &self.metadata.model) {
                return Err(IoError::Unsupported(
                    "Camera profile does not match this photo".into(),
                ));
            }
        }
        if let Some(digest) = params.local_edits {
            crate::develop_edits::load(&digest)?;
        }
        if let Some(digest) = params.depth_map {
            load_mask(&digest)?;
        }
        if params.sensor_ai_denoise
            && emulsion_ai::models::installed_for(emulsion_ai::models::Task::SensorDenoise)
                .is_none()
        {
            return Err(IoError::Unsupported(
                "Install RawNIND Bayer denoise in Models before enabling AI sensor denoise".into(),
            ));
        }
        Ok(())
    }
    /// Cache one developed frame under a global budget; panning extracts only the requested pixels.
    pub fn develop_region(
        &self,
        params: &DevelopParams,
        center: [f32; 2],
        side: u32,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Raster> {
        use std::sync::atomic::Ordering;
        if side == 0
            || side > 4096
            || center
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(IoError::Unsupported("Invalid inspection region".into()));
        }
        let mut cache = self.inspection.lock().unwrap_or_else(|e| e.into_inner());
        if cache.as_ref().is_some_and(|c| c.params != *params) {
            *cache = None;
        }
        let raster = if let Some(c) = cache.as_ref() {
            c.raster.clone()
        } else {
            let raster = self.develop_with_cancel(params, cancel)?;
            let bytes = raster.width() as usize * raster.height() as usize * 8;
            if INSPECTION_BYTES
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                    used.checked_add(bytes).filter(|n| *n <= INSPECTION_BUDGET)
                })
                .is_ok()
            {
                *cache = Some(Inspection {
                    params: *params,
                    raster: raster.clone(),
                    bytes,
                });
            }
            raster
        };
        if cancel.load(Ordering::Relaxed) {
            return Err(IoError::Unsupported("Inspection cancelled".into()));
        }
        let (w, h) = (raster.width().min(side), raster.height().min(side));
        let x = (center[0] * raster.width() as f32 - w as f32 * 0.5)
            .clamp(0., (raster.width() - w) as f32) as u32;
        let y = (center[1] * raster.height() as f32 - h as f32 * 0.5)
            .clamp(0., (raster.height() - h) as f32) as u32;
        let pixels = (0..h)
            .flat_map(|dy| (0..w).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| raster.get(x + dx, y + dy))
            .collect::<Vec<_>>();
        Ok(Raster::from_pixels(w, h, [0; 4], &pixels))
    }
    fn wide_rgb(&self) -> Result<&Raster> {
        if self.wide_rgb.get().is_none() {
            let path = if is_virtual(&self.source) {
                reference(&self.source)?.source
            } else {
                self.source.clone()
            };
            let expected = if is_virtual(&self.source) {
                reference(&self.source)?.source_sha256
            } else {
                self.source_sha256.clone()
            };
            if raw::source_digest(&path)? != expected {
                return Err(IoError::Manifest(
                    "Photo original SHA-256 changed; reload before editing".into(),
                ));
            }
            let image = crate::photo_wide::decode(&path)?;
            let _ = self.wide_rgb.set(image);
        }
        Ok(self.wide_rgb.get().unwrap())
    }
    pub fn develop_working(&self, params: &DevelopParams) -> Result<Raster> {
        self.validate_settings(params)?;
        match &self.pixels {
            Pixels::Raw(raw) => raw.develop_working(params),
            Pixels::Rgb(_) if params.wide_gamut => {
                raw::develop_wide_raster(self.wide_rgb()?, params, true)
            }
            _ => self.develop_with(params),
        }
    }
    pub fn develop_with_cancel(
        &self,
        params: &DevelopParams,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Raster> {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(IoError::Unsupported("Development cancelled".into()));
        }
        match &self.pixels {
            Pixels::Raw(raw) => raw.develop_with_cancel(params, cancel),
            Pixels::Hdr(image, exposure, panorama) => {
                image.develop_display(params, *exposure, *panorama, cancel)
            }
            _ => self.develop_with(params),
        }
    }
    pub fn develop_with(&self, params: &DevelopParams) -> Result<Raster> {
        self.validate_settings(params)?;
        match &self.pixels {
            Pixels::Raw(raw) => raw.develop_with(params),
            Pixels::Hdr(image, exposure, panorama) => image.develop_display(
                params,
                *exposure,
                *panorama,
                &std::sync::atomic::AtomicBool::new(false),
            ),
            Pixels::Rgb(_) if params.wide_gamut => {
                raw::develop_wide_raster(self.wide_rgb()?, params, false)
            }
            Pixels::Rgb(rgb) => raw::develop_raster(rgb, params),
            Pixels::Proxy(_) => Err(IoError::Unsupported(
                "Reconnect the verified original for full-quality rendering or export".into(),
            )),
        }
    }
    pub fn develop_preview(
        &self,
        params: &DevelopParams,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Raster> {
        match &self.pixels {
            Pixels::Raw(raw) => raw.develop_preview(params, cancel),
            Pixels::Hdr(image, exposure, panorama) => image
                .resized(1280)
                .develop_display(params, *exposure, *panorama, cancel),
            Pixels::Rgb(_) if params.wide_gamut => {
                let wide = self.wide_rgb()?;
                let scale = (1280. / wide.width().max(wide.height()) as f64).min(1.);
                let image = crate::photo_export::resize(
                    wide,
                    (wide.width() as f64 * scale).round().max(1.) as u32,
                    (wide.height() as f64 * scale).round().max(1.) as u32,
                )?;
                raw::develop_wide_raster(&image, params, false)
            }
            Pixels::Rgb(rgb) => {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(IoError::Unsupported("Development cancelled".into()));
                }
                if self.rgb_preview.get().is_none() {
                    let (w, h) = (rgb.width(), rgb.height());
                    let scale = (1280. / w.max(h) as f64).min(1.);
                    let small = if scale < 1. {
                        crate::photo_export::resize(
                            rgb,
                            (w as f64 * scale).round().max(1.) as u32,
                            (h as f64 * scale).round().max(1.) as u32,
                        )?
                    } else {
                        rgb.clone()
                    };
                    let _ = self.rgb_preview.set(small);
                }
                raw::develop_raster(self.rgb_preview.get().unwrap(), params)
            }
            Pixels::Proxy(rgb) => raw::develop_raster(
                rgb,
                &DevelopParams {
                    sensor_noise_reduction: 0.,
                    camera_profile: None,
                    wb_override: None,
                    wide_gamut: false,
                    ..*params
                },
            ),
        }
    }
    pub fn neutral_white_balance(
        &self,
        params: &DevelopParams,
        x: u32,
        y: u32,
    ) -> Result<DevelopParams> {
        if let Pixels::Raw(raw) = &self.pixels {
            return raw.neutral_white_balance(params, x, y);
        }
        let image = self.develop_with(params)?;
        if x >= image.width() || y >= image.height() {
            return Err(IoError::Unsupported(
                "Neutral sample is outside the image".into(),
            ));
        }
        let p = image.get(x, y);
        if p[..3].iter().any(|v| *v < 64 || *v > 65000) {
            return Err(IoError::Unsupported(
                "Choose an unclipped neutral midtone".into(),
            ));
        }
        let [r, g, b] = [p[0] as f32, p[1] as f32, p[2] as f32];
        Ok(DevelopParams {
            temperature: (params.temperature + (b / r).log2() / 1.4).clamp(-1., 1.),
            tint: (params.tint + (g / (r * b).sqrt()).log2() / 0.4).clamp(-1., 1.),
            ..*params
        })
    }
    pub fn auto_adjust(&self, params: &DevelopParams) -> Result<DevelopParams> {
        match &self.pixels {
            Pixels::Raw(raw) => raw.auto_adjust(params),
            Pixels::Hdr(image, display, _) => {
                let mean = (image
                    .pixels
                    .iter()
                    .map(|p| {
                        (p[0].max(0.) * 0.2126
                            + p[1].max(0.) * 0.7152
                            + p[2].max(0.) * 0.0722
                            + 1e-6)
                            .ln() as f64
                    })
                    .sum::<f64>()
                    / image.pixels.len() as f64)
                    .exp() as f32;
                Ok(DevelopParams {
                    exposure: ((0.22 / mean.max(1e-6)).log2() - display).clamp(-5., 5.),
                    ..*params
                })
            }
            Pixels::Rgb(rgb) | Pixels::Proxy(rgb) => {
                let pixels = rgb.to_pixels();
                let stride = pixels.len().div_ceil(65536).max(1);
                let mut levels: Vec<_> = pixels
                    .iter()
                    .step_by(stride)
                    .filter(|p| p[3] > 0)
                    .map(|p| *p[..3].iter().max().unwrap() as f32 / 65535.)
                    .collect();
                levels.sort_by(f32::total_cmp);
                let white = levels
                    .get(levels.len().saturating_sub(1) * 995 / 1000)
                    .copied()
                    .unwrap_or(0.);
                let mut next = *params;
                next.exposure = if white > 1e-6 {
                    (0.95 / white).log2().clamp(-5., 5.)
                } else {
                    0.
                };
                Ok(next)
            }
        }
    }
    pub fn save(&self, params: DevelopParams) -> Result<()> {
        raw_settings::save_photo_settings(&self.source, &self.source_sha256, params)
    }
}

pub fn open_saved(path: &Path) -> Result<emulsion_core::Document> {
    crate::open(path)
}

/// Full-quality Library export keeps RAW wide working pixels until encoding.
pub fn open_saved_working(
    path: &Path,
) -> Result<(emulsion_core::Document, crate::photo_color::Space)> {
    use crate::photo_color::Space;
    if !is_raw_photo(path) {
        return Ok((open_saved(path)?, Space::Srgb));
    }
    let source = PhotoSource::load(path)?;
    let params = raw_settings::adjacent_settings(&source.source, &source.source_sha256)?;
    let raster = source.develop_working(&params)?;
    let mut doc = emulsion_core::Document::new(raster.width(), raster.height());
    doc.source_depth = 16;
    doc.nodes.push(emulsion_core::Node::raster(
        1,
        "Developed photo",
        std::sync::Arc::new(raster),
        Default::default(),
    ));
    doc.next_id = 2;
    Ok((
        doc,
        if params.wide_gamut {
            Space::ProPhoto
        } else {
            Space::Srgb
        },
    ))
}

/// Resolve measured correction from installed Lensfun data and source EXIF.
pub fn matched_lens(path: &Path) -> Result<emulsion_core::raw::LensCorrection> {
    let exif = crate::exif::read(path)
        .ok_or_else(|| IoError::Unsupported("No camera/lens EXIF is available".into()))?;
    if !crate::lensfun::installed() {
        crate::lensfun::install(&|_, _| {}, &std::sync::atomic::AtomicBool::new(false))?;
    }
    let db = crate::lensfun::Database::shared()
        .ok_or_else(|| IoError::Unsupported("Could not load the Lensfun database".into()))?;
    let p = crate::lensfun::profile_for(
        &db,
        &exif.make,
        &exif.model,
        &exif.lens,
        exif.focal_mm,
        exif.f_number,
    )
    .ok_or_else(|| {
        IoError::Unsupported("No matching measured profile for this camera and lens".into())
    })?;
    Ok(emulsion_core::raw::LensCorrection {
        distortion: p.distortion.unwrap_or([0.; 3]),
        vignette: p.vignetting.unwrap_or([0.; 3]),
        tca: p.tca.unwrap_or([0., 0., 1., 0., 0., 1.]),
        scale: p.scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendered_photo_sidecars_preserve_original_depth_alpha_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("photo.png");
        let pixels = [
            120, 90, 60, 255, 90, 160, 70, 128, 20, 60, 190, 255, 100, 100, 100, 0,
        ];
        let original = crate::export::png8(2, 2, &pixels).unwrap();
        std::fs::write(&file, &original).unwrap();
        let source = PhotoSource::load(&file).unwrap();
        let baseline = source.develop_with(&DevelopParams::default()).unwrap();
        assert_eq!(
            baseline.to_pixels(),
            crate::import::decode(&file).unwrap().raster.to_pixels()
        );
        let params = DevelopParams {
            exposure: 1.,
            crop: [0., 0., 0.5, 1.],
            ..Default::default()
        };
        source.save(params).unwrap();
        raw_settings::save_snapshot(&file, &source.source_sha256, "Bright crop", params).unwrap();
        let next = DevelopParams {
            exposure: -0.5,
            ..params
        };
        source.save(next).unwrap();
        let (history, snapshots) =
            raw_settings::photo_history(&file, &source.source_sha256).unwrap();
        assert_eq!(history.last(), Some(&params));
        assert_eq!(snapshots["Bright crop"], params);
        let doc = crate::open(&file).unwrap();
        assert_eq!((doc.width, doc.height, doc.source_depth), (1, 2, 8));
        let rendered = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
        assert_eq!(
            rendered.to_pixels(),
            source.develop_with(&next).unwrap().to_pixels()
        );
        assert_eq!(std::fs::read(&file).unwrap(), original);
        assert!(raw_settings::photo_history(&file, &"b".repeat(64)).is_err());
    }
    #[test]
    fn rendered_adjustments_keep_premultiplied_alpha() {
        let source = Raster::solid(3, 3, [0.06, 0.03, 0.015, 0.25]);
        let params = DevelopParams {
            exposure: 1.,
            ..Default::default()
        };
        let edited = raw::develop_raster(&source, &params).unwrap();
        let pixel = edited.get(1, 1);
        let original = source.get(1, 1);
        assert_eq!(pixel[3], original[3]);
        assert!(pixel[0] > original[0]);
        assert!(pixel[..3].iter().all(|c| *c <= pixel[3]));
        let opaque = Raster::solid(3, 3, [0.24, 0.12, 0.06, 1.]);
        let expected = raw::develop_raster(&opaque, &params).unwrap().get(1, 1);
        for channel in 0..3 {
            assert!((pixel[channel] as f32 - expected[channel] as f32 * 0.25).abs() < 5.);
        }
    }
    #[test]
    fn hsl_grading_local_masks_and_geometry_change_pixels_not_other_settings() {
        let source = Raster::solid(16, 16, [0.2, 0.1, 0.05, 1.]);
        let base = raw::develop_raster(&source, &DevelopParams::default()).unwrap();
        let mut p = DevelopParams::default();
        p.hsl[0][1] = -1.;
        p.hsl[1][1] = -1.;
        let mixed = raw::develop_raster(&source, &p).unwrap();
        assert_ne!(mixed.get(8, 8), base.get(8, 8));
        p = DevelopParams::default();
        p.grading[1] = [240., 0.8, 0.];
        assert_ne!(
            raw::develop_raster(&source, &p).unwrap().get(8, 8),
            base.get(8, 8)
        );
        p = DevelopParams::default();
        p.masks[0].enabled = true;
        p.masks[0].exposure = 1.;
        let masked = raw::develop_raster(&source, &p).unwrap();
        assert!(masked.get(8, 8)[0] > base.get(8, 8)[0]);
        assert_eq!(masked.get(0, 0), base.get(0, 0));
        p = DevelopParams {
            crop: [0.25, 0.25, 0.75, 0.75],
            ..Default::default()
        };
        let cropped = raw::develop_raster(&source, &p).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (8, 8));
        assert_eq!(cropped.get(0, 0), base.get(4, 4));
        p = DevelopParams {
            straighten: 30.,
            ..Default::default()
        };
        let rotated = raw::develop_raster(&source, &p).unwrap();
        assert_eq!(rotated.get(0, 0)[3], 0);
        assert_eq!(rotated.get(8, 8)[3], 65535);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VirtualCopy {
    format: String,
    version: u32,
    pub(crate) source: PathBuf,
    pub(crate) source_sha256: String,
}
pub fn is_virtual(path: &Path) -> bool {
    path.extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("emuphoto"))
}
pub(crate) fn reference(path: &Path) -> Result<VirtualCopy> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(IoError::Manifest("Virtual copy reference too large".into()));
    }
    let copy: VirtualCopy =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    if copy.format != "emulsion-virtual-photo"
        || copy.version != 1
        || !copy.source.is_absolute()
        || is_virtual(&copy.source)
        || !supported(&copy.source)
    {
        return Err(IoError::Manifest("Invalid virtual photo reference".into()));
    }
    Ok(copy)
}
/// Resolve references for EXIF, publishing and original-file operations.
pub fn original_path(path: &Path) -> Result<PathBuf> {
    if is_virtual(path) {
        Ok(reference(path)?.source)
    } else {
        Ok(path.to_path_buf())
    }
}
pub fn is_raw_photo(path: &Path) -> bool {
    raw::is_raw(path) || (is_virtual(path) && reference(path).is_ok_and(|r| raw::is_raw(&r.source)))
}
pub fn create_virtual(source: &Path, params: DevelopParams, directory: &Path) -> Result<PathBuf> {
    params.validate().map_err(|e| IoError::Manifest(e.into()))?;
    let original = if is_virtual(source) {
        reference(source)?.source
    } else {
        source.canonicalize()?
    };
    let reference = VirtualCopy {
        format: "emulsion-virtual-photo".into(),
        version: 1,
        source_sha256: raw::source_digest(&original)?,
        source: original,
    };
    std::fs::create_dir_all(directory)?;
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    use std::io::Write;
    for i in 1..100000 {
        let path = directory.join(format!("{stem}-copy-{i}.emuphoto"));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        };
        let bytes =
            serde_json::to_vec_pretty(&reference).map_err(|e| IoError::Manifest(e.to_string()))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        raw_settings::save_photo_settings(&path, &raw::source_digest(&path)?, params)?;
        return Ok(path);
    }
    Err(IoError::Manifest(
        "Too many virtual copies with this name".into(),
    ))
}
pub fn open_virtual(path: &Path) -> Result<emulsion_core::Document> {
    let source = PhotoSource::load(path)?;
    let params = raw_settings::adjacent_settings(path, &source.source_sha256)?;
    let raster = source.develop_with(&params)?;
    let reference = reference(path)?;
    let mut doc = emulsion_core::Document::new(raster.width(), raster.height());
    doc.source_depth = source.metadata.bits_per_sample.clamp(8, 16) as u8;
    doc.nodes.push(emulsion_core::Node::raster(
        1,
        "Virtual copy",
        std::sync::Arc::new(raster),
        Default::default(),
    ));
    doc.next_id = 2;
    {
        doc.raw_originals.push(reference.source.clone());
        doc.raw = Some(emulsion_core::raw::RawDocument {
            schema_version: 1,
            node_id: 1,
            source: reference.source,
            source_sha256: reference.source_sha256,
            params,
            metadata: source.metadata,
        });
        doc.source_depth = 16;
    }
    Ok(doc)
}

/// Content-addressed mask assets keep recipes small and cannot name arbitrary files.
pub fn save_mask(mask: &emulsion_raster::Mask) -> Result<[u8; 32]> {
    use image::ImageEncoder;
    use sha2::{Digest, Sha256};
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes).write_image(
        &mask.to_pixels(),
        mask.width(),
        mask.height(),
        image::ExtendedColorType::L8,
    )?;
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&Sha256::digest(&bytes));
    let path = mask_path(digest);
    std::fs::create_dir_all(path.parent().unwrap())?;
    if !path.exists() {
        crate::write_atomic(&path, |file| {
            use std::io::Write;
            file.write_all(&bytes)?;
            Ok(())
        })?;
    }
    Ok(digest)
}
fn mask_path(digest: [u8; 32]) -> PathBuf {
    let name: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    crate::recent::data_dir()
        .join("develop-masks")
        .join(format!("{name}.png"))
}
pub(crate) fn load_mask(digest: &[u8; 32]) -> Result<emulsion_raster::Mask> {
    let path = mask_path(*digest);
    let expected: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if raw::source_digest(&path)? != expected {
        return Err(IoError::Manifest("Local mask asset changed".into()));
    }
    let (w, h) = image::ImageReader::open(&path)?
        .with_guessed_format()?
        .into_dimensions()?;
    crate::import::check_size(w, h)?;
    let gray = image::open(path)?.into_luma8();
    Ok(emulsion_raster::Mask::from_pixels(w, h, 0, gray.as_raw()))
}
pub(crate) fn load_masks(params: &DevelopParams) -> Result<Vec<Option<emulsion_raster::Mask>>> {
    params
        .masks
        .iter()
        .map(|m| {
            let Some(digest) = m.bitmap.filter(|_| m.enabled) else {
                return Ok(None);
            };
            load_mask(&digest).map(Some)
        })
        .collect()
}

#[cfg(test)]
mod virtual_tests {
    use super::*;
    #[test]
    fn virtual_copies_keep_independent_recipes_without_copying_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.png");
        let bytes = crate::export::png8(2, 1, &[80, 80, 80, 255, 120, 120, 120, 255]).unwrap();
        std::fs::write(&source, &bytes).unwrap();
        let params = DevelopParams {
            exposure: 1.,
            ..Default::default()
        };
        let copy = create_virtual(&source, params, &dir.path().join("copies")).unwrap();
        assert!(std::fs::metadata(&copy).unwrap().len() < 4096);
        let developed = PhotoSource::load(&copy).unwrap();
        assert_eq!(
            raw_settings::adjacent_settings(&copy, &developed.source_sha256).unwrap(),
            params
        );
        let baseline = PhotoSource::load(&source).unwrap();
        assert_eq!(
            raw_settings::adjacent_settings(&source, &baseline.source_sha256).unwrap(),
            DevelopParams::default()
        );
        assert_ne!(
            crate::open(&copy).unwrap().nodes,
            crate::open(&source).unwrap().nodes
        );
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        std::fs::write(&source, b"replaced").unwrap();
        assert!(PhotoSource::load(&copy).is_err());
    }
}

#[cfg(test)]
mod preview_cache_tests {
    use super::*;
    #[test]
    fn rendered_preview_is_bounded_but_full_output_keeps_original_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide.png");
        let bytes = crate::export::png8(2560, 2, &[100, 90, 80, 255].repeat(5120)).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let source = PhotoSource::load(&path).unwrap();
        let p = DevelopParams::default();
        let preview = source
            .develop_preview(&p, &std::sync::atomic::AtomicBool::new(false))
            .unwrap();
        assert_eq!((preview.width(), preview.height()), (1280, 1));
        assert!(source.rgb_preview.get().is_some());
        let changed = source
            .develop_preview(
                &DevelopParams { exposure: 0.5, ..p },
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_ne!(preview.to_pixels(), changed.to_pixels());
        assert_eq!(source.develop_with(&p).unwrap().width(), 2560);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
