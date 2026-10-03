//! Review and verify every curated design family without opening the editor.
//! cargo run -p emulsion-io --example design_family_preview -- NEW_DIRECTORY
//! Optional: append --invitation-baseline EXISTING_INVITATION_PREVIEW_DIRECTORY
//!
//! All layout/palette combinations are rendered at proportional preview sizes,
//! round-tripped through .emu, and checked at native size for text and SVG export.
//! Default choices also produce full-resolution editable fixtures and exports.
use anyhow::{Context, ensure};
use emulsion_core::{
    Document, NodeKind,
    design::template_families::{Category, FAMILIES, Family, Selection},
    project::{Project, ProjectKind},
    text::{self, Align, TextSpec},
};
use image::{GenericImage, Rgba, RgbaImage};
use serde::Serialize;
use std::{collections::HashSet, path::Path};

const PREVIEW_EDGE: u32 = 720;
const INK: [u8; 4] = [237, 240, 244, 255];
const MUTED: [u8; 4] = [165, 175, 190, 255];
const PAPER: [u8; 4] = [24, 28, 35, 255];
const GUTTER: u32 = 24;
const HEADER: u32 = 112;
const CAPTION: u32 = 74;

#[derive(Default, Serialize)]
struct Verification {
    families: usize,
    selections: usize,
    preview_pages_rendered: usize,
    native_pages_checked: usize,
    native_text_nodes_checked: usize,
    strict_vector_exports_checked: usize,
    preview_projects_round_tripped: usize,
    native_projects_round_tripped: usize,
    native_pages_exported: usize,
    pdf_files_exported: usize,
    invitation_baseline_images_compared: usize,
}

struct Tile {
    image: RgbaImage,
    title: String,
    detail: String,
}

fn scaled_size(size: (u32, u32), edge: u32) -> (u32, u32) {
    let scale = f64::from(edge) / f64::from(size.0.max(size.1));
    (
        (f64::from(size.0) * scale).round().max(1.) as u32,
        (f64::from(size.1) * scale).round().max(1.) as u32,
    )
}

fn slug(label: &str) -> String {
    label
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

fn render(doc: &Document) -> anyhow::Result<RgbaImage> {
    let raster = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
    let image = RgbaImage::from_raw(doc.width, doc.height, raster.to_srgba8())
        .context("Invalid native render")?;
    let first = image.get_pixel(0, 0);
    ensure!(
        image.pixels().any(|pixel| pixel != first),
        "Page rendered as a flat color"
    );
    ensure!(
        image.pixels().all(|pixel| pixel[3] == 255),
        "Authored opaque background did not cover the page"
    );
    Ok(image)
}

fn label(
    sheet: &mut RgbaImage,
    text: &str,
    position: (u32, u32),
    width: u32,
    size: f32,
    color: [u8; 4],
) -> anyhow::Result<()> {
    let height = (size * 2.7).ceil() as u32;
    let raster = text::rasterize(
        &TextSpec {
            text: text.into(),
            font: "Geist".into(),
            size,
            color,
            width: Some(width as f32),
            line_height: 1.2,
            ..Default::default()
        },
        width,
        height,
    );
    let image = RgbaImage::from_raw(width, height, raster.to_srgba8())
        .context("Invalid contact sheet label")?;
    image::imageops::overlay(sheet, &image, position.0.into(), position.1.into());
    Ok(())
}

/// Each sheet fits images into a category-appropriate box without stretching.
fn contact_sheet(
    title: &str,
    subtitle: &str,
    tiles: &[Tile],
    columns: u32,
    image_box: (u32, u32),
) -> anyhow::Result<RgbaImage> {
    ensure!(!tiles.is_empty() && columns > 0, "An empty contact sheet");
    let rows = (tiles.len() as u32).div_ceil(columns);
    let cell = (image_box.0 + GUTTER, image_box.1 + CAPTION + GUTTER);
    let width = GUTTER + columns * cell.0;
    let height = HEADER + rows * cell.1;
    let mut sheet = RgbaImage::from_pixel(width, height, Rgba(PAPER));
    label(
        &mut sheet,
        title,
        (GUTTER, 22),
        width - GUTTER * 2,
        25.,
        INK,
    )?;
    label(
        &mut sheet,
        subtitle,
        (GUTTER, 60),
        width - GUTTER * 2,
        14.,
        MUTED,
    )?;
    for (index, tile) in tiles.iter().enumerate() {
        let x = GUTTER + index as u32 % columns * cell.0;
        let y = HEADER + index as u32 / columns * cell.1;
        let thumb = image::imageops::thumbnail(&tile.image, image_box.0, image_box.1);
        // A quiet backing gives each native aspect ratio a consistent visual edge.
        for py in y..y + image_box.1 {
            for px in x..x + image_box.0 {
                sheet.put_pixel(px, py, Rgba([35, 41, 50, 255]));
            }
        }
        sheet.copy_from(
            &thumb,
            x + (image_box.0 - thumb.width()) / 2,
            y + (image_box.1 - thumb.height()) / 2,
        )?;
        label(
            &mut sheet,
            &tile.title,
            (x, y + image_box.1 + 13),
            image_box.0,
            15.,
            INK,
        )?;
        label(
            &mut sheet,
            &tile.detail,
            (x, y + image_box.1 + 39),
            image_box.0,
            12.,
            MUTED,
        )?;
    }
    Ok(sheet)
}

fn check_project(project: &Project, family: &Family, size: (u32, u32)) -> anyhow::Result<()> {
    project.validate().map_err(anyhow::Error::msg)?;
    ensure!(project.kind == ProjectKind::Design, "Not a Design project");
    ensure!(
        project.pages.len() == family.page_labels().len(),
        "{} contains alternative layouts instead of the chosen content set",
        family.label
    );
    for (page, expected) in project.pages.iter().zip(family.page_labels()) {
        ensure!(
            page.meta.name == format!("{} · {expected}", family.label),
            "Wrong page label: {}",
            page.meta.name
        );
        ensure!(
            (page.doc.width, page.doc.height) == size,
            "Wrong canvas size: {}",
            page.meta.name
        );
        let resolution = match family.occasion {
            Category::Wedding | Category::Birthday | Category::Posters => 300.,
            Category::Social | Category::Presentations => 72.,
        };
        ensure!(
            page.doc.resolution == resolution,
            "Wrong page resolution: {}",
            page.meta.name
        );
        ensure!(
            page.doc
                .nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Text { .. }))
                && page
                    .doc
                    .nodes
                    .iter()
                    .any(|n| matches!(n.kind, NodeKind::Path { .. })),
            "Missing editable text or vector artwork: {}",
            page.meta.name
        );
        ensure!(
            page.doc.nodes.iter().all(|n| matches!(
                n.kind,
                NodeKind::Text { .. } | NodeKind::Path { .. } | NodeKind::Fill { .. }
            )),
            "Flattened or unexpected template content: {}",
            page.meta.name
        );
    }
    Ok(())
}

