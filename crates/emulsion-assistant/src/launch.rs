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

/// Longest a single tool call may run, in seconds: painting plays at a
/// hand's pace on the canvas and answers only when it has finished.
pub const TOOL_TIMEOUT_SECS: u64 = 900;

fn common_env() -> Vec<(String, String)> {
    vec![
        ("NO_COLOR".into(), "1".into()),
        ("FORCE_COLOR".into(), "0".into()),
        // Claude Code reads this for MCP tool execution.
        (
            "MCP_TOOL_TIMEOUT".into(),
            (TOOL_TIMEOUT_SECS * 1000).to_string(),
        ),
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
pub fn write_codex_home(
    session_dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
) -> std::io::Result<PathBuf> {
    let home = session_dir.join("codex-home");
    std::fs::create_dir_all(&home)?;
    let user_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::provider::home().map(|h| h.join(".codex")));
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
                if !skipping {
                    config.push_str(line);
                    config.push('\n');
                }
            }
        }
    }
    config.push_str(&format!(
        "\n[mcp_servers.{}]\ncommand = {}\nargs = [\"mcp-serve\"]\ntool_timeout_sec = {}\n",
        emulsion_mcp::SERVER_NAME,
        toml_str(&exe.to_string_lossy()),
        TOOL_TIMEOUT_SECS
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

/// The person's own OpenCode config, so their provider, model and keys
/// carry into the session (`OPENCODE_CONFIG` replaces the config wholesale).
fn user_opencode_config() -> serde_json::Value {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::provider::home().map(|h| h.join(".config")));
    let candidates = base.into_iter().flat_map(|b| {
        [
            b.join("opencode/opencode.json"),
            b.join("opencode/opencode.jsonc"),
        ]
    });
    for p in candidates {
        if let Ok(text) = std::fs::read_to_string(&p) {
            // jsonc: strip // comments naively (outside strings is good enough here).
            let stripped: String = text
                .lines()
                .map(|l| {
                    let t = l.trim_start();
                    if t.starts_with("//") { "" } else { l }
                })
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stripped)
                && v.is_object()
            {
                return v;
            }
        }
    }
    json!({})
}

