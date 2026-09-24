//! Render the JSON studies in a drawing guide without an assistant or model.
//!
//! cargo run --release -p emulsion-assistant --example playbook -- \
//!     --guide manga --out target/manga-studies
//! --guide also accepts a Markdown path, for comparison with an earlier guide.

use anyhow::{Context, bail, ensure};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node};
use emulsion_raster::{Placement, Raster};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Deserialize)]
struct Call {
    name: String,
    arguments: Value,
}

fn main() -> anyhow::Result<()> {
    let mut guide = "manga".to_string();
    let mut out = PathBuf::from("target/manga-studies");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--guide" => {
                guide = args
                    .next()
                    .context("--guide requires a name or Markdown path")?
            }
            "--out" => out = args.next().context("--out requires a directory")?.into(),
            "--help" | "-h" => {
                println!(
                    "playbook [--guide manga|renaissance|watercolour|media|styles|composition|PATH] [--out DIR]"
                );
                println!(
                    "Writes one PNG and editable ORA per JSON study on an 800×600 white canvas."
                );
                return Ok(());
            }
            _ => bail!("unknown argument {arg:?}; use --help"),
        }
    }
    let text = match guide.as_str() {
        "manga" => include_str!("../src/prompts/manga.md").to_string(),
        "renaissance" => include_str!("../src/prompts/renaissance.md").to_string(),
        "watercolour" => include_str!("../src/prompts/watercolour.md").to_string(),
        "media" => include_str!("../src/prompts/media.md").to_string(),
        "styles" => include_str!("../src/prompts/styles.md").to_string(),
        "composition" => include_str!("../src/prompts/composition.md").to_string(),
        path => std::fs::read_to_string(path).with_context(|| format!("read guide {path:?}"))?,
    };
    let studies: Vec<Vec<Call>> = text
        .split("```json")
        .skip(1)
        .enumerate()
        .map(|(index, block)| {
            let label = format!("guide {guide:?}, study {}", index + 1);
            let (json, _) = block
                .split_once("```")
                .with_context(|| format!("{label}: missing closing code fence"))?;
            let calls: Vec<Call> = serde_json::from_str(json)
                .with_context(|| format!("{label}: invalid tool calls"))?;
            ensure!(!calls.is_empty(), "{label}: no tool calls");
            Ok(calls)
        })
        .collect::<anyhow::Result<_>>()?;
    ensure!(!studies.is_empty(), "guide {guide:?} has no JSON studies");
    std::fs::create_dir_all(&out).with_context(|| format!("create {}", out.display()))?;

    for (index, calls) in studies.iter().enumerate() {
        let label = format!("guide {guide:?}, study {}", index + 1);
        let mut editor = Editor::new(Document::new(800, 600), None);
        for (call_index, call) in calls.iter().enumerate() {
            let call_label = format!("{label}, call {} ({})", call_index + 1, call.name);
            // Keep file-supplied guides confined to local drawing operations;
            // the full MCP catalogue also includes network and model tools.
            ensure!(
                offline_tool(&call.name),
                "{call_label}: not an offline drawing tool"
            );
            if call.name == "critique" {
                ensure!(
                    std::env::var("TYPESAFE_API_KEY")
                        .unwrap_or_default()
                        .is_empty(),
                    "{call_label}: unset TYPESAFE_API_KEY when running this example; critique otherwise invokes optional model ranking"
                );
            }
            let result = emulsion_mcp::exec::execute(&mut editor, &call.name, &call.arguments);
            if result.is_error {
                let error = result
                    .content
                    .iter()
                    .filter_map(|block| block["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                bail!("{call_label}: {error}");
            }
        }

        // Add paper afterwards: study node IDs refer to an initially empty
        // document. Index zero inserts underneath all of the study's marks.
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Paper",
                    Arc::new(Raster::solid(800, 600, [1.0; 4])),
                    Placement::default(),
                )),
                slot: Slot {
                    parent: None,
                    index: 0,
                },
            })
            .with_context(|| format!("{label}: add white paper"))?;

        let stem = format!("study-{:02}", index + 1);
        let png = out.join(format!("{stem}.png"));
        let native = out.join(format!("{stem}.ora"));
        emulsion_io::export(
            &editor.doc,
            &png,
            emulsion_io::ExportOptions::for_doc(&editor.doc),
        )
        .with_context(|| format!("{label}: export {}", png.display()))?;
        emulsion_io::save(&editor.doc, &native)
            .with_context(|| format!("{label}: save {}", native.display()))?;
        println!(
            "{label}: {} calls → {} and {}",
            calls.len(),
            png.display(),
            native.display()
        );
    }
    Ok(())
}

fn offline_tool(name: &str) -> bool {
    matches!(
        name,
        "add_layer"
            | "paint"
            | "hatch"
            | "draw_path"
            | "set_path"
            | "select_rect"
            | "select_ellipse"
            | "select_all"
            | "select_path"
            | "path_to_selection"
            | "deselect"
            | "fill_selection"
            | "set_visibility"
            | "set_opacity"
            | "set_blend"
            | "move_node"
            | "add_text"
            | "set_text"
            | "get_view"
            | "critique"
            | "list_brushes"
            | "list_fonts"
            | "describe_document"
    )
}
