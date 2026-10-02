//! Review layers draw on the Stage but never leave the project: one cheap
//! check per export path, plus the optional Review column in PDFs.
use super::tests::project;
use super::{Profile, Scope, images, movie, sheet};
use emulsion_core::project::Project;
use emulsion_core::storyboard::{RenderArea, ReviewStatus};
use emulsion_core::{Command, Node, NodeKind, command::Slot};
use std::sync::atomic::AtomicBool;

/// The fixture with a solid red review layer over its first panel.
fn reviewed() -> Project {
    let mut project = project();
    let mut notes = Node::new(
        0,
        "Notes",
        NodeKind::Fill {
            rgba: [255, 0, 0, 255],
        },
    );
    notes.review = true;
    Command::AddNode {
        node: Box::new(notes),
        slot: Slot::TOP,
    }
    .apply(&mut project.pages[0].doc)
    .unwrap();
    project
}

fn no_review_layers(doc: &emulsion_core::Document) -> bool {
    doc.nodes.iter().all(|n| !n.review)
}

#[test]
fn pdf_sheets_leave_review_layers_out() {
    let project = reviewed();
    let job = sheet::Job::new(&project, "Film", &Scope::All, None, "2026-01-01".into()).unwrap();
    let sources = sheet::sources(&project, &job, &AtomicBool::new(false)).unwrap();
    let first = sources[0].document.as_ref().unwrap();
    assert!(no_review_layers(first));
    assert!(!sources[0].svg.contains("Notes"));
}

#[test]
fn panel_images_leave_review_layers_out() {
    let project = reviewed();
    let dir = tempfile::tempdir().unwrap();
    let options = images::Options {
        pattern: "{index}".into(),
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);
    let files =
        images::write(&project, "Film", &Scope::All, &options, dir.path(), &cancel).unwrap();
    let image = image::open(&files[0]).unwrap().to_rgba8();
    assert_ne!(
        image.get_pixel(5, 5).0,
        [255, 0, 0, 255],
        "the review fill printed"
    );
    // One image per layer never writes the review layer either.
    let options = images::Options {
        pattern: "{index}_{layer}".into(),
        per_layer: true,
        ..Default::default()
    };
    let layered = dir.path().join("layers");
    let files = images::write(&project, "Film", &Scope::All, &options, &layered, &cancel).unwrap();
    assert!(files.iter().all(|f| !f.to_string_lossy().contains("Notes")));
}

#[test]
fn movie_and_gif_frames_leave_review_layers_out() {
    let project = reviewed();
    let rect = movie::area_rect(&project, RenderArea::Camera).unwrap();
    let mut renderer = movie::AnimaticRenderer::new(&project, rect, (16, 9)).unwrap();
    let frame = renderer.frame(0, None).unwrap();
    assert_ne!(&frame[..4], &[255, 0, 0, 255], "the review fill rendered");
    // GIF export samples the same renderer; check one GIF end to end.
    let dir = tempfile::tempdir().unwrap();
    let gif = dir.path().join("a.gif");
    let options = movie::GifOptions {
        width: 16,
        fps: 1,
        end: Some(1),
        ..Default::default()
    };
    movie::write_gif(
        &project,
        &options,
        &gif,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let first = image::open(&gif).unwrap().to_rgba8();
    let [r, g, b, _] = first.get_pixel(4, 4).0;
    assert!(
        !(r > 200 && g < 60 && b < 60),
        "the review fill reached the GIF"
    );
}

#[test]
fn the_review_column_prints_only_when_asked() {
    let mut project = project();
    let first = project.pages[0].meta.id;
    let board = project.storyboard.as_mut().unwrap();
    board
        .set_review_status(first, ReviewStatus::InReview)
        .unwrap();
    board
        .add_review_note(first, "Ana", "Hold longer", 1)
        .unwrap();
    let job = sheet::Job::new(&project, "Film", &Scope::All, None, "2026-01-01".into()).unwrap();
    let texts = |profile: &Profile| -> String {
        let sources = sheet::sources(&project, &job, &AtomicBool::new(false)).unwrap();
        let all: Vec<_> = (0..job.entries.len()).collect();
        let layout = sheet::layout(&job, &sources, &all, profile).unwrap();
        format!("{:?}", layout.sheets[0].marks)
    };
    let mut profile = Profile::default();
    assert!(!texts(&profile).contains("Hold longer"));
    profile.review_notes = true;
    let printed = texts(&profile);
    assert!(printed.contains("In review") && printed.contains("Ana: Hold longer"));
}
