//! Live check against the installed Claude Code: a three-node document, the
//! relay, and two turns. Confirmations are approved automatically here; in
//! the app they are Apply/Skip cards.
//!
//! cargo run -p emulsion-assistant --example smoke -- path/to/emulsion

use emulsion_assistant::{Event, ProdLauncher, Session, launch, provider};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node};
use emulsion_mcp::relay::Relay;
use emulsion_raster::{Placement, Raster};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let exe = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target/debug/emulsion".into());
    let exe = std::fs::canonicalize(&exe)?;
    let cli = provider::find("claude", None).ok_or_else(|| anyhow::anyhow!("claude not found"))?;
    println!(
        "cli: {} ({})",
        cli.display(),
        provider::version(&cli).unwrap_or_default()
    );

    let mut d = Document::new(600, 400);
    for (name, c) in [
        ("Grass", [0.05, 0.4, 0.05, 1.0]),
        ("Sun", [0.9, 0.7, 0.1, 1.0]),
        ("Clouds", [0.8, 0.8, 0.85, 1.0]),
    ] {
        let (w, h, y) = match name {
            "Grass" => (600, 150, 250.0),
            "Sun" => (120, 120, 40.0),
            _ => (300, 80, 60.0),
        };
        let x = if name == "Sun" { 440.0 } else { 0.0 };
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                name,
                Arc::new(Raster::solid(w, h, c)),
                Placement::at(x, y),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut d)?;
    }
    let editor = Arc::new(Mutex::new(Editor::new(d, None)));

    let relay = Relay::start()?;
    let (calls, ed) = (relay.calls.clone(), editor.clone());
    std::thread::spawn(move || {
        while let Ok(call) = calls.recv_blocking() {
            let r = emulsion_mcp::exec::execute(&mut ed.lock(), &call.name, &call.arguments);
            call.reply(r);
        }
    });

    let dir = std::env::temp_dir().join(format!("emulsion-smoke-{}", std::process::id()));
    let spec = launch::claude(cli, &dir, &exe, &relay.env(), &launch::Options::default())?;
    let mut s = Session::start(&ProdLauncher, &spec)?;

    for prompt in [
        "hide the top two nodes and rename the third to Sky",
        "Look at the image and describe it in one sentence.",
    ] {
        println!("\n> {prompt}");
        editor.lock().begin(format!("Assistant: {prompt}"));
        s.send(prompt, &[])?;
        let start = Instant::now();
        loop {
            let ev = match s.events.recv_blocking() {
                Ok(e) => e,
                Err(_) => anyhow::bail!("session closed"),
            };
            match &ev {
                Event::Text(t) => print!("{t}"),
                Event::ToolUse { name, input, .. } => println!("\n  tool {name} {input}"),
                Event::ToolResult { text, is_error, .. } => {
                    println!(
                        "  result{}: {}",
                        if *is_error { " (error)" } else { "" },
                        text.chars().take(160).collect::<String>()
                    )
                }
                Event::Permission {
                    request_id,
                    tool_name,
                    input,
                    tool_use_id,
                } => {
                    println!("  confirm {tool_name} {input} -> apply");
                    s.allow(request_id, tool_use_id, input)?;
                }
                Event::Result {
                    cost_usd,
                    duration_ms,
                    ..
                } => {
                    println!("\n  [turn done: {duration_ms} ms, ${cost_usd:.4}]");
                    break;
                }
                Event::Error(e) => anyhow::bail!("error: {e}"),
                Event::Exited(c) => anyhow::bail!("cli exited: {c:?}"),
                Event::Stderr(l) => eprintln!("  stderr: {l}"),
                _ => {}
            }
            if start.elapsed() > Duration::from_secs(240) {
                anyhow::bail!("turn timed out");
            }
        }
        editor.lock().end();
    }

    let e = editor.lock();
    println!("\nfinal document (top first):");
    for n in e.doc.nodes.iter().rev() {
        println!("  #{} {:<8} visible={}", n.id, n.name, n.visible);
    }
    println!("history steps: {}", e.history.len());
    Ok(())
}
