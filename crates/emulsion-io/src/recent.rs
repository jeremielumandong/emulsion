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

/// Absolute `$XDG_DATA_HOME/emulsion`, falling back to `~/.local/share/emulsion`.
/// On Windows, resolve the native user profile even when `HOME` is unset.
/// Existing Windows installations retain their original per-user data folder.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
    {
        return d.join("emulsion");
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|local| local.is_absolute())
    {
        let legacy = local.join("Emulsion/.local/share/emulsion");
        if legacy.is_dir() {
            return legacy;
        }
    }
    // Explorer's file associations and shortcuts can start in different
    // directories. Never use the working directory for shared user state.
    std::env::home_dir()
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".local/share/emulsion"))
        .unwrap_or_else(|| std::env::temp_dir().join("emulsion"))
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
    let path = file();
    let list = load_from(&path);
    #[cfg(windows)]
    {
        // An explicit data root is an isolated store, including in UI tests.
        if std::env::var_os("XDG_DATA_HOME").is_some_and(|root| Path::new(&root).is_absolute()) {
            return list;
        }
        // Old builds wrote relative to the launch folder. Recover the known
        // launch locations, including folders represented in the saved list.
        let roots = std::env::current_dir()
            .ok()
            .into_iter()
            .chain(
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(Path::to_path_buf)),
            )
            .chain(std::env::home_dir())
            .chain(
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|home| home.is_absolute()),
            );
        let sources = roots
            .chain(
                list.iter()
                    .filter_map(|r| r.path.parent().map(Path::to_path_buf)),
            )
            .map(|root| root.join(".local/share/emulsion/recent.json"))
            .collect::<Vec<_>>();
        import_legacy(&path, list, sources)
    }
    #[cfg(not(windows))]
    list
}

#[cfg(any(windows, test))]
fn import_legacy(path: &Path, mut list: Vec<Recent>, sources: Vec<PathBuf>) -> Vec<Recent> {
    let marker = path.with_file_name("recent-imports.json");
    let mut imported: Vec<PathBuf> = std::fs::read(&marker)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let destination = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut changed = false;
    for source in sources {
        let Ok(source) = std::fs::canonicalize(source) else {
            continue;
        };
        if source == destination || imported.contains(&source) {
            continue;
        }
        // Read legacy history without modifying it, even when it is corrupt.
        let Some(legacy) = std::fs::read(&source)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Recent>>(&bytes).ok())
        else {
            continue;
        };
        list.extend(legacy.into_iter().filter(|r| r.path.exists()));
        imported.push(source);
        changed = true;
    }
    if changed {
        for entry in &mut list {
            entry.path = std::fs::canonicalize(&entry.path).unwrap_or_else(|_| entry.path.clone());
        }
        list.sort_by_key(|r| std::cmp::Reverse(r.opened));
        let mut seen = std::collections::HashSet::new();
        list.retain(|r| seen.insert(r.path.clone()));
        list.truncate(MAX);
        // Only mark sources imported after saving, so a failed write can retry.
        // Remember sources so forgotten files do not reappear on every launch.
        if let Err(error) =
            crate::save_config(path, &list).and_then(|()| crate::save_config(&marker, &imported))
        {
            tracing::warn!(%error, "could not save imported recent files");
        }
    }
    list
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

    #[cfg(windows)]
    #[test]
    fn data_dir_is_shared_between_windows_launch_directories() {
        const EXPECTED: &str = "EMULSION_TEST_EXPECTED_DATA_DIR";
        if let Some(expected) = std::env::var_os(EXPECTED) {
            assert_eq!(data_dir(), PathBuf::from(expected));
            return;
        }

        // Separate processes exercise the real environment lookup without
        // changing the environment or working directory of parallel tests.
        let root = std::env::temp_dir().join(format!("emulsion-data-dir-{}", std::process::id()));
        let profile = root.join("profile");
        let custom = root.join("custom-data");
        let local = root.join("local-app-data");
        let installed = local.join("Emulsion/.local/share/emulsion");
        for (launch, existing_install) in [
            ("shortcut", false),
            ("open-with", false),
            ("shortcut", true),
            ("open-with", true),
        ] {
            if existing_install {
                std::fs::create_dir_all(&installed).unwrap();
            }
            let cwd = root.join(launch);
            std::fs::create_dir_all(&cwd).unwrap();
            for xdg in [
                None,
                Some(Path::new("")),
                Some(Path::new("relative")),
                Some(custom.as_path()),
            ] {
                let expected = if xdg == Some(custom.as_path()) {
                    custom.join("emulsion")
                } else if existing_install {
                    installed.clone()
                } else {
                    profile.join(".local/share/emulsion")
                };
                let mut child = std::process::Command::new(std::env::current_exe().unwrap());
                child
                    .args([
                        "--exact",
                        "recent::tests::data_dir_is_shared_between_windows_launch_directories",
                        "--nocapture",
                    ])
                    .current_dir(&cwd)
                    .env_remove("HOME")
                    .env("USERPROFILE", &profile)
                    .env("LOCALAPPDATA", &local)
                    .env(EXPECTED, &expected);
                if let Some(xdg) = xdg {
                    child.env("XDG_DATA_HOME", xdg);
                } else {
                    child.env_remove("XDG_DATA_HOME");
                }
                let output = child.output().unwrap();
                assert!(
                    output.status.success(),
                    "{launch}, XDG_DATA_HOME={xdg:?}:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_histories_merge_once_without_resurrecting_forgotten_files() {
        let root =
            std::env::temp_dir().join(format!("emulsion-recent-import-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let first = root.join("first.png");
        let second = root.join("second.png");
        std::fs::write(&first, []).unwrap();
        std::fs::write(&second, []).unwrap();
        let entry = |path: PathBuf, opened| Recent {
            path,
            opened,
            summary: "1 node".into(),
        };
        let destination = root.join("shared/recent.json");
        let source = root.join("downloads/recent.json");
        let original = vec![entry(first.clone(), 1)];
        crate::save_config(&destination, &original).unwrap();
        crate::save_config(
            &source,
            &vec![
                entry(first, 3),
                entry(second, 2),
                entry(root.join("missing.png"), 4),
            ],
        )
        .unwrap();
        let legacy_bytes = std::fs::read(&source).unwrap();
        let sources = vec![destination.clone(), source.clone(), source.clone()];
        let merged = import_legacy(&destination, original, sources.clone());
        assert_eq!(merged.len(), 2);
        assert_eq!(merged.iter().map(|r| r.opened).collect::<Vec<_>>(), [3, 2]);
        assert_eq!(load_from(&destination), merged);
        assert_eq!(std::fs::read(&source).unwrap(), legacy_bytes);

        crate::save_config(&destination, &Vec::<Recent>::new()).unwrap();
        assert!(import_legacy(&destination, Vec::new(), sources).is_empty());
        assert!(load_from(&destination).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

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
