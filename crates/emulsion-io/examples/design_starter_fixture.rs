//! Generate an editable starter contact project for native UI review.
//! cargo run -p emulsion-io --example design_starter_fixture -- NEW_DIRECTORY
use emulsion_core::{
    design::Template,
    project::{ProjectEditor, ProjectKind},
};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Supply a new output directory"))?,
    );
    std::fs::create_dir(&directory)?;
    for (category, spec) in Template::CATEGORIES.iter().enumerate() {
        let mut editor: Option<ProjectEditor> = None;
        for template in Template::catalog().filter(|t| t.category() == Some(category)) {
            let (w, h) = template.native_size();
            let scale = (1080. / w.max(h) as f64).min(1.);
            let doc = template
                .create(
                    (w as f64 * scale).round() as u32,
                    (h as f64 * scale).round() as u32,
                )
                .map_err(anyhow::Error::msg)?;
            if let Some(editor) = &mut editor {
                editor
                    .add_page(doc, template.label().into(), 0.)
                    .map_err(anyhow::Error::msg)?;
            } else {
                let mut first = ProjectEditor::new_project(ProjectKind::Design, doc)
                    .map_err(anyhow::Error::msg)?;
                first
                    .rename_page(1, template.label().into(), 0.)
                    .map_err(anyhow::Error::msg)?;
                editor = Some(first);
            }
        }
        let mut editor = editor.expect("nonempty bundled category");
        editor.set_active_page(1).map_err(anyhow::Error::msg)?;
        let file = directory.join(format!(
            "{:02}-{}.emu",
            category + 1,
            spec.preset.to_lowercase().replace(' ', "-")
        ));
        emulsion_io::project::write(&editor.snapshot().unwrap(), &file)?;
        println!("{}", file.display());
    }
    Ok(())
}
