//! User settings, stored in `<data dir>/settings.json` with owner-only
//! permissions because it may hold an API key.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cli_path: None,
            model: None,
            jev_api_key: None,
            auto_apply: false,
            suggestions: true,
        }
    }
}

fn file() -> PathBuf {
    crate::recent::data_dir().join("settings.json")
}

impl Settings {
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
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
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
