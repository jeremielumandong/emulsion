//! Native artwork contact sheet plus representative editable SVGs for handoff review.
//! cargo run -p emulsion-io --example design_starter_preview -- NEW_DIRECTORY
use emulsion_core::design::Template;
use image::{GenericImage, Rgba, RgbaImage};
fn main() -> anyhow::Result<()> {
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Pass a new output directory"))?,
    );
    std::fs::create_dir(&directory)?;
    let templates: Vec<_> = Template::catalog()
        .filter(|t| !matches!(t, Template::Responsive(_)))
        .collect();
    let mut sheet = RgbaImage::from_pixel(
        7 * 240,
        templates.len().div_ceil(7) as u32 * 240,
        Rgba([30, 30, 32, 255]),
    );
    let mut labels = String::new();
    for (i, template) in templates.into_iter().enumerate() {
        let (w, h) = template.native_size();
        let k = 224. / w.max(h) as f64;
        let doc = template
            .create((w as f64 * k).round() as u32, (h as f64 * k).round() as u32)
            .map_err(anyhow::Error::msg)?;
        let raster = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
        let image = RgbaImage::from_raw(doc.width, doc.height, raster.to_srgba8())
            .ok_or_else(|| anyhow::anyhow!("Invalid render"))?;
        sheet.copy_from(
            &image,
            (i % 7) as u32 * 240 + (240 - doc.width) / 2,
            (i / 7) as u32 * 240 + (240 - doc.height) / 2,
        )?;
        labels.push_str(&format!("{}\t{}\n", i + 1, template.label()));
        if !(22..140).contains(&i) {
            std::fs::write(
                directory.join(format!("{:03}.svg", i + 1)),
                emulsion_io::project_export::vector_svg(&doc)?,
            )?;
            image.save(directory.join(format!("{:03}.png", i + 1)))?;
        }
    }
    sheet.save(directory.join("contact-sheet.png"))?;
    std::fs::write(directory.join("index.tsv"), labels)?;
    println!("{}", directory.display());
    Ok(())
}
