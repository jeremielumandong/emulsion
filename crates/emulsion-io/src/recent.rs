//! Recently opened files.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX: usize = 24;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Recent {
    pub path: PathBuf,
    /// Seconds since the Unix epoch.
    pub opened: u64,
    /// e.g. "6 nodes".
    pub summary: String,
}

/// `$XDG_DATA_HOME/emulsion`, falling back to `~/.local/share/emulsion`.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("emulsion");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/emulsion")
}

fn file() -> PathBuf {
    data_dir().join("recent.json")
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load the list, dropping files that no longer exist.
pub fn load() -> Vec<Recent> {
    load_from(&file())
}

fn load_from(path: &Path) -> Vec<Recent> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let list: Vec<Recent> = crate::parse_config(path, &bytes).unwrap_or_default();
    list.into_iter().filter(|r| r.path.exists()).collect()
}

fn save(list: &[Recent]) {
    if let Err(error) = crate::save_config(&file(), &list) {
        tracing::warn!(%error, "could not save recent files");
    }
}

/// Put `path` at the front and save.
pub fn push(path: &Path, summary: String) -> Vec<Recent> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut list = load();
    list.retain(|r| r.path != path);
    list.insert(
        0,
        Recent {
            path,
            opened: now(),
            summary,
        },
    );
    list.truncate(MAX);
    save(&list);
    list
}

/// Forget `path` (the file itself is untouched) and save.
pub fn remove(path: &Path) -> Vec<Recent> {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut list = load();
    list.retain(|r| r.path != canon && r.path != path);
    save(&list);
    list
}

/// "2m ago", "yesterday", "3 days", "2 wks".
pub fn ago(opened: u64) -> String {
    let d = now().saturating_sub(opened);
    match d {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", d / 60),
        3600..86_400 => format!("{}h ago", d / 3600),
        86_400..172_800 => "yesterday".into(),
        172_800..1_209_600 => format!("{} days", d / 86_400),
        1_209_600..5_184_000 => format!("{} wks", d / 604_800),
        _ => format!("{} mo", d / 2_592_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_recent_list_is_kept_as_backup() {
        let dir = std::env::temp_dir().join(format!("emulsion-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("recent.json");
        std::fs::write(&path, b"[{").unwrap();
        assert!(load_from(&path).is_empty());
        assert_eq!(std::fs::read(dir.join("recent.json.bak")).unwrap(), b"[{");
        let list = vec![Recent {
            path: dir.clone(),
            opened: 1,
            summary: "1 node".into(),
        }];
        crate::save_config(&path, &list).unwrap();
        assert_eq!(load_from(&path), list);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
