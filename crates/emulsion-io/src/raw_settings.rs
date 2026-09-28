//! Portable Emulsion RAW recipes and camera defaults, not Adobe XMP.
//!
//! Reading settings never changes a document. Callers develop the returned
//! parameters and commit pixels and recipe together with `Command::DevelopRaw`.
use crate::{IoError, Result};
use emulsion_core::{
    Document,
    raw::{DevelopParams, RawMetadata},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

const VERSION: u32 = 1;
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RawSettingsGroup {
    #[default]
    All,
    WhiteBalance,
    Tone,
    Curve,
}

/// Copy one development group, leaving the other groups untouched.
pub fn merge_settings(
    mut target: DevelopParams,
    source: DevelopParams,
    group: RawSettingsGroup,
) -> DevelopParams {
    match group {
        RawSettingsGroup::All => return source,
        RawSettingsGroup::WhiteBalance => {
            target.kelvin = source.kelvin;
            target.temperature = source.temperature;
            target.tint = source.tint;
            target.wb_override = source.wb_override;
        }
        RawSettingsGroup::Tone => {
            target.process_version = source.process_version;
            target.camera_profile = source.camera_profile;
            target.wide_gamut = source.wide_gamut;
            target.calibration = source.calibration;
            target.shadow_tint = source.shadow_tint;
            target.global_grading = source.global_grading;
            target.grading_balance = source.grading_balance;
            target.grading_blending = source.grading_blending;
            target.sharpening_radius = source.sharpening_radius;
            target.sharpening_detail = source.sharpening_detail;
            target.sharpening_masking = source.sharpening_masking;
            target.luminance_detail = source.luminance_detail;
            target.luminance_contrast = source.luminance_contrast;
            target.color_noise_reduction = source.color_noise_reduction;
            target.color_noise_detail = source.color_noise_detail;
            target.color_noise_smoothness = source.color_noise_smoothness;
            target.hsl = source.hsl;
            target.grading = source.grading;
            target.exposure = source.exposure;
            target.highlights = source.highlights;
            target.shadows = source.shadows;
            target.whites = source.whites;
            target.blacks = source.blacks;
            target.black_point = source.black_point;
            target.brightness = source.brightness;
            target.contrast = source.contrast;
            target.saturation = source.saturation;
            target.vibrance = source.vibrance;
            target.texture = source.texture;
            target.clarity = source.clarity;
            target.dehaze = source.dehaze;
            target.vignette = source.vignette;
            target.sharpening = source.sharpening;
            target.noise_reduction = source.noise_reduction;
            target.sensor_noise_reduction = source.sensor_noise_reduction;
        }
        RawSettingsGroup::Curve => {
            target.process_version = source.process_version;
            target.parametric = source.parametric;
            target.parametric_splits = source.parametric_splits;
            target.point_curves = source.point_curves;
            target.tone_curve = source.tone_curve;
            target.smooth_curve = source.smooth_curve;
        }
    }
    target
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFile {
    format: String,
    version: u32,
    params: DevelopParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera: Option<CameraKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    history: Vec<DevelopParams>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    snapshots: std::collections::BTreeMap<String, DevelopParams>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraKey {
    make: String,
    model: String,
}

fn invalid(message: impl Into<String>) -> IoError {
    IoError::Manifest(format!("RAW settings: {}", message.into()))
}

fn record(format: &str, params: DevelopParams) -> Result<SettingsFile> {
    params.validate().map_err(invalid)?;
    Ok(SettingsFile {
        format: format.into(),
        version: VERSION,
        params,
        source_sha256: None,
        camera: None,
        history: Vec::new(),
        snapshots: Default::default(),
    })
}

fn read(path: &Path, format: &str) -> Result<SettingsFile> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("file exceeds the 4 MiB limit"));
    }
    let saved: SettingsFile = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    if saved.format != format {
        return Err(invalid(format!(
            "expected {format}; Adobe XMP is not supported"
        )));
    }
    if saved.version != VERSION {
        return Err(invalid(format!(
            "unsupported version {}; update Emulsion",
            saved.version
        )));
    }
    saved.params.validate().map_err(invalid)?;
    if saved.history.len() > 100 || saved.snapshots.len() > 100 {
        return Err(invalid("Too many history entries or snapshots"));
    }
    for p in saved.history.iter().chain(saved.snapshots.values()) {
        p.validate().map_err(invalid)?;
    }
    Ok(saved)
}

