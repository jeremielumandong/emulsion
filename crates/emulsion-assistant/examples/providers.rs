//! Smoke-test a provider end to end against a tiny document:
//! `cargo run -p emulsion-assistant --example providers -- codex target/debug/emulsion ["prompt"]`
use emulsion_assistant::protocol::Flavor;
use emulsion_assistant::provider::Mode;
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
    let id = args.next().unwrap_or_else(|| "claude".into());
    let exe = std::fs::canonicalize(
        args.next()
            .unwrap_or_else(|| "target/debug/emulsion".into()),
    )?;
    let prompt = args.next().unwrap_or_else(|| {
        "Call describe_document, then set the opacity of the top node to 50 percent, then tell me in one sentence what you did.".into()
    });
    let prov = provider::by_id(&id);
    let cli = provider::find(prov.binary, None)
        .ok_or_else(|| anyhow::anyhow!("{} not found ({})", prov.label, prov.install_hint))?;
    println!(
        "{}: {} ({})",
        prov.label,
        cli.display(),
        provider::version(&cli).unwrap_or_default()
    );

    let mut d = Document::new(320, 200);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Photo",
            Arc::new(Raster::solid(320, 200, [0.4, 0.5, 0.6, 1.0])),
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
            println!(
                "  relay: {} {}",
                call.name,
                call.arguments
                    .to_string()
                    .chars()
                    .take(120)
                    .collect::<String>()
            );
            let r = emulsion_mcp::exec::execute(&mut ed.lock(), &call.name, &call.arguments);
            call.reply(r);
        }
    });
    let dir = std::env::temp_dir().join(format!("emulsion-prov-{}-{}", id, std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let relay_env = relay.env();
    let mut s = match prov.mode {
        Mode::Persistent => {
            let spec = launch::spec_for(
                prov,
                cli,
                &dir,
                &exe,
                &relay_env,
                &launch::Options::default(),
                None,
            )?;
            Session::start(&ProdLauncher, &spec)?
        }
        Mode::OneShot => {
            let flavor = match prov.id {
                "codex" => Flavor::Codex,
                "opencode" => Flavor::OpenCode,
                _ => Flavor::Kimi,
            };
            let (dir2, exe2, env2) = (dir.clone(), exe.clone(), relay_env.clone());
            Session::one_shot(
                flavor,
                Box::new(move |prompt: &str, resume: Option<String>| {
                    let opts = launch::Options {
                        model: None,
                        resume,
                    };
                    let spec = launch::spec_for(
                        prov,
                        cli.clone(),
                        &dir2,
                        &exe2,
                        &env2,
                        &opts,
                        Some(prompt),
                    )?;
                    println!(
                        "  argv: {} {}",
                        spec.program.display(),
                        spec.args.join(" ").chars().take(160).collect::<String>()
                    );
                    Ok(spec)
                }),
            )
        }
    };
    println!("> {prompt}");
    s.send(&prompt, &[])?;
    let start = Instant::now();
    let mut done = false;
    while let Ok(ev) = s.events.recv_blocking() {
        match &ev {
            Event::Init { session_id, .. } => println!("  session {session_id}"),
            Event::Text(t) => print!("{t}"),
            Event::Thinking(_) => {}
            Event::ToolUse { name, input, .. } => println!(
                "\n  tool {name} {}",
                input.to_string().chars().take(120).collect::<String>()
            ),
            Event::ToolResult { text, is_error, .. } => println!(
                "  result{}: {}",
                if *is_error { " (error)" } else { "" },
                text.chars().take(120).collect::<String>()
            ),
            Event::Permission {
                request_id,
                tool_use_id,
                input,
                ..
            } => s.allow(request_id, tool_use_id, input)?,
            Event::Result {
                input_tokens,
                output_tokens,
                ..
            } => {
                println!(
                    "\n  [done in {:?}, tokens {input_tokens}/{output_tokens}]",
                    start.elapsed()
                );
                done = true;
                if !s.is_one_shot() {
                    break;
                }
            }
            Event::Error(e) => {
                println!("\n  error: {e}");
                done = true;
                if !s.is_one_shot() {
                    break;
                }
            }
            Event::Exited(c) => {
                println!("  exited {c:?}");
                if done || s.is_one_shot() {
                    break;
                }
            }
            Event::Stderr(l) => eprintln!("  stderr: {}", l.chars().take(200).collect::<String>()),
        }
        if start.elapsed() > Duration::from_secs(240) {
            anyhow::bail!("timed out");
        }
    }
    let e = editor.lock();
    println!(
        "top node opacity now {:.0}%",
        e.doc.nodes.last().map(|n| n.opacity * 100.0).unwrap_or(0.0)
    );
    Ok(())
}
