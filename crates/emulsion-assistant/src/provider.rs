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
            ".opencode/bin",
            ".cargo/bin",
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

#[cfg(test)]
mod tests {
    use super::*;

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
