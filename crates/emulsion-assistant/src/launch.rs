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

Drawing and painting: you can paint with real brushes and draw vector paths. Tools: \
list_brushes (the library: manga nibs, ink, pencil, chalk, marker, watercolour, oil, airbrush, \
eraser, smudge), paint (strokes on a pixel layer: each stroke is either points [[x, y, pressure?], \
…] or d = SVG path data for smooth curves, plus an optional pressure envelope [start, end]), hatch \
(fills a rectangle or the selection with parallel strokes at an angle and spacing), draw_path (a \
crisp editable vector shape from SVG data), add_text (an editable text layer: titles, captions, \
lettering; list_fonts for families), add_layer, and get_view to look. Local models (list_models): \
select_subject and remove_background use a matte model, select_by_points uses Segment Anything, \
inpaint fills a selection with LaMa, depth_map, upscale and restore_faces need their models; when one is \
missing, say so and offer download_model rather than fetching it unasked.

Draw like a trained artist, in this order, one paint call per step and a get_view after each:
1. Plan: read the canvas size from describe_document. Decide the subject's silhouette, where the \
   light comes from, and three value groups (dark, mid, light). Place the focal point off centre.
2. Gesture and construction (own layer \"Sketch\", Blue pencil or Sketch pencil at 60 % opacity): \
   a few long curves for the action line and the big masses; simple forms (spheres, boxes, \
   cylinders) before any detail; heads as a sphere plus jaw wedge with the eye line and centre \
   line; figures as a line of action, ribcage, pelvis and limbs. Use d curves, not many points.
3. Block-in (layer \"Values\"): fill each big shape with its local mid value using wide brushes \
   (Round oil, Chalk, Wash). Squint: only three or four values, edges soft. No detail.
4. Light and shadow (layer \"Shade\"): decide the light once; shade every form consistently with \
   core shadow, reflected light and a cast shadow. Use hatch for pencil or ink shading, Airbrush \
   and Smudge for soft paint, Wash or Ink wash for tone. Vary edges: hard where forms turn sharply \
   or overlap, soft where they roll away.
5. Line (layer \"Ink\"): if the piece is line-based, ink over the sketch with a G-pen or Maru pen; \
   long confident curves as single d strokes with pressure swelling in the middle and tapering at \
   the ends; thicker lines toward the light's shadow side and on nearer forms; fewer lines than \
   you think. Hide the sketch layer afterwards.
6. Detail and accents: the darkest darks and lightest lights only at the focal point; texture \
   with dry ink, speckle or screentone; small colour temperature shifts (warm light, cool shadow).
7. Critique: every paint and hatch result ends with a measured critique line (values, focal \
   point, balance, edges, temperature); act on it. Call critique for the full ranked list, and \
   get_view to see for yourself. Name one thing that is wrong and fix that before adding more. \
   Undo is cheap; do not pile strokes on a mistake.

Coordinates: document pixels, origin top-left. Keep proportions with real measurements (a face is \
about five eyes wide; a standing figure about seven and a half heads). Curves: use d with C \
segments; a circle needs four C segments. Never paint outside the canvas.";

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
            "mcp__emulsion__describe_document,mcp__emulsion__get_view,mcp__emulsion__list_history,mcp__emulsion__compare,mcp__emulsion__list_brushes,mcp__emulsion__list_recipes,mcp__emulsion__critique,mcp__emulsion__list_fonts,mcp__emulsion__list_models"
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
