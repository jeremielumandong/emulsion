//! Explicit, platform-owned runtime installation. No browser or codecs are bundled.
use std::sync::atomic::{AtomicBool, Ordering};

static INSTALLING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstallPlan {
    pub program: &'static str,
    pub args: Vec<&'static str>,
    pub packages: &'static str,
}

pub(crate) struct Setup {
    pub message: String,
    pub install: Option<InstallPlan>,
    pub website: Option<&'static str>,
}

pub(crate) fn installing() -> bool {
    INSTALLING.load(Ordering::Acquire)
}

#[cfg(target_os = "linux")]
fn linux_plan(release: &str) -> Option<InstallPlan> {
    let ids: Vec<_> = release
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            matches!(key, "ID" | "ID_LIKE").then(|| value.trim().trim_matches(['"', '\'']))
        })
        .flat_map(str::split_whitespace)
        .collect();
    if ids.contains(&"arch") {
        Some(InstallPlan {
            program: "/usr/bin/pacman",
            args: vec![
                "-S",
                "--needed",
                "--noconfirm",
                "webkit2gtk-4.1",
                "gst-plugins-good",
                "gst-libav",
            ],
            packages: "webkit2gtk-4.1, gst-plugins-good, gst-libav",
        })
    } else if ids.contains(&"debian") || ids.contains(&"ubuntu") {
        Some(InstallPlan {
            program: "/usr/bin/apt-get",
            args: vec![
                "install",
                "-y",
                "libwebkit2gtk-4.1-0",
                "gstreamer1.0-plugins-good",
                "gstreamer1.0-libav",
            ],
            packages: "libwebkit2gtk-4.1-0, gstreamer1.0-plugins-good, gstreamer1.0-libav",
        })
    } else {
        None
    }
}

pub(crate) fn current() -> Setup {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("FLATPAK_ID").is_some()
            || std::path::Path::new("/.flatpak-info").exists()
        {
            return Setup {
                message: "This Flatpak uses the shared GNOME runtime for WebKitGTK and GStreamer playback. Update Emulsion and its runtime through your Flatpak software manager if playback is unavailable. Host codec packages do not modify the sandbox.".into(),
                install: None, website: None,
            };
        }
        let plan = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|release| linux_plan(&release));
        let available = plan.filter(|plan| std::path::Path::new(plan.program).is_file());
        if let Some(plan) = available {
            let message = format!(
                "Install or verify these system playback packages: {}. Your operating system will request administrator authentication. Downloads and system disk space are managed by your distribution; nothing is bundled into Emulsion. Close any playing video before installation, then retry playback.",
                plan.packages
            );
            if !std::path::Path::new("/usr/bin/pkexec").is_file() {
                return Setup {
                    message: format!(
                        "{message}\n\nThe system authentication helper is unavailable. Install these packages with your distribution's package manager."
                    ),
                    install: None,
                    website: None,
                };
            }
            return Setup {
                message,
                install: Some(plan),
                website: None,
            };
        }
        Setup { message: "Install WebKitGTK 4.1 and the GStreamer good and libav video decoders using your distribution's package manager, then retry playback. Automatic installation is available on supported Arch and Debian/Ubuntu systems.".into(), install: None, website: Some("https://webkitgtk.org/") }
    }
    #[cfg(target_os = "windows")]
    {
        Setup { message: "Embedded video uses the Microsoft Edge WebView2 Runtime. Open Microsoft's installer page, choose the Evergreen Runtime for this machine, install it, then retry playback. Emulsion does not bundle a browser.".into(), install: None, website: Some("https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section") }
    }
    #[cfg(target_os = "macos")]
    {
        Setup { message: "Embedded video uses macOS's built-in WebKit. No separate browser or codec installation is needed. System updates provide WebKit fixes; playback also requires internet access and a video that allows embedding.".into(), install: None, website: None }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Setup {
            message: "Embedded video playback is unavailable on this platform.".into(),
            install: None,
            website: None,
        }
    }
}

/// Called by the explicit setup-dialog action or native MCP installation tool.
/// Arguments are a fixed allowlist and never come from a document, URL or shell.
pub(crate) fn install(plan: InstallPlan) -> Result<String, String> {
    if INSTALLING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("Playback setup is already running.".into());
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            INSTALLING.store(false, Ordering::Release);
        }
    }
    let _reset = Reset;
    let output = std::process::Command::new("/usr/bin/pkexec")
        .arg(plan.program)
        .args(plan.args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("Could not start the system installer: {error}"))?;
    if output.status.success() {
        Ok("Playback packages are installed. Retry the video; reopen Emulsion if the runtime was newly installed.".into())
    } else {
        let details = String::from_utf8_lossy(&output.stderr);
        let details: String = details.chars().take(1600).collect();
        Err(format!(
            "Playback setup was canceled or failed. Your document is unchanged. {details}"
        ))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn distro_detection_only_builds_fixed_package_commands() {
        let arch = linux_plan("ID=omarchy\nID_LIKE=\"arch\"\n").unwrap();
        assert_eq!(arch.program, "/usr/bin/pacman");
        assert_eq!(
            &arch.args[3..],
            ["webkit2gtk-4.1", "gst-plugins-good", "gst-libav"]
        );
        let ubuntu = linux_plan("ID=ubuntu\nID_LIKE=debian").unwrap();
        assert_eq!(ubuntu.program, "/usr/bin/apt-get");
        assert!(linux_plan("ID=fedora").is_none());
        assert!(linux_plan("ID=arch;touch /tmp/unwanted").is_none());
    }
}
