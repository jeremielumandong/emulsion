//! Import the portable handoff emitted by our Lightroom-side companion plug-in.
use crate::{
    IoError, Result,
    creative_library::{AssetKind, Catalog},
    lightroom_catalog::ImportReport,
};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    version: u32,
    photos: Vec<Photo>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Photo {
    original: String,
    rendered: Option<String>,
    #[serde(default)]
    settings: serde_json::Value,
    #[serde(default)]
    rating: u8,
    #[serde(default)]
    flag: i8,
    #[serde(default)]
    collections: Vec<String>,
}
fn safe(root: &Path, path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(IoError::Manifest(
            "Handoff paths must stay inside the handoff folder".into(),
        ));
    }
    let resolved = root.join(path).canonicalize()?;
    if !resolved.starts_with(root) {
        return Err(IoError::Manifest(
            "Handoff file leaves the selected folder".into(),
        ));
    }
    Ok(resolved)
}
pub fn import(path: &Path, catalog: &mut Catalog) -> Result<ImportReport> {
    let mut bytes = vec![];
    std::fs::File::open(path)?
        .take((32 << 20) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 32 << 20 {
        return Err(IoError::Manifest("Handoff manifest too large".into()));
    }
    let bundle: Bundle =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    if bundle.format != "emulsion-lightroom-handoff"
        || bundle.version != 1
        || bundle.photos.len() > 10000
    {
        return Err(IoError::Manifest(
            "Not a supported Lightroom handoff".into(),
        ));
    }
    let root = path.parent().unwrap_or(Path::new(".")).canonicalize()?;
    let mut report = ImportReport {
        imported: 0,
        collections: 0,
        histories: 0,
        missing: vec![],
        warnings: vec![],
    };
    let mut staged = catalog.clone();
    let mut groups = BTreeMap::<String, Vec<u64>>::new();
    let mut recipes = vec![];
    for photo in bundle.photos {
        let original = match safe(&root, &photo.original) {
            Ok(p) => p,
            Err(e) => {
                report.warnings.push(format!("Original skipped: {e}"));
                report.missing.push(root.join(&photo.original));
                continue;
            }
        };
        if !crate::photo_develop::supported(&original) {
            report
                .warnings
                .push(format!("Unsupported original: {}", original.display()));
            continue;
        }
        let id = staged.add_asset(original.clone(), AssetKind::Image)?;
        let asset = staged.assets.iter_mut().find(|a| a.id == id).unwrap();
        asset.rating = photo.rating.min(5);
        asset.flagged = photo.flag > 0;
        asset.rejected = photo.flag < 0;
        let digest = crate::raw::source_digest(&original)?;
        staged
            .photos
            .fingerprints
            .insert(original.clone(), digest.clone());
        report.imported += 1;
        for name in photo.collections {
            if name.len() > 200 || name.trim().is_empty() {
                return Err(IoError::Manifest("Invalid handoff collection".into()));
            }
            groups.entry(name).or_default().push(id);
        }
        if !photo.settings.is_null() && !crate::raw_settings::sidecar_path(&original)?.exists() {
            match crate::lightroom_presets::from_adobe_settings(&photo.settings, Default::default())
            {
                Ok(p) => {
                    report.warnings.extend(p.warnings);
                    recipes.push((original, digest, p.params));
                }
                Err(e) => report
                    .warnings
                    .push(format!("Settings not translated: {e}")),
            }
        }
        if let Some(rendered) = photo.rendered {
            let rendered = safe(&root, &rendered)?;
            if !matches!(
                rendered
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("tif" | "tiff")
            ) {
                return Err(IoError::Manifest(
                    "Lightroom references must be TIFF images".into(),
                ));
            }
            let id = staged.add_asset(rendered.clone(), AssetKind::Image)?;
            staged
                .photos
                .fingerprints
                .insert(rendered.clone(), crate::raw::source_digest(&rendered)?);
            groups
                .entry("Lightroom rendered references".into())
                .or_default()
                .push(id);
            report.imported += 1;
        }
    }
    for (name, ids) in groups {
        staged.add_collection(name, ids)?;
        report.collections += 1;
    }
    staged.validate()?;
    for (source, digest, params) in recipes {
        crate::raw_settings::import_history(
            &source,
            &digest,
            &[("Lightroom handoff".into(), params)],
        )?;
        report.histories += 1;
    }
    report.warnings.push("Rendered references preserve Lightroom's output, including applied VSCO profiles. Editable RAW settings are translated; Adobe plug-ins continue to run in Lightroom.".into());
    *catalog = staged;
    Ok(report)
}
