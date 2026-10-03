//! Colour management preferences and the OpenColorIO hooks: one shared
//! place for display (the canvas, Stage and player call
//! [`display_lut`]) and one for export (image, movie and PDF exports call
//! [`ExportTransform::for_project`]). With OpenColorIO off both return
//! `None` and the ICC behaviour is exactly as before.
//!
//! Pixel values are taken to be in the working colour space: the project's
//! own (a storyboard stores it) or the default from these preferences,
//! which falls back to the config's `texture_paint`/`color_picking` role.
//!
//! The preferences live in their own file (`color_management.json` in the
//! data directory) behind a process-wide value, so the Settings screen and
//! the assistant's tools change one shared state. Nothing is read from or
//! written to disk until [`init`] runs at app start, so tests see the
//! defaults.

pub use emulsion_color::ocio::{self, BakedLut, Config, Processor, Shaper};
use image::ImageEncoder as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

/// Where the OpenColorIO config comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    /// Emulsion's built-in ACES config.
    #[default]
    Builtin,
    /// The file `$OCIO` names.
    Environment,
    /// A config file the person chose.
    File(PathBuf),
}

impl ConfigSource {
    fn is_builtin(&self) -> bool {
        *self == Self::Builtin
    }

    pub fn label(&self) -> String {
        match self {
            Self::Builtin => "built-in".into(),
            Self::Environment => "$OCIO".into(),
            Self::File(p) => p.display().to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorManagement {
    /// OpenColorIO viewing and export; off keeps ICC colour management.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ocio: bool,
    #[serde(skip_serializing_if = "ConfigSource::is_builtin")]
    pub config: ConfigSource,
    /// Working colour space of documents that do not set their own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// A look applied instead of the view's own (`Some("")` turns them
    /// off).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<String>,
    /// The colour space exports are written in; `None` uses the display
    /// and view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_colorspace: Option<String>,
}

/// Everything a transform needs, with defaults filled in from the config.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub config: Arc<Config>,
    pub working: String,
    pub display: String,
    pub view: String,
    pub look: Option<String>,
    pub export_colorspace: Option<String>,
}

/// Loaded configs by source, with the file time they were read at.
type ConfigCache = Mutex<HashMap<ConfigSource, (Option<std::time::SystemTime>, Arc<Config>)>>;

fn config_cache() -> &'static ConfigCache {
    static CACHE: std::sync::OnceLock<ConfigCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Load (or reuse) the config a source names; a changed file is re-read.
pub fn load_config(source: &ConfigSource) -> Result<Arc<Config>, String> {
    let path = match source {
        ConfigSource::Builtin => None,
        ConfigSource::Environment => Some(
            std::env::var_os("OCIO")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .ok_or("$OCIO is not set")?,
        ),
        ConfigSource::File(p) => Some(p.clone()),
    };
    let stamp = path
        .as_deref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let mut cache = config_cache().lock().map_err(|e| e.to_string())?;
    if let Some((s, c)) = cache.get(source)
        && *s == stamp
    {
        return Ok(c.clone());
    }
    let config = Arc::new(match &path {
        None => Config::builtin(),
        Some(p) => Config::from_file(p).map_err(|e| e.to_string())?,
    });
    cache.insert(source.clone(), (stamp, config.clone()));
    Ok(config)
}

impl ColorManagement {
    /// Use another config, keeping the chosen names it also has and
    /// dropping the rest (so the result resolves).
    pub fn switch_config(&mut self, source: ConfigSource) {
        if let Ok(config) = load_config(&source) {
            let keep_space = |name: &mut Option<String>| {
                if name
                    .as_deref()
                    .is_some_and(|n| config.colorspace(n).is_none())
                {
                    *name = None;
                }
            };
            keep_space(&mut self.working);
            keep_space(&mut self.export_colorspace);
            if self
                .display
                .as_deref()
                .is_some_and(|d| config.display(d).is_none())
            {
                self.display = None;
            }
            let display = self
                .display
                .clone()
                .or_else(|| config.default_display().map(|d| d.name.clone()))
                .unwrap_or_default();
            if self
                .view
                .as_deref()
                .is_some_and(|v| config.find_view(&display, v).is_err())
            {
                self.view = None;
            }
            if self.look.as_deref().is_some_and(|l| {
                l.split(',')
                    .map(|n| n.trim().trim_start_matches(['+', '-']).trim())
                    .any(|n| !n.is_empty() && config.look(n).is_none())
            }) {
                self.look = None;
            }
        }
        self.config = source;
    }

    /// Fill in defaults and check every name against the config. `None`
    /// when OpenColorIO is off.
    pub fn resolve(&self, project_working: Option<&str>) -> Result<Option<Resolved>, String> {
        if !self.ocio {
            return Ok(None);
        }
        let config = load_config(&self.config)?;
        let known = |name: &str, what: &str| -> Result<String, String> {
            config
                .colorspace(name)
                .map(|c| c.name.clone())
                .ok_or_else(|| format!("The {what} “{name}” is not in the OpenColorIO config"))
        };
        let working = match project_working.or(self.working.as_deref()) {
            Some(name) => known(name, "working colour space")?,
            None => ["texture_paint", "color_picking", "default"]
                .iter()
                .find_map(|r| config.colorspace(r))
                .or(config.colorspaces.first())
                .map(|c| c.name.clone())
                .ok_or("The OpenColorIO config has no colour spaces")?,
        };
        let display = match &self.display {
            Some(d) => config
                .display(d)
                .map(|d| d.name.clone())
                .ok_or_else(|| format!("The display “{d}” is not in the OpenColorIO config"))?,
            None => config
                .default_display()
                .map(|d| d.name.clone())
                .ok_or("The OpenColorIO config has no displays")?,
        };
        let view = match &self.view {
            Some(v) => config
                .find_view(&display, v)
                .map(|v| v.name.clone())
                .map_err(|e| e.to_string())?,
            None => config
                .default_view(&display)
                .map(|v| v.name.clone())
                .ok_or_else(|| format!("The display “{display}” has no views"))?,
        };
        if let Some(look) = &self.look {
            for name in look
                .split(',')
                .map(|l| l.trim().trim_start_matches(['+', '-']).trim())
            {
                if !name.is_empty() && config.look(name).is_none() {
                    return Err(format!(
                        "The look “{name}” is not in the OpenColorIO config"
                    ));
                }
            }
        }
        let export_colorspace = self
            .export_colorspace
            .as_deref()
            .map(|c| known(c, "export colour space"))
            .transpose()?;
        Ok(Some(Resolved {
            config,
            working,
            display,
            view,
            look: self.look.clone(),
            export_colorspace,
        }))
    }
}

impl Resolved {
    /// Working space → display/view, exact.
    pub fn display_processor(&self) -> Result<Processor, String> {
        self.config
            .display_processor(
                &self.working,
                &self.display,
                &self.view,
                self.look.as_deref(),
            )
            .map_err(|e| e.to_string())
    }

    /// Working space → the export colour space (or the display/view).
    pub fn export_processor(&self) -> Result<Processor, String> {
        match &self.export_colorspace {
            Some(cs) => self
                .config
                .processor(&self.working, cs)
                .map_err(|e| e.to_string()),
            None => self.display_processor(),
        }
    }

    /// What exports are written in, for describing it.
    pub fn export_label(&self) -> String {
        match &self.export_colorspace {
            Some(cs) => cs.clone(),
            None => format!("{} / {}", self.display, self.view),
        }
    }
}

// ── The shared state ─────────────────────────────────────────────────────

struct State {
    settings: ColorManagement,
    generation: u64,
    persist: bool,
}

fn state() -> &'static RwLock<State> {
    static STATE: std::sync::OnceLock<RwLock<State>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| {
        RwLock::new(State {
            settings: ColorManagement::default(),
            generation: 0,
            persist: false,
        })
    })
}

