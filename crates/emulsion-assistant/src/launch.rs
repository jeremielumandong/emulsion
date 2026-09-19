//! Building the CLI invocation.
//!
//! Scope, deliberately narrow:
//! * `--tools ""` removes every built-in tool (shell, file edits, web), and
//!   `--restricted` ignores user and project settings, so the only tools are
//!   Emulsion's.
//! * `--strict-mcp-config --mcp-config <file>` attaches only Emulsion's MCP
//!   server. The relay address and token travel in the file's `env` block,
//!   never on a command line.
//! * `--permission-prompts host` with the stdio prompt tool sends every
//!   confirmation to Emulsion, which
//!   shows it as an Apply/Skip card. Only read-only tools are pre-allowed.

use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// e.g. "sonnet", "opus"; `None` uses the CLI's default.
    pub model: Option<String>,
    /// Resume an earlier conversation.
    pub resume: Option<String>,
}

pub const SYSTEM_PROMPT: &str = "\
You are the assistant inside Emulsion, a non-destructive image editor. The person has a document \
open and talks to you instead of clicking. You change the document only through the emulsion \
tools; there are no other tools.

How the document works: it is a stack of nodes, listed top first. Pixel nodes hold images; \
adjustment nodes (exposure, levels, hue/saturation, white balance, brightness/contrast, invert) \
change everything below them in the same group; groups contain nodes. Nothing is destroyed: \
prefer adding or tuning an adjustment node over anything else, and never delete unless asked.

Working rules:
- Call describe_document before changing anything, and refer to nodes by the ids it returns. \
  Row 1 is the top of the stack. 'The top two nodes' means rows 1 and 2.
- When a request depends on what the picture looks like, call get_view and base the change on \
  what you see.
- Do exactly what was asked, with the fewest changes. Each change is shown to the person, who \
  can apply or skip it. If a change is skipped, do not retry it.
- If the request is ambiguous, ask one short question instead of guessing.
- Finish with one or two plain sentences saying what you changed.

Drawing and painting: you can paint with real brushes. list_brushes gives the library (ink, \
pencil, chalk, marker, watercolour, oil, airbrush, eraser, smudge) and paint lays strokes on a \
pixel layer as polylines in document pixels, with optional pressure per point. Work like an \
artist: add_layer for each stage (sketch, lines, colour, shading) so stages stay separable; \
block in big shapes first with large soft or wet brushes, then structure with mid brushes, then \
line work with an ink brush using many close points for smooth curves and pressure that swells \
in the middle of a stroke; shade with hatching (many short parallel strokes) or with airbrush \
and smudge. A stroke is a polyline, so a circle needs 24 or more points and a curve needs \
points every few pixels. Keep every paint call to one stage of the drawing and call get_view \
after each stage to look at the result and correct it before moving on. Use the canvas size \
from describe_document to place things; never paint outside it.";

/// Qualified names of the tools that never need confirmation.
pub fn read_only_tools() -> Vec<String> {
    emulsion_mcp::tools::READ_ONLY
        .iter()
        .map(|t| emulsion_mcp::tools::qualified(t))
        .collect()
}

/// Write the MCP config for this session and return its path.
pub fn write_mcp_config(
    dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let env: serde_json::Map<String, serde_json::Value> = relay_env
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    let config = json!({
        "mcpServers": {
            emulsion_mcp::SERVER_NAME: {
                "command": exe.to_string_lossy(),
                "args": ["mcp-serve"],
                "env": env,
            }
        }
    });
    let path = dir.join("mcp.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

/// The argv for a persistent, bidirectional stream-json session.
pub fn claude_args(mcp_config: &Path, opts: &Options) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--restricted",
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(mcp_config.to_string_lossy().into_owned());
    // Confirmations come to Emulsion over the stdio control channel.
    a.extend(["--permission-prompts".into(), "host".into()]);
    a.extend(["--permission-prompt-tool".into(), "stdio".into()]);
    a.push("--allowedTools".into());
    a.push(read_only_tools().join(","));
    a.extend(["--append-system-prompt".into(), SYSTEM_PROMPT.into()]);
    if let Some(m) = opts.model.as_ref().filter(|m| !m.is_empty()) {
        a.extend(["--model".into(), m.clone()]);
    }
    if let Some(r) = opts.resume.as_ref().filter(|r| !r.is_empty()) {
        a.extend(["--resume".into(), r.clone()]);
    }
    a
}

/// Everything needed to start the CLI for one document.
pub fn claude(
    program: PathBuf,
    session_dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
    opts: &Options,
) -> std::io::Result<LaunchSpec> {
    let config = write_mcp_config(session_dir, exe, relay_env)?;
    Ok(LaunchSpec {
        program,
        args: claude_args(&config, opts),
        env: vec![
            ("NO_COLOR".into(), "1".into()),
            ("FORCE_COLOR".into(), "0".into()),
            (
                "PATH".into(),
                crate::provider::child_path().to_string_lossy().into_owned(),
            ),
        ],
        cwd: session_dir.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_is_scoped_to_emulsion_tools() {
        let a = claude_args(
            Path::new("/tmp/s/mcp.json"),
            &Options {
                model: Some("sonnet".into()),
                resume: None,
            },
        );
        let pos = |f: &str| {
            a.iter()
                .position(|x| x == f)
                .unwrap_or_else(|| panic!("{f} missing"))
        };
        assert_eq!(a[pos("--tools") + 1], "", "no built-in tools");
        assert_eq!(a[pos("--permission-prompts") + 1], "host");
        assert_eq!(
            a[pos("--permission-prompt-tool") + 1],
            "stdio",
            "confirmations come to Emulsion"
        );
        assert_eq!(a[pos("--mcp-config") + 1], "/tmp/s/mcp.json");
        assert_eq!(
            a[pos("--allowedTools") + 1],
            "mcp__emulsion__describe_document,mcp__emulsion__get_view,mcp__emulsion__list_history,mcp__emulsion__compare,mcp__emulsion__list_brushes,mcp__emulsion__list_recipes"
        );
        assert_eq!(a[pos("--model") + 1], "sonnet");
        assert!(
            a.contains(&"--restricted".to_string())
                && a.contains(&"--strict-mcp-config".to_string())
        );
        assert!(
            !a.iter()
                .any(|x| x.contains("skip-permissions") || x == "bypassPermissions")
        );
    }

    #[test]
    fn mcp_config_carries_relay_env_privately() {
        let dir = std::env::temp_dir().join(format!("emulsion-launch-test-{}", std::process::id()));
        let p = write_mcp_config(
            &dir,
            Path::new("/usr/bin/emulsion"),
            &[("EMULSION_TOKEN".into(), "abc".into())],
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["emulsion"]["args"][0], "mcp-serve");
        assert_eq!(v["mcpServers"]["emulsion"]["env"]["EMULSION_TOKEN"], "abc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
