//! Export every shared diagram template as SVG and a PNG contact sheet for review.
//! Usage: cargo run -p emulsion-io --example diagram_template_preview -- <directory>
fn main() -> anyhow::Result<()> {
    let directory = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Pass an output directory"))?,
    );
    std::fs::create_dir_all(&directory)?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>Editable diagram templates</title><style>body{font:15px system-ui;background:#eef1f5;padding:24px}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(480px,1fr));gap:24px}article{background:white;padding:20px;border:1px solid #d1d5db;border-radius:12px}img{width:100%}p{color:#535b66}</style><h1>Editable diagram templates</h1><main>",
    );
    for t in emulsion_core::diagram_library::TEMPLATES {
        let doc = t.build().map_err(anyhow::Error::msg)?;
        let editor = emulsion_core::project::ProjectEditor::new_project(
            emulsion_core::project::ProjectKind::Diagram, doc.clone(),
        ).map_err(anyhow::Error::msg)?;
        let mut project = editor.snapshot().unwrap();
        project.pages[0].meta.name = t.name.into();
        let native_path = directory.join(format!("{}.emu", t.id));
        emulsion_io::project::write(&project, &native_path)?;
        let reopened = emulsion_io::project::read(&native_path)?;
        anyhow::ensure!(reopened.pages[0].doc == doc, "{} changed during native save/reopen", t.id);
        let (svg, fallback) = emulsion_io::project_export::svg(&doc)?;
        anyhow::ensure!(!fallback, "{} unexpectedly flattened to raster", t.id);
        std::fs::write(directory.join(format!("{}.svg", t.id)), &svg)?;
        let tree = resvg::usvg::Tree::from_data(&svg, &options)?;
        let scale = (1400. / doc.width.max(doc.height) as f32).min(1.5);
        let mut pixmap = resvg::tiny_skia::Pixmap::new(
            (doc.width as f32 * scale).ceil() as u32,
            (doc.height as f32 * scale).ceil() as u32,
        )
        .unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap.save_png(directory.join(format!("{}.png", t.id)))?;
        html.push_str(&format!("<article><h2>{}</h2><p>{}</p><a href='{}.svg'><img src='{}.png' alt='{}'></a><p><a href='{}.emu'>Editable Emulsion project</a> · <a href='{}.svg'>SVG</a></p></article>",t.name,t.description,t.id,t.id,t.name,t.id,t.id));
        println!(
            "{}: {} shapes, {} connectors, vector export",
            t.id,
            doc.diagram.as_ref().unwrap().shapes.len(),
            doc.diagram.as_ref().unwrap().edges.len()
        );
    }
    html.push_str("</main>");
    std::fs::write(directory.join("index.html"), html)?;
    Ok(())
}