fn file() -> PathBuf {
    crate::recent::data_dir().join("color_management.json")
}

/// Read the saved preferences and save every later change. Called once at
/// app start.
pub fn init() {
    let path = file();
    let settings = std::fs::read(&path)
        .ok()
        .and_then(|b| crate::parse_config::<ColorManagement>(&path, &b))
        .unwrap_or_default();
    if let Ok(mut s) = state().write() {
        s.settings = settings;
        s.generation += 1;
        s.persist = true;
    }
}

/// The current preferences.
pub fn current() -> ColorManagement {
    state()
        .read()
        .map(|s| s.settings.clone())
        .unwrap_or_default()
}

/// Changes whenever the preferences do; views compare it to know when to
/// redraw.
pub fn generation() -> u64 {
    state().read().map(|s| s.generation).unwrap_or_default()
}

/// Change the preferences. The result is checked against its config
/// (when OpenColorIO is on) before it is kept, and saved once [`init`] ran.
pub fn update(change: impl FnOnce(&mut ColorManagement)) -> Result<ColorManagement, String> {
    let mut next = current();
    change(&mut next);
    next.resolve(None)?;
    let persist = {
        let mut s = state().write().map_err(|e| e.to_string())?;
        if s.settings == next {
            return Ok(next);
        }
        s.settings = next.clone();
        s.generation += 1;
        s.persist
    };
    if persist {
        crate::save_config(&file(), &next)
            .map_err(|e| format!("Could not save colour settings: {e}"))?;
    }
    Ok(next)
}