/// Refuse to replace unrelated content even when its name ends in `.json`.
fn write(path: &Path, saved: &SettingsFile) -> Result<()> {
    if !path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("json"))
    {
        return Err(invalid(
            "choose a .json settings file; original images cannot be overwritten",
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(invalid("settings destination must be a regular file"));
            }
            read(path, &saved.format)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let bytes = serde_json::to_vec_pretty(saved).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("settings exceed the 4 MiB limit"));
    }
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Exclusive creation protects originals even if a predictable temporary
    // filename has already been occupied by a file or symbolic link.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (temp, mut file) = loop {
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = dir.join(format!(".emulsion-raw-{}-{seq}.tmp", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

/// Save a fingerprint-bound recipe. This does not modify the RAW original.
pub fn save_sidecar(doc: &Document, path: &Path) -> Result<()> {
    crate::ora::ensure_not_raw_original(doc, path)?;
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| invalid("document has no editable RAW source"))?;
    raw.validate().map_err(invalid)?;
    let mut saved = record("emulsion-raw-sidecar", raw.params)?;
    saved.source_sha256 = Some(raw.source_sha256.to_ascii_lowercase());
    write(path, &saved)
}

/// Append to the original filename, retaining its camera-file extension.
pub fn suggested_sidecar_path(doc: &Document) -> Result<PathBuf> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| invalid("document has no editable RAW source"))?;
    sidecar_path(&raw.source)
}

/// The automatically discovered recipe beside an original camera file.
pub fn sidecar_path(source: &Path) -> Result<PathBuf> {
    let managed = managed_sidecar_path(source);
    if managed.is_file() {
        return Ok(managed);
    }
    adjacent_sidecar_path(source)
}
/// Stable application-managed location for read-only media. The document still
/// verifies the original content fingerprint before any settings are applied.
pub fn managed_sidecar_path(source: &Path) -> PathBuf {
    let identity = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let digest = Sha256::digest(identity.as_os_str().as_encoded_bytes());
    crate::recent::data_dir()
        .join("photo-sidecars")
        .join(format!(
            "{}.json",
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
}
fn write_photo(source: &Path, path: &Path, saved: &SettingsFile) -> Result<()> {
    if !source.exists() {
        let target = managed_sidecar_path(source);
        std::fs::create_dir_all(target.parent().unwrap())?;
        return write(&target, saved);
    }
    match write(path, saved) {
        Err(IoError::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            let managed = managed_sidecar_path(source);
            std::fs::create_dir_all(managed.parent().unwrap())?;
            write(&managed, saved)
        }
        result => result,
    }
}
pub(crate) fn make_managed(source: &Path, digest: &str) -> Result<()> {
    let current = sidecar_path(source)?;
    let saved = if current.exists() {
        load_sidecar_verified(&current, digest)?;
        read(&current, "emulsion-raw-sidecar")?
    } else {
        let mut r = record("emulsion-raw-sidecar", DevelopParams::default())?;
        r.source_sha256 = Some(digest.into());
        r
    };
    let target = managed_sidecar_path(source);
    std::fs::create_dir_all(target.parent().unwrap())?;
    write(&target, &saved)
}
fn adjacent_sidecar_path(source: &Path) -> Result<PathBuf> {
    let mut name = source
        .file_name()
        .ok_or_else(|| invalid("RAW source has no filename"))?
        .to_os_string();
    name.push(".emulsion-raw.json");
    Ok(source.with_file_name(name))
}

/// Load only settings whose fingerprint matches this document's linked RAW.
/// The subsequent development verifies the actual original bytes separately.
pub fn load_sidecar(doc: &Document, path: &Path) -> Result<DevelopParams> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| invalid("document has no editable RAW source"))?;
    raw.validate().map_err(invalid)?;
    load_sidecar_verified(path, &raw.source_sha256)
}

