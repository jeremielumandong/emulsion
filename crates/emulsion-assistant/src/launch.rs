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

/// The relay's address and token as an env map for MCP config files.
fn env_map(relay_env: &[(String, String)]) -> serde_json::Map<String, serde_json::Value> {
    relay_env
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect()
}

fn common_env() -> Vec<(String, String)> {
    vec![
        ("NO_COLOR".into(), "1".into()),
        ("FORCE_COLOR".into(), "0".into()),
        (
            "PATH".into(),
            crate::provider::child_path().to_string_lossy().into_owned(),
        ),
    ]
}

/// A TOML string literal.
fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Codex: a scoped `CODEX_HOME` next to the session with the person's
/// sign-in copied in, their config kept, and Emulsion's MCP server added.
/// Approvals are off (Emulsion gates tool calls itself) and the shell
/// sandbox is read-only, so the only way Codex changes anything is through
/// Emulsion's tools.
pub fn write_codex_home(
    session_dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
) -> std::io::Result<PathBuf> {
    let home = session_dir.join("codex-home");
    std::fs::create_dir_all(&home)?;
    let user_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")));
    let mut config = String::new();
    if let Some(u) = &user_home {
        for f in ["auth.json", "version.json", "instructions.md"] {
            let _ = std::fs::copy(u.join(f), home.join(f));
        }
        if let Ok(existing) = std::fs::read_to_string(u.join("config.toml")) {
            // Keep everything except an earlier emulsion server block.
            let mut skipping = false;
            for line in existing.lines() {
                if line.trim_start().starts_with('[') {
                    skipping = line.contains("mcp_servers.emulsion");
                }
                if !skipping
                    && !line.trim_start().starts_with("approval_policy")
                    && !line.trim_start().starts_with("sandbox_mode")
                {
                    config.push_str(line);
                    config.push('\n');
                }
            }
        }
    }
    config.push_str("\napproval_policy = \"never\"\nsandbox_mode = \"read-only\"\n");
    config.push_str(&format!(
        "\n[mcp_servers.{}]\ncommand = {}\nargs = [\"mcp-serve\"]\n",
        emulsion_mcp::SERVER_NAME,
        toml_str(&exe.to_string_lossy())
    ));
    let env: Vec<String> = relay_env
        .iter()
        .map(|(k, v)| format!("{k} = {}", toml_str(v)))
        .collect();
    config.push_str(&format!("env = {{ {} }}\n", env.join(", ")));
    std::fs::write(home.join("config.toml"), config)?;
    std::fs::write(home.join("AGENTS.md"), SYSTEM_PROMPT)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            home.join("config.toml"),
            std::fs::Permissions::from_mode(0o600),
        )?;
    }
    Ok(home)
}

