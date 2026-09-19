//! Live check of the painting tools against the installed Claude Code: a
//! blank canvas, one drawing request, and the result saved as a PNG.
//! Confirmations are approved automatically here; in the app they are
//! Apply/Skip cards.
//!
//! cargo run -p emulsion-assistant --example draw -- path/to/emulsion [out.png] ["what to draw"]

use emulsion_assistant::{Event, ProdLauncher, Session, launch, provider};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node};
use emulsion_mcp::relay::Relay;
use emulsion_raster::{Placement, Raster};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let exe = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target/debug/emulsion".into());
    let exe = std::fs::canonicalize(&exe)?;
    let out = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target/draw.png".into());
    let prompt = args.next().unwrap_or_else(|| {
        "Draw a small sailboat on a calm sea at sunset. Work in stages on your own layers: block in \
         the sky and sea with wet brushes, then the boat, then ink the outlines with the G-pen, then a \
         little shading. Look at the result between stages."
            .into()
    });
    let cli = provider::find("claude", None).ok_or_else(|| anyhow::anyhow!("claude not found"))?;
    println!(
        "cli: {} ({})",
        cli.display(),
        provider::version(&cli).unwrap_or_default()
    );

    let mut d = Document::new(800, 600);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Paper",
            Arc::new(Raster::solid(800, 600, [0.95, 0.94, 0.9, 1.0])),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut d)?;
    let editor = Arc::new(Mutex::new(Editor::new(d, None)));

    let relay = Relay::start()?;
    let (calls, ed) = (relay.calls.clone(), editor.clone());
    std::thread::spawn(move || {
        while let Ok(call) = calls.recv_blocking() {
            let t = Instant::now();
            let r = emulsion_mcp::exec::execute(&mut ed.lock(), &call.name, &call.arguments);
            if call.name == "paint" {
                println!("  (painted in {} ms)", t.elapsed().as_millis());
            }
            call.reply(r);
        }
    });

    let dir = std::env::temp_dir().join(format!("emulsion-draw-{}", std::process::id()));
    let spec = launch::claude(cli, &dir, &exe, &relay.env(), &launch::Options::default())?;
    let mut s = Session::start(&ProdLauncher, &spec)?;

    println!("\n> {prompt}");
    editor.lock().begin("Assistant drawing");
    s.send(&prompt, &[])?;
    let start = Instant::now();
    let (mut paints, mut strokes) = (0usize, 0usize);
    loop {
        let ev = match s.events.recv_blocking() {
            Ok(e) => e,
            Err(_) => anyhow::bail!("session closed"),
        };
        match &ev {
            Event::Text(t) => print!("{t}"),
            Event::ToolUse { name, input, .. } => {
                if name.ends_with("paint") {
                    paints += 1;
                    let n = input["strokes"].as_array().map_or(0, |a| a.len());
                    strokes += n;
                    println!(
                        "\n  tool paint: {n} strokes with {}",
                        input["brush"].as_str().unwrap_or("settings")
                    );
                } else {
                    println!(
                        "\n  tool {name} {}",
                        input.to_string().chars().take(200).collect::<String>()
                    );
                }
            }
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
                println!("  confirm {tool_name} -> apply");
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
        if start.elapsed() > Duration::from_secs(900) {
            anyhow::bail!("turn timed out");
        }
    }
    editor.lock().end();

    let e = editor.lock();
    println!("\n{paints} paint calls, {strokes} strokes. Layers (top first):");
    for n in e.doc.nodes.iter().rev() {
        println!("  #{} {}", n.id, n.name);
    }
    let opts = emulsion_io::ExportOptions::for_doc(&e.doc);
    emulsion_io::export(&e.doc, &out, opts)?;
    println!("saved {}", out.display());
    Ok(())
}
