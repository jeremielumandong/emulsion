//! Write real v1/v2 projects for a separately frozen, pre-v2 public reader.
//! Usage: project_envelope_proof OUTPUT_DIRECTORY
//! The coordinator owns compilation/execution and old-reader provenance.
use emulsion_core::{
    Document, Node, NodeKind,
    graph::Graph,
    node::SmartEditable,
    project::{Project, ProjectEditor, ProjectKind},
    storyboard::Panel,
};
use emulsion_raster::{Placement, Raster};
use std::{path::Path, sync::Arc};

fn plain() -> Document {
    let mut doc = Document::new(2, 1);
    doc.nodes.push(Node::new(
        1,
        "Legacy",
        NodeKind::Fill {
            rgba: [23, 45, 67, 255],
        },
    ));
    doc.next_id = 2;
    doc
}

fn protected(opaque: bool) -> Document {
    let mut doc = Document::new(2, 1);
    let mut node = Node::smart(
        1,
        "Retained hidden source",
        Arc::new(Raster::solid(2, 1, [0.25, 0.5, 0.75, 1.])),
        Vec::new(),
        Placement::default(),
    );
    node.visible = false;
    if let NodeKind::Smart {
        editable,
        filters_enabled,
        ..
    } = &mut node.kind
    {
        if opaque {
            *editable = Some(SmartEditable::Document {
                archive: Arc::new(b"opaque future editable source; not opened".to_vec()),
                external: None,
            });
        } else {
            *filters_enabled = false;
        }
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}

fn retired(doc: &Document) -> (Project, u64) {
    let mut editor = ProjectEditor::new_project(ProjectKind::Storyboard, plain()).unwrap();
    let id = editor
        .insert_panels(
            Some(1),
            doc,
            vec![("Removed panel".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap()[0];
    editor.create_board_version("Before removal").unwrap();
    editor.remove_page(id).unwrap();
    (editor.snapshot().unwrap(), id)
}

fn save(directory: &Path, name: &str, project: &Project, expected: u32) -> anyhow::Result<()> {
    assert_eq!(emulsion_io::project::required_version(project), expected);
    let path = directory.join(name);
    emulsion_io::project::write(project, &path)?;
    let reopened = emulsion_io::project::read(&path)?;
    assert_eq!(emulsion_io::project::required_version(&reopened), expected);
    println!("WROTE: {} project_version={expected}", path.display());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: project_envelope_proof OUTPUT_DIRECTORY"))?;
    anyhow::ensure!(args.next().is_none(), "expected one output directory");
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;

    let (legacy, _) = retired(&plain());
    save(directory, "legacy-retired-v1.emu", &legacy, 1)?;
    let (opaque, _) = retired(&protected(true));
    save(directory, "opaque-retired-v2.emu", &opaque, 2)?;
    let (disabled, _) = retired(&protected(false));
    save(directory, "disabled-retired-v2.emu", &disabled, 2)?;

    let (mut history_only, id) = retired(&plain());
    let mut graph = Graph::new(protected(true), "Protected old artwork");
    graph.record(&plain(), "Legacy head", false).unwrap();
    history_only
        .storyboard
        .as_mut()
        .unwrap()
        .versions
        .retired
        .insert(id, graph);
    save(directory, "history-only-retired-v2.emu", &history_only, 2)?;

    let live = ProjectEditor::new_project(ProjectKind::Storyboard, protected(true))
        .map_err(anyhow::Error::msg)?
        .snapshot()
        .unwrap();
    save(directory, "live-protected-v1.emu", &live, 1)?;
    Ok(())
}