/// Measure the unclipped paragraph and each authored line, not the clipped box.
fn check_text(doc: &Document) -> anyhow::Result<usize> {
    let mut count = 0;
    let mut rectangles: Vec<(&str, [f64; 4])> = Vec::new();
    for node in &doc.nodes {
        let NodeKind::Text { spec, .. } = &node.kind else {
            continue;
        };
        ensure!(
            spec.text_path.is_none() && spec.warp.is_identity() && spec.runs.is_empty(),
            "{} needs a separate effect/rich-text overflow review",
            node.name
        );
        let mut full = spec.as_ref().clone();
        full.height = None;
        let bounds = text::layout(&full).bounds();
        if let Some(height) = spec.height {
            ensure!(
                bounds.y + bounds.height <= height + 0.5,
                "{} clips vertically",
                node.name
            );
        }
        if let Some(width) = spec.width {
            for line in spec.text.lines() {
                let unwrapped = TextSpec {
                    text: line.into(),
                    width: None,
                    height: None,
                    align: Align::Left,
                    ..full.clone()
                };
                ensure!(
                    text::layout(&unwrapped).bounds().width <= width + 0.5,
                    "{} unexpectedly wraps: {line}",
                    node.name
                );
            }
        }
        let transform = spec.transform();
        let x = f64::from(bounds.x);
        let y = f64::from(bounds.y);
        let right = x + f64::from(bounds.width);
        let bottom = y + f64::from(bounds.height);
        let corners = [(x, y), (right, y), (x, bottom), (right, bottom)]
            .map(|(x, y)| transform.transform_point2(glam::dvec2(x, y)));
        let rect = [
            corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            corners
                .iter()
                .map(|p| p.x)
                .fold(f64::NEG_INFINITY, f64::max),
            corners
                .iter()
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max),
        ];
        ensure!(
            rect[0] >= -0.5
                && rect[1] >= -0.5
                && rect[2] <= f64::from(doc.width) + 0.5
                && rect[3] <= f64::from(doc.height) + 0.5,
            "{} is outside its native canvas: {rect:?}",
            node.name
        );
        for (name, other) in &rectangles {
            let overlap_x = rect[2].min(other[2]) - rect[0].max(other[0]);
            let overlap_y = rect[3].min(other[3]) - rect[1].max(other[1]);
            ensure!(
                overlap_x <= 0.5 || overlap_y <= 0.5,
                "Text collision: {} / {name} ({overlap_x:.2} × {overlap_y:.2})",
                node.name
            );
        }
        rectangles.push((&node.name, rect));
        count += 1;
    }
    Ok(count)
}

