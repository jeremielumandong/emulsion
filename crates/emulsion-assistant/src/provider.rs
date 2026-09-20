//! The coding CLIs Emulsion can drive, and how to find them. A GUI-launched
//! process does not inherit the shell's PATH, so common install locations
//! are searched too.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// How a CLI runs a conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One long-lived process; turns are written to its stdin (Claude Code).
    Persistent,
    /// One process per turn, resumed by session id where the CLI allows.
    OneShot,
}

/// How the CLI learns about Emulsion's MCP server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpConfig {
    /// `--mcp-config file.json` (Claude Code).
    ClaudeJson,
    /// A scoped `CODEX_HOME` with `config.toml` (Codex).
    CodexHome,
    /// `OPENCODE_CONFIG=opencode.json` (OpenCode).
    OpenCodeJson,
    /// `.kimi-code/mcp.json` in the working directory (Kimi Code).
    KimiJson,
}

pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub binary: &'static str,
    pub install_hint: &'static str,
    pub mode: Mode,
    pub mcp: McpConfig,
    /// The CLI asks the host before each tool call (Claude Code's stdio
    /// control channel). Others run tools at once, so Emulsion gates them
    /// itself at the relay.
    pub permission_prompts: bool,
    /// Model choices to offer: (label, value passed to the CLI); `None` is
    /// the CLI's own default.
    pub models: &'static [(&'static str, Option<&'static str>)],
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "claude",
        label: "Claude Code",
        binary: "claude",
        install_hint: "npm install -g @anthropic-ai/claude-code",
        mode: Mode::Persistent,
        mcp: McpConfig::ClaudeJson,
        permission_prompts: true,
        models: &[
            ("default", None),
            ("sonnet", Some("sonnet")),
            ("opus", Some("opus")),
            ("haiku", Some("haiku")),
        ],
    },
    Provider {
        id: "codex",
        label: "Codex",
        binary: "codex",
        install_hint: "npm install -g @openai/codex",
        mode: Mode::OneShot,
        mcp: McpConfig::CodexHome,
        permission_prompts: false,
        models: &[
            ("default", None),
            ("gpt-5.5", Some("gpt-5.5")),
            ("gpt-5.5-codex", Some("gpt-5.5-codex")),
            ("gpt-5-codex-mini", Some("gpt-5-codex-mini")),
        ],
    },
    Provider {
        id: "opencode",
        label: "OpenCode",
        binary: "opencode",
        install_hint: "npm install -g opencode-ai",
        mode: Mode::OneShot,
        mcp: McpConfig::OpenCodeJson,
        permission_prompts: false,
        models: &[("default", None)],
    },
    Provider {
        id: "kimi",
        label: "Kimi Code",
        binary: "kimi",
        install_hint: "npm install -g @moonshot-ai/kimi-cli",
        mode: Mode::OneShot,
        mcp: McpConfig::KimiJson,
        permission_prompts: false,
        models: &[("default", None)],
    },
];

pub fn default_provider() -> &'static Provider {
    &PROVIDERS[0]
}

/// The provider with this id, else the default.
pub fn by_id(id: &str) -> &'static Provider {
    PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .unwrap_or_else(default_provider)
}

pub(crate) fn home() -> Option<PathBuf> {
    home_from_env(|key| std::env::var_os(key))
}

fn home_from_env(mut get: impl FnMut(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(home) = get("USERPROFILE") {
        return Some(PathBuf::from(home));
    }
    get("HOME").map(PathBuf::from)
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
            ".opencode/bin",
            ".cargo/bin",
        ] {
            v.push(h.join(d));
        }
        #[cfg(windows)]
        {
            v.push(h.join("AppData/Roaming/npm"));
            v.push(h.join("AppData/Local/Programs/claude"));
        }
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(appdata).join("npm"));
    }
    #[cfg(windows)]
    {
        // Explorer can keep the PATH from before Node was installed.
        for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(key) {
                v.push(PathBuf::from(root).join("nodejs"));
            }
        }
        for key in ["NVM_SYMLINK", "VOLTA_HOME", "FNM_MULTISHELL_PATH"] {
            if let Some(root) = std::env::var_os(key) {
                let root = PathBuf::from(root);
                v.push(if key == "VOLTA_HOME" {
                    root.join("bin")
                } else {
                    root
                });
            }
        }
    }
    v
}

/// An extensionless npm shim is a Unix shell script on Windows. Resolve its
/// Windows sibling, including paths previously saved by older app versions.
fn resolve_path(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if path.extension().is_none() {
            return ["exe", "com", "cmd", "bat", "ps1"]
                .into_iter()
                .map(|extension| path.with_extension(extension))
                .find(|candidate| candidate.is_file());
        }
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        if !["exe", "com", "cmd", "bat", "ps1"].contains(&extension.as_str()) {
            return None;
        }
    }
    executable(path).then(|| path.to_path_buf())
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
    if let Some(p) = explicit.and_then(resolve_path) {
        return Some(p);
    }
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    path_dirs
        .into_iter()
        .chain(extra_dirs())
        .map(|d| d.join(binary))
        .find_map(|p| resolve_path(&p))
}

/// Build the same invocation for both discovery and chat. Standard npm shims
/// point at a Node script or a native executable; launch that target directly
/// so prompts containing newlines, quotes and shell characters stay arguments.
pub(crate) fn command(path: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let path = resolve_path(path).unwrap_or_else(|| path.to_path_buf());
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let mut command = if let Some((program, script)) = npm_target(&path) {
            let mut command = Command::new(program);
            if let Some(script) = script {
                command.arg(script);
            }
            command
        } else if extension.eq_ignore_ascii_case("ps1") {
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]);
            command.arg(&path);
            command
        } else {
            // Rust invokes .cmd/.bat through cmd.exe with its batch escaping.
            Command::new(&path)
        };
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    }
    #[cfg(not(windows))]
    Command::new(path)
}

