//! Native editable PPTX fixture and bounded independent-reader inspection utility.
use emulsion_core::{
    Command, Document, Node, NodeKind,
    command::Slot,
    design_interactions::Action,
    project::{ProjectEditor, ProjectKind},
    text::{TextRun, TextSpec},
};
use emulsion_raster::{
    Placement, Raster,
    vector::{Path, PathStyle},
    vector_geometry,
};
use std::sync::Arc;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let path = std::path::Path::new(
        args.get(2)
            .ok_or("Usage: pptx_fixture write|read FILE.pptx")?,
    );
    if args[1] == "read" {
        let imported = emulsion_io::pptx::read(path)?;
        let texts = imported
            .project
            .pages
            .iter()
            .flat_map(|p| {
                p.doc.nodes.iter().filter_map(|n| {
                    if let NodeKind::Text { spec, .. } = &n.kind {
                        Some(spec.text.clone())
                    } else {
                        None
                    }
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::json!({"pages":imported.project.pages.len(),"texts":texts,"objects":imported.project.pages.iter().map(|p|p.doc.nodes.len()).sum::<usize>(),"warnings":imported.warnings})
        );
        return Ok(());
    }
    let mut doc = Document::new(960, 540);
    let group = Command::AddNode {
        node: Box::new(Node::group(0, "Editable group")),
        slot: Slot::TOP,
    }
    .apply(&mut doc)?
    .unwrap();
    let mut spec = TextSpec {
        text: "Emulsion editable PowerPoint\nNative text, vectors and pictures".into(),
        font: "DejaVu Sans".into(),
        size: 32.,
        x: 56.,
        y: 44.,
        width: Some(780.),
        height: Some(120.),
        ..Default::default()
    };
    let mut strong = spec.base_style();
    strong.bold = true;
    strong.underline = true;
    strong.color = [35, 70, 175, 255];
    spec.runs.push(TextRun {
        start: 0,
        end: 8,
        style: strong,
    });
    let text = Command::AddNode {
        node: Box::new(Node::text(0, "Editable heading", spec, 960, 540)),
        slot: Slot::top_of(Some(group)),
    }
    .apply(&mut doc)?
    .unwrap();
    Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Cubic vector",
            Arc::new(Path::from_svg(
                "M60 220 C130 130 250 130 330 220 L330 350 L60 350 Z",
            )?),
            PathStyle {
                fill: Some([240, 110, 70, 255]),
                stroke: Some([90, 30, 10, 255]),
                width: 4.,
                ..Default::default()
            },
            960,
            540,
        )),
        slot: Slot::top_of(Some(group)),
    }
    .apply(&mut doc)?;
    Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Ellipse",
            Arc::new(vector_geometry::ellipse(400., 220., 180., 130.)),
            PathStyle {
                fill: Some([30, 165, 175, 255]),
                stroke: None,
                ..Default::default()
            },
            960,
            540,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)?;
    let picture_pixels = (0..64 * 64)
        .flat_map(|index| match (index % 64 >= 32, index / 64 >= 32) {
            (false, false) => [255, 0, 0, 255],
            (true, false) => [0, 255, 0, 255],
            (false, true) => [0, 0, 255, 255],
            (true, true) => [255, 255, 0, 255],
        })
        .collect::<Vec<u8>>();
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Editable picture",
            Arc::new(Raster::from_srgba8(64, 64, &picture_pixels)),
            Placement {
                x: 680.,
                y: 220.,
                scale_x: 120. / 64.,
                scale_y: 120. / 64.,
                rotation: 10.,
                ..Default::default()
            },
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)?;
    doc.design.speaker_notes = "Presenter notes survive an independent PowerPoint reader.".into();
    doc.design.interactions.insert(
        text,
        vec![Action::Url {
            url: "https://example.com/emulsion".into(),
        }],
    );
    let mut project = ProjectEditor::new_project(ProjectKind::Design, doc)?;
    let mut second = Document::new(960, 540);
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Second slide",
            TextSpec {
                text: "Second editable slide".into(),
                font: "DejaVu Sans".into(),
                size: 42.,
                x: 60.,
                y: 60.,
                ..Default::default()
            },
            960,
            540,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut second)?;
    project.add_page(second, "Second slide".into(), 0.)?;
    let report = emulsion_io::pptx::write(&project.snapshot().unwrap(), &[1, 2], path)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
