use super::*;
use crate::{editor::EditorView, workspace::Workspace};
use emulsion_core::{Document, project::ProjectEditor, project::ProjectKind, storyboard::Panel};
use emulsion_raster::blend::BlendSpace;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};

#[test]
fn completed_status_preserves_cloud_outcomes_and_legacy_messages() {
    let path = Path::new("a-very-long-directory/project.emu");
    for (cloud, expected, error) in [
        (
            Ok(false),
            t!("shell.saved", path = path.display()).into_owned(),
            false,
        ),
        (
            Ok(true),
            t!("shell.saved_cloud_queued", path = path.display()).into_owned(),
            false,
        ),
        (
            Err("cloud unavailable".into()),
            t!("shell.saved_cloud_failed", error = "cloud unavailable").into_owned(),
            true,
        ),
    ] {
        for version in [None, Some(1)] {
            assert_eq!(
                completed_status(path, version, &cloud),
                (expected.clone(), error)
            );
        }
        let (message, severity) = completed_status(path, Some(2), &cloud);
        assert_eq!(
            message,
            format!("{} {expected}", project_notice(2).unwrap())
        );
        assert_eq!(
            severity, error,
            "compatibility does not change error severity"
        );
    }
    assert!(project_notice(1).is_none());
}

fn storyboard(protected: bool) -> ProjectEditor {
    let mut doc = Document::new(32, 18);
    if protected {
        doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    }
    let mut editor = ProjectEditor::new_project(ProjectKind::Storyboard, doc).unwrap();
    editor
        .insert_panels(
            Some(1),
            &Document::new(32, 18),
            vec![("Remaining panel".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap();
    editor.create_board_version("Before removal").unwrap();
    editor.remove_page(1).unwrap();
    assert_eq!(
        emulsion_io::project::required_version(&editor.snapshot().unwrap()),
        if protected { 2 } else { 1 }
    );
    editor
}

fn open(
    cx: &mut TestAppContext,
    protected: bool,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let session = storyboard(protected);
    let (workspace, cx) = crate::tests::open(cx, Document::new(32, 18));
    let editor = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.install_project(session, "Saved history".into(), window, cx));
        });
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (workspace, editor, cx)
}

#[gpui_kit::test]
fn legacy_completed_native_save_keeps_existing_status(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("legacy.emu");
    let (workspace, editor, cx) = open(cx, false);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.write(editor.clone(), path.clone(), cx)
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        let (message, error) = editor.status.as_ref().unwrap();
        assert_eq!(message.as_ref(), t!("shell.saved", path = path.display()));
        assert!(!error);
        assert!(!editor.editor.is_modified());
        assert!(!editor.history.save_busy);
        assert!(editor.history.save_queued.is_none());
        assert_eq!(editor.editor.path.as_ref(), Some(&path));
    });
    assert_eq!(
        emulsion_io::project::required_version(&emulsion_io::project::read(&path).unwrap()),
        1
    );
}

#[gpui_kit::test]
fn completed_native_save_discloses_written_snapshot_after_editor_changes(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;

    let folder = tempfile::tempdir().unwrap();
    let path = folder
        .path()
        .join(format!("{}.emu", "long-project-name-".repeat(10)));
    let (workspace, editor, cx) = open(cx, true);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.write(editor.clone(), path.clone(), cx);
            editor.update(cx, |editor, _| {
                assert!(editor.history.save_busy);
                assert_eq!(
                    editor.status.as_ref().unwrap().0.as_ref(),
                    t!("shell.saving", path = path.display())
                );
                // This later state needs v1, but the in-flight snapshot needs v2.
                assert!(editor.editor.undo());
                assert_eq!(
                    emulsion_io::project::required_version(&editor.editor.snapshot().unwrap()),
                    1
                );
            });
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let editor = editor.read(cx);
        let (message, error) = editor.status.as_ref().unwrap();
        let expected = completed_status(&path, Some(2), &Ok(false));
        assert_eq!(message.as_ref(), expected.0);
        assert!(!error);
        assert!(
            editor.editor.is_modified(),
            "the later Undo still needs saving"
        );
        assert_eq!(editor.editor.path.as_ref(), Some(&path));
        assert!(message.starts_with(&project_notice(2).unwrap()));
        // The unchanged status strip keeps the complete message for its tooltip;
        // a long path must not precede the compatibility notice or displace metadata.
        assert!(message.ends_with(&path.display().to_string()));
        let status = window.find("editor-status-message");
        let meta = window.find("editor-status-meta");
        assert!(status.visible() && meta.visible());
        assert!(status.bounds().size.width > gpui_kit::px(0.));
        assert!(status.bounds().size.height <= gpui_kit::px(16.));
        assert!(status.bounds().right() <= meta.bounds().left());
    });
    assert_eq!(
        emulsion_io::project::required_version(&emulsion_io::project::read(&path).unwrap()),
        2
    );
}

#[gpui_kit::test]
fn queued_native_save_uses_its_own_snapshot_and_clears_obsolete_notice(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let first = folder.path().join("protected.emu");
    let latest = folder.path().join("live-only.emu");
    let (workspace, editor, cx) = open(cx, true);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.write(editor.clone(), first.clone(), cx);
            editor.update(cx, |editor, _| assert!(editor.editor.undo()));
            workspace.write(editor.clone(), latest.clone(), cx);
            assert_eq!(
                editor.read(cx).history.save_queued.as_ref().unwrap().path(),
                latest.as_path()
            );
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        let (message, error) = editor.status.as_ref().unwrap();
        assert_eq!(message.as_ref(), t!("shell.saved", path = latest.display()));
        assert!(!error);
        assert!(!editor.editor.is_modified());
        assert!(!editor.history.save_busy);
        assert!(editor.history.save_queued.is_none());
        assert_eq!(editor.editor.path.as_ref(), Some(&latest));
    });
    for (path, version) in [(first, 2), (latest, 1)] {
        assert_eq!(
            emulsion_io::project::required_version(&emulsion_io::project::read(&path).unwrap()),
            version
        );
    }
}

#[gpui_kit::test]
fn failed_native_write_never_claims_saved_format_or_marks_clean(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let blocked = folder.path().join("blocked");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let path = blocked.join("protected.emu");
    let (workspace, editor, cx) = open(cx, true);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.write(editor.clone(), path.clone(), cx)
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        let (message, error) = editor.status.as_ref().unwrap();
        assert!(*error);
        assert!(!message.contains(&project_notice(2).unwrap()));
        assert!(editor.editor.is_modified());
        assert!(editor.editor.path.is_none());
        assert!(!editor.history.save_busy);
        assert!(editor.history.save_queued.is_none());
    });
    assert!(!path.exists());
    assert_eq!(std::fs::read(&blocked).unwrap(), b"not a directory");
}
