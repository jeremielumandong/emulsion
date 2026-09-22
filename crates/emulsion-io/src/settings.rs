//! User settings, stored in `<data dir>/settings.json` with owner-only
//! permissions because it may hold an API key.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How the assistant's strokes play on the canvas.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DrawingPace {
    /// About the speed of a hand: each stroke eases in and out and the
    /// pen lifts between strokes, so the drawing can be watched.
    #[default]
    Natural,
    /// A few seconds for the whole call, however long it is.
    Quick,
}

/// A named reusable shape stroke; applying it leaves the shape fill unchanged.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ShapeStrokePreset {
    pub name: String,
    pub style: emulsion_raster::vector::PathStyle,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Home items starred by the user, stored by their canonical recent path.
    pub starred_files: Vec<PathBuf>,
    pub shape_stroke_presets: Vec<ShapeStrokePreset>,
    /// Per-effect defaults used when adding a layer style.
    pub layer_style_defaults: Vec<emulsion_core::styles::LayerStyle>,
    pub layer_style_option_defaults:
        std::collections::BTreeMap<String, emulsion_core::style_options::StyleOptions>,
    /// Which coding CLI drives the assistant: "claude", "codex", "opencode", "kimi".
    pub provider: String,
    /// Explicit path to the coding CLI; otherwise it is searched for.
    pub cli_path: Option<PathBuf>,
    /// Model alias passed to the CLI ("sonnet", "opus", …); `None` = CLI default.
    pub model: Option<String>,
    /// TypeSafe API key for Jev. `TYPESAFE_API_KEY` in the environment wins.
    pub jev_api_key: Option<String>,
    /// Apply the assistant's non-destructive changes without a card.
    pub auto_apply: bool,
    /// Show suggestions from image statistics.
    pub suggestions: bool,
    /// Light theme instead of the default dark one.
    pub light_mode: bool,
    /// Follow the current Omarchy palette on Linux, retaining `light_mode` as fallback.
    pub follow_omarchy: bool,
    /// Apply every assistant change without asking, deletes and merges too.
    pub approve_all: bool,
    /// Compact editor header and movable canvas toolbars, with native
    /// window controls retained in the header.
    pub compact_chrome: bool,
    /// Settings migration marker for the compact single-row editor header.
    /// Version zero is the legacy layout preference written before compact
    /// became the primary editor design.
    #[serde(default = "legacy_compact_chrome_revision")]
    pub compact_chrome_revision: u8,
    /// Height of the Layers list in the side panel, in logical pixels;
    /// dragged by its handle.
    pub layers_height: f32,
    /// Play the assistant's brush strokes on the canvas as it paints.
    pub show_drawing: bool,
    /// How fast those strokes play.
    pub drawing_pace: DrawingPace,
    /// Show the power-user row of tool options (dynamics, symmetry,
    /// guides). Off for a beginner-friendly bar.
    pub advanced_tools: bool,
    /// Draw mode: a Procreate-like shell with only the drawing tools and
    /// the Layers dock, for painting sessions.
    pub draw_mode: bool,
    /// Default image provider: "", "a1111", "openai", or "google".
    pub image_provider: String,
    /// Base URL of that server; empty for its default.
    pub image_endpoint: Option<String>,
    /// Checkpoint to ask the server for; empty for its current one.
    pub image_model: Option<String>,
    pub openai_image_key: Option<String>,
    pub openai_image_model: Option<String>,
    pub google_image_key: Option<String>,
    pub google_image_model: Option<String>,
}

/// Whether this desktop is Omarchy (its current-theme colours exist).
pub fn omarchy_present() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let root = |var: &str, fallback: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.as_ref().map(|h| h.join(fallback)))
    };
    [
        root("XDG_STATE_HOME", ".local/state"),
        root("XDG_CONFIG_HOME", ".config"),
    ]
    .into_iter()
    .flatten()
    .any(|r| r.join("omarchy/current/theme/colors.toml").is_file())
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            starred_files: Vec::new(),
            provider: "claude".into(),
            cli_path: None,
            model: None,
            layer_style_defaults: Vec::new(),
            shape_stroke_presets: Vec::new(),
            layer_style_option_defaults: Default::default(),
            jev_api_key: None,
            auto_apply: false,
            suggestions: true,
            light_mode: false,
            follow_omarchy: false,
            approve_all: false,
            compact_chrome: true,
            compact_chrome_revision: 1,
            layers_height: 400.0,
            show_drawing: true,
            drawing_pace: DrawingPace::Natural,
            advanced_tools: false,
            draw_mode: false,
            image_provider: String::new(),
            image_endpoint: None,
            image_model: None,
            openai_image_key: None,
            openai_image_model: None,
            google_image_key: None,
            google_image_model: None,
        }
    }
}

fn file() -> PathBuf {
    crate::recent::data_dir().join("settings.json")
}

fn legacy_compact_chrome_revision() -> u8 {
    0
}

impl Settings {
    /// Image credentials are separate from the assistant CLI subscription.
    /// Environment variables take precedence over saved keys.
    pub fn image_key(&self, provider: &str) -> Option<(String, &'static str)> {
        let (names, saved): (&[&str], &Option<String>) = match provider {
            "openai" => (&["OPENAI_API_KEY"], &self.openai_image_key),
            "google" => (
                &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
                &self.google_image_key,
            ),
            _ => return None,
        };
        for name in names {
            if let Ok(key) = std::env::var(name)
                && !key.trim().is_empty()
            {
                return Some((key.trim().to_string(), "environment"));
            }
        }
        saved
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(|key| (key.to_string(), "settings"))
    }

