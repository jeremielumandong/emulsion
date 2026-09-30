//! Export the bundled Agentic AI libraries and a contact sheet; optionally install them.
use emulsion_io::{diagram_packs, template_pack};
fn main() -> anyhow::Result<()> {
    let output = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Pass an output directory [--install]"))?,
    );
    std::fs::create_dir_all(&output)?;
    let mut gallery = image::RgbaImage::from_pixel(140 * 12, 180 * 8, image::Rgba([255; 4]));
    let mut index = 0;
    let mut installed = Vec::new();
    for &(id, _, _) in diagram_packs::packs()
        .iter()
        .filter(|p| diagram_packs::is_builtin(p.0))
    {
        let (pack, notes) = diagram_packs::build(id)?;
        anyhow::ensure!(notes.is_empty(), "{notes:?}");
        let path = output.join(format!("{id}.emustencil"));
        template_pack::write(&pack.project, &pack.manifest, &path)?;
        for page in &pack.project.pages {
            let mut doc = page.doc.clone();
            emulsion_core::diagram::caption_icon_labels(&mut doc, &page.meta.name);
            let mut pixels = emulsion_io::svg_viewport::SvgViewport::new(&doc)?
                .render((140, 180), [1., 0., 0., 1., 0., 0.])?;
            // Viewport pixels are BGRA for GPUI; PNG expects RGBA.
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            let tile = image::RgbaImage::from_raw(140, 180, pixels).unwrap();
            image::imageops::overlay(&mut gallery, &tile, (index % 12) * 140, (index / 12) * 180);
            index += 1;
        }
        if std::env::args().any(|a| a == "--install") {
            let (_, asset) = template_pack::install(&emulsion_io::creative_library::root(), pack)?;
            installed.push(asset);
            println!("Installed {id}: {asset}");
        }
        println!("Wrote {}", path.display());
    }
    gallery.save(output.join("agentic-ai.png"))?;
    if !installed.is_empty() {
        let mut settings = emulsion_io::settings::Settings::load();
        for id in installed {
            if !settings.diagram_stencil_packs.contains(&id) {
                settings.diagram_stencil_packs.push(id);
            }
        }
        settings.save()?;
    }
    println!("Verified {index} editable symbols");
    Ok(())
}
