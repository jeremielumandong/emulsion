//! User settings, stored in `<data dir>/settings.json` with owner-only
//! permissions because it may hold an API key.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
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
    /// Play the assistant's brush strokes on the canvas as it paints.
    pub show_drawing: bool,
    /// Show the power-user row of tool options (dynamics, symmetry,
    /// guides). Off for a beginner-friendly bar.
    pub advanced_tools: bool,
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "claude".into(),
            cli_path: None,
            model: None,
            jev_api_key: None,
            auto_apply: false,
            suggestions: true,
            light_mode: false,
            follow_omarchy: false,
            approve_all: false,
            show_drawing: true,
            advanced_tools: false,
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
        std::fs::read(file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = crate::recent::data_dir();
        std::fs::create_dir_all(&dir)?;
        let path = file();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        serde_json::to_writer_pretty(&mut file, self).map_err(std::io::Error::other)?;
        Ok(())
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
