//! Generate a small editable project and export set for manual interoperability checks.
use emulsion_core::{
    command::Slot,
    creation::{CanvasKind, CanvasSpec},
    design::{Element, Template},
};
use emulsion_io::{
    project,
    project_export::{self, Format},
};
use std::{path::PathBuf, sync::Arc};

fn main() -> anyhow::Result<()> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Usage: project_fixture <new-output-directory>"))?,
    );
    std::fs::create_dir(&directory)?;
    let mut editor = CanvasSpec {
        kind: CanvasKind::Design,
        width: 640.,
        height: 480.,
        bleed_mm: 3.,
        ..Default::default()
    }
    .create_project()
    .map_err(anyhow::Error::msg)?;
    editor
        .add_page(
            Template::Announcement
                .create(640, 480)
                .map_err(anyhow::Error::msg)?,
            "Announcement".into(),
            3.,
        )
        .map_err(anyhow::Error::msg)?;
    let frame = emulsion_core::design::frame(&editor.doc, Element::Circle);
    let group = frame
        .paste(&mut editor, Slot::TOP, (140., 80.))
        .map_err(anyhow::Error::msg)?[0];
    let raster = Arc::new(emulsion_raster::Raster::from_fn(
        100,
        100,
        [0; 4],
        |x, y| emulsion_raster::color::f_to_px([x as f32 / 100., 0.3, y as f32 / 100., 1.]),
    ));
    emulsion_core::design::place_in_frame(&mut editor, group, raster)
        .map_err(anyhow::Error::msg)?;
    editor.remove_page(1).map_err(anyhow::Error::msg)?;
    editor
        .add_page(
            Template::Editorial
                .create(640, 480)
                .map_err(anyhow::Error::msg)?,
            "Editorial".into(),
            3.,
        )
        .map_err(anyhow::Error::msg)?;
    let project = editor.snapshot().unwrap();
    let ids = project.pages.iter().map(|p| p.meta.id).collect::<Vec<_>>();
    project::write(&project, &directory.join("campaign.emu"))?;
    for format in Format::ALL {
        let path = directory.join(if format == Format::Pdf {
            "campaign.pdf".into()
        } else {
            format!("campaign-{}.zip", format.extension())
        });
        let report = project_export::write(&project, &ids, format, true, &path)?;
        println!(
            "{}: {} pages, {} raster appearance fallbacks",
            path.display(),
            report.pages,
            report.rasterized_pages.len()
        );
    }
    Ok(())
}
