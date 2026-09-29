//! Export every shared diagram template as SVG and a PNG contact sheet for review.
//! Usage: cargo run -p emulsion-io --example diagram_template_preview -- <directory> [template-id-prefix]
fn main() -> anyhow::Result<()> {
    let directory = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Pass an output directory"))?,
    );
    let prefix = std::env::args().nth(2).unwrap_or_default();
    anyhow::ensure!(
        emulsion_core::diagram_library::TEMPLATES
            .iter()
            .any(|t| t.id.starts_with(&prefix)),
        "No templates match the prefix"
    );
    std::fs::create_dir_all(&directory)?;
    let mut collection: Option<emulsion_core::project::ProjectEditor> = None;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let mut html = String::from(
        "<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>Emulsion · Editable diagram templates</title><style>*{box-sizing:border-box}body{margin:0;font:15px/1.6 system-ui;background:#f3f5fa;color:#172033}header{padding:44px max(24px,5vw);background:linear-gradient(115deg,#0f172a,#302955);color:white;border-bottom:5px solid #8b5cf6}header p{color:#d4dcf0;max-width:760px}h1{font-size:clamp(26px,4vw,42px);letter-spacing:-1.2px;margin:10px 0}small{letter-spacing:2px;font-weight:700;color:#c4b5fd}main{padding:32px max(20px,4vw);display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,600px),1fr));gap:28px}article{background:white;border:1px solid #dce2ed;border-radius:16px;overflow:hidden;box-shadow:0 4px 20px #18254a08}article h2,article p{margin:18px 24px}article h2{margin-bottom:6px;font-size:22px}article p{color:#526179;margin-top:0}img{display:block;width:100%;height:auto;border-block:1px solid #edf0f5}a{color:#5b3ab5;text-underline-offset:4px}header a{display:inline-block;background:#ede9fe;color:#452080;font-weight:700;padding:10px 18px;border-radius:9px;text-decoration:none}.downloads{padding-top:16px}a:focus-visible{outline:3px solid #8b5cf6;outline-offset:4px}</style><header><small>EMULSION / TEMPLATE COLLECTION</small><h1>Understand the entire request.</h1><p>Detailed, color-coded browser and server systems. Explore each vector diagram, then open the native project to change shapes, labels, colors and connections.</p><a href='all-templates.emu'>Open the complete editable collection</a></header><main>",
    );
    for t in emulsion_core::diagram_library::TEMPLATES
        .iter()
        .filter(|t| t.id.starts_with(&prefix))
    {
        let doc = t.build().map_err(anyhow::Error::msg)?;
        let editor = emulsion_core::project::ProjectEditor::new_project(
            emulsion_core::project::ProjectKind::Diagram,
            doc.clone(),
        )
        .map_err(anyhow::Error::msg)?;
        let mut project = editor.snapshot().unwrap();
        project.pages[0].meta.name = t.name.into();
        let native_path = directory.join(format!("{}.emu", t.id));
        emulsion_io::project::write(&project, &native_path)?;
        let reopened = emulsion_io::project::read(&native_path)?;
        anyhow::ensure!(
            reopened.pages[0].doc == doc,
            "{} changed during native save/reopen",
            t.id
        );
        if let Some(collection) = &mut collection {
            collection
                .import_pages(project)
                .map_err(anyhow::Error::msg)?;
        } else {
            collection = Some(
                emulsion_core::project::ProjectEditor::open(project, None)
                    .map_err(anyhow::Error::msg)?,
            );
        }
        let (svg, fallback) = emulsion_io::project_export::svg(&doc)?;
        anyhow::ensure!(!fallback, "{} unexpectedly flattened to raster", t.id);
        std::fs::write(directory.join(format!("{}.svg", t.id)), &svg)?;
        let tree = resvg::usvg::Tree::from_data(&svg, &options)?;
        let scale = (1920. / doc.width.max(doc.height) as f32).min(2.);
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
        html.push_str(&format!("<article><h2>{}</h2><p>{}</p><a href='{}.svg'><img src='{}.png' alt='{}'></a><p class='downloads'><a href='{}.emu'>Editable Emulsion project</a> · <a href='{}.svg'>Full vector SVG</a></p></article>",t.name,t.description,t.id,t.id,t.name,t.id,t.id));
        println!(
            "{}: {} shapes, {} connectors, vector export",
            t.id,
            doc.diagram.as_ref().unwrap().shapes.len(),
            doc.diagram.as_ref().unwrap().edges.len()
        );
    }
    if let Some(collection) = collection {
        let mut project = collection.snapshot().unwrap();
        project.active = project.pages[0].meta.id;
        let path = directory.join("all-templates.emu");
        emulsion_io::project::write(&project, &path)?;
        let reopened = emulsion_io::project::read(&path)?;
        anyhow::ensure!(
            reopened.active == project.active
                && reopened.pages.len() == project.pages.len()
                && reopened
                    .pages
                    .iter()
                    .zip(&project.pages)
                    .all(|(a, b)| a.meta == b.meta && a.doc == b.doc),
            "Combined project changed during save/reopen"
        );
    }
    html.push_str("</main></html>");
    std::fs::write(directory.join("index.html"), html)?;
    Ok(())
}
