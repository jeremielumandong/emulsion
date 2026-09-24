//! Local startup checks only: no inference requests or user prompts.
//! Run with the absolute path to the Emulsion executable as the first argument.
use emulsion_assistant::{Launcher, ProdLauncher, launch, provider, session::Line};
use std::{path::PathBuf, sync::mpsc, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("provide Emulsion executable path")?,
    );
    if !std::env::args().any(|arg| arg == "--child") {
        let root = std::env::var("SYSTEMROOT")?;
        let status = std::process::Command::new(std::env::current_exe()?)
            .arg(&app)
            .arg("--child")
            .current_dir(app.parent().ok_or("executable needs an absolute path")?)
            .env_remove("HOME")
            .env(
                "PATH",
                format!("{root}\\System32;{root};{root}\\System32\\WindowsPowerShell\\v1.0"),
            )
            .status()?;
        if !status.success() {
            return Err("minimal-environment smoke failed".into());
        }
        return Ok(());
    }
    let dir = std::env::temp_dir().join(format!("emulsion cli smoke {}", std::process::id()));
    let result = check(&app, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn check(app: &std::path::Path, dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    for id in ["codex", "claude"] {
        let provider = provider::by_id(id);
        let program = provider::find(provider.binary, None).ok_or("CLI not found")?;
        let mut spec = launch::spec_for(
            provider,
            program,
            &dir.join(id),
            app,
            &[],
            &launch::Options::default(),
            None,
        )?;
        if id == "codex" {
            spec.args = vec!["mcp".into(), "list".into()];
        }
        let (tx, rx) = mpsc::channel();
        let mut process = ProdLauncher.spawn(
            &spec,
            Box::new(move |line| {
                let _ = tx.send(line);
            }),
        )?;
        if id == "claude" {
            process.write_line(&emulsion_assistant::protocol::initialize().to_string())?;
        } else {
            process.close_stdin();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut saw_emulsion = false;
        let mut success = false;
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(line) = rx.recv_timeout(remaining) else {
                break;
            };
            match line {
                Line::Stdout(line) if id == "claude" => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                        && value["type"] == "control_response"
                        && value["response"]["subtype"] == "success"
                    {
                        success = true;
                        break;
                    }
                }
                Line::Stdout(line) => saw_emulsion |= line.contains("emulsion"),
                Line::Exit(code) => {
                    success = id == "codex" && code == Some(0) && saw_emulsion;
                    break;
                }
                Line::Stderr(line) => eprintln!("{id}: {line}"),
            }
        }
        process.kill();
        if !success {
            return Err(format!("{id} startup check failed").into());
        }
        println!("{id}: startup passed with missing HOME and minimal PATH");
    }
    Ok(())
}
