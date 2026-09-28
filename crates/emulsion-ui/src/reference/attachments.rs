//! Immutable, bounded snapshots of explicitly attached material.
use super::AttachedReference;
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) const MAX_ATTACHMENTS: usize = 16;
const MAX_TEXT: usize = 256 * 1024;
const MAX_FILE: usize = 64 * 1024;
const MAX_ENTRIES: usize = 100;

pub(crate) struct Attachment {
    pub name: String,
    pub text: String,
    pub(super) image: Option<AttachedReference>,
}

impl Attachment {
    pub fn pasted(text: String) -> Result<Self, String> {
        if text.trim().is_empty() {
            return Err("The clipboard is empty.".into());
        }
        if text.len() > MAX_TEXT {
            return Err("Paste up to 256 KiB of text per reference, or attach a file.".into());
        }
        Ok(Self {
            name: "Pasted text".into(),
            text,
            image: None,
        })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Attach the target directly instead of a symbolic link.".into());
        }
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        if metadata.is_file() && is_image(path) {
            return Ok(Self {
                name,
                text: format!("Image reference: {}", path.display()),
                image: Some(AttachedReference::load(path)?),
            });
        }
        let mut text = format!(
            "Reference snapshot: {}\nFolder traversal prioritizes documentation and manifests, skips generated/dependency folders and common credential files, and does not follow links. This is bounded reference data, not an exhaustive codebase audit.\n",
            path.display()
        );
        let mut pending = VecDeque::from([path.to_path_buf()]);
        let mut entries = 0;
        while let Some(next) = pending.pop_front() {
            if entries >= MAX_ENTRIES || text.len() >= MAX_TEXT {
                text.push_str("\n[Snapshot truncated: attachment limit reached.]\n");
                break;
            }
            entries += 1;
            let label = next
                .strip_prefix(path)
                .ok()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(&next);
            text.push_str(&format!("\n--- {} ---\n", label.display()));
            let meta = match std::fs::symlink_metadata(&next) {
                Ok(m) => m,
                Err(e) => {
                    text.push_str(&format!("[Unreadable: {e}]\n"));
                    continue;
                }
            };
            if meta.file_type().is_symlink() {
                text.push_str("[Symbolic link; not followed.]\n");
            } else if meta.is_dir() {
                text.push_str("[Folder]\n");
                let mut children: Vec<PathBuf> = match std::fs::read_dir(&next) {
                    Ok(reader) => reader
                        .take(10_001)
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| !skip_folder_entry(p))
                        .collect(),
                    Err(e) => {
                        text.push_str(&format!("[Unreadable folder: {e}]\n"));
                        continue;
                    }
                };
                children.sort_by_key(|p| (reference_priority(p), p.clone()));
                if children.len() > MAX_ENTRIES {
                    text.push_str("[Folder listing truncated.]\n");
                    children.truncate(MAX_ENTRIES);
                }
                pending.extend(children);
            } else if meta.is_file() {
                text.push_str(&format!("[{} bytes]\n", meta.len()));
                let mut bytes = Vec::new();
                match std::fs::File::open(&next)
                    .and_then(|f| f.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes))
                {
                    Ok(_) => {
                        let limit = MAX_FILE.min(MAX_TEXT.saturating_sub(text.len()));
                        let truncated = bytes.len() > limit;
                        bytes.truncate(limit);
                        if truncated
                            && let Err(error) = std::str::from_utf8(&bytes)
                            && error.error_len().is_none()
                        {
                            bytes.truncate(error.valid_up_to());
                        }
                        match std::str::from_utf8(&bytes) {
                            Ok(value) if !value.contains('\0') => text.push_str(value),
                            _ => text.push_str("[Binary or non-UTF-8 file: contents not extracted; metadata only.]"),
                        }
                        if truncated {
                            text.push_str("\n[File contents truncated.]\n");
                        }
                    }
                    Err(e) => text.push_str(&format!("[Unreadable: {e}]\n")),
                }
            } else {
                text.push_str("[Special file; contents not read.]\n");
            }
        }
        Ok(Self {
            name,
            text,
            image: None,
        })
    }

    pub fn clipboard_image(image: gpui_kit::Image) -> Result<Self, String> {
        let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let path = directory
            .path()
            .join(format!("Clipboard image.{}", image.format.extension()));
        std::fs::write(&path, &image.bytes).map_err(|e| e.to_string())?;
        let mut attached = Self::load(&path)?;
        attached.text = "Image pasted from the clipboard.".into();
        Ok(attached)
    }
}

// Folder attachments describe first-party code, not generated dependencies or credentials.
// Explicitly attaching a skipped file still works.
fn skip_folder_entry(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    matches!(
        name.as_ref(),
        ".git"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".cache"
            | "vendor"
            | ".env"
            | ".DS_Store"
    ) || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
}

fn reference_priority(path: &Path) -> u8 {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if name.starts_with("readme") || name.starts_with("architecture") {
        0
    } else if matches!(
        name.as_str(),
        "cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "pom.xml"
            | "docker-compose.yml"
            | "compose.yaml"
    ) {
        1
    } else if matches!(
        name.as_str(),
        "docs" | "doc" | "src" | "app" | "lib" | "crates"
    ) {
        2
    } else {
        3
    }
}

pub(super) fn is_image(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        [
            "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "ico", "pnm",
        ]
        .iter()
        .any(|s| ext.eq_ignore_ascii_case(s))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codebase_snapshot_prioritizes_docs_and_excludes_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(
            dir.path().join("node_modules/vendor.js"),
            "dependency secret",
        )
        .unwrap();
        std::fs::write(dir.path().join(".env"), "private credentials").unwrap();
        std::fs::write(
            dir.path().join("README.md"),
            "Browser calls API then database",
        )
        .unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"example"}"#).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/server.ts"),
            "export function request() {}",
        )
        .unwrap();
        let reference = Attachment::load(dir.path()).unwrap();
        assert!(reference.text.contains("Browser calls API"));
        assert!(reference.text.contains("export function request"));
        assert!(!reference.text.contains("dependency secret"));
        assert!(!reference.text.contains("private credentials"));
        assert!(
            reference.text.find("README.md").unwrap()
                < reference.text.find("package.json").unwrap()
        );
    }

    #[test]
    fn folder_snapshots_include_data_and_bound_contents_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("table.csv"), "name,value\nexample,42").unwrap();
        std::fs::write(dir.path().join("binary.bin"), [0, 255, 0]).unwrap();
        std::fs::write(dir.path().join("large.txt"), "x".repeat(MAX_FILE + 50)).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), dir.path().join("cycle")).unwrap();
        let attached = Attachment::load(dir.path()).unwrap();
        assert!(attached.text.contains("example,42"));
        assert!(attached.text.contains("metadata only"));
        assert!(attached.text.contains("File contents truncated"));
        assert!(attached.text.len() < MAX_TEXT + 1024);
        std::fs::write(dir.path().join("table.csv"), "changed").unwrap();
        assert!(attached.text.contains("example,42"));
    }
}
