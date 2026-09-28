//! Local named layout presets. Queue-specific options and copies are never saved.
use super::*;
use std::io::{Read, Write};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    pub settings: Settings,
}
fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 100 || name.chars().any(char::is_control) {
        bail!("Use a preset name of 1–100 characters without control characters")
    }
    Ok(())
}
fn sanitized(mut settings: Settings) -> Settings {
    settings.copies = 1;
    settings.production.driver_color_disabled = false;
    settings.media = None;
    settings.tray = None;
    settings.quality = None;
    settings.sides = None;
    settings.paper.margins = [0.; 4];
    settings
}
pub fn path() -> std::path::PathBuf {
    crate::recent::data_dir().join("print-presets.json")
}
pub fn load(path: &Path) -> Result<Vec<Preset>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = vec![];
    file.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 {
        bail!("Print preset file exceeds 256 KiB")
    }
    let mut presets: Vec<Preset> = serde_json::from_slice(&bytes)?;
    if presets.len() > 100 {
        bail!("At most 100 print presets are supported")
    }
    let mut names = std::collections::HashSet::new();
    for p in &mut presets {
        validate_name(&p.name)?;
        if !names.insert(p.name.clone()) {
            bail!("Duplicate print preset name")
        }
        validate_settings(&p.settings)?;
        p.settings = sanitized(p.settings.clone());
    }
    Ok(presets)
}
fn validate_settings(s: &Settings) -> Result<()> {
    // Profile paths are resolved at preview/output, allowing an unavailable
    // profile to remain in a preset without making the entire catalog unreadable.
    s.production.validate()?;
    // Exercise the same validation and limits as output, without any device I/O.
    let source = Source {
        name: "Preset".into(),
        width: 100,
        height: 100,
        ppi: 100.,
        svg: String::new(),
        rasterized: false,
        document: None,
        original_paths: vec![],
    };
    layout(&[source], &[0], s)?;
    Ok(())
}
/// Re-read before mutation so stale dialogs do not erase other preset names.
pub fn save(path: &Path, name: &str, settings: &Settings) -> Result<Vec<Preset>> {
    let name = name.trim();
    validate_name(name)?;
    let settings = sanitized(settings.clone());
    validate_settings(&settings)?;
    let _guard = lock(path)?;
    let mut presets = load(path)?;
    if let Some(preset) = presets.iter_mut().find(|p| p.name == name) {
        preset.settings = settings;
    } else {
        if presets.len() >= 100 {
            bail!("At most 100 print presets are supported")
        }
        presets.push(Preset {
            name: name.into(),
            settings,
        });
    }
    persist(path, &presets)?;
    Ok(presets)
}
pub fn remove(path: &Path, name: &str) -> Result<Vec<Preset>> {
    let _guard = lock(path)?;
    let mut presets = load(path)?;
    presets.retain(|p| p.name != name);
    persist(path, &presets)?;
    Ok(presets)
}
fn lock(path: &Path) -> Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    file.lock()?;
    Ok(file)
}
fn persist(path: &Path, presets: &[Preset]) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(presets)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::write_atomic(path, |file| {
        file.write_all(&bytes)?;
        Ok(())
    })?;
    Ok(())
}
/// Apply only creative preferences; keep the chosen destination's valid options.
/// Unsupported paper is retained from the current device, with an explicit notice.
pub fn apply(
    preset: &Preset,
    current: &Settings,
    caps: &Capabilities,
    pdf: bool,
) -> (Settings, Option<String>) {
    let mut settings = preset.settings.clone();
    settings.media = current.media.clone();
    settings.tray = current.tray.clone();
    settings.quality = current.quality.clone();
    settings.sides = current.sides.clone();
    settings.copies = current.copies;
    settings.grayscale |= !caps.color;
    let mut notes = vec![];
    if let Some(paper) = caps
        .papers
        .iter()
        .find(|p| {
            p.id == settings.paper.id
                && (p.width - settings.paper.width).abs() < 0.5
                && (p.height - settings.paper.height).abs() < 0.5
        })
        .or_else(|| {
            caps.papers.iter().find(|p| {
                (p.width - settings.paper.width).abs() < 0.5
                    && (p.height - settings.paper.height).abs() < 0.5
            })
        })
    {
        settings.paper = paper.clone();
    } else {
        settings.paper = current.paper.clone();
        notes.push("Preset paper is unavailable; retained the current paper.");
    }
    if !pdf && settings.production.standard != super::production::PdfStandard::Pdf {
        settings.production.standard = super::production::PdfStandard::Pdf;
        notes.push("PDF/X applies to Save PDF; this queue uses ordinary ICC-managed output.");
    }
    if !pdf && settings.layout == Layout::Document {
        settings.layout = Layout::Single;
        settings.placement = Placement::Actual;
        settings.scale = 100.;
        notes.push("Document-size PDF preset changed to actual size on printer paper.");
    }
    (settings, (!notes.is_empty()).then(|| notes.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preset_roundtrip_replace_delete_and_device_revalidation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        let mut settings = Settings {
            copies: 5,
            media: Some("glossy".into()),
            ..Default::default()
        };
        settings.creative.artwork_mm = Some([100., 100.]);
        settings.creative.crop = [0., 1.];
        settings.creative.bleed_mm = 3.;
        settings.creative.crop_marks = true;
        save(&path, "Square", &settings).unwrap();
        settings.creative.crop = [1., 0.];
        save(&path, "Other", &settings).unwrap();
        save(&path, "Square", &settings).unwrap();
        let presets = load(&path).unwrap();
        assert_eq!(presets.len(), 2);
        let p = &presets[0];
        assert_eq!(p.settings.creative.crop, [1., 0.]);
        assert_eq!(p.settings.copies, 1);
        assert!(p.settings.media.is_none());
        let mut caps = Capabilities::pdf();
        caps.papers[0].margins = [4.; 4];
        caps.color = false;
        let (applied, notice) = apply(p, &settings, &caps, false);
        assert!(notice.is_none());
        assert_eq!(applied.paper.margins, [4.; 4]);
        assert_eq!(applied.media, settings.media);
        assert!(applied.grayscale);
        caps.papers.remove(0);
        assert!(
            apply(p, &settings, &caps, false)
                .1
                .unwrap()
                .contains("unavailable")
        );
        assert_eq!(remove(&path, "Square").unwrap().len(), 1);
    }
    #[test]
    fn malformed_store_is_not_overwritten_and_legacy_settings_get_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        std::fs::write(&path, b"bad data").unwrap();
        assert!(save(&path, "Proof", &Settings::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"bad data");
        let legacy: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.creative.columns, 2);
        assert!(save(&path, "\n", &Settings::default()).is_err());
    }
}
