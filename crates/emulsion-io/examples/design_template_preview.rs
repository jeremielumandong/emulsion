//! Render a native responsive starter for visual review, without launching the UI.
use emulsion_core::design::Template;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args().nth(1).ok_or("Pass an output directory")?;
    let directory = std::path::Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    let template = Template::Responsive(0);
    let (w, h) = template.native_size();
    let doc = template.create(w, h)?;
    for (name, doc) in [
        ("desktop", doc.clone()),
        (
            "phone",
            emulsion_core::design_metadata::resize_variant(&doc, 375, 1600)?.doc,
        ),
    ] {
        let raster = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
        std::fs::write(
            directory.join(format!("{name}.png")),
            emulsion_io::export::png8(doc.width, doc.height, &raster.to_srgba8())?,
        )?;
        std::fs::write(
            directory.join(format!("{name}.svg")),
            emulsion_io::project_export::vector_svg(&doc)?,
        )?;
    }
    Ok(())
}