/// The working colour space a project stores, if any.
pub fn project_working(project: &emulsion_core::project::Project) -> Option<&str> {
    project
        .storyboard
        .as_ref()
        .and_then(|b| b.working_colorspace.as_deref())
}

// ── Display ──────────────────────────────────────────────────────────────

/// Lattice size of the display LUT.
pub const DISPLAY_LUT_SIZE: usize = 65;

type DisplayKey = (u64, Option<String>);

/// The last display LUT made, by what it was made for.
type DisplayCache = Mutex<Option<(DisplayKey, Option<Arc<BakedLut>>)>>;

fn display_cache() -> &'static DisplayCache {
    static CACHE: std::sync::OnceLock<DisplayCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The display transform for 8-bit display pixels, baked to a 3D LUT, or
/// `None` when OpenColorIO is off (or the settings cannot be resolved, which
/// the Settings screen reports). The shared display hook.
pub fn display_lut(project_working: Option<&str>) -> Option<Arc<BakedLut>> {
    let key = (generation(), project_working.map(str::to_string));
    if let Ok(cache) = display_cache().lock()
        && let Some((k, lut)) = cache.as_ref()
        && *k == key
    {
        return lut.clone();
    }
    let lut = match bake_display(&current(), project_working) {
        Ok(lut) => lut.map(Arc::new),
        Err(e) => {
            tracing::warn!(target: "emulsion_io::color", "OpenColorIO display transform: {e}");
            None
        }
    };
    if let Ok(mut cache) = display_cache().lock() {
        *cache = Some((key, lut.clone()));
    }
    lut
}

/// Bake the display transform of `settings` (no caching).
pub fn bake_display(
    settings: &ColorManagement,
    project_working: Option<&str>,
) -> Result<Option<BakedLut>, String> {
    let Some(r) = settings.resolve(project_working)? else {
        return Ok(None);
    };
    let processor = r.display_processor()?;
    // Display pixels are 0–1; a linear working space spends the lattice on
    // the shadows through a log shaper.
    let shaper = if r
        .config
        .colorspace(&r.working)
        .is_some_and(|c| c.is_linear())
    {
        Shaper::Log2 { lo: -10., hi: 0. }
    } else {
        Shaper::Identity
    };
    Ok(Some(processor.bake(DISPLAY_LUT_SIZE, shaper)))
}

/// A key that changes when the display transform for `project_working`
/// would; `None` when OpenColorIO is off.
pub fn display_key(project_working: Option<&str>) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let settings = current();
    if !settings.ocio {
        return None;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    generation().hash(&mut h);
    project_working.hash(&mut h);
    Some(h.finish())
}

// ── Export ───────────────────────────────────────────────────────────────

/// The exact transform exports apply, resolved once per export.
#[derive(Clone, Debug)]
pub struct ExportTransform {
    processor: Arc<Processor>,
    label: String,
}

impl ExportTransform {
    /// The export transform for `project` under the current preferences;
    /// `None` when OpenColorIO is off. The shared export hook.
    pub fn for_project(
        project: Option<&emulsion_core::project::Project>,
    ) -> anyhow::Result<Option<Self>> {
        Self::from_settings(&current(), project.and_then(project_working))
    }

    pub fn from_settings(
        settings: &ColorManagement,
        project_working: Option<&str>,
    ) -> anyhow::Result<Option<Self>> {
        let Some(r) = settings
            .resolve(project_working)
            .map_err(anyhow::Error::msg)?
        else {
            return Ok(None);
        };
        let processor = r.export_processor().map_err(anyhow::Error::msg)?;
        Ok(Some(Self {
            processor: Arc::new(processor),
            label: r.export_label(),
        }))
    }

    /// What the output is encoded as.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Interleaved RGBA8 (straight alpha) in place; alpha untouched.
    pub fn apply_rgba8(&self, pixels: &mut [u8]) {
        use rayon::prelude::*;
        pixels
            .par_chunks_mut(4 * 4096)
            .for_each(|chunk| self.processor.apply_rgba8(chunk));
    }