    pub fn load() -> Self {
        Self::load_from(&file())
    }

    fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read(path) {
            Ok(b) => {
                let mut settings: Self = crate::parse_config(path, &b).unwrap_or_default();
                if settings.compact_chrome_revision == 0 {
                    settings.compact_chrome = true;
                    settings.compact_chrome_revision = 1;
                    // Best effort: the in-memory migration still fixes this
                    // launch if the settings directory is temporarily read-only.
                    let _ = crate::save_config(path, &settings);
                }
                settings
            }
            // First run: on an Omarchy desktop, start in its colours.
            Err(_) => Self {
                follow_omarchy: omarchy_present(),
                ..Self::default()
            },
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::save_config(&file(), self)
    }

    /// The Jev key in effect, and where it came from.
    pub fn jev_key(&self) -> Option<(String, &'static str)> {
        if let Ok(k) = std::env::var("TYPESAFE_API_KEY")
            && !k.trim().is_empty()
        {
            return Some((k.trim().to_string(), "environment"));
        }
        self.jev_api_key
            .clone()
            .filter(|k| !k.trim().is_empty())
            .map(|k| (k, "settings"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_is_owner_only_and_corrupt_file_is_kept_as_backup() {
        let dir = std::env::temp_dir().join(format!("emulsion-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let settings = Settings {
            jev_api_key: Some("secret".into()),
            ..Settings::default()
        };
        crate::save_config(&path, &settings).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(
            Settings::load_from(&path).jev_api_key.as_deref(),
            Some("secret")
        );
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(Settings::load_from(&path).jev_api_key, None);
        let backup = dir.join("settings.json.bak");
        assert_eq!(std::fs::read(&backup).unwrap(), b"{ not json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&backup).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_roomy_preference_migrates_once_to_the_compact_header() {
        let dir = std::env::temp_dir().join(format!(
            "emulsion-compact-layout-migration-{}",
            std::process::id()
        ));
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, br#"{"compact_chrome":false}"#).unwrap();

        let migrated = Settings::load_from(&path);
        assert!(migrated.compact_chrome);
        assert_eq!(migrated.compact_chrome_revision, 1);

        let deliberately_roomy = Settings {
            compact_chrome: false,
            ..migrated
        };
        crate::save_config(&path, &deliberately_roomy).unwrap();
        assert!(!Settings::load_from(&path).compact_chrome);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn existing_theme_preferences_do_not_enable_omarchy() {
        for light_mode in [false, true] {
            let settings: Settings = serde_json::from_value(serde_json::json!({
                "light_mode": light_mode
            }))
            .unwrap();
            assert_eq!(settings.light_mode, light_mode);
            assert!(!settings.follow_omarchy);
        }
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.light_mode);
        assert!(!settings.follow_omarchy);
    }

    #[test]
    fn omarchy_preference_round_trips_with_explicit_theme_fallback() {
        for light_mode in [false, true] {
            let settings = Settings {
                light_mode,
                follow_omarchy: true,
                ..Settings::default()
            };
            let restored: Settings =
                serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
            assert_eq!(restored, settings);
        }
    }

    #[test]
    fn old_local_settings_load_and_cloud_profiles_round_trip_separately() {
        let mut settings: Settings = serde_json::from_str(
            r#"{"image_provider":"a1111","image_endpoint":"http://localhost:7860","image_model":"local-checkpoint"}"#,
        ).unwrap();
        assert!(settings.openai_image_key.is_none());
        assert!(settings.google_image_model.is_none());
        settings.openai_image_key = Some("openai-test-only".into());
        settings.openai_image_model = Some("openai-model".into());
        settings.google_image_key = Some("google-test-only".into());
        settings.google_image_model = Some("google-model".into());
        settings.image_provider = "google".into();
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        assert_eq!(restored.image_model.as_deref(), Some("local-checkpoint"));
        assert_eq!(
            restored.image_endpoint.as_deref(),
            Some("http://localhost:7860")
        );
    }
}

#[cfg(test)]
mod style_default_tests {
    use super::*;
    #[test]
    fn shape_stroke_presets_roundtrip_and_legacy_settings_stay_empty() {
        use emulsion_raster::vector::{PathPaint, PathStyle, StrokeCap};
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.shape_stroke_presets.is_empty());
        settings.shape_stroke_presets.push(ShapeStrokePreset {
            name: "Dotted gradient".into(),
            style: PathStyle {
                width: 8.0,
                cap: StrokeCap::Round,
                stroke_paint: PathPaint::LinearGradient {
                    end: [255, 100, 30, 255],
                    angle: 25.0,
                },
                dash: [1.0, 8.0, 0.0, 0.0, 0.0, 0.0],
                dash_count: 2,
                ..Default::default()
            },
        });
        let decoded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn extended_style_defaults_roundtrip_and_legacy_defaults_stay_empty() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(old.layer_style_option_defaults.is_empty());
        let mut settings = old;
        let mut option = emulsion_core::style_options::StyleOptions::default();
        option.pattern.image = Some(std::sync::Arc::new(
            emulsion_core::style_options::PatternImage {
                width: 1,
                height: 1,
                pixels: vec![12, 34, 56, 78],
            },
        ));
        settings
            .layer_style_option_defaults
            .insert("pattern-overlay".into(), option);
        let decoded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(settings, decoded);
    }
}