pub(crate) fn load_sidecar_verified(path: &Path, digest: &str) -> Result<DevelopParams> {
    let saved = read(path, "emulsion-raw-sidecar")?;
    if !saved
        .source_sha256
        .as_ref()
        .is_some_and(|saved_digest| saved_digest.eq_ignore_ascii_case(digest))
    {
        return Err(invalid(
            "this sidecar belongs to a different RAW original (SHA-256 mismatch)",
        ));
    }
    Ok(saved.params)
}

/// Carry a verified recipe, including history and snapshots, to a relinked photo.
/// Existing destination recipes must agree; neither source recipe is removed.
pub(crate) fn relink_sidecar(old: &Path, new: &Path, digest: &str) -> Result<()> {
    let old = sidecar_path(old)?;
    let new = sidecar_path(new)?;
    if old == new || !old.try_exists()? {
        return Ok(());
    }
    load_sidecar_verified(&old, digest)?;
    let saved = read(&old, "emulsion-raw-sidecar")?;
    let bytes = serde_json::to_vec_pretty(&saved).map_err(|e| invalid(e.to_string()))?;
    if new.try_exists()? {
        load_sidecar_verified(&new, digest)?;
        let existing = read(&new, "emulsion-raw-sidecar")?;
        if serde_json::to_value(existing).map_err(|e| invalid(e.to_string()))?
            != serde_json::to_value(saved).map_err(|e| invalid(e.to_string()))?
        {
            return Err(invalid(
                "replacement already has different saved edits; keep both recipes and resolve them before relinking",
            ));
        }
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&new)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&new);
        return Err(error.into());
    }
    Ok(())
}

/// Missing means an unedited original; any present but unreadable recipe is an
/// error, never an excuse to silently discard previously saved adjustments.
pub fn adjacent_settings(source: &Path, digest: &str) -> Result<DevelopParams> {
    let path = sidecar_path(source)?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DevelopParams::default());
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    load_sidecar_verified(&path, digest).map_err(|error| {
        invalid(format!(
            "could not restore {}: {error}. Restore a valid matching sidecar, or move it aside to open the original without saved edits",
            path.display()
        ))
    })
}

/// Persist library development with the same fingerprint-bound format as Photo.
pub fn save_source_settings(source: &crate::raw::RawSource, params: DevelopParams) -> Result<()> {
    save_photo_settings(&source.source, &source.source_sha256, params)
}

pub(crate) fn original_layer_name(source: &Path, model: &str) -> String {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "RAW".into());
    if model.is_empty() {
        stem
    } else {
        format!("{stem} ({model})")
    }
}

/// Whether an intact RAW recipe represents this entire document. Pixel edits
/// must use core commands, which detach the RAW link. This intentionally rejects
/// even non-rendering project edits (names, locks, guides, selection) that a
/// recipe cannot persist. It does not include undo history or editor UI state.
pub fn sidecar_only(doc: &Document) -> bool {
    let Some(raw) = doc.raw.as_ref() else {
        return false;
    };
    let [node] = doc.nodes.as_slice() else {
        return false;
    };
    let emulsion_core::NodeKind::Raster { raster, placement } = &node.kind else {
        return false;
    };
    if raw.validate().is_err()
        || node.id != raw.node_id
        || *placement != Default::default()
        || (doc.width, doc.height) != (raster.width(), raster.height())
        || doc.source_depth != 16
        || doc.selection.is_some()
        || !doc.guides.is_empty()
        || doc.resolution != 72.0
        || doc.global_light != Default::default()
        || doc.blend_space != emulsion_raster::blend::BlendSpace::Linear
        || doc.raw_originals.as_slice() != std::slice::from_ref(&raw.source)
    {
        return false;
    }
    let expected = emulsion_core::Node::raster(
        node.id,
        original_layer_name(&raw.source, &raw.metadata.model),
        raster.clone(),
        Default::default(),
    );
    *node == expected
}

/// Save reusable development settings, without binding them to a source image.
pub fn save_preset(params: DevelopParams, path: &Path) -> Result<()> {
    if params.wb_override.is_some() {
        return Err(invalid(
            "sampled white balance needs camera identity; save a document preset instead",
        ));
    }
    write(path, &record("emulsion-raw-preset", params)?)
}

