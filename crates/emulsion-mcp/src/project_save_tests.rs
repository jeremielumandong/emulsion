//! Native-save results disclose only the format of a successfully written snapshot.
use super::*;
use emulsion_core::{Document, project::ProjectKind, storyboard::Panel};
use emulsion_raster::blend::BlendSpace;
use std::path::PathBuf;

struct Folder(PathBuf);
impl Folder {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "emulsion-mcp-save-format-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn storyboard(protected: bool) -> ProjectEditor {
    let mut doc = Document::new(2, 1);
    if protected {
        doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    }
    let mut editor = ProjectEditor::new_project(ProjectKind::Storyboard, doc).unwrap();
    editor
        .insert_panels(
            Some(1),
            &Document::new(2, 1),
            vec![("Remaining panel".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap();
    editor.create_board_version("Before removal").unwrap();
    editor.remove_page(1).unwrap();
    editor
}

#[test]
fn native_save_result_preserves_v1_shape_and_discloses_v2_only_after_writing() {
    let folder = Folder::new();
    for protected in [false, true] {
        let mut editor = storyboard(protected);
        let snapshot = editor.snapshot().unwrap();
        // Later edits cannot change what the snapshot or its result promise.
        assert!(editor.undo());
        assert_eq!(
            emulsion_io::project::required_version(&editor.snapshot().unwrap()),
            1
        );
        let path = folder.0.join(format!("saved-{protected}.emu"));
        let result = write_snapshot(&snapshot, "save_project", &json!({"path":path})).unwrap();
        let expected_version = if protected { 2 } else { 1 };
        let restored = emulsion_io::project::read(&path).unwrap();
        assert_eq!(
            emulsion_io::project::required_version(&restored),
            expected_version
        );
        assert_eq!(restored.storyboard.unwrap().versions.retired.len(), 1);
        assert_eq!(result["path"], json!(path));
        if protected {
            assert_eq!(result["project_format_version"], 2);
            let warnings = result["warnings"].as_array().unwrap();
            assert_eq!(warnings.len(), 1);
            let warning = warnings[0].as_str().unwrap();
            for disclosure in [
                "removed-panel history",
                "project format 2",
                "newer Emulsion",
            ] {
                assert!(warning.contains(disclosure), "{warning}");
            }
        } else {
            assert_eq!(result, json!({"path":path}), "v1 response stays unchanged");
        }
    }
}

#[test]
fn failed_native_save_returns_error_instead_of_compatibility_success() {
    let folder = Folder::new();
    let path = folder.0.join("original.emu");
    std::fs::write(&path, b"original destination").unwrap();
    let mut project = storyboard(true).snapshot().unwrap();
    project.pages[0].meta.name.clear();
    assert_eq!(emulsion_io::project::required_version(&project), 2);
    assert!(write_snapshot(&project, "save_project", &json!({"path":path})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original destination");

    let valid = storyboard(true).snapshot().unwrap();
    assert!(
        write_snapshot(
            &valid,
            "save_project",
            &json!({"path":path.join("cannot-write.emu")})
        )
        .is_err()
    );
    assert!(
        write_snapshot(
            &valid,
            "save_project",
            &json!({"path":path.with_extension("ora")})
        )
        .is_err()
    );
}

#[test]
fn export_of_v2_capable_project_does_not_claim_native_format_or_history_preservation() {
    let folder = Folder::new();
    let path = folder.0.join("pages.zip");
    let project = storyboard(true).snapshot().unwrap();
    assert_eq!(emulsion_io::project::required_version(&project), 2);
    let result = write_snapshot(
        &project,
        "export_project",
        &json!({"path":path,"format":"svg"}),
    )
    .unwrap();
    assert!(path.is_file());
    assert!(result.get("project_format_version").is_none());
    assert!(result.get("warnings").is_none());
    assert_eq!(result["pages"], 1);
}