fn checked_svg(doc: &Document) -> anyhow::Result<Vec<u8>> {
    let svg = emulsion_io::project_export::vector_svg(doc)?;
    ensure!(
        !std::str::from_utf8(&svg)?.contains("<image"),
        "Strict vector export contains a raster image"
    );
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
    ensure!(
        tree.size().width() == doc.width as f32 && tree.size().height() == doc.height as f32,
        "SVG does not preserve native dimensions"
    );
    Ok(svg)
}

fn round_trip(project: &Project, path: &Path) -> anyhow::Result<()> {
    emulsion_io::project::write(project, path)?;
    let reopened = emulsion_io::project::read(path)?;
    reopened.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        reopened.kind == project.kind
            && reopened.active == project.active
            && reopened.next_page_id == project.next_page_id
            && reopened.pages.len() == project.pages.len(),
        "Project identity or page count changed during save/reopen"
    );
    for (before, after) in project.pages.iter().zip(&reopened.pages) {
        ensure!(
            before.meta == after.meta,
            "Page metadata changed during save/reopen"
        );
        ensure!(
            before.doc == after.doc,
            "Editable page changed during save/reopen: {}",
            before.meta.name
        );
        ensure!(
            before.graph.len() == after.graph.len() && before.graph.head() == after.graph.head(),
            "Page history changed during save/reopen: {}",
            before.meta.name
        );
    }
    Ok(())
}

fn save_native_set(
    directory: &Path,
    family: &Family,
    report: &mut Verification,
) -> anyhow::Result<Vec<Tile>> {
    let native = family.selection().create().map_err(anyhow::Error::msg)?;
    round_trip(&native, &directory.join("selected-set.emu"))?;
    report.native_projects_round_tripped += 1;
    let mut tiles = Vec::new();
    for (index, page) in native.pages.iter().enumerate() {
        let stem = format!("page-{:02}", index + 1);
        let image = render(&page.doc)?;
        image.save(directory.join(format!("{stem}.png")))?;
        std::fs::write(
            directory.join(format!("{stem}.svg")),
            checked_svg(&page.doc)?,
        )?;
        report.native_pages_exported += 1;
        tiles.push(Tile {
            image,
            title: family.page_labels()[index].into(),
            detail: format!("{} × {} px", page.doc.width, page.doc.height),
        });
    }
    let ids: Vec<_> = native.pages.iter().map(|page| page.meta.id).collect();
    let export = emulsion_io::project_export::write(
        &native,
        &ids,
        emulsion_io::project_export::Format::Pdf,
        false,
        &directory.join("selected-set.pdf"),
    )?;
    ensure!(
        export.pages == native.pages.len() && export.rasterized_pages.is_empty(),
        "PDF fallback or incorrect page count"
    );
    report.pdf_files_exported += 1;
    Ok(tiles)
}

