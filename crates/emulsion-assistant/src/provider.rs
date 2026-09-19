//! Finding the coding CLI. A GUI-launched process does not inherit the shell's
//! PATH, so common install locations are searched too.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub binary: &'static str,
    pub install_hint: &'static str,
}

/// One row for now; more CLIs join as the table is proven.
pub const PROVIDERS: &[Provider] = &[Provider {
    id: "claude",
    label: "Claude Code",
    binary: "claude",
    install_hint: "npm install -g @anthropic-ai/claude-code",
}];

pub fn default_provider() -> &'static Provider {
    &PROVIDERS[0]
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Directories to search after PATH.
pub fn extra_dirs() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
    ];
    if let Some(h) = home() {
        for d in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".local/share/mise/shims",
            ".volta/bin",
            ".bun/bin",
        ] {
            v.push(h.join(d));
        }
    }
    v
}

fn executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Resolve the CLI: an explicit path wins, then PATH, then common locations.
pub fn find(binary: &str, explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|p| executable(p)) {
        return Some(p.to_path_buf());
    }
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    path_dirs
        .into_iter()
        .chain(extra_dirs())
        .map(|d| d.join(binary))
        .find(|p| executable(p))
}

/// PATH for the child: ours plus the extra locations, so the CLI can find
/// node and friends when Emulsion was started from a launcher.
pub fn child_path() -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for d in extra_dirs() {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// `claude --version`, with a timeout.
pub fn version(path: &Path) -> Option<String> {
    let mut child = Command::new(path)
        .arg("--version")
        .env("PATH", child_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let v = out.lines().next()?.trim().to_string();
    (!v.is_empty()).then_some(v)
}
