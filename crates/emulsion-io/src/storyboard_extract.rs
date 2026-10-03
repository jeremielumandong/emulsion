//! Extract and merge files: an extract is an ordinary `.emu` storyboard
//! package written by the project writer, whose storyboard carries the
//! extract record (see `emulsion_core::storyboard_extract`). Reading one
//! back checks it is an extract before a merge looks at it.
use crate::{IoError, Result, project};
use emulsion_core::project::Project;
use emulsion_core::storyboard::GroupId;
use emulsion_core::storyboard_extract::extract_scenes;
use std::path::Path;

/// Seconds since 1970, for extract records.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Write the scenes `groups` names (scenes, or the sequences and acts that
/// hold them) of `source` to a new `.emu` at `path`. Returns the extract.
pub fn write_extract(
    source: &Project,
    groups: &[GroupId],
    source_name: &str,
    path: &Path,
) -> Result<Project> {
    if !project::is_project(path) {
        return Err(IoError::Manifest(
            "Save the extract as an .emu project.".into(),
        ));
    }
    let extract = extract_scenes(source, groups, source_name, now()).map_err(IoError::Manifest)?;
    project::write(&extract, path)?;
    Ok(extract)
}

/// Read an extract made by `write_extract` (and perhaps edited since).
pub fn read_extract(path: &Path) -> Result<Project> {
    let extract = project::read(path)?;
    if extract
        .storyboard
        .as_ref()
        .is_none_or(|b| b.extract.is_none())
    {
        return Err(IoError::Manifest(format!(
            "{} is not an extract made with Extract Scenes.",
            path.display()
        )));
    }
    Ok(extract)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::command::Slot;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard::{Level, Panel};
    use emulsion_core::storyboard_extract::{ConflictKind, MergeOptions};
    use emulsion_core::storyboard_fingerprint::panel_fingerprint;
    use emulsion_core::{Command, Document, Node, NodeKind};
    use emulsion_raster::{BlendMode, Placement, Raster};
    use std::sync::Arc;

    fn layout(p: &ProjectEditor) -> Vec<u64> {
        p.page_list().iter().map(|m| m.id).collect()
    }

    /// Three panels in two scenes, with a few kinds of layer.
    fn source() -> ProjectEditor {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(300, 40)).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let items = (2..=3)
            .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
            .collect();
        p.insert_panels(Some(1), &blank, items, None).unwrap();
        let ids = layout(&p);
        p.edit_storyboard(|b| {
            b.split(&ids, ids[1], Level::Scene, Some("Chase"))
                .map(|_| ())
        })
        .unwrap();
        p.set_active_page(ids[1]).unwrap();
        let mut ink = Node::raster(
            0,
            "Ink",
            Arc::new(Raster::solid(300, 40, [0.3, 0.1, 0.05, 0.7])),
            Placement::default(),
        );
        ink.blend = BlendMode::Multiply;
        ink.opacity = 0.37;
        p.execute(Command::AddNode {
            node: Box::new(ink),
            slot: Slot::TOP,
        })
        .unwrap();
        p.execute(Command::AddNode {
            node: Box::new(Node::group(0, "Characters")),
            slot: Slot::TOP,
        })
        .unwrap();
        p
    }

    #[test]
    fn fingerprints_survive_saving_and_opening() {
        let p = source();
        let project = p.snapshot().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("board.emu");
        project::write(&project, &file).unwrap();
        let back = project::read(&file).unwrap();
        let board = back.storyboard.as_ref().unwrap();
        assert_eq!(
            board.project_id,
            project.storyboard.as_ref().unwrap().project_id
        );
        for (a, b) in project.pages.iter().zip(&back.pages) {
            let panel = &board.panels[&a.meta.id];
            assert_eq!(
                panel_fingerprint(&a.meta.name, &a.doc, panel),
                panel_fingerprint(&b.meta.name, &b.doc, panel),
                "{}",
                a.meta.name
            );
        }
    }

    #[test]
    fn a_project_without_an_id_gets_one_that_saving_keeps() {
        let mut old = source().snapshot().unwrap();
        // Saved before project IDs: the field is not written.
        old.storyboard.as_mut().unwrap().project_id.clear();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("old.emu");
        project::write(&old, &file).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut json = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("storyboard.json").unwrap(), &mut json)
            .unwrap();
        assert!(!json.contains("project_id"));
        let opened = project::read(&file).unwrap();
        let id = opened.storyboard.as_ref().unwrap().project_id.clone();
        assert_eq!(id.len(), 36);
        project::write(&opened, &file).unwrap();
        let again = project::read(&file).unwrap();
        assert_eq!(again.storyboard.as_ref().unwrap().project_id, id);
    }

    #[test]
    fn extracts_round_trip_through_files_and_merge_back() {
        let mut p = source();
        let ids = layout(&p);
        let chase = p.storyboard().unwrap().panels[&ids[1]].scene;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chase.emu");
        assert!(
            write_extract(
                &p.snapshot().unwrap(),
                &[chase],
                "Film",
                &dir.path().join("x.ora")
            )
            .is_err()
        );
        write_extract(&p.snapshot().unwrap(), &[chase], "Film", &file).unwrap();
        assert!(read_extract(&dir.path().join("missing.emu")).is_err());

        // The other artist opens it, draws and saves.
        let opened = read_extract(&file).unwrap();
        let mut artist = ProjectEditor::open(opened, Some(file.clone())).unwrap();
        artist.set_active_page(ids[2]).unwrap();
        artist
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Clean",
                    NodeKind::Fill {
                        rgba: [1, 2, 3, 255],
                    },
                )),
                slot: Slot::TOP,
            })
            .unwrap();
        project::write(&artist.snapshot().unwrap(), &file).unwrap();

        // Unchanged panels have no conflict after the trip through files;
        // a panel changed here since has one.
        let theirs = read_extract(&file).unwrap();
        assert!(p.plan_merge(&theirs).unwrap().conflicts.is_empty());
        let plain = dir.path().join("plain.emu");
        project::write(&p.snapshot().unwrap(), &plain).unwrap();
        assert!(read_extract(&plain).is_err());
        p.set_active_page(ids[1]).unwrap();
        let id = p.doc.nodes[0].id;
        p.execute(Command::Rename {
            id,
            name: "Paper".into(),
        })
        .unwrap();
        let report = p.plan_merge(&theirs).unwrap();
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(report.conflicts[0].kind, ConflictKind::ChangedHere);
        assert!(!report.conflicts[0].changed_there);
        p.merge_extract(&theirs, &MergeOptions::default()).unwrap();
        let after = layout(&p);
        assert_eq!(
            p.page(after[1]).unwrap().doc.nodes[0].name,
            "Paper",
            "mine was kept"
        );
        assert!(
            p.page(after[2])
                .unwrap()
                .doc
                .nodes
                .iter()
                .any(|n| n.name == "Clean")
        );
    }
}