/// Save a document's preset while also protecting every retained original path.
pub fn save_document_preset(doc: &Document, path: &Path) -> Result<()> {
    crate::ora::ensure_not_raw_original(doc, path)?;
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| invalid("document has no editable RAW source"))?;
    raw.validate().map_err(invalid)?;
    let mut saved = record("emulsion-raw-preset", raw.params)?;
    if raw.params.wb_override.is_some() {
        saved.camera = Some(camera_key(&raw.metadata)?);
    }
    write(path, &saved)
}

pub fn load_preset(path: &Path) -> Result<DevelopParams> {
    let saved = read(path, "emulsion-raw-preset")?;
    if saved.source_sha256.is_some() || saved.camera.is_some() || saved.params.wb_override.is_some()
    {
        return Err(invalid(
            "preset unexpectedly contains a source or camera binding",
        ));
    }
    Ok(saved.params)
}

/// Load a preset for a document, protecting camera-specific sampled gains.
/// Tone/curve-only copies do not require the white-balance camera to match.
pub fn load_document_preset(
    doc: &Document,
    path: &Path,
    group: RawSettingsGroup,
) -> Result<DevelopParams> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| invalid("document has no editable RAW source"))?;
    let saved = read(path, "emulsion-raw-preset")?;
    if saved.source_sha256.is_some() {
        return Err(invalid("a preset cannot be source-bound"));
    }
    if saved.params.wb_override.is_some()
        && matches!(
            group,
            RawSettingsGroup::All | RawSettingsGroup::WhiteBalance
        )
        && saved.camera.as_ref() != Some(&camera_key(&raw.metadata)?)
    {
        return Err(invalid(
            "sampled white balance belongs to another camera; apply Tone or Curve only",
        ));
    }
    Ok(saved.params)
}

