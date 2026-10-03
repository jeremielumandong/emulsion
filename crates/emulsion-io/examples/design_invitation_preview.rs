//! Render the invitation family catalog and write editable three-card fixtures.
//! cargo run -p emulsion-io --example design_invitation_preview -- NEW_DIRECTORY
use emulsion_core::{
    Document,
    design::invitations::{FAMILIES, Selection},
    text::{self, TextSpec},
};
use image::{GenericImage, Rgba, RgbaImage};
use std::path::Path;

const WIDTH: u32 = 600;
const HEIGHT: u32 = 840;
const CELL_WIDTH: u32 = 324;
const CELL_HEIGHT: u32 = 488;

fn render(doc: &Document) -> anyhow::Result<RgbaImage> {
    let raster = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
    RgbaImage::from_raw(doc.width, doc.height, raster.to_srgba8())
        .ok_or_else(|| anyhow::anyhow!("Invalid native render"))
}

fn sheet() -> RgbaImage {
    RgbaImage::from_pixel(3 * CELL_WIDTH, 4 * CELL_HEIGHT, Rgba([32, 34, 37, 255]))
}

fn add_cell(
    sheet: &mut RgbaImage,
    image: &RgbaImage,
    row: u32,
    column: u32,
    label: &str,
) -> anyhow::Result<()> {
    let x = column * CELL_WIDTH + 12;
    let y = row * CELL_HEIGHT + 12;
    let thumb = image::imageops::thumbnail(image, 300, 420);
    sheet.copy_from(&thumb, x, y)?;
    let label = text::rasterize(
        &TextSpec {
            text: label.into(),
            font: "Geist".into(),
            size: 13.,
            color: [239, 241, 245, 255],
            x: 0.,
            y: 0.,
            width: Some(300.),
            line_height: 1.2,
            ..Default::default()
        },
        300,
        48,
    );
    let label = RgbaImage::from_raw(300, 48, label.to_srgba8())
        .ok_or_else(|| anyhow::anyhow!("Invalid contact sheet label"))?;
    image::imageops::overlay(sheet, &label, x as i64, (y + 428) as i64);
    Ok(())
}

fn save_artifact(
    directory: &Path,
    stem: &str,
    doc: &Document,
    image: &RgbaImage,
) -> anyhow::Result<()> {
    image.save(directory.join(format!("{stem}.png")))?;
    std::fs::write(
        directory.join(format!("{stem}.svg")),
        emulsion_io::project_export::vector_svg(doc)?,
    )?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Pass a new output directory"))?,
    );
    std::fs::create_dir(&directory)?;
    let mut layouts = sheet();
    let mut palettes = sheet();
    let mut matching = sheet();
    let mut index = String::from("file\tfamily\tlayout\tpalette\tcard\n");
    for (row, family) in FAMILIES.iter().enumerate() {
        let default = Selection::for_family(family.id);
        for (column, variant) in family.variants.iter().enumerate() {
            let selection = Selection {
                variant: variant.id,
                ..default
            };
            let doc = selection
                .create_primary(WIDTH, HEIGHT)
                .map_err(anyhow::Error::msg)?;
            let image = render(&doc)?;
            let stem = format!("family-{}-layout-{}", row + 1, column + 1);
            save_artifact(&directory, &stem, &doc, &image)?;
            add_cell(
                &mut layouts,
                &image,
                row as u32,
                column as u32,
                &format!("{}\n{}", family.label, variant.label),
            )?;
            index.push_str(&format!(
                "{stem}\t{}\t{}\t{}\tInvitation\n",
                family.label, variant.label, family.palettes[0].label
            ));
        }
        for (column, palette) in family.palettes.iter().enumerate() {
            let selection = Selection {
                palette: column,
                ..default
            };
            let doc = selection
                .create_primary(WIDTH, HEIGHT)
                .map_err(anyhow::Error::msg)?;
            let image = render(&doc)?;
            let stem = format!("family-{}-palette-{}", row + 1, column + 1);
            save_artifact(&directory, &stem, &doc, &image)?;
            add_cell(
                &mut palettes,
                &image,
                row as u32,
                column as u32,
                &format!("{}\n{}", family.label, palette.label),
            )?;
            index.push_str(&format!(
                "{stem}\t{}\t{}\t{}\tInvitation\n",
                family.label, family.variants[0].label, palette.label
            ));
        }
        let preview = default
            .create_sized(WIDTH, HEIGHT)
            .map_err(anyhow::Error::msg)?;
        for (column, page) in preview.pages.iter().enumerate() {
            let image = render(&page.doc)?;
            let stem = format!("family-{}-card-{}", row + 1, column + 1);
            save_artifact(&directory, &stem, &page.doc, &image)?;
            add_cell(
                &mut matching,
                &image,
                row as u32,
                column as u32,
                &page.meta.name,
            )?;
            index.push_str(&format!(
                "{stem}\t{}\t{}\t{}\t{}\n",
                family.label, family.variants[0].label, family.palettes[0].label, page.meta.name
            ));
        }
        // Each native fixture has only its selected invitation, Details and RSVP.
        let native = default.create().map_err(anyhow::Error::msg)?;
        let path = directory.join(format!("family-{}-matching-set.emu", row + 1));
        emulsion_io::project::write(&native, &path)?;
        let reopened = emulsion_io::project::read(&path)?;
        anyhow::ensure!(
            reopened.pages.len() == 3,
            "A matching fixture must have exactly three cards"
        );
    }
    layouts.save(directory.join("layouts-contact-sheet.png"))?;
    palettes.save(directory.join("palettes-contact-sheet.png"))?;
    matching.save(directory.join("matching-sets-contact-sheet.png"))?;
    std::fs::write(directory.join("index.tsv"), index)?;
    println!("{}", directory.display());
    Ok(())
}