#[cfg(windows)]
fn npm_target(shim: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let extension = shim.extension()?.to_str()?;
    let prefix = if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
        "%dp0%"
    } else if extension.eq_ignore_ascii_case("ps1") {
        "$basedir"
    } else {
        return None;
    };
    let text = std::fs::read_to_string(shim).ok()?;
    let base = shim.parent()?;
    for quoted in text.split('"').skip(1).step_by(2) {
        let Some(relative) = quoted.strip_prefix(prefix) else {
            continue;
        };
        let relative = relative.trim_start_matches(['/', '\\']);
        // Only collapse npm's standard node_modules launchers, never an
        // unrelated batch script whose setup could be significant.
        if !relative.starts_with("node_modules/") && !relative.starts_with("node_modules\\") {
            continue;
        }
        let target = base.join(relative);
        if !target.is_file() {
            continue;
        }
        match target.extension().and_then(|s| s.to_str()) {
            Some("exe") => return Some((target, None)),
            Some("js" | "cjs" | "mjs") => {
                let node = resolve_path(&base.join("node")).or_else(|| find("node", None))?;
                return Some((node, Some(target)));
            }
            _ => {}
        }
    }
    None
}

/// Every provider whose binary is installed.
pub fn installed() -> Vec<&'static Provider> {
    PROVIDERS
        .iter()
        .filter(|p| find(p.binary, None).is_some())
        .collect()
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

/// `<cli> --version`, with a timeout.
pub fn version(path: &Path) -> Option<String> {
    let mut command = command(path);
    command
        .arg("--version")
        .env("PATH", child_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("emulsion provider {name} {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(windows)]
    #[test]
    fn windows_home_uses_userprofile_without_posix_home() {
        assert_eq!(
            home_from_env(|key| (key == "USERPROFILE").then(|| r"C:\Users\Example User".into())),
            Some(PathBuf::from(r"C:\Users\Example User"))
        );
        assert_eq!(
            home_from_env(|key| (key == "HOME").then(|| r"D:\home".into())),
            Some(PathBuf::from(r"D:\home"))
        );
        assert_eq!(
            home_from_env(|key| Some(
                if key == "USERPROFILE" {
                    r"C:\Users\Windows"
                } else {
                    r"D:\Unix"
                }
                .into()
            )),
            Some(PathBuf::from(r"C:\Users\Windows"))
        );
        assert_eq!(home_from_env(|_| None), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_resolution_ignores_unix_shims_and_repairs_saved_paths() {
        let dir = test_dir("resolution");
        let bare = dir.join("codex");
        std::fs::write(&bare, "#!/bin/sh\n").unwrap();
        assert_eq!(resolve_path(&bare), None);
        std::fs::write(bare.with_extension("cmd"), "@echo off\r\n").unwrap();
        assert_eq!(find("codex", Some(&bare)), Some(bare.with_extension("cmd")));
        std::fs::write(bare.with_extension("exe"), "native").unwrap();
        assert_eq!(resolve_path(&bare), Some(bare.with_extension("exe")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_batch_and_powershell_versions_work_in_paths_with_spaces() {
        let dir = test_dir("versions");
        let batch = dir.join("fake.cmd");
        std::fs::write(&batch, "@echo off\r\necho fake 1.2.3\r\n").unwrap();
        assert_eq!(version(&batch).as_deref(), Some("fake 1.2.3"));
        let ps1 = dir.join("fake.ps1");
        std::fs::write(&ps1, "Write-Output 'fake 2.3.4'\r\n").unwrap();
        assert_eq!(version(&ps1).as_deref(), Some("fake 2.3.4"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn npm_shims_preserve_multiline_quoted_arguments_and_stdin() {
        if find("node", None).is_none() {
            eprintln!("Node is not installed; skipping npm runtime integration test");
            return;
        }
        let dir = test_dir("npm arguments");
        std::fs::create_dir_all(dir.join("node_modules/fake/bin")).unwrap();
        std::fs::write(
            dir.join("node_modules/fake/bin/cli.js"),
            "process.stdout.write(process.argv[2]); process.stdin.pipe(process.stdout);",
        )
        .unwrap();
        let prompt = "first line\r\nsecond line \"quoted\" & | < > %PATH% !hello! ^ \\ trailing\\";
        for (extension, body) in [
            (
                "cmd",
                "@echo off\r\n\"%_prog%\" \"%dp0%\\node_modules\\fake\\bin\\cli.js\" %*\r\n",
            ),
            (
                "ps1",
                "& \"node$exe\" \"$basedir/node_modules/fake/bin/cli.js\" $args\r\n",
            ),
        ] {
            let shim = dir.join(format!("fake.{extension}"));
            std::fs::write(&shim, body).unwrap();
            let mut child = command(&shim)
                .arg(prompt)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"\nstdin survived")
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("{prompt}\nstdin survived")
            );
        }
        let native = dir.join("node_modules/fake/bin/fake.exe");
        std::fs::write(&native, "native").unwrap();
        let shim = dir.join("native.cmd");
        std::fs::write(&shim, "\"%dp0%\\node_modules\\fake\\bin\\fake.exe\" %*").unwrap();
        assert_eq!(
            Path::new(command(&shim).get_program())
                .canonicalize()
                .unwrap(),
            native.canonicalize().unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn providers_are_distinct_and_lookup_falls_back() {
        let mut ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PROVIDERS.len());
        assert_eq!(by_id("codex").mode, Mode::OneShot);
        assert_eq!(by_id("nope").id, "claude");
        assert!(by_id("claude").permission_prompts && !by_id("opencode").permission_prompts);
    }
}