/// Read the previous invitation helper's files without replacing its goldens.
/// Record every comparison before failing so a regression is easy to diagnose.
fn compare_invitation_baseline(baseline: &Path, directory: &Path) -> anyhow::Result<usize> {
    let mut results = Vec::new();
    let mut changed = Vec::new();
    for (family_index, family) in FAMILIES
        .iter()
        .filter(|family| matches!(family.occasion, Category::Wedding | Category::Birthday))
        .enumerate()
    {
        for kind in ["layout", "palette", "card"] {
            for index in 0..3 {
                let filename = format!("family-{}-{kind}-{}.png", family_index + 1, index + 1);
                let expected = image::open(baseline.join(&filename))
                    .with_context(|| format!("Read invitation baseline {filename}"))?
                    .into_rgba8();
                ensure!(
                    expected.dimensions() == (600, 840),
                    "Unexpected baseline size: {filename}"
                );
                let choice = Selection {
                    variant: if kind == "layout" {
                        family.variants[index].id
                    } else {
                        family.variants[0].id
                    },
                    palette: if kind == "palette" { index } else { 0 },
                    ..family.selection()
                };
                let doc = if kind == "card" {
                    choice
                        .create_sized(600, 840)
                        .map_err(anyhow::Error::msg)?
                        .pages[index]
                        .doc
                        .clone()
                } else {
                    choice
                        .create_primary(600, 840)
                        .map_err(anyhow::Error::msg)?
                };
                let actual = render(&doc)?;
                let changed_pixels = actual
                    .pixels()
                    .zip(expected.pixels())
                    .filter(|(a, b)| a != b)
                    .count();
                let maximum_channel_difference = actual
                    .as_raw()
                    .iter()
                    .zip(expected.as_raw())
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                results.push(serde_json::json!({
                    "file": filename,
                    "changed_pixels": changed_pixels,
                    "maximum_channel_difference": maximum_channel_difference,
                    "pixel_identical": changed_pixels == 0,
                }));
                if changed_pixels > 0 {
                    actual.save(directory.join(format!("baseline-actual-{filename}")))?;
                    changed.push(filename);
                }
            }
        }
    }
    std::fs::write(
        directory.join("invitation-baseline-comparison.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    ensure!(
        changed.is_empty(),
        "Invitation baseline differences in {}. See invitation-baseline-comparison.json and baseline-actual PNGs.",
        changed.join(", ")
    );
    Ok(results.len())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = std::path::PathBuf::from(args.next().context("Pass a new output directory")?);
    let baseline = if let Some(flag) = args.next() {
        ensure!(
            flag == "--invitation-baseline",
            "Expected --invitation-baseline DIRECTORY"
        );
        Some(std::path::PathBuf::from(args.next().context(
            "Pass the existing invitation baseline directory",
        )?))
    } else {
        None
    };
    ensure!(args.next().is_none(), "Unexpected argument");
    // Never replace a previous review run or an existing user directory.
    std::fs::create_dir(&directory)?;
    let scratch = tempfile::tempdir()?;
    let mut report = Verification::default();
    let mut index =
        String::from("family\tcategory\tlayout\tpalette\tpage\tnative_width\tnative_height\n");
    let mut overview = Vec::new();
    for category in Category::ALL {
        let mut layouts = Vec::new();
        let mut palettes = Vec::new();
        for family in FAMILIES.iter().filter(|family| family.occasion == category) {
            let family_dir = directory.join(slug(family.label));
            std::fs::create_dir(&family_dir)?;
            let native_size = family.native_size();
            let preview_size = scaled_size(native_size, PREVIEW_EDGE);
            let mut choices = Vec::new();
            let mut distinct_fronts = HashSet::new();
            for variant in family.variants {
                for (palette_index, palette) in family.palettes.iter().enumerate() {
                    let selection = Selection {
                        family: family.id,
                        variant: variant.id,
                        palette: palette_index,
                    };
                    let choice =
                        format!("{} / {} / {}", family.label, variant.label, palette.label);
                    let serialized = serde_json::to_vec(&selection)?;
                    ensure!(
                        serde_json::from_slice::<Selection>(&serialized)? == selection,
                        "Selection did not round-trip: {choice}"
                    );
                    let native = selection
                        .create()
                        .map_err(anyhow::Error::msg)
                        .with_context(|| choice.clone())?;
                    check_project(&native, family, native_size).with_context(|| choice.clone())?;
                    for page in &native.pages {
                        report.native_text_nodes_checked += check_text(&page.doc)
                            .with_context(|| format!("{choice} / {}", page.meta.name))?;
                        checked_svg(&page.doc)
                            .with_context(|| format!("{choice} / {}", page.meta.name))?;
                        report.native_pages_checked += 1;
                        report.strict_vector_exports_checked += 1;
                        index.push_str(&format!(
                            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                            family.label,
                            category.label(),
                            variant.label,
                            palette.label,
                            page.meta.name,
                            native_size.0,
                            native_size.1
                        ));
                    }
                    let preview = selection
                        .create_sized(preview_size.0, preview_size.1)
                        .map_err(anyhow::Error::msg)?;
                    check_project(&preview, family, preview_size)?;
                    round_trip(&preview, &scratch.path().join("choice.emu"))
                        .with_context(|| choice.clone())?;
                    report.preview_projects_round_tripped += 1;
                    let primary = selection
                        .create_primary(preview_size.0, preview_size.1)
                        .map_err(anyhow::Error::msg)?;
                    ensure!(
                        primary == preview.pages[0].doc,
                        "Preview front differs from selected content set: {choice}"
                    );
                    for (page_index, page) in preview.pages.iter().enumerate() {
                        let image = render(&page.doc)
                            .with_context(|| format!("{choice} / {}", page.meta.name))?;
                        report.preview_pages_rendered += 1;
                        if page_index == 0 {
                            ensure!(
                                distinct_fronts.insert(image.as_raw().clone()),
                                "Duplicate layout/palette rendering: {choice}"
                            );
                            if palette_index == 0 {
                                layouts.push(Tile {
                                    image: image.clone(),
                                    title: family.label.into(),
                                    detail: variant.label.into(),
                                });
                            }
                            if variant.id == family.variants[0].id {
                                palettes.push(Tile {
                                    image: image.clone(),
                                    title: family.label.into(),
                                    detail: palette.label.into(),
                                });
                            }
                            choices.push(Tile {
                                image,
                                title: variant.label.into(),
                                detail: palette.label.into(),
                            });
                        }
                    }
                    report.selections += 1;
                }
            }
            let image_box = scaled_size(native_size, 420);
            contact_sheet(
                family.label,
                "Three layouts × three palettes · alternatives, not extra pages",
                &choices,
                3,
                image_box,
            )?
            .save(family_dir.join("layout-palette-matrix.png"))?;
            let pages = save_native_set(&family_dir, family, &mut report)?;
            let columns = pages.len() as u32;
            // A single poster gets a roomy label/header rather than a narrow strip.
            let set_box = scaled_size(native_size, if columns == 1 { 840 } else { 630 });
            contact_sheet(
                family.label,
                &format!(
                    "{} · {} · {}",
                    category.label(),
                    family.variants[0].label,
                    family.palettes[0].label
                ),
                &pages,
                columns,
                set_box,
            )?
            .save(family_dir.join("selected-set.png"))?;
            overview.push(Tile {
                image: image::imageops::thumbnail(&pages[0].image, 360, 504),
                title: family.label.into(),
                detail: format!(
                    "{} · {} page{}",
                    category.label(),
                    pages.len(),
                    if pages.len() == 1 { "" } else { "s" }
                ),
            });
            report.families += 1;
            println!(
                "Verified {}: {} choices, {} content pages per choice",
                family.label,
                family.variants.len() * family.palettes.len(),
                family.page_labels().len()
            );
        }
        let family = FAMILIES
            .iter()
            .find(|family| family.occasion == category)
            .context("Empty category")?;
        let image_box = scaled_size(family.native_size(), 420);
        contact_sheet(
            &format!("{} · layouts", category.label()),
            "Each row is one family · default palette",
            &layouts,
            3,
            image_box,
        )?
        .save(directory.join(format!("{}-layouts.png", slug(category.label()))))?;
        contact_sheet(
            &format!("{} · palettes", category.label()),
            "Each row is one family · default layout",
            &palettes,
            3,
            image_box,
        )?
        .save(directory.join(format!("{}-palettes.png", slug(category.label()))))?;
    }
    contact_sheet(
        "Design families",
        "Five purposes · ten editable families · layouts and colors chosen in preview",
        &overview,
        2,
        (360, 504),
    )?
    .save(directory.join("families-overview.png"))?;
    std::fs::write(directory.join("index.tsv"), index)?;
    if let Some(baseline) = baseline {
        report.invitation_baseline_images_compared =
            compare_invitation_baseline(&baseline, &directory)?;
    }
    std::fs::write(
        directory.join("verification.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", directory.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_sizes_preserve_each_categorys_authored_proportions() {
        for family in &FAMILIES {
            let native = family.native_size();
            let size = scaled_size(native, PREVIEW_EDGE);
            assert_eq!(size.0.max(size.1), PREVIEW_EDGE);
            assert!(
                (f64::from(size.0) / f64::from(size.1) - f64::from(native.0) / f64::from(native.1))
                    .abs()
                    < 0.003
            );
        }
    }

    #[test]
    fn each_familys_native_content_set_saves_and_exports_as_vectors() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        for family in &FAMILIES {
            let project = family.selection().create().map_err(anyhow::Error::msg)?;
            check_project(&project, family, family.native_size())?;
            for page in &project.pages {
                check_text(&page.doc).with_context(|| page.meta.name.clone())?;
                checked_svg(&page.doc)?;
            }
            // Compact round trips keep routine example tests inexpensive. The
            // review runner additionally saves/reopens all native default sets.
            let size = scaled_size(family.native_size(), 240);
            let preview = family
                .selection()
                .create_sized(size.0, size.1)
                .map_err(anyhow::Error::msg)?;
            round_trip(&preview, &directory.path().join("family.emu"))?;
        }
        Ok(())
    }
}
