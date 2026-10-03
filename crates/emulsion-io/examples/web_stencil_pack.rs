//! Export the permanent Web systems cards as a portable pack for existing builds.
use emulsion_core::{
    Document, Editor,
    diagram::stencils::STENCILS,
    graph::Graph,
    project::{PageMeta, Project, ProjectKind, ProjectPage},
};
use emulsion_io::template_pack::{self, Kind, Manifest};
fn main() -> anyhow::Result<()> {
    let path = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Pass an output .emustencil path"))?,
    );
    let mut pages = Vec::new();
    for stencil in STENCILS.iter().filter(|s| s.category == "Web systems") {
        let mut editor = Editor::new(Document::new(336, 196), None);
        let root = stencil
            .insert(&mut editor, [24., 24., 288., 148.])
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            editor.doc.subtree(root).len() >= 7,
            "Missing editable card artwork"
        );
        editor.doc.validate()?;
        let doc = editor.doc.clone();
        let id = pages.len() as u64 + 1;
        pages.push(ProjectPage {
            meta: PageMeta {
                id,
                name: stencil.label.into(),
                bleed_mm: 0.,
            },
            graph: Graph::new(doc.clone(), "Web systems stencil"),
            doc,
        });
    }
    let project = Project {
        storyboard: None,
        kind: ProjectKind::Diagram,
        next_page_id: pages.len() as u64 + 1,
        pages,
        active: 1,
    };
    let mut manifest = Manifest::new(Kind::Stencil, "Emulsion Web Systems".into());
    manifest.author = "Emulsion".into();
    manifest.description = "Editable cards from the browser/server templates: client, edge, gateway, auth, service, data, async and realtime.".into();
    manifest.tags = vec![
        "Web systems".into(),
        "Architecture".into(),
        "Permanent".into(),
    ];
    template_pack::write(&project, &manifest, &path)?;
    let reopened = template_pack::read(&path)?;
    anyhow::ensure!(reopened.project.pages.len() == 14);
    for (a, b) in project.pages.iter().zip(&reopened.project.pages) {
        anyhow::ensure!(a.doc == b.doc, "Pack roundtrip changed {}", a.meta.name);
    }
    if std::env::args().any(|a| a == "--install") {
        let (_, id) = template_pack::install(&emulsion_io::creative_library::root(), reopened)?;
        println!("Installed permanent Web Systems pack {id}");
    }
    println!("Verified 14 editable stencils: {}", path.display());
    Ok(())
}
