//! Render an imported page for visual compatibility review.
//! Usage: drawio_preview <input.drawio|xml|svg> <output.png> [page-index]
fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    anyhow::ensure!(args.len() >= 3, "Pass an input file and output PNG path");
    let imported = emulsion_io::drawio::read(std::path::Path::new(&args[1]))?;
    let index = args
        .get(3)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    let page = imported
        .project
        .pages
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Page index out of range"))?;
    let (svg, fallback) = emulsion_io::project_export::svg(&page.doc)?;
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default())?;
    let scale = (1600. / tree.size().width().max(tree.size().height())).min(1.);
    let mut pixels = resvg::tiny_skia::Pixmap::new(
        (tree.size().width() * scale).ceil() as u32,
        (tree.size().height() * scale).ceil() as u32,
    )
    .ok_or_else(|| anyhow::anyhow!("Cannot allocate preview"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixels.as_mut(),
    );
    pixels.save_png(&args[2])?;
    println!(
        "{}",
        serde_json::json!({"page":page.meta.name,"export_raster_fallback":fallback,"warnings":imported.warnings})
    );
    Ok(())
}
