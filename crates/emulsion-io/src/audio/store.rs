//! Where sound bytes live: imported files are copied into a media cache (one
//! folder per running Emulsion, under the system temporary folder), and
//! packages stream them in and out of `audio/{id}.{format}` entries. An
//! asset's `source` always points into the cache, never at the file the
//! user picked, so moving or editing that file cannot change the document.
use super::AudioAsset;
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Sound file extensions import accepts, lower case.
pub const EXTENSIONS: &[&str] = &[
    "wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "aif", "aiff",
];

/// Most sound bytes one package holds (storyboard and timeline audio; the
/// 32 MiB Design media limit does not apply).
pub const MAX_PACKAGE_AUDIO: u64 = 2 << 30;

/// The package entry holding asset `id`.
pub fn entry_name(id: u64, format: &str) -> String {
    format!("audio/{id}.{format}")
}

/// This process's media cache folder, created on first use. Folders left
/// by Emulsion processes that are no longer running are removed then.
pub fn cache_root() -> Result<PathBuf> {
    static ROOT: OnceLock<std::result::Result<PathBuf, String>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let parent = std::env::temp_dir().join("emulsion-media");
        let root = parent.join(std::process::id().to_string());
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        remove_stale(&parent);
        Ok(root)
    })
    .clone()
    .map_err(|e| anyhow::anyhow!("Cannot create the media cache: {e}"))
}

/// Remove cache folders of processes that have exited (on Linux, by
/// checking `/proc`; elsewhere, folders untouched for a week).
fn remove_stale(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let me = std::process::id().to_string();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == me || !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let gone = if Path::new("/proc/self").exists() {
            !Path::new("/proc").join(&name).exists()
        } else {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 7 * 86_400)
        };
        if gone {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// A new, unused file path in the cache for bytes of `format`.
pub fn cache_path(format: &str) -> Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    Ok(cache_root()?.join(format!("{seq}-{nanos:08x}.{format}")))
}

/// Copy at most `limit` bytes from `reader` into a new cache file. Fails
/// (leaving nothing behind) when there are more.
pub fn copy_to_cache(reader: impl Read, format: &str, limit: u64) -> Result<(PathBuf, u64)> {
    let path = cache_path(format)?;
    let result = (|| {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let copied = std::io::copy(&mut reader.take(limit.saturating_add(1)), &mut out)?;
        if copied > limit {
            bail!("The sound is larger than {} MiB", limit >> 20)
        }
        out.flush()?;
        Ok(copied)
    })();
    match result {
        Ok(copied) => Ok((path, copied)),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

/// The format (lower-case extension) of a sound file, when import takes it.
pub fn format_of(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// Import a sound file: probe it, copy its bytes into the media cache and
/// return the asset (named after the file, in library `folder`) with
/// `source` set. Add it with `Timeline::add_asset`. The file is not changed.
pub fn import(path: &Path, folder: &str) -> Result<AudioAsset> {
    let format = format_of(path).with_context(|| {
        format!(
            "Choose a sound file ({})",
            EXTENSIONS
                .iter()
                .map(|e| format!(".{e}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let folder = folder.trim().trim_matches('/').to_string();
    if folder.chars().count() > 400
        || folder.chars().any(char::is_control)
        || folder.split('/').any(|p| p == "..")
    {
        bail!("Sound folders are up to 400 characters, without “..”")
    }
    let file =
        std::fs::File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("Choose a sound file, not a folder")
    }
    let probe = super::probe::probe(path)?;
    let (source, _) = copy_to_cache(file, &format, MAX_PACKAGE_AUDIO)?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let mut name: String = stem
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect();
    if name.trim().is_empty() {
        name = "Sound".into();
    }
    Ok(AudioAsset {
        name,
        format,
        duration_ms: probe.duration_ms,
        sample_rate: probe.sample_rate,
        channels: probe.channels,
        folder,
        source: Some(source),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;

    #[test]
    fn import_copies_into_the_cache_and_checks_the_file() {
        assert!(import(Path::new("/nope/a.txt"), "").is_err());
        assert!(import(Path::new("/nope/a.wav"), "").is_err());
        assert!(copy_to_cache(&b"12345"[..], "wav", 4).is_err());
        let (path, n) = copy_to_cache(&b"1234"[..], "wav", 4).unwrap();
        assert_eq!((std::fs::read(&path).unwrap(), n), (b"1234".to_vec(), 4));
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = test_audio::tone(dir.path(), "Door slam.mp3", 220, 1.);
        let asset = import(&file, "/Foley/Doors/").unwrap();
        assert_eq!(asset.name, "Door slam");
        assert_eq!(asset.format, "mp3");
        assert_eq!(asset.folder, "Foley/Doors");
        assert_eq!(asset.channels, 2);
        assert!((950..=1100).contains(&asset.duration_ms), "{asset:?}");
        let source = asset.source.unwrap();
        assert!(source.starts_with(cache_root().unwrap()));
        assert_eq!(
            std::fs::read(source).unwrap(),
            std::fs::read(&file).unwrap()
        );
        assert!(import(&file, "../up").is_err());
    }
}