fn camera_key(metadata: &RawMetadata) -> Result<CameraKey> {
    fn normalize(value: &str) -> String {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
    let key = CameraKey {
        make: normalize(&metadata.make),
        model: normalize(&metadata.model),
    };
    if key.make.is_empty()
        || key.model.is_empty()
        || key.make == "unknown"
        || key.model == "unknown"
    {
        return Err(invalid(
            "camera make and model are required for camera defaults",
        ));
    }
    Ok(key)
}

fn camera_path(root: &Path, key: &CameraKey) -> PathBuf {
    // Length prefixes disambiguate make/model boundaries; the digest also
    // prevents camera names from becoming paths on any host platform.
    let mut hash = Sha256::new();
    hash.update((key.make.len() as u64).to_le_bytes());
    hash.update(key.make.as_bytes());
    hash.update(key.model.as_bytes());
    let name: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    root.join(format!("{name}.json"))
}

fn defaults_dir() -> PathBuf {
    crate::recent::data_dir().join("raw-camera-defaults")
}

/// Persist defaults for exactly this normalized camera make/model.
/// Applying them is explicit: importing an image never silently changes them.
pub fn save_camera_defaults(metadata: &RawMetadata, params: DevelopParams) -> Result<()> {
    save_camera_defaults_in(&defaults_dir(), metadata, params)
}

fn save_camera_defaults_in(
    root: &Path,
    metadata: &RawMetadata,
    params: DevelopParams,
) -> Result<()> {
    let key = camera_key(metadata)?;
    let path = camera_path(root, &key);
    let mut saved = record("emulsion-raw-camera-defaults", params)?;
    saved.camera = Some(key);
    std::fs::create_dir_all(root)?;
    write(&path, &saved)
}

pub fn camera_defaults(metadata: &RawMetadata) -> Result<Option<DevelopParams>> {
    camera_defaults_in(&defaults_dir(), metadata)
}

fn camera_defaults_in(root: &Path, metadata: &RawMetadata) -> Result<Option<DevelopParams>> {
    let key = camera_key(metadata)?;
    match read(&camera_path(root, &key), "emulsion-raw-camera-defaults") {
        Ok(saved) if saved.camera.as_ref() == Some(&key) && saved.source_sha256.is_none() => {
            Ok(Some(saved.params))
        }
        Ok(_) => Err(invalid("camera-defaults identity mismatch")),
        Err(IoError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remove only this camera's saved defaults; future explicit application uses
/// the built-in as-shot settings. Existing document recipes remain unchanged.
pub fn reset_camera_defaults(metadata: &RawMetadata) -> Result<()> {
    reset_camera_defaults_in(&defaults_dir(), metadata)
}

fn reset_camera_defaults_in(root: &Path, metadata: &RawMetadata) -> Result<()> {
    if camera_defaults_in(root, metadata)?.is_some() {
        std::fs::remove_file(camera_path(root, &camera_key(metadata)?))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::raw::RawDocument;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "emulsion-raw-settings-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn document(dir: &Temp) -> Document {
        let source = dir.path("original.dng");
        std::fs::write(&source, b"original RAW bytes").unwrap();
        let mut doc = Document::new(1, 1);
        doc.raw = Some(RawDocument {
            schema_version: 1,
            node_id: 1,
            source: source.clone(),
            source_sha256: crate::raw::source_digest(&source).unwrap(),
            params: DevelopParams {
                exposure: 0.75,
                contrast: 0.2,
                ..Default::default()
            },
            metadata: RawMetadata::default(),
        });
        doc.raw_originals.push(source);
        doc
    }

    #[test]
    fn sidecar_roundtrip_binds_source_and_preserves_original() {
        let dir = Temp::new();
        let mut doc = document(&dir);
        let source = doc.raw.as_ref().unwrap().source.clone();
        let path = suggested_sidecar_path(&doc).unwrap();
        assert_eq!(path, dir.path("original.dng.emulsion-raw.json"));
        save_sidecar(&doc, &path).unwrap();
        assert_eq!(
            load_sidecar(&doc, &path).unwrap(),
            doc.raw.as_ref().unwrap().params
        );
        doc.raw.as_mut().unwrap().params.exposure = -1.0;
        save_sidecar(&doc, &path).unwrap();
        assert_eq!(load_sidecar(&doc, &path).unwrap().exposure, -1.0);
        doc.raw.as_mut().unwrap().source_sha256 = "f".repeat(64);
        assert!(
            load_sidecar(&doc, &path)
                .unwrap_err()
                .to_string()
                .contains("SHA-256")
        );
        assert!(save_sidecar(&doc, &source).is_err());
        assert!(save_document_preset(&doc, &source).is_err());
        assert_eq!(std::fs::read(source).unwrap(), b"original RAW bytes");
    }

    #[test]
    fn settings_reject_corrupt_new_version_invalid_and_oversized_without_overwriting() {
        let dir = Temp::new();
        let path = dir.path("preset.json");
        assert!(load_preset(&path).is_err());
        std::fs::write(&path, b"not settings or a renamed original").unwrap();
        assert!(load_preset(&path).is_err());
        assert!(save_preset(Default::default(), &path).is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"not settings or a renamed original"
        );
        let mut saved = record("emulsion-raw-preset", Default::default()).unwrap();
        saved.version = 99;
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(
            load_preset(&path)
                .unwrap_err()
                .to_string()
                .contains("version 99")
        );
        saved.version = VERSION;
        saved.params.contrast = 2.0;
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(load_preset(&path).is_err());
        std::fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(
            load_preset(&path)
                .unwrap_err()
                .to_string()
                .contains("4 MiB")
        );
        assert!(
            save_preset(
                DevelopParams {
                    exposure: f32::NAN,
                    ..Default::default()
                },
                &dir.path("invalid.json")
            )
            .is_err()
        );
        assert!(!dir.path("invalid.json").exists());
    }

    #[test]
    fn selective_copy_leaves_unselected_groups_unchanged() {
        let target = DevelopParams {
            temperature: -0.3,
            tint: 0.2,
            ..Default::default()
        };
        let source = DevelopParams {
            exposure: 1.0,
            contrast: 0.2,
            saturation: -0.1,
            temperature: 0.7,
            wb_override: Some([2., 1., 1.5, 1.]),
            tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
            ..Default::default()
        };
        let tone = merge_settings(target, source, RawSettingsGroup::Tone);
        assert_eq!(tone.exposure, source.exposure);
        assert_eq!(tone.contrast, source.contrast);
        assert_eq!(tone.saturation, source.saturation);
        assert_eq!(tone.temperature, target.temperature);
        assert_eq!(tone.tint, target.tint);
        assert_eq!(tone.wb_override, target.wb_override);
        assert_eq!(tone.tone_curve, target.tone_curve);
        let wb = merge_settings(target, source, RawSettingsGroup::WhiteBalance);
        assert_eq!(wb.wb_override, source.wb_override);
        assert_eq!(wb.temperature, source.temperature);
        assert_eq!(wb.exposure, target.exposure);
        let curve = merge_settings(target, source, RawSettingsGroup::Curve);
        assert_eq!(curve.tone_curve, source.tone_curve);
        assert_eq!(curve.temperature, target.temperature);
        assert_eq!(
            merge_settings(target, source, RawSettingsGroup::All),
            source
        );
    }

    #[test]
    fn presets_are_portable_but_never_accept_sidecars_or_xmp() {
        let dir = Temp::new();
        let doc = document(&dir);
        let path = dir.path("preset.json");
        save_preset(doc.raw.as_ref().unwrap().params, &path).unwrap();
        assert_eq!(
            load_preset(&path).unwrap(),
            doc.raw.as_ref().unwrap().params
        );
        assert!(load_sidecar(&doc, &path).is_err());
        assert!(save_sidecar(&doc, &path).is_err());
        assert!(save_preset(Default::default(), &dir.path("settings.xmp")).is_err());
    }

    #[test]
    fn sampled_white_balance_presets_require_matching_camera_or_tone_only() {
        let dir = Temp::new();
        let mut doc = document(&dir);
        let raw = doc.raw.as_mut().unwrap();
        raw.metadata.make = "Canon".into();
        raw.metadata.model = "R5".into();
        raw.params.wb_override = Some([2.0, 1.0, 1.5, 1.0]);
        let path = dir.path("sampled.json");
        assert!(save_preset(raw.params, &path).is_err());
        save_document_preset(&doc, &path).unwrap();
        assert!(load_preset(&path).is_err());
        assert_eq!(
            load_document_preset(&doc, &path, RawSettingsGroup::All).unwrap(),
            doc.raw.as_ref().unwrap().params
        );
        doc.raw.as_mut().unwrap().metadata.model = "R6".into();
        assert!(load_document_preset(&doc, &path, RawSettingsGroup::All).is_err());
        assert!(load_document_preset(&doc, &path, RawSettingsGroup::WhiteBalance).is_err());
        assert!(load_document_preset(&doc, &path, RawSettingsGroup::Tone).is_ok());
        assert!(load_document_preset(&doc, &path, RawSettingsGroup::Curve).is_ok());
    }

    #[test]
    fn camera_defaults_are_exact_normalized_model_scoped_and_resettable() {
        let dir = Temp::new();
        let metadata = RawMetadata {
            make: " Canon ".into(),
            model: "EOS  R5".into(),
            ..Default::default()
        };
        let equivalent = RawMetadata {
            make: "canon".into(),
            model: "eos r5".into(),
            ..Default::default()
        };
        let other = RawMetadata {
            model: "EOS R5 II".into(),
            ..metadata.clone()
        };
        assert_eq!(camera_defaults_in(&dir.0, &metadata).unwrap(), None);
        let params = DevelopParams {
            exposure: 0.5,
            ..Default::default()
        };
        save_camera_defaults_in(&dir.0, &metadata, params).unwrap();
        assert_eq!(
            camera_defaults_in(&dir.0, &equivalent).unwrap(),
            Some(params)
        );
        assert_eq!(camera_defaults_in(&dir.0, &other).unwrap(), None);
        let hostile = RawMetadata {
            make: "../bad".into(),
            model: "../../camera".into(),
            ..Default::default()
        };
        let safe = camera_path(&dir.0, &camera_key(&hostile).unwrap());
        assert_eq!(safe.parent(), Some(dir.0.as_path()));
        assert_eq!(safe.file_name().unwrap().to_string_lossy().len(), 69);
        reset_camera_defaults_in(&dir.0, &metadata).unwrap();
        assert_eq!(camera_defaults_in(&dir.0, &metadata).unwrap(), None);
        reset_camera_defaults_in(&dir.0, &metadata).unwrap();
        assert!(save_camera_defaults_in(&dir.0, &RawMetadata::default(), params).is_err());
    }
}

/// Fingerprint-bound sidecars also serve nondestructive rendered-photo development.
pub fn save_photo_settings(source: &Path, digest: &str, params: DevelopParams) -> Result<()> {
    let mut saved = record("emulsion-raw-sidecar", params)?;
    saved.source_sha256 = Some(digest.to_ascii_lowercase());
    let path = sidecar_path(source)?;
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {
            let previous = read(&path, "emulsion-raw-sidecar")?;
            if !previous
                .source_sha256
                .as_ref()
                .is_some_and(|d| d.eq_ignore_ascii_case(digest))
            {
                return Err(invalid("Original changed before save"));
            }
            saved.history = previous.history;
            saved.snapshots = previous.snapshots;
            if previous.params != params {
                saved.history.push(previous.params);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if params != DevelopParams::default() {
                saved.history.push(DevelopParams::default());
            }
        }
        Err(e) => return Err(e.into()),
    }
    if saved.history.len() > 100 {
        saved.history.drain(..saved.history.len() - 100);
    }
    write_photo(source, &path, &saved)
}

pub fn photo_history(
    source: &Path,
    digest: &str,
) -> Result<(
    Vec<DevelopParams>,
    std::collections::BTreeMap<String, DevelopParams>,
)> {
    let path = sidecar_path(source)?;
    if !path.exists() {
        return Ok(Default::default());
    }
    load_sidecar_verified(&path, digest)?;
    let saved = read(&path, "emulsion-raw-sidecar")?;
    Ok((saved.history, saved.snapshots))
}
pub fn save_snapshot(source: &Path, digest: &str, name: &str, params: DevelopParams) -> Result<()> {
    if name.trim().is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
        return Err(invalid("Snapshot name must be 1–200 characters"));
    }
    let path = sidecar_path(source)?;
    let mut saved = if path.exists() {
        load_sidecar_verified(&path, digest)?;
        read(&path, "emulsion-raw-sidecar")?
    } else {
        let mut saved = record("emulsion-raw-sidecar", DevelopParams::default())?;
        saved.source_sha256 = Some(digest.into());
        saved
    };
    if saved.snapshots.len() >= 100 && !saved.snapshots.contains_key(name) {
        return Err(invalid("Maximum 100 snapshots per photo"));
    }
    params.validate().map_err(invalid)?;
    saved.snapshots.insert(name.into(), params);
    write_photo(source, &path, &saved)
}

pub(crate) fn rebind_bytes(path: &Path, old: &str, new: &str) -> Result<Vec<u8>> {
    load_sidecar_verified(path, old)?;
    let mut saved = read(path, "emulsion-raw-sidecar")?;
    saved.source_sha256 = Some(new.into());
    serde_json::to_vec_pretty(&saved).map_err(|e| invalid(e.to_string()))
}

/// Import translated history into a new sidecar only; never replace existing work.
pub(crate) fn import_history(
    source: &Path,
    digest: &str,
    states: &[(String, DevelopParams)],
) -> Result<()> {
    let Some((_, latest)) = states.last() else {
        return Ok(());
    };
    let mut saved = record("emulsion-raw-sidecar", *latest)?;
    saved.source_sha256 = Some(digest.into());
    saved.history = states
        .iter()
        .rev()
        .skip(1)
        .take(100)
        .map(|(_, p)| *p)
        .collect();
    saved.history.reverse();
    for (index, (name, params)) in states.iter().rev().take(100).enumerate() {
        params.validate().map_err(invalid)?;
        saved.snapshots.insert(
            format!(
                "{} · {}",
                states.len() - index,
                name.chars()
                    .filter(|c| !c.is_control())
                    .take(100)
                    .collect::<String>()
            ),
            *params,
        );
    }
    let sidecar = sidecar_path(source)?;
    let bytes = serde_json::to_vec_pretty(&saved).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("Imported history too large"));
    }
    let mut file = tempfile::NamedTempFile::new_in(sidecar.parent().unwrap())?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(sidecar).map_err(|e| e.error)?;
    Ok(())
}
