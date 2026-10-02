//! Updates from the project's published GitHub releases.
//!
//! The latest published release is compared with this build's version. When
//! it is newer and Emulsion was installed from a release package, the matching
//! asset is downloaded and verified against the SHA-256 digest GitHub records
//! for it (or the `.sha256` file published beside it) before anything is
//! replaced. Development builds and unpacked copies are only told about the
//! release; they are never modified.

use serde::Deserialize;
use sha2::Digest;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The repository whose releases this build follows.
pub const REPOSITORY: &str = "jeremielumandong/emulsion";

/// This build's version.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("could not reach GitHub: {0}")]
    Http(String),
    #[error("GitHub returned an unexpected release description")]
    Format,
    #[error("the release has no {0} download")]
    MissingAsset(String),
    #[error("the release publishes no checksum for {0}, so it was not installed")]
    MissingChecksum(String),
    #[error("{0} did not match its published checksum and was discarded")]
    Checksum(String),
    #[error("download cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    /// "sha256:<hex>", recorded by GitHub at upload.
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    /// "v0.2.0" → "0.2.0".
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    /// Whether this is a published, stable release newer than `current`.
    pub fn is_newer_than(&self, current: &str) -> bool {
        !self.draft
            && !self.prerelease
            && matches!(
                (parse_version(self.version()), parse_version(current)),
                (Some(new), Some(old)) if new > old
            )
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// "1.2.3" → (1, 2, 3). Pre-release and build suffixes are not stable
/// releases and parse as `None`.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// How this copy of Emulsion was installed, which decides how it updates.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// A Linux AppImage at this path, replaced in place.
    AppImage(PathBuf),
    /// A Windows per-user installation; the new installer runs after exit.
    WindowsInstaller,
    /// A macOS application bundle; the new disk image is opened.
    MacApp(PathBuf),
    /// A development build, Flatpak, or unpacked copy: nothing is replaced.
    Unmanaged,
}

impl Install {
    pub fn detect() -> Self {
        if cfg!(debug_assertions) {
            return Install::Unmanaged;
        }
        let exe = std::env::current_exe().ok();
        Self::from_environment(
            exe.as_deref(),
            std::env::var_os("APPIMAGE").map(PathBuf::from),
            std::env::var_os("FLATPAK_ID").is_some(),
        )
    }

    fn from_environment(exe: Option<&Path>, appimage: Option<PathBuf>, flatpak: bool) -> Self {
        if cfg!(target_os = "linux") {
            if flatpak {
                return Install::Unmanaged;
            }
            return match appimage {
                Some(path) if path.is_absolute() && path.is_file() => Install::AppImage(path),
                _ => Install::Unmanaged,
            };
        }
        let Some(exe) = exe else {
            return Install::Unmanaged;
        };
        if cfg!(windows) {
            // The NSIS installer places its uninstaller beside the executable.
            return match exe.parent() {
                Some(dir) if dir.join("uninstall.exe").is_file() => Install::WindowsInstaller,
                _ => Install::Unmanaged,
            };
        }
        if cfg!(target_os = "macos") {
            // …/Emulsion.app/Contents/MacOS/emulsion
            let bundle = exe.ancestors().nth(3);
            return match bundle {
                Some(app) if app.extension().is_some_and(|e| e == "app") => {
                    Install::MacApp(app.to_path_buf())
                }
                _ => Install::Unmanaged,
            };
        }
        Install::Unmanaged
    }

    /// The release asset this installation updates from, if it can update.
    pub fn asset_name(&self, version: &str) -> Option<String> {
        match self {
            Install::AppImage(_) if cfg!(target_arch = "x86_64") => {
                Some(format!("Emulsion-{version}-x86_64.AppImage"))
            }
            Install::WindowsInstaller if cfg!(target_arch = "x86_64") => {
                Some("Emulsion-windows-x64-setup.exe".into())
            }
            Install::MacApp(_) if cfg!(target_arch = "aarch64") => {
                Some("Emulsion-macos-arm64.dmg".into())
            }
            _ => None,
        }
    }
}

fn agent(body_timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_body(Some(body_timeout))
            .max_redirects(10)
            .build(),
    )
}

fn get(url: &str, accept: &str, body_timeout: Duration) -> Result<ureq::Body, UpdateError> {
    agent(body_timeout)
        .get(url)
        .header(
            "User-Agent",
            concat!("emulsion/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", accept)
        .call()
        .map(|r| r.into_body())
        .map_err(|e| UpdateError::Http(e.to_string()))
}

/// The latest published release. Drafts and pre-releases are never returned.
pub fn latest_release() -> Result<Release, UpdateError> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let mut body = get(&url, "application/vnd.github+json", Duration::from_secs(30))?;
    let text = body
        .read_to_string()
        .map_err(|e| UpdateError::Http(e.to_string()))?;
    serde_json::from_str(&text).map_err(|_| UpdateError::Format)
}

