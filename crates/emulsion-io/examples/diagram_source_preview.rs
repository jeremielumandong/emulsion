//! Render a diagram source through the real import path, save/reopen it, and export a preview.
//! Usage: cargo run -p emulsion-io --example diagram_source_preview -- input.mmd output-directory
use std::path::Path;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 3, "Pass an input source and output directory");
    let imported = emulsion_io::diagram_import::read(Path::new(&args[1]))?;
    let directory = Path::new(&args[2]);
    std::fs::create_dir_all(directory)?;
    let native = directory.join("diagram.emu");
    emulsion_io::project::write(&imported.project, &native)?;
    let reopened = emulsion_io::project::read(&native)?;
    for (i, page) in reopened.pages.iter().enumerate() {
        let (svg, rasterized) = emulsion_io::project_export::svg(&page.doc)?;
        anyhow::ensure!(!rasterized, "Expected vector export");
        let (before, _) = emulsion_io::project_export::svg(&imported.project.pages[i].doc)?;
        anyhow::ensure!(svg == before, "Save/reopen changed the vector artwork");
        anyhow::ensure!(
            page.doc.diagram == imported.project.pages[i].doc.diagram,
            "Save/reopen changed diagram metadata"
        );
        std::fs::write(directory.join(format!("page-{}.svg", i + 1)), &svg)?;
        let mut options = resvg::usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        options
            .fontdb_mut()
            .load_font_data(include_bytes!("../../../assets/fonts/Geist.ttf").to_vec());
        options.fontdb_mut().set_sans_serif_family("Geist");
        options.fontdb_mut().set_serif_family("Liberation Serif");
        options.image_href_resolver.resolve_string = Box::new(|_, _| None);
        let tree = resvg::usvg::Tree::from_data(&svg, &options)?;
        let scale = (1800. / page.doc.width.max(page.doc.height) as f32).min(1.);
        let mut pixels = resvg::tiny_skia::Pixmap::new(
            (page.doc.width as f32 * scale).ceil() as u32,
            (page.doc.height as f32 * scale).ceil() as u32,
        )
        .unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixels.as_mut(),
        );
        pixels.save_png(directory.join(format!("page-{}.png", i + 1)))?;
        println!(
            "Page {}: {} × {}, {} native objects, vector export",
            i + 1,
            page.doc.width,
            page.doc.height,
            page.doc.nodes.len()
        );
    }
    for warning in imported.warnings {
        println!("{warning}");
    }
    Ok(())
}
