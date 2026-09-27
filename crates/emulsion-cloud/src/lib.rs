//! Desktop cloud integration. Credentials never enter the sync index or projects.
pub mod auth;
pub mod http;
pub mod photos;
pub mod providers;
pub mod store;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use store::{Index, Store};
pub type Result<T> = anyhow::Result<T>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    GoogleDrive,
    GooglePhotos,
    Dropbox,
    #[serde(rename = "onedrive", alias = "one_drive")]
    OneDrive,
}
impl Provider {
    pub const ALL: [Self; 4] = [
        Self::GoogleDrive,
        Self::GooglePhotos,
        Self::Dropbox,
        Self::OneDrive,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::GoogleDrive => "google_drive",
            Self::GooglePhotos => "google_photos",
            Self::Dropbox => "dropbox",
            Self::OneDrive => "onedrive",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::GoogleDrive => "Google Drive",
            Self::GooglePhotos => "Google Photos",
            Self::Dropbox => "Dropbox",
            Self::OneDrive => "OneDrive",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub provider: Provider,
    pub id: String,
    /// Public OAuth client ID. Changing registrations requires a new sign-in.
    #[serde(default)]
    pub registration: String,
    pub label: String,
    pub root: String,
    pub persistent_credentials: bool,
}

/// An immutable object is its own commit marker: it contains the entire portable file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub project: String,
    pub id: String,
    pub parent: Option<String>,
    pub hash: String,
    pub name: String,
    pub created: u64,
    pub device: String,
    pub bytes: u64,
}
impl Revision {
    pub fn validate(&self) -> Result<()> {
        for id in [&self.project, &self.id, &self.device] {
            uuid::Uuid::parse_str(id)?;
        }
        if let Some(parent) = &self.parent {
            uuid::Uuid::parse_str(parent)?;
            anyhow::ensure!(parent != &self.id, "Revision cannot parent itself");
        }
        anyhow::ensure!(
            self.hash.len() == 64 && self.hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid content fingerprint"
        );
        anyhow::ensure!(
            !self.name.is_empty()
                && self.name.len() <= 240
                && !self.name.contains(['/', '\\'])
                && !self.name.chars().any(char::is_control)
                && self.name != "."
                && self.name != "..",
            "Invalid file name"
        );
        anyhow::ensure!(
            self.bytes > 0 && self.bytes <= store::MAX_FILE_BYTES,
            "Unsupported file size"
        );
        Ok(())
    }
    pub fn object_name(&self) -> String {
        format!("{}-{}.emulsion", self.project, self.id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteRevision {
    pub remote_id: String,
    pub revision: Revision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub path: PathBuf,
    pub provider: Provider,
    pub account_id: String,
    pub project: String,
    pub base: Option<String>,
    pub saved_hash: Option<String>,
    pub paused: bool,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Keep every concurrent head, independent of timestamps and listing order.
pub fn heads(revisions: &[RemoteRevision]) -> Vec<RemoteRevision> {
    revisions
        .iter()
        .filter(|r| {
            !revisions.iter().any(|other| {
                other.revision.project == r.revision.project
                    && other.revision.parent.as_ref() == Some(&r.revision.id)
            })
        })
        .cloned()
        .collect()
}