/// The expected SHA-256 of `asset`: GitHub's recorded digest, else the
/// published `<name>.sha256` file.
fn expected_sha256(release: &Release, asset: &Asset) -> Result<String, UpdateError> {
    if let Some(hex) = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|hex| is_sha256(hex))
    {
        return Ok(hex.to_ascii_lowercase());
    }
    let sidecar = release
        .asset(&format!("{}.sha256", asset.name))
        .ok_or_else(|| UpdateError::MissingChecksum(asset.name.clone()))?;
    let mut body = get(
        &sidecar.browser_download_url,
        "application/octet-stream",
        Duration::from_secs(30),
    )?;
    let text = body
        .with_config()
        .limit(4096)
        .read_to_string()
        .map_err(|e| UpdateError::Http(e.to_string()))?;
    parse_checksum_file(&text, &asset.name)
        .ok_or_else(|| UpdateError::MissingChecksum(asset.name.clone()))
}

fn is_sha256(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// "<hex>  <name>" as written by sha256sum, for exactly this file.
fn parse_checksum_file(text: &str, name: &str) -> Option<String> {
    let mut fields = text.trim_start_matches('\u{feff}').split_whitespace();
    let hex = fields.next()?;
    let file = fields.next()?.trim_start_matches('*');
    (is_sha256(hex) && file == name).then(|| hex.to_ascii_lowercase())
}

/// Where downloads wait until they are installed.
pub fn download_dir() -> PathBuf {
    crate::recent::data_dir().join("updates")
}

/// Download this installation's asset from `release` into [`download_dir`],
/// verifying its size and checksum. `progress` receives (bytes, total).
pub fn download(
    release: &Release,
    install: &Install,
    progress: &dyn Fn(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, UpdateError> {
    let name = install
        .asset_name(release.version())
        .ok_or_else(|| UpdateError::MissingAsset(std::env::consts::OS.into()))?;
    download_asset(release, &name, &download_dir(), progress, cancel)
}

fn download_asset(
    release: &Release,
    name: &str,
    dir: &Path,
    progress: &dyn Fn(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, UpdateError> {
    let asset = release
        .asset(name)
        .ok_or_else(|| UpdateError::MissingAsset(name.into()))?;
    let expected = expected_sha256(release, asset)?;

    // Only one pending update is kept.
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir)?;
    let dest = dir.join(&asset.name);
    let part = dir.join(format!("{}.part", asset.name));

    let result = fetch(asset, &part, &expected, progress, cancel);
    if let Err(error) = result {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    fs::rename(&part, &dest)?;
    Ok(dest)
}

fn fetch(
    asset: &Asset,
    part: &Path,
    expected: &str,
    progress: &dyn Fn(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), UpdateError> {
    let body = get(
        &asset.browser_download_url,
        "application/octet-stream",
        Duration::from_secs(1800),
    )?;
    let mut reader = body
        .into_with_config()
        .limit(asset.size.saturating_add(1))
        .reader();
    let mut out = std::io::BufWriter::new(fs::File::create(part)?);
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut got = 0u64;
    let mut reported = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(UpdateError::Cancelled);
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        got += n as u64;
        if got - reported >= 1 << 20 {
            reported = got;
            progress(got, asset.size);
        }
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    progress(got, asset.size);
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != asset.size || digest != expected {
        return Err(UpdateError::Checksum(asset.name.clone()));
    }
    Ok(())
}

/// Atomically replace the AppImage at `target` with the verified `download`.
/// The running copy keeps its open file, so it is unaffected until restart.
pub fn replace_appimage(download: &Path, target: &Path) -> Result<(), UpdateError> {
    let dir = target
        .parent()
        .ok_or_else(|| std::io::Error::other("the AppImage has no parent directory"))?;
    let staged = dir.join(format!(
        ".{}.update-{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Emulsion.AppImage".into()),
        std::process::id()
    ));
    let result = (|| {
        fs::copy(download, &staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
        }
        fs::File::open(&staged)?.sync_all()?;
        fs::rename(&staged, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result?;
    let _ = fs::remove_file(download);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"{
        "tag_name": "v0.2.0",
        "html_url": "https://github.com/jeremielumandong/emulsion/releases/tag/v0.2.0",
        "name": "Emulsion v0.2.0",
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": "Emulsion-0.2.0-x86_64.AppImage", "size": 3,
             "browser_download_url": "https://example.invalid/a",
             "digest": "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"},
            {"name": "Emulsion-windows-x64-setup.exe", "size": 3,
             "browser_download_url": "https://example.invalid/b", "digest": null},
            {"name": "Emulsion-windows-x64-setup.exe.sha256", "size": 99,
             "browser_download_url": "https://example.invalid/c"}
        ]
    }"#;

    #[test]
    fn github_release_json_is_read() {
        let release: Release = serde_json::from_str(RELEASE).unwrap();
        assert_eq!(release.version(), "0.2.0");
        assert_eq!(release.assets.len(), 3);
        assert_eq!(release.assets[1].digest, None);
        let appimage = release.asset("Emulsion-0.2.0-x86_64.AppImage").unwrap();
        assert_eq!(
            expected_sha256(&release, appimage).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn only_newer_stable_releases_are_offered() {
        let mut release: Release = serde_json::from_str(RELEASE).unwrap();
        assert!(release.is_newer_than("0.1.1"));
        assert!(release.is_newer_than("0.1.10"));
        assert!(!release.is_newer_than("0.2.0"));
        assert!(!release.is_newer_than("0.10.0"));
        release.prerelease = true;
        assert!(!release.is_newer_than("0.1.1"));
        release.prerelease = false;
        release.tag_name = "v0.3.0-rc.1".into();
        assert!(!release.is_newer_than("0.1.1"));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("10.0.2"), Some((10, 0, 2)));
    }

    #[test]
    fn checksum_files_must_name_the_asset() {
        let hex = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";
        assert_eq!(
            parse_checksum_file(&format!("\u{feff}{hex}  app.exe\r\n"), "app.exe").as_deref(),
            Some(hex.to_ascii_lowercase().as_str())
        );
        assert_eq!(
            parse_checksum_file(&format!("{hex} *app.exe"), "app.exe").as_deref(),
            Some(hex.to_ascii_lowercase().as_str())
        );
        assert_eq!(
            parse_checksum_file(&format!("{hex}  other.exe"), "app.exe"),
            None
        );
        assert_eq!(parse_checksum_file("abc  app.exe", "app.exe"), None);
    }

    #[test]
    fn installation_kind_follows_the_package() {
        let dir = tempfile::tempdir().unwrap();
        if cfg!(target_os = "linux") {
            let image = dir.path().join("Emulsion.AppImage");
            fs::write(&image, b"x").unwrap();
            assert_eq!(
                Install::from_environment(None, Some(image.clone()), false),
                Install::AppImage(image.clone())
            );
            assert_eq!(
                Install::from_environment(None, Some(image), true),
                Install::Unmanaged
            );
            assert_eq!(
                Install::from_environment(None, Some("relative".into()), false),
                Install::Unmanaged
            );
            assert_eq!(
                Install::from_environment(None, None, false),
                Install::Unmanaged
            );
        }
        assert_eq!(Install::Unmanaged.asset_name("0.2.0"), None);
    }

    /// Serve each body once per request on a local port, in order.
    fn serve(bodies: Vec<Vec<u8>>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for body in bodies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        format!("http://{address}")
    }

    fn served_release(base: &str, digest: Option<&str>, sidecar: bool) -> Release {
        let mut assets = vec![Asset {
            name: "app.bin".into(),
            size: 3,
            browser_download_url: format!("{base}/app.bin"),
            digest: digest.map(|d| format!("sha256:{d}")),
        }];
        if sidecar {
            assets.push(Asset {
                name: "app.bin.sha256".into(),
                size: 75,
                browser_download_url: format!("{base}/app.bin.sha256"),
                digest: None,
            });
        }
        Release {
            tag_name: "v9.0.0".into(),
            html_url: String::new(),
            name: None,
            draft: false,
            prerelease: false,
            assets,
        }
    }

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn downloads_are_verified_before_they_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let updates = dir.path().join("updates");
        let none = AtomicBool::new(false);

        // GitHub's digest.
        let release = served_release(&serve(vec![b"abc".to_vec()]), Some(ABC), false);
        let file = download_asset(&release, "app.bin", &updates, &|_, _| {}, &none).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"abc");

        // The published .sha256 file when there is no digest.
        let base = serve(vec![
            format!("{ABC}  app.bin\n").into_bytes(),
            b"abc".to_vec(),
        ]);
        let release = served_release(&base, None, true);
        let file = download_asset(&release, "app.bin", &updates, &|_, _| {}, &none).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"abc");

        // Tampered bytes are discarded.
        let release = served_release(&serve(vec![b"abd".to_vec()]), Some(ABC), false);
        assert!(matches!(
            download_asset(&release, "app.bin", &updates, &|_, _| {}, &none),
            Err(UpdateError::Checksum(_))
        ));
        assert_eq!(fs::read_dir(&updates).unwrap().count(), 0);

        // No checksum at all: nothing is downloaded.
        let release = served_release("http://127.0.0.1:9", None, false);
        assert!(matches!(
            download_asset(&release, "app.bin", &updates, &|_, _| {}, &none),
            Err(UpdateError::MissingChecksum(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn appimage_is_replaced_atomically_and_made_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Emulsion.AppImage");
        let download = dir.path().join("new.AppImage");
        fs::write(&target, b"old").unwrap();
        fs::write(&download, b"new").unwrap();
        replace_appimage(&download, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!download.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