/// OpenCode: a per-session `opencode.json` — the person's config with
/// Emulsion's server added as an MCP and the system prompt as its
/// instructions.
pub fn write_opencode_config(
    session_dir: &Path,
    exe: &Path,
    relay_env: &[(String, String)],
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(session_dir)?;
    std::fs::write(session_dir.join("AGENTS.md"), SYSTEM_PROMPT)?;
    let mut config = user_opencode_config();
    let obj = config.as_object_mut().expect("object");
    obj.entry("$schema")
        .or_insert_with(|| json!("https://opencode.ai/config.json"));
    let instructions = obj.entry("instructions").or_insert_with(|| json!([]));
    if let Some(arr) = instructions.as_array_mut() {
        let agents = json!(session_dir.join("AGENTS.md").to_string_lossy());
        if !arr.contains(&agents) {
            arr.push(agents);
        }
    }
    let mcp = obj.entry("mcp").or_insert_with(|| json!({}));
    if let Some(m) = mcp.as_object_mut() {
        m.insert(
            emulsion_mcp::SERVER_NAME.into(),
            json!({
                "type": "local",
                "command": [exe.to_string_lossy(), "mcp-serve"],
                "environment": env_map(relay_env),
                "enabled": true,
            }),
        );
    }
    // OpenCode's built-in tools stay on: turning them off (or denying them
    // in `permission`) makes its free-tier provider refuse headless runs.
    // The instructions tell the model to use Emulsion's tools only.
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
    // Codex has no host approval channel: with a restrictive policy it
    // refuses MCP tools outright, so approvals are bypassed here and
    // Emulsion holds each change for the person at the relay instead.
    a.extend([
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--dangerously-bypass-approvals-and-sandbox".into(),
    ]);
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
    let absolute_dir = std::path::absolute(session_dir)?;
    let session_dir = absolute_dir.as_path();
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

/// Shared studio rules and a brief-selected catalogue for every provider.
pub const SYSTEM_PROMPT: &str = concat!(
    include_str!("prompts/studio.md"),
    include_str!("prompts/manga.md"),
    include_str!("prompts/renaissance.md"),
    include_str!("prompts/watercolour.md"),
    include_str!("prompts/media.md"),
    include_str!("prompts/styles.md"),
);

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
pub fn claude_args(mcp_config: &Path, system_prompt: &Path, opts: &Options) -> Vec<String> {
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
    a.extend([
        "--append-system-prompt-file".into(),
        system_prompt.to_string_lossy().into_owned(),
    ]);
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
    let session_dir = std::path::absolute(session_dir)?;
    let config = write_mcp_config(&session_dir, exe, relay_env)?;
    // The studio instructions exceed Windows' command-line limit. Keep the
    // full prompt in a file, as AgentOps does, instead of putting it in argv.
    let system_prompt = session_dir.join("system-prompt.md");
    std::fs::write(&system_prompt, SYSTEM_PROMPT)?;
    Ok(LaunchSpec {
        program,
        args: claude_args(&config, &system_prompt, opts),
        env: common_env(),
        cwd: session_dir.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_guidance_matches_discoverable_tools_and_permissions() {
        let definitions = emulsion_mcp::tools::definitions();
        let read_only = read_only_tools();
        for name in [
            "draw_shape",
            "combine_path",
            "resize_path",
            "align_path_components",
            "list_shape_stroke_presets",
            "save_shape_stroke_preset",
            "apply_shape_stroke_preset",
        ] {
            assert!(SYSTEM_PROMPT.contains(name), "missing guidance: {name}");
            assert!(
                definitions.iter().any(|tool| tool.name == name),
                "undiscoverable tool: {name}"
            );
            assert_eq!(
                read_only.contains(&emulsion_mcp::tools::qualified(name)),
                name == "list_shape_stroke_presets",
                "incorrect approval classification: {name}"
            );
        }
    }

    #[test]
    fn playbook_examples_execute_with_current_tools() {
        let definitions = emulsion_mcp::tools::definitions();
        let mut example_count = 0;
        let mut call_count = 0;
        // Exercise every example actually delivered to providers, including
        // multiple studies per file, so new catalogue entries cannot be missed.
        for (index, block) in SYSTEM_PROMPT.split("```json\n").skip(1).enumerate() {
            example_count += 1;
            let mut editor =
                emulsion_core::Editor::new(emulsion_core::Document::new(800, 600), None);
            let example = block.split_once("\n```").expect("closed JSON example").0;
            let calls: Vec<serde_json::Value> = serde_json::from_str(example).unwrap();
            assert!(!calls.is_empty(), "example {index}: empty study");
            for call in calls {
                call_count += 1;
                let name = call["name"].as_str().unwrap();
                let args = &call["arguments"];
                let definition = definitions.iter().find(|d| d.name == name).unwrap();
                for key in args.as_object().unwrap().keys() {
                    assert!(
                        definition.input_schema["properties"].get(key).is_some(),
                        "example {index}: {name} has unknown argument {key}"
                    );
                }
                for key in definition.input_schema["required"].as_array().unwrap() {
                    assert!(args.get(key.as_str().unwrap()).is_some());
                }
                let result = emulsion_mcp::exec::execute(&mut editor, name, args);
                assert!(
                    !result.is_error,
                    "example {index}: {name} failed: {:?}",
                    result.content
                );
                if name == "get_view" {
                    assert!(result.content.iter().any(|c| c["type"] == "image"));
                    if let Some(region) = args.get("region") {
                        let mapping = result
                            .content
                            .iter()
                            .filter_map(|c| c["text"].as_str())
                            .filter_map(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                            .find(|value| value.get("image_to_document").is_some())
                            .expect("region preview returns document coordinate mapping");
                        assert_eq!(&mapping["region"], region);
                    }
                }
                if name == "critique" {
                    let review = result
                        .content
                        .iter()
                        .filter_map(|c| c["text"].as_str())
                        .filter_map(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                        .find(|value| value.get("context").is_some())
                        .expect("critique returns the artistic brief");
                    for (key, value) in args["context"].as_object().unwrap() {
                        assert_eq!(&review["context"][key], value);
                    }
                }
            }
            assert!(
                editor.doc.nodes.len() >= 2,
                "example {index}: study keeps independently editable layers/nodes"
            );
            assert!(
                editor.doc.selection.is_none(),
                "example {index}: selection restored"
            );
        }
        assert!(example_count > 0, "provider prompt includes worked studies");
        eprintln!("Executed {call_count} tool calls across {example_count} worked studies");
    }

    #[test]
    fn argv_is_scoped_to_emulsion_tools() {
        let a = claude_args(
            Path::new("/tmp/s/mcp.json"),
            Path::new("/tmp/s/system-prompt.md"),
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
            "mcp__emulsion__describe_document,mcp__emulsion__get_view,mcp__emulsion__get_reference_image,mcp__emulsion__list_history,mcp__emulsion__compare,mcp__emulsion__list_brushes,mcp__emulsion__list_shape_stroke_presets,mcp__emulsion__list_recipes,mcp__emulsion__critique,mcp__emulsion__list_fonts,mcp__emulsion__list_models"
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
    fn relative_session_paths_are_absolute_and_claude_prompt_stays_out_of_argv() {
        let dir = PathBuf::from(format!(
            "target/relative assistant session {}",
            std::process::id()
        ));
        for id in ["claude", "codex", "opencode", "kimi"] {
            let spec = spec_for(
                provider::by_id(id),
                PathBuf::from(id),
                &dir,
                &std::env::current_exe().unwrap(),
                &[],
                &Options::default(),
                Some("hello"),
            )
            .unwrap();
            assert!(spec.cwd.is_absolute());
            for (key, value) in &spec.env {
                if key == "CODEX_HOME" || key == "OPENCODE_CONFIG" {
                    assert!(Path::new(value).is_absolute(), "{key}: {value}");
                }
            }
            if id == "claude" {
                for flag in ["--mcp-config", "--append-system-prompt-file"] {
                    let position = spec.args.iter().position(|arg| arg == flag).unwrap();
                    let file = Path::new(&spec.args[position + 1]);
                    assert!(file.is_absolute() && file.is_file());
                    if flag == "--append-system-prompt-file" {
                        assert_eq!(std::fs::read_to_string(file).unwrap(), SYSTEM_PROMPT);
                    }
                }
                assert!(
                    spec.args
                        .iter()
                        .map(|arg| arg.encode_utf16().count() + 3)
                        .sum::<usize>()
                        < 8000
                );
                assert!(!spec.args.iter().any(|arg| arg.contains('\n')));
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
        assert!(toml.contains("[mcp_servers.emulsion]"));
        assert!(
            spec.args
                .iter()
                .any(|a| a == "--dangerously-bypass-approvals-and-sandbox")
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
        assert!(v.get("tools").is_none_or(|t| t["bash"] != false));

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
