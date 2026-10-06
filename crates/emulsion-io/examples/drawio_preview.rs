//! Render an imported page for visual compatibility review.
//! Usage: drawio_preview <input diagram> <output.png> [page-index] [--fit] [--native]
fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    anyhow::ensure!(args.len() >= 3, "Pass an input file and output PNG path");
    let imported = emulsion_io::diagram_import::read(std::path::Path::new(&args[1]))?;
    if args.iter().any(|s| s == "--native") {
        let path = std::path::Path::new(&args[2]).with_extension("emu");
        emulsion_io::project::write(&imported.project, &path)?;
        let reopened = emulsion_io::project::read(&path)?;
        anyhow::ensure!(
            reopened.pages.len() == imported.project.pages.len(),
            "Native page count changed"
        );
        for (original, saved) in imported.project.pages.iter().zip(&reopened.pages) {
            let mut comparable = saved.doc.clone();
            for original_node in &original.doc.nodes {
                let node = comparable
                    .node_mut(original_node.id)
                    .ok_or_else(|| anyhow::anyhow!("Missing saved node"))?;
                if let (
                    emulsion_core::NodeKind::Raster { raster: before, .. },
                    emulsion_core::NodeKind::Raster { raster: after, .. },
                ) = (&original_node.kind, &mut node.kind)
                {
                    anyhow::ensure!(
                        (before.width(), before.height()) == (after.width(), after.height()),
                        "Saved image dimensions changed"
                    );
                    anyhow::ensure!(
                        before
                            .to_srgba8()
                            .iter()
                            .zip(after.to_srgba8())
                            .all(|(a, b)| a.abs_diff(b) <= 1),
                        "Saved image pixels changed beyond 16-bit storage rounding"
                    );
                    *after = before.clone();
                }
            }
            anyhow::ensure!(
                original.doc == comparable,
                "Native diagram round trip changed vector content or image placement"
            );
            let mut editor = emulsion_core::Editor::new(original.doc.clone(), None);
            let roots = original
                .doc
                .nodes
                .iter()
                .filter(|n| {
                    n.parent.is_none() && !matches!(n.kind, emulsion_core::NodeKind::Fill { .. })
                })
                .map(|n| n.id)
                .collect::<Vec<_>>();
            for id in roots {
                editor.execute(emulsion_core::Command::TranslateNode { id, dx: 3., dy: 2. })?;
                editor.undo();
                anyhow::ensure!(
                    editor.doc == original.doc,
                    "Imported object movement/undo changed the document"
                );
            }
        }
    }
    let index = args
        .iter()
        .skip(3)
        .find(|s| !s.starts_with("--"))
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    let page = imported
        .project
        .pages
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Page index out of range"))?;
    let (svg, fallback) = emulsion_io::project_export::svg(&page.doc)?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = resvg::usvg::Tree::from_data(&svg, &options)?;
    let fit = if args.iter().any(|s| s == "--fit") {
        let bounds = page
            .doc
            .nodes
            .iter()
            .filter(|n| {
                !matches!(
                    n.kind,
                    emulsion_core::NodeKind::Fill { .. } | emulsion_core::NodeKind::Group { .. }
                )
            })
            .try_fold(emulsion_raster::IRect::default(), |a, n| {
                Ok::<_, emulsion_core::GeometryError>(
                    emulsion_core::geometry::node_bounds(&page.doc, n.id)?
                        .map_or(a, |b| a.union(&b)),
                )
            })?;
        Some((
            bounds.x as f32 - 12.,
            bounds.y as f32 - 12.,
            bounds.w as f32 + 24.,
            bounds.h as f32 + 24.,
        ))
    } else {
        None
    };
    let (x, y, w, h) = fit.unwrap_or((0., 0., tree.size().width(), tree.size().height()));
    let scale = (1600. / w.max(h)).min(2.);
    std::fs::write(std::path::Path::new(&args[2]).with_extension("svg"), &svg)?;
    let mut pixels =
        resvg::tiny_skia::Pixmap::new((w * scale).ceil() as u32, (h * scale).ceil() as u32)
            .ok_or_else(|| anyhow::anyhow!("Cannot allocate preview"))?;
    pixels.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_row(scale, 0., 0., scale, -x * scale, -y * scale),
        &mut pixels.as_mut(),
    );
    pixels.save_png(&args[2])?;
    println!(
        "{}",
        serde_json::json!({"page":page.meta.name,"export_raster_fallback":fallback,"warnings":imported.warnings})
    );
    Ok(())
}
