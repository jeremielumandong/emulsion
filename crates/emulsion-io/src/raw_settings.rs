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
const MAX_BYTES: u64 = 64 * 1024;

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
            target.temperature = source.temperature;
            target.tint = source.tint;
            target.wb_override = source.wb_override;
        }
        RawSettingsGroup::Tone => {
            target.exposure = source.exposure;
            target.highlights = source.highlights;
            target.shadows = source.shadows;
            target.black_point = source.black_point;
            target.brightness = source.brightness;
            target.contrast = source.contrast;
            target.saturation = source.saturation;
        }
        RawSettingsGroup::Curve => target.tone_curve = source.tone_curve,
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
    })
}

fn read(path: &Path, format: &str) -> Result<SettingsFile> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("file exceeds the 64 KiB limit"));
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
        return Err(invalid("settings exceed the 64 KiB limit"));
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
    let mut name = raw
        .source
        .file_name()
        .ok_or_else(|| invalid("RAW source has no filename"))?
        .to_os_string();
    name.push(".emulsion-raw.json");
    Ok(raw.source.with_file_name(name))
}

/// Load only settings whose fingerprint matches this document's linked RAW.
/// The subsequent development verifies the actual original bytes separately.
pub fn load_sidecar(doc: &Document, path: &Path) -> Result<DevelopParams> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| invalid("document has no editable RAW source"))?;
    raw.validate().map_err(invalid)?;
    let saved = read(path, "emulsion-raw-sidecar")?;
    if !saved
        .source_sha256
        .as_ref()
        .is_some_and(|digest| digest.eq_ignore_ascii_case(&raw.source_sha256))
    {
        return Err(invalid(
            "this sidecar belongs to a different RAW original (SHA-256 mismatch)",
        ));
    }
    Ok(saved.params)
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
                .contains("64 KiB")
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
