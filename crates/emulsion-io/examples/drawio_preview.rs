//! Render an imported page for visual compatibility review.
//! Usage: drawio_preview <input.drawio|xml|svg> <output.png> [page-index] [--fit]
fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    anyhow::ensure!(args.len() >= 3, "Pass an input file and output PNG path");
    let imported = emulsion_io::diagram_import::read(std::path::Path::new(&args[1]))?;
    let index = args
        .iter().skip(3).find(|s|!s.starts_with("--"))
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
    let fit=if args.iter().any(|s|s=="--fit") {
        let bounds=page.doc.nodes.iter().filter(|n| !matches!(n.kind,emulsion_core::NodeKind::Fill{..}|emulsion_core::NodeKind::Group{..}))
            .filter_map(|n|emulsion_core::geometry::node_bounds(&page.doc,n.id))
            .fold(emulsion_raster::IRect::default(),|a,b|a.union(&b));
        Some((bounds.x as f32-12.,bounds.y as f32-12.,bounds.w as f32+24.,bounds.h as f32+24.))
    }else{None};
    let (x,y,w,h)=fit.unwrap_or((0.,0.,tree.size().width(),tree.size().height()));
    let scale = (1600. / w.max(h)).min(2.);
    std::fs::write(std::path::Path::new(&args[2]).with_extension("svg"),&svg)?;
    let mut pixels = resvg::tiny_skia::Pixmap::new(
        (w * scale).ceil() as u32,
        (h * scale).ceil() as u32,
    )
    .ok_or_else(|| anyhow::anyhow!("Cannot allocate preview"))?;
    pixels.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_row(scale,0.,0.,scale,-x*scale,-y*scale),
        &mut pixels.as_mut(),
    );
    pixels.save_png(&args[2])?;
    println!(
        "{}",
        serde_json::json!({"page":page.meta.name,"export_raster_fallback":fallback,"warnings":imported.warnings})
    );
    Ok(())
}