    /// A document flattened and converted, as straight RGBA8.
    pub fn document_rgba8(&self, doc: &emulsion_core::Document) -> anyhow::Result<Vec<u8>> {
        let developed;
        let doc = if doc.raw.is_some() {
            developed = crate::export::develop_document(doc)?;
            &developed
        } else {
            doc
        };
        let flat = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
        let mut px = flat.to_srgba8();
        self.apply_rgba8(&mut px);
        Ok(px)
    }

    /// Write `doc` converted to `path` as PNG or JPEG. No sRGB tag or ICC
    /// profile is written: the output is in the OCIO colour space.
    pub fn write_document(
        &self,
        doc: &emulsion_core::Document,
        path: &Path,
        jpeg_quality: u8,
    ) -> anyhow::Result<()> {
        let px = self.document_rgba8(doc)?;
        let (w, h) = (doc.width, doc.height);
        let jpeg = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg"));
        let mut bytes = Vec::new();
        if jpeg {
            let rgb: Vec<u8> = px
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = u32::from(p[3]);
                    [0, 1, 2].map(|i| ((u32::from(p[i]) * a + 255 * (255 - a) + 127) / 255) as u8)
                })
                .collect();
            image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut bytes,
                jpeg_quality.clamp(1, 100),
            )
            .write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)?;
        } else {
            image::codecs::png::PngEncoder::new(&mut bytes).write_image(
                &px,
                w,
                h,
                image::ExtendedColorType::Rgba8,
            )?;
        }
        crate::write_atomic(path, |f| {
            use std::io::Write;
            f.write_all(&bytes)?;
            Ok(())
        })?;
        Ok(())
    }

    /// Replace a print source's artwork with its converted pixels (the
    /// vector artwork cannot carry an OCIO transform).
    pub fn convert_print_source(&self, source: &mut crate::printing::Source) -> anyhow::Result<()> {
        let Some(doc) = source.document.take() else {
            return Ok(());
        };
        let px = self.document_rgba8(&doc)?;
        source.svg = png_svg(doc.width, doc.height, &px)?;
        source.rasterized = true;
        Ok(())
    }
}

/// Write `pixels` (straight RGBA8) as a PNG wrapped in an SVG document, for
/// print sources whose colours have been converted.
pub fn png_svg(width: u32, height: u32, pixels: &[u8]) -> anyhow::Result<String> {
    use base64::Engine as _;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(
        pixels,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><image width=\"{width}\" height=\"{height}\" href=\"data:image/png;base64,{encoded}\"/></svg>"
    ))
}

/// Describe the preferences and what they resolve to, for tools.
pub fn describe(settings: &ColorManagement, project_working: Option<&str>) -> serde_json::Value {
    use serde_json::json;
    let config = load_config(&settings.config);
    let mut out = json!({
        "opencolorio": settings.ocio,
        "config": settings.config.label(),
        "working_colorspace_default": settings.working,
        "project_working_colorspace": project_working,
        "display": settings.display,
        "view": settings.view,
        "look": settings.look,
        "export_colorspace": settings.export_colorspace,
        "ocio_env": std::env::var("OCIO").ok(),
    });
    match &config {
        Ok(c) => {
            out["config_name"] = json!(c.name);
            out["colorspaces"] = json!(
                c.active_colorspaces()
                    .iter()
                    .map(|s| json!({"name": s.name, "family": s.family, "encoding": s.encoding, "isdata": s.isdata,
                        "reference": if s.reference == ocio::Reference::Display { "display" } else { "scene" }}))
                    .collect::<Vec<_>>()
            );
            out["displays"] = json!(
                c.active_displays()
                    .iter()
                    .map(|d| json!({"name": d.name, "views": c.active_views(&d.name).iter().map(|v| v.name.clone()).collect::<Vec<_>>()}))
                    .collect::<Vec<_>>()
            );
            out["looks"] = json!(c.looks.iter().map(|l| l.name.clone()).collect::<Vec<_>>());
            out["roles"] = json!(c.roles);
        }
        Err(e) => out["config_error"] = json!(e),
    }
    match settings.resolve(project_working) {
        Ok(Some(r)) => {
            out["resolved"] = json!({
                "working": r.working, "display": r.display, "view": r.view,
                "export": r.export_label(),
            });
            if let Err(e) = r.display_processor() {
                out["display_error"] = json!(e);
            }
            if let Err(e) = r.export_processor() {
                out["export_error"] = json!(e);
            }
        }
        Ok(None) => {}
        Err(e) => out["error"] = json!(e),
    }
    out
}

