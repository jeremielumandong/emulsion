//! Board versions, review notes and review layers in `.emu` packages.
use super::{STORYBOARD_ENTRY, read, write};
use emulsion_core::command::Slot;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::{Panel, ReviewStatus};
use emulsion_core::storyboard_versions::Baseline;
use emulsion_core::{Command, Document, Node, NodeKind};
use std::io::Read;
use zip::ZipArchive;

fn path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "emulsion-versions-{label}-{}.emu",
        std::process::id()
    ))
}

fn storyboard_json(file: &std::path::Path) -> String {
    let mut zip = ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name(STORYBOARD_ENTRY)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[test]
fn versions_reviews_and_removed_panels_round_trip() {
    let file = path("round-trip");
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let second = p
        .insert_panels(
            Some(1),
            &blank,
            vec![("Two".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap()[0];
    p.set_active_page(second).unwrap();
    p.execute(Command::AddNode {
        node: Box::new(Node::new(0, "Ink", NodeKind::Fill { rgba: [0; 4] })),
        slot: Slot::TOP,
    })
    .unwrap();
    let review = emulsion_core::storyboard_review::review_layer(&p.doc, "Review");
    let review = p
        .execute(Command::AddNode {
            node: Box::new(review),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    p.edit_storyboard(|b| {
        b.set_review_status(1, ReviewStatus::NeedsChanges)?;
        b.add_review_note(1, "Ana", "Push in more", 7).map(|_| ())
    })
    .unwrap();
    let v = p.create_board_version("Pass 1").unwrap();
    p.set_active_page(1).unwrap();
    p.remove_page(second).unwrap();
    write(&p.snapshot().unwrap(), &file).unwrap();
    let back = read(&file).unwrap();
    let opened = ProjectEditor::open(back, Some(file.clone())).unwrap();
    assert!(!opened.is_modified());
    let board = opened.storyboard().unwrap();
    assert_eq!(board.panels[&1].review.status, ReviewStatus::NeedsChanges);
    assert_eq!(board.panels[&1].review.notes[0].text, "Push in more");
    assert_eq!(opened.board_versions().len(), 1);
    // The removed panel's drawing, review layer included, comes back.
    let state = opened.board_state(Baseline::Version(v)).unwrap();
    let doc = state.doc(second).unwrap();
    assert_eq!(doc.nodes.len(), 3);
    assert!(doc.node(review).unwrap().review);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn boards_without_reviews_or_versions_save_and_load_unchanged() {
    let file = path("unchanged");
    let p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
    let project = p.snapshot().unwrap();
    write(&project, &file).unwrap();
    let json = storyboard_json(&file);
    for key in ["\"versions\"", "\"review\""] {
        assert!(!json.contains(key), "{key} written for a board without it");
    }
    let back = read(&file).unwrap();
    assert_eq!(back.storyboard, project.storyboard);
    assert!(back.pages[0].doc.nodes.iter().all(|n| !n.review));
    // Saving what was opened writes the same board data.
    let again = path("unchanged-again");
    write(&back, &again).unwrap();
    assert_eq!(storyboard_json(&again), json);
    std::fs::remove_file(file).unwrap();
    std::fs::remove_file(again).unwrap();
}
