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
}
pub struct PhotoSource {
    pub source: PathBuf,
    pub source_sha256: String,
    pub metadata: RawMetadata,
    pub info: RawInfo,
    pixels: Pixels,
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
    pub fn load(path: &Path) -> Result<Self> {
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
        let decoded = crate::import::decode(&source)?;
        let (width, height) = (decoded.raster.width(), decoded.raster.height());
        Ok(Self {
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
        let source = Self::load(path)?;
        if !source.source_sha256.eq_ignore_ascii_case(digest) {
            return Err(IoError::Manifest(
                "Photo original changed; reload before editing".into(),
            ));
        }
        Ok(source)
    }
    pub fn develop_with(&self, params: &DevelopParams) -> Result<Raster> {
        match &self.pixels {
            Pixels::Raw(raw) => raw.develop_with(params),
            Pixels::Rgb(rgb) => raw::develop_raster(rgb, params),
        }
    }
    pub fn auto_adjust(&self, params: &DevelopParams) -> Result<DevelopParams> {
        match &self.pixels {
            Pixels::Raw(raw) => raw.auto_adjust(params),
            Pixels::Rgb(rgb) => {
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
    if raw::is_raw(&reference.source) {
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
pub(crate) fn load_masks(params: &DevelopParams) -> Result<Vec<Option<emulsion_raster::Mask>>> {
    params
        .masks
        .iter()
        .map(|m| {
            let Some(digest) = m.bitmap.filter(|_| m.enabled) else {
                return Ok(None);
            };
            let path = mask_path(digest);
            let expected: String = digest.iter().map(|b| format!("{b:02x}")).collect();
            if raw::source_digest(&path)? != expected {
                return Err(IoError::Manifest("Local mask asset changed".into()));
            }
            let reader = image::ImageReader::open(&path)?.with_guessed_format()?;
            let (w, h) = reader.into_dimensions()?;
            crate::import::check_size(w, h)?;
            let gray = image::open(&path)?.into_luma8();
            Ok(Some(emulsion_raster::Mask::from_pixels(
                w,
                h,
                0,
                gray.as_raw(),
            )))
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