/// Whether `path` looks like an OCIO config file.
pub fn is_config_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ocio"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_changes_nothing_and_on_resolves_defaults() {
        let off = ColorManagement::default();
        assert!(off.resolve(None).unwrap().is_none());
        assert!(bake_display(&off, None).unwrap().is_none());
        assert!(
            ExportTransform::from_settings(&off, Some("ACEScg"))
                .unwrap()
                .is_none()
        );
        assert_eq!(serde_json::to_string(&off).unwrap(), "{}");

        let on = ColorManagement {
            ocio: true,
            ..Default::default()
        };
        let r = on.resolve(None).unwrap().unwrap();
        assert_eq!(r.working, "sRGB - Texture");
        assert_eq!(r.display, "sRGB - Display");
        assert_eq!(r.view, "ACES 1.0 - SDR Video");
        let r = on.resolve(Some("acescg")).unwrap().unwrap();
        assert_eq!(r.working, "ACEScg");
        assert!(on.resolve(Some("Nope")).unwrap_err().contains("Nope"));
        let bad_view = ColorManagement {
            view: Some("Nope".into()),
            ..on.clone()
        };
        assert!(bad_view.resolve(None).is_err());
        let json = serde_json::to_string(&on).unwrap();
        assert_eq!(serde_json::from_str::<ColorManagement>(&json).unwrap(), on);
    }

    #[test]
    fn untonemapped_display_matches_icc_viewing_and_aces_tone_maps() {
        // The un-tone-mapped view of sRGB textures is the ICC (sRGB)
        // result within a code value; the ACES view changes it.
        let plain = ColorManagement {
            ocio: true,
            view: Some("Un-tone-mapped".into()),
            ..Default::default()
        };
        let lut = bake_display(&plain, None).unwrap().unwrap();
        let input: Vec<u8> = (0..=255u8).flat_map(|v| [v, 255 - v, v / 2, 128]).collect();
        let mut shown = input.clone();
        lut.apply_bgra8(&mut shown);
        for (a, b) in input.iter().zip(&shown) {
            assert!((i16::from(*a) - i16::from(*b)).abs() <= 1, "{a} vs {b}");
        }
        let aces = ColorManagement {
            ocio: true,
            ..Default::default()
        };
        let lut = bake_display(&aces, None).unwrap().unwrap();
        let mut white = [255u8, 255, 255, 200];
        lut.apply_bgra8(&mut white);
        assert!(white[0] < 245 && white[3] == 200, "{white:?}");
        // Exports through the exact ops agree with the display LUT.
        let export = ExportTransform::from_settings(&aces, None)
            .unwrap()
            .unwrap();
        let mut px = [255u8, 255, 255, 200];
        export.apply_rgba8(&mut px);
        assert!((i16::from(px[0]) - i16::from(white[2])).abs() <= 1);
        assert_eq!(export.label(), "sRGB - Display / ACES 1.0 - SDR Video");
        let to_cct = ColorManagement {
            export_colorspace: Some("ACEScct".into()),
            ..aces
        };
        let export = ExportTransform::from_settings(&to_cct, None)
            .unwrap()
            .unwrap();
        assert_eq!(export.label(), "ACEScct");
    }

    #[test]
    fn switching_configs_keeps_the_names_the_new_one_has() {
        let mut s = ColorManagement {
            ocio: true,
            working: Some("ACEScg".into()),
            view: Some("Un-tone-mapped".into()),
            export_colorspace: Some("sRGB - Texture".into()),
            ..Default::default()
        };
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../emulsion-color/testdata/ocio/v2/config.ocio");
        s.switch_config(ConfigSource::File(file));
        assert_eq!(s.working.as_deref(), Some("ACEScg"));
        assert_eq!(
            (s.view.as_deref(), s.export_colorspace.as_deref()),
            (None, None)
        );
        let r = s.resolve(None).unwrap().unwrap();
        assert_eq!((r.display.as_str(), r.view.as_str()), ("sRGB", "Film"));
        // The fixture's Film view ends in its sRGB display colour space.
        let export = ExportTransform::from_settings(&s, None).unwrap().unwrap();
        assert_eq!(export.label(), "sRGB / Film");
    }

    #[test]
    fn a_broken_config_is_reported_not_applied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.ocio");
        std::fs::write(
            &path,
            "ocio_profile_version: 2\nroles: {default: missing}\n",
        )
        .unwrap();
        let s = ColorManagement {
            ocio: true,
            config: ConfigSource::File(path),
            ..Default::default()
        };
        let err = s.resolve(None).unwrap_err();
        assert!(err.contains("missing"), "{err}");
        assert!(bake_display(&s, None).is_err());
        assert!(describe(&s, None)["config_error"].is_string());
    }
}
