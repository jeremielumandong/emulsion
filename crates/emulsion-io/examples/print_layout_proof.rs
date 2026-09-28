//! Generate a physical trim/bleed proof without discovering or submitting to printers.
//! cargo run -p emulsion-io --example print_layout_proof -- /tmp/print-proof
use emulsion_core::{Command, Document, Node, command::Slot, text::TextSpec};
use emulsion_io::printing::{self as print, Layout, Settings};
use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
fn main() -> anyhow::Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Supply an output directory"))?;
    std::fs::create_dir_all(&directory)?;
    let mut doc = Document::new(1000, 600);
    doc.resolution = 254.; // 10 source pixels per millimeter.
    for (name, rect, fill, stroke) in [
        (
            "Authored bleed",
            [-30., -30., 1060., 660.],
            Some([224, 238, 250, 255]),
            None,
        ),
        (
            "Trim rectangle",
            [0., 0., 1000., 600.],
            None,
            Some([20, 30, 40, 255]),
        ),
    ] {
        Command::AddNode {
            node: Box::new(Node::path(
                0,
                name,
                Arc::new(rectangle(rect[0], rect[1], rect[2], rect[3])),
                PathStyle {
                    fill,
                    stroke,
                    width: 2.,
                    ..Default::default()
                },
                1000,
                600,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)?;
    }
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Proof dimensions",
            TextSpec {
                text: "100 × 60 mm trim\n3 mm authored bleed\nPrint at 100% / actual size".into(),
                x: 80.,
                y: 160.,
                size: 44.,
                ..Default::default()
            },
            1000,
            600,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)?;
    let cancel = AtomicBool::new(false);
    let sources = print::prepare_sources(vec![("100 × 60 mm proof".into(), doc)], &cancel)?;
    for (name, layout) in [
        ("document-proof", Layout::Document),
        ("contact-proof", Layout::Repeat),
    ] {
        let mut settings = Settings {
            layout,
            ..Default::default()
        };
        settings.creative.bleed_mm = 3.;
        settings.creative.crop_marks = true;
        if layout == Layout::Repeat {
            settings.creative.labels = print::LabelMode::Name;
            settings.creative.artwork_mm = Some([100., 60.]);
            settings.creative.columns = 1;
            settings.creative.rows = 3;
        }
        let job = print::layout(&sources, &[0], &settings)?;
        print::write_pdf(
            &sources,
            &job,
            false,
            &directory.join(format!("{name}.pdf")),
            &cancel,
        )?;
        print::preview(&sources, &job.sheets[0], false, 1200)?
            .save(directory.join(format!("{name}.png")))?;
        println!(
            "{name}: {:.2} × {:.2} mm sheet; {:.2} × {:.2} mm trim",
            job.sheets[0].width,
            job.sheets[0].height,
            job.sheets[0].items[0].trim.w,
            job.sheets[0].items[0].trim.h
        );
    }
    if let Some(profile) = std::env::args_os().nth(2).map(PathBuf::from) {
        for (name, standard) in [
            ("pdfx1a", print::production::PdfStandard::PdfX1a2001),
            ("pdfx3", print::production::PdfStandard::PdfX32002),
        ] {
            let mut settings = Settings {
                layout: Layout::Document,
                ..Default::default()
            };
            settings.creative.bleed_mm = 3.;
            settings.creative.crop_marks = true;
            settings.production.standard = standard;
            settings.production.profile = Some(profile.clone());
            settings.production.condition = "User-selected press profile".into();
            let job = print::layout(&sources, &[0], &settings)?;
            print::production::write_pdf(
                &sources,
                &job,
                &settings,
                &directory.join(format!("{name}.pdf")),
                &cancel,
            )?;
            print::production::preview(&sources, &job.sheets[0], &settings, 1200)?
                .save(directory.join(format!("{name}-proof.png")))?;
            println!("Saved {name}: flattened CMYK, embedded output profile, 100 × 60 mm trim");
        }
    }
    Ok(())
}