/// OpenCode: a per-session `opencode.json` with Emulsion's server as the
/// only MCP, built-in file and shell tools off, and the system prompt as
/// its instructions.
pub fn write_opencode_config(
    session_dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(session_dir)?;
    std::fs::write(session_dir.join("AGENTS.md"), SYSTEM_PROMPT)?;
    let config = json!({
        "$schema": "https://opencode.ai/config.json",
        "instructions": ["AGENTS.md"],
        "mcp": {
            emulsion_mcp::SERVER_NAME: {
                "type": "local",
                "command": [exe.to_string_lossy(), "mcp-serve"],
                "environment": env_map(relay_env),
                "enabled": true,
            }
        },
        "tools": {
            "bash": false, "edit": false, "write": false, "read": false, "glob": false,
            "grep": false, "list": false, "patch": false, "webfetch": false, "todowrite": false,
            "todoread": false, "task": false,
        },
        "permission": { "edit": "deny", "bash": "deny", "webfetch": "deny" },
    });
    let path = session_dir.join("opencode.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

/// Kimi Code reads `.kimi-code/mcp.json` from its working directory.
pub fn write_kimi_config(
    session_dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
) -> std::io::Result<PathBuf> {
    let dir = session_dir.join(".kimi-code");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(session_dir.join("AGENTS.md"), SYSTEM_PROMPT)?;
    let config = json!({
        "mcpServers": {
            emulsion_mcp::SERVER_NAME: {
                "command": exe.to_string_lossy(),
                "args": ["mcp-serve"],
                "env": env_map(relay_env),
            }
        }
    });
    let path = dir.join("mcp.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
    Ok(path)
}

/// Argv for one Codex turn.
pub fn codex_args(opts: &Options, prompt: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["exec".into()];
    if let Some(r) = opts.resume.as_ref().filter(|r| !r.is_empty()) {
        a.extend(["resume".into(), r.clone()]);
    }
    a.extend(["--json".into(), "--skip-git-repo-check".into()]);
    if let Some(m) = opts.model.as_ref().filter(|m| !m.is_empty()) {
        a.extend(["-m".into(), m.clone()]);
    }
    a.push(prompt.to_string());
    a
}

/// Argv for one OpenCode turn.
pub fn opencode_args(opts: &Options, prompt: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["run".into(), "--format".into(), "json".into()];
    if let Some(m) = opts.model.as_ref().filter(|m| !m.is_empty()) {
        a.extend(["--model".into(), m.clone()]);
    }
    if let Some(r) = opts.resume.as_ref().filter(|r| !r.is_empty()) {
        a.extend(["--session".into(), r.clone()]);
    }
    a.push(prompt.to_string());
    a
}

/// Argv for one Kimi turn.
pub fn kimi_args(opts: &Options, prompt: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-p".into(),
        prompt.to_string(),
        "--output-format".into(),
        "stream-json".into(),
    ];
    if let Some(m) = opts.model.as_ref().filter(|m| !m.is_empty()) {
        a.extend(["--model".into(), m.clone()]);
    }
    a
}

/// Everything needed to run `provider` for one document. Persistent CLIs
/// ignore `prompt` (turns go over stdin); one-shot CLIs need it.
pub fn spec_for(
    provider: &crate::provider::Provider,
    program: PathBuf,
    session_dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
    opts: &Options,
    prompt: Option<&str>,
) -> std::io::Result<LaunchSpec> {
    use crate::provider::McpConfig;
    let prompt = prompt.unwrap_or_default();
    let mut env = common_env();
    let args = match provider.mcp {
        McpConfig::ClaudeJson => return claude(program, session_dir, exe, relay_env, opts),
        McpConfig::CodexHome => {
            let home = write_codex_home(session_dir, exe, relay_env)?;
            env.push(("CODEX_HOME".into(), home.to_string_lossy().into_owned()));
            codex_args(opts, prompt)
        }
        McpConfig::OpenCodeJson => {
            let cfg = write_opencode_config(session_dir, exe, relay_env)?;
            env.push(("OPENCODE_CONFIG".into(), cfg.to_string_lossy().into_owned()));
            opencode_args(opts, prompt)
        }
        McpConfig::KimiJson => {
            write_kimi_config(session_dir, exe, relay_env)?;
            kimi_args(opts, prompt)
        }
    };
    Ok(LaunchSpec {
        program,
        args,
        env,
        cwd: session_dir.to_path_buf(),
    })
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
missing, say so and offer download_model rather than fetching it unasked. Everything else in the \
editor is a tool too: masks (add_mask, remove_mask, set_mask_enabled), clipping (set_clip), locks, \
rasterize, save_document, export_image, import_recipe (text, file or URL) and batch_export.

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
        env: common_env(),
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

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::provider;

    #[test]
    fn one_shot_argv_and_configs() {
        let opts = Options {
            model: Some("gpt-5.5".into()),
            resume: Some("t1".into()),
        };
        let a = codex_args(&opts, "hello");
        assert_eq!(a[..3], ["exec", "resume", "t1"]);
        assert!(a.contains(&"--json".to_string()) && a.last().unwrap() == "hello");
        let a = opencode_args(&opts, "hi");
        assert_eq!(a[..3], ["run", "--format", "json"]);
        assert!(a.contains(&"--session".to_string()) && a.contains(&"gpt-5.5".to_string()));
        let a = kimi_args(&Options::default(), "yo");
        assert_eq!(a, ["-p", "yo", "--output-format", "stream-json"]);

        let dir = std::env::temp_dir().join(format!("emulsion-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let env = vec![("EMULSION_RELAY_TOKEN".to_string(), "se\"cret".to_string())];
        let exe = Path::new("/opt/emulsion/emulsion");
        let spec = spec_for(
            provider::by_id("codex"),
            PathBuf::from("/usr/bin/codex"),
            &dir,
            exe,
            &env,
            &Options::default(),
            Some("do it"),
        )
        .unwrap();
        let home = spec
            .env
            .iter()
            .find(|(k, _)| k == "CODEX_HOME")
            .map(|(_, v)| PathBuf::from(v))
            .unwrap();
        let toml = std::fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(
            toml.contains("[mcp_servers.emulsion]") && toml.contains("approval_policy = \"never\"")
        );
        assert!(
            toml.contains(r#"EMULSION_RELAY_TOKEN = "se\"cret""#),
            "{toml}"
        );
        assert!(home.join("AGENTS.md").exists());
        assert_eq!(spec.args[0], "exec");

        let spec = spec_for(
            provider::by_id("opencode"),
            PathBuf::from("/usr/bin/opencode"),
            &dir,
            exe,
            &env,
            &Options::default(),
            Some("do it"),
        )
        .unwrap();
        let cfg = spec
            .env
            .iter()
            .find(|(k, _)| k == "OPENCODE_CONFIG")
            .map(|(_, v)| PathBuf::from(v))
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcp"]["emulsion"]["type"], "local");
        assert_eq!(v["mcp"]["emulsion"]["command"][1], "mcp-serve");
        assert_eq!(v["tools"]["bash"], false);

        let spec = spec_for(
            provider::by_id("kimi"),
            PathBuf::from("/usr/bin/kimi"),
            &dir,
            exe,
            &env,
            &Options::default(),
            Some("do it"),
        )
        .unwrap();
        assert!(dir.join(".kimi-code/mcp.json").exists());
        assert_eq!(spec.args[0], "-p");
        std::fs::remove_dir_all(&dir).ok();
    }
}
