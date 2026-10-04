//! Full-project shortcuts preserve the existing native chooser and writer contract.
use super::*;
use ::core::prelude::v1::test;
use emulsion_core::{
    design::template_families::{FamilyId, Selection},
    project::{Project, ProjectEditor, ProjectKind},
};
use emulsion_io::project_export::Format;
use gpui_kit::test::TestWindowExt;

fn open_project(
    cx: &mut TestAppContext,
    project: Project,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let first = project.pages[0].doc.clone();
    let (workspace, cx) = crate::tests::open(cx, first);
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let view = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(
                ProjectEditor::open(project, None).unwrap(),
                "Export fixture".into(),
                window,
                cx,
            )
        });
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (view, cx)
}

fn small_project(count: u32) -> Project {
    let mut first = Document::new(32, 24);
    first.resolution = 240.;
    let mut session = ProjectEditor::new_project(ProjectKind::Design, first).unwrap();
    session.rename_page(1, "Page 1".into(), 0.5).unwrap();
    for index in 2..=count {
        let mut doc = Document::new(32 + index, 24 + index);
        doc.resolution = 144. + index as f32;
        session.add_page(doc, format!("Page {index}"), 0.5).unwrap();
    }
    session.set_active_page(1).unwrap();
    session.snapshot().unwrap()
}

fn open_dialog(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_export_dialog(window, cx)));
    cx.run_until_parked();
}

fn assert_counts(window: &mut Window, count: usize) {
    for (id, format) in [
        ("export-all-pages-png", "PNG ZIP"),
        ("export-all-pages-pdf", "PDF"),
    ] {
        assert_eq!(
            window.find(id).label(),
            Some(
                format!(
                    "{format} · {count} page{}",
                    if count == 1 { "" } else { "s" }
                )
                .as_str()
            )
        );
    }
    assert_eq!(
        window.find("export-go").label(),
        Some("Export current page…")
    );
}

fn family_short_export_layout(cx: &mut TestAppContext, family: FamilyId, count: usize) {
    let project = Selection::for_family(family).create_sized(160, 90).unwrap();
    let mut session = ProjectEditor::open(project, None).unwrap();
    if count == 5 {
        // The three-slide starter becomes the five-slide finishing fixture.
        session.duplicate_pages(&[1, 2]).unwrap();
    }
    let project = session.snapshot().unwrap();
    assert_eq!(project.pages.len(), count);
    let (view, cx) = open_project(cx, project);
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    for viewport in [size(px(610.), px(510.)), size(px(480.), px(500.))] {
        cx.simulate_resize(viewport);
        open_dialog(&view, cx);
        cx.update(|window, cx| {
            assert_counts(window, count);
            let footer = window.find("export-go").bounds();
            for id in [
                "export-all-pages-png",
                "export-all-pages-pdf",
                "export-all-pages-bleed",
                "export-cancel",
                "export-go",
            ] {
                let control = window.find(id);
                assert!(control.visible(), "{id} hidden at {viewport:?}");
                let bounds = control.bounds();
                assert!(
                    bounds.left() >= px(0.) && bounds.right() <= viewport.width,
                    "{id}: {bounds:?}"
                );
                assert!(
                    bounds.top() >= px(0.) && bounds.bottom() <= viewport.height,
                    "{id}: {bounds:?}"
                );
                if id.starts_with("export-all-pages-") {
                    assert!(bounds.bottom() <= footer.top(), "{id} overlaps footer");
                }
            }
            window.click("export-cancel", cx);
        });
        cx.run_until_parked();
        assert!(!cx.did_prompt_for_new_path());
        cx.update(|_, cx| {
            assert!(!view.read(cx).export_prefs.open);
            assert_eq!(view.read(cx).editor.stamp(), original);
        });
    }
}

#[gpui_kit::test]
fn carousel_export_actions_fit_short_and_narrow_windows(cx: &mut TestAppContext) {
    family_short_export_layout(cx, FamilyId::FieldNotes, 3);
}

#[gpui_kit::test]
fn deck_export_actions_fit_short_and_narrow_windows(cx: &mut TestAppContext) {
    family_short_export_layout(cx, FamilyId::StudioBrief, 5);
}

#[gpui_kit::test]
fn project_export_counts_refresh_after_page_changes_and_undo(cx: &mut TestAppContext) {
    let (view, cx) = open_project(cx, small_project(1));
    open_dialog(&view, cx);
    cx.update(|window, _| assert_counts(window, 1));
    for count in [3, 5] {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                for index in (view.editor.page_list().len() + 1)..=count {
                    view.editor
                        .add_page(Document::new(32, 24), format!("Page {index}"), 0.)
                        .unwrap();
                }
                cx.notify();
            });
        });
        cx.run_until_parked();
        cx.update(|window, _| assert_counts(window, count));
    }
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.editor.remove_pages(&[2, 3, 4, 5]).unwrap();
            cx.notify();
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_counts(window, 1);
        view.update(cx, |view, cx| {
            view.undo(cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_counts(window, 5);
        window.click("export-cancel", cx);
    });
}

#[gpui_kit::test]
fn project_shortcuts_cancel_without_writing_and_guard_repeated_exports(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (view, cx) = open_project(cx, small_project(3));
    let original = cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.editor.path = Some(folder.path().join("source.emu"))
        });
        view.read(cx).editor.stamp()
    });
    for (id, format) in [
        ("export-all-pages-png", Format::Png),
        ("export-all-pages-pdf", Format::Pdf),
    ] {
        open_dialog(&view, cx);
        cx.update(|window, cx| window.click(id, cx));
        cx.run_until_parked();
        assert!(cx.did_prompt_for_new_path());
        cx.update(|window, cx| {
            assert!(window.try_find("export-dialog-body").is_none());
            view.update(cx, |view, cx| view.export_project_pages(format, true, cx));
        });
        // A reopened dialog cannot start a second whole-project export.
        open_dialog(&view, cx);
        cx.update(|window, cx| {
            assert!(view.read(cx).pages_ui.export_pending);
            window.click(id, cx);
            assert!(
                view.read(cx).export_prefs.open,
                "a busy shortcut must not activate or dismiss the reopened dialog"
            );
            assert!(view.read(cx).pages_ui.export_pending);
            window.click("export-cancel", cx);
        });
        cx.run_until_parked();
        cx.simulate_new_path_selection(|directory| {
            assert_eq!(directory, folder.path());
            None
        });
        cx.run_until_parked();
        assert!(!cx.did_prompt_for_new_path());
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
        cx.update(|_, cx| {
            assert!(!view.read(cx).pages_ui.export_pending);
            assert_eq!(view.read(cx).editor.stamp(), original);
        });
    }
}

fn shortcut_uses_existing_full_size_writer(cx: &mut TestAppContext, format: Format, count: u32) {
    let folder = tempfile::tempdir().unwrap();
    let project = small_project(count);
    let ids: Vec<_> = project.pages.iter().map(|page| page.meta.id).collect();
    let extension = if format == Format::Png { "zip" } else { "pdf" };
    let expected = folder.path().join(format!("expected.{extension}"));
    let chosen = folder.path().join("actual.jpg");
    let actual = chosen.with_extension(extension);
    emulsion_io::project_export::write(&project, &ids, format, true, &expected).unwrap();
    let (view, cx) = open_project(cx, project);
    let prefs = cx.update(|_, cx| {
        view.update(cx, |view, _| {
            // Deliberately conflict with the full-size page writer's contract.
            view.export_prefs.ext = "jpg";
            view.export_prefs.scale = ExportScale::Quarter;
            view.export_prefs.quality = 17;
            view.export_prefs.depth16 = false;
            view.export_prefs.color_space = ExportColorSpace::ProPhoto;
            view.export_prefs.dpi = Some(72);
            view.export_prefs
        })
    });
    open_dialog(&view, cx);
    cx.update(|window, cx| window.click("export-all-pages-bleed", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(view.read(cx).pages_ui.include_bleed);
        window.click(
            if format == Format::Png {
                "export-all-pages-png"
            } else {
                "export-all-pages-pdf"
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    assert!(!actual.exists());
    // The action captures all pages, order, saved PPI and bleed before the chooser.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.pages_ui.include_bleed = false;
            view.editor.move_pages(&[u64::from(count)], 0).unwrap();
            view.editor.remove_pages(&[2]).unwrap();
            cx.notify();
        });
    });
    let changed = cx.update(|_, cx| view.read(cx).editor.stamp());
    cx.simulate_new_path_selection(|_| Some(chosen.clone()));
    cx.run_until_parked();
    assert!(
        !chosen.exists(),
        "project file extension must match the selected action"
    );
    assert_eq!(
        std::fs::read(&actual).unwrap(),
        std::fs::read(&expected).unwrap()
    );
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(!view.export_prefs.open);
        assert!(!view.pages_ui.export_pending);
        assert_eq!(view.export_prefs, prefs);
        assert_eq!(view.editor.stamp(), changed);
        assert!(
            view.status
                .as_ref()
                .unwrap()
                .0
                .contains(&format!("Exported {count} page(s)"))
        );
    });
}

#[gpui_kit::test]
fn carousel_png_shortcut_uses_full_size_snapshot_bleed_and_zip_extension(cx: &mut TestAppContext) {
    shortcut_uses_existing_full_size_writer(cx, Format::Png, 3);
}

#[gpui_kit::test]
fn deck_pdf_shortcut_uses_full_size_snapshot_bleed_and_pdf_extension(cx: &mut TestAppContext) {
    shortcut_uses_existing_full_size_writer(cx, Format::Pdf, 5);
}

#[gpui_kit::test]
fn project_shortcut_reports_write_failure_and_allows_retry(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (view, cx) = open_project(cx, small_project(3));
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    open_dialog(&view, cx);
    cx.update(|window, cx| window.click("export-all-pages-png", cx));
    cx.run_until_parked();
    cx.simulate_new_path_selection(|_| Some(folder.path().join("missing").join("failed.zip")));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(!view.pages_ui.export_pending);
        assert!(!view.export_prefs.open);
        let (message, error) = view.status.as_ref().unwrap();
        assert!(*error && message.contains("Export failed"), "{message}");
        assert_eq!(view.editor.stamp(), original);
    });
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    open_dialog(&view, cx);
    cx.update(|window, cx| window.click("export-all-pages-pdf", cx));
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    cx.update(|_, cx| assert!(!view.read(cx).pages_ui.export_pending));
}

#[gpui_kit::test]
fn export_destination_distinguishes_cancel_portal_error_and_closed_channel(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open_project(cx, small_project(3));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let original = view.editor.stamp();
            view.set_status("Existing status", false, cx);
            assert!(view.begin_file_export(cx));
            assert!(
                view.export_destination(Ok::<_, &str>(Ok(None)), cx)
                    .is_none()
            );
            assert!(!view.pages_ui.export_pending);
            assert_eq!(view.status.as_ref().unwrap().0, "Existing status");
            for result in [
                Ok(Err(anyhow::anyhow!("File picker portal is unavailable"))),
                Err("File picker response channel closed"),
            ] {
                assert!(view.begin_file_export(cx));
                assert!(view.export_destination(result, cx).is_none());
                assert!(!view.pages_ui.export_pending);
                let (message, error) = view.status.as_ref().unwrap();
                assert!(
                    *error && message.starts_with("Export failed: File picker"),
                    "{message}"
                );
                assert_eq!(view.editor.stamp(), original);
            }
            assert!(view.begin_file_export(cx));
            let path = PathBuf::from("chosen.zip");
            assert_eq!(
                view.export_destination(Ok::<_, &str>(Ok(Some(path.clone()))), cx),
                Some(path)
            );
            assert!(
                view.pages_ui.export_pending,
                "writing still owns the busy guard"
            );
            view.finish_file_export(cx);
            assert!(!view.pages_ui.export_pending);
        });
    });
}

#[gpui_kit::test]
fn current_page_export_preserves_settings_and_releases_shared_busy_guard(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let output = folder.path().join("current.png");
    let (view, cx) = open_project(cx, small_project(3));
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    open_dialog(&view, cx);
    cx.update(|window, cx| {
        view.update(cx, |view, _| {
            view.export_prefs.ext = "png";
            view.export_prefs.scale = ExportScale::Quarter;
            view.export_prefs.depth16 = false;
            view.export_prefs.dpi = Some(72);
        });
        window.click("export-go", cx);
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    cx.update(|window, cx| {
        assert!(view.read(cx).pages_ui.export_pending);
        window.dispatch_action(Box::new(crate::actions::ConfirmExport), cx);
        view.update(cx, |view, cx| {
            view.export_project_pages(Format::Pdf, true, cx)
        });
    });
    cx.run_until_parked();
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert!(
        !cx.did_prompt_for_new_path(),
        "repeat actions must not queue another chooser"
    );
    cx.update(|_, cx| assert!(!view.read(cx).pages_ui.export_pending));
    open_dialog(&view, cx);
    cx.update(|window, cx| window.click("export-go", cx));
    cx.run_until_parked();
    cx.simulate_new_path_selection(|_| Some(output.clone()));
    cx.run_until_parked();
    let image = image::open(&output).unwrap();
    assert_eq!((image.width(), image.height()), (8, 6));
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(!view.pages_ui.export_pending);
        assert!(!view.export_prefs.open);
        assert_eq!(view.editor.stamp(), original);
        assert_eq!(view.export_prefs.scale, ExportScale::Quarter);
        assert_eq!(view.export_prefs.dpi, Some(72));
    });
}

fn assert_builtin_prompt(cx: &mut VisualTestContext) {
    assert!(
        !cx.did_prompt_for_new_path(),
        "the native chooser is skipped"
    );
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_prompt());
        assert!(window.find("file-prompt").visible());
    });
}

fn type_builtin_path(cx: &mut VisualTestContext, path: &std::path::Path) {
    // Exercise the focused path input rather than a private dialog setter.
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(&path.to_string_lossy());
    cx.run_until_parked();
}

fn start_builtin_export(view: &Entity<EditorView>, cx: &mut VisualTestContext, id: &'static str) {
    open_dialog(view, cx);
    cx.update(|window, cx| window.click(id, cx));
    cx.run_until_parked();
    assert_builtin_prompt(cx);
    cx.update(|_, cx| assert!(view.read(cx).pages_ui.export_pending));
}

fn builtin_export_cancel_retry_and_completion(
    cx: &mut TestAppContext,
    format: Option<Format>,
    count: u32,
) {
    let folder = tempfile::tempdir().unwrap();
    let project = small_project(count);
    let (view, cx) = open_project(cx, project.clone());
    let original = cx.update(|_, cx| {
        cx.set_global(crate::file_prompt::InAppFileDialog);
        view.update(cx, |view, _| {
            view.export_prefs.ext = "png";
            view.export_prefs.scale = ExportScale::Quarter;
            view.export_prefs.depth16 = false;
            view.export_prefs.dpi = Some(72);
            view.editor.stamp()
        })
    });
    let (id, extension) = match format {
        Some(Format::Png) => ("export-all-pages-png", "zip"),
        Some(Format::Pdf) => ("export-all-pages-pdf", "pdf"),
        None => ("export-go", "png"),
        _ => unreachable!(),
    };
    let output = folder.path().join(format!("exported.{extension}"));
    for cancel in ["escape", "button"] {
        start_builtin_export(&view, cx, id);
        cx.update(|window, cx| {
            // Both export paths share the busy guard while the fallback waits.
            window.dispatch_action(Box::new(crate::actions::ConfirmExport), cx);
            view.update(cx, |view, cx| {
                view.export_project_pages(Format::Pdf, true, cx)
            });
        });
        cx.run_until_parked();
        assert_builtin_prompt(cx);
        type_builtin_path(cx, &output);
        if cancel == "escape" {
            cx.simulate_keystrokes("escape");
        } else {
            cx.update(|window, cx| window.click("file-prompt-cancel", cx));
        }
        cx.run_until_parked();
        assert!(!output.exists());
        assert!(!cx.did_prompt_for_new_path());
        cx.update(|window, cx| {
            assert!(
                !window.has_active_prompt(),
                "one cancel closes the only chooser"
            );
            assert!(window.try_find("file-prompt").is_none());
            assert!(!view.read(cx).pages_ui.export_pending);
            assert_eq!(view.read(cx).editor.stamp(), original);
        });
    }

    start_builtin_export(&view, cx, id);
    type_builtin_path(
        cx,
        &folder
            .path()
            .join("missing")
            .join(format!("bad.{extension}")),
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_builtin_prompt(cx);
    cx.update(|window, cx| {
        assert!(window.find("file-prompt-error").visible());
        assert!(view.read(cx).pages_ui.export_pending);
    });
    assert!(!output.exists());

    // Enter validates without losing input focus; a corrected destination can
    // finish via the actual Save button without reopening the export chooser.
    type_builtin_path(cx, &output);
    cx.update(|window, cx| window.click("file-prompt-ok", cx));
    cx.run_until_parked();
    assert!(output.is_file());
    assert!(!cx.did_prompt_for_new_path());
    if let Some(format) = format {
        let expected = folder.path().join(format!("expected.{extension}"));
        let ids: Vec<_> = project.pages.iter().map(|page| page.meta.id).collect();
        emulsion_io::project_export::write(&project, &ids, format, false, &expected).unwrap();
        assert_eq!(
            std::fs::read(&output).unwrap(),
            std::fs::read(expected).unwrap()
        );
    } else {
        let image = image::open(&output).unwrap();
        assert_eq!((image.width(), image.height()), (8, 6));
    }
    cx.update(|window, cx| {
        assert!(!window.has_active_prompt());
        assert!(window.try_find("file-prompt").is_none());
        let view = view.read(cx);
        assert!(!view.export_prefs.open);
        assert!(!view.pages_ui.export_pending);
        assert_eq!(view.editor.stamp(), original);
        assert_eq!(view.export_prefs.scale, ExportScale::Quarter);
        assert_eq!(view.export_prefs.dpi, Some(72));
        assert!(!view.status.as_ref().unwrap().1);
    });
}

#[gpui_kit::test]
fn builtin_carousel_png_export_cancels_retries_and_writes_all_pages(cx: &mut TestAppContext) {
    builtin_export_cancel_retry_and_completion(cx, Some(Format::Png), 3);
}

#[gpui_kit::test]
fn builtin_deck_pdf_export_cancels_retries_and_writes_all_pages(cx: &mut TestAppContext) {
    builtin_export_cancel_retry_and_completion(cx, Some(Format::Pdf), 5);
}

#[gpui_kit::test]
fn builtin_current_page_export_cancels_retries_and_uses_current_settings(cx: &mut TestAppContext) {
    builtin_export_cancel_retry_and_completion(cx, None, 3);
}

fn normalized_export_target_requires_confirmation(
    cx: &mut TestAppContext,
    format: Option<Format>,
    count: u32,
) {
    let folder = tempfile::tempdir().unwrap();
    let (view, cx) = open_project(cx, small_project(count));
    cx.update(|_, cx| {
        cx.set_global(crate::file_prompt::InAppFileDialog);
        view.update(cx, |view, _| {
            view.export_prefs.ext = "png";
            view.export_prefs.scale = ExportScale::Quarter;
        });
    });
    let (id, extension) = match format {
        Some(Format::Png) => ("export-all-pages-png", "zip"),
        Some(Format::Pdf) => ("export-all-pages-pdf", "pdf"),
        None => ("export-go", "png"),
        _ => unreachable!(),
    };
    let target = folder.path().join(format!("output.{extension}"));
    let previous = b"Existing user artwork must survive until Replace is approved";
    std::fs::write(&target, previous).unwrap();
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());

    for (typed, cancel) in [("output", "escape"), ("output.wrong", "button")] {
        start_builtin_export(&view, cx, id);
        type_builtin_path(cx, &folder.path().join(typed));
        cx.update(|window, cx| window.click("file-prompt-ok", cx));
        cx.run_until_parked();
        assert_eq!(
            std::fs::read(&target).unwrap(),
            previous,
            "extension normalization must not bypass confirmation for {extension}"
        );
        cx.update(|window, cx| {
            assert!(window.has_active_prompt());
            assert!(window.find("message-box").visible());
            assert!(view.read(cx).pages_ui.export_pending);
            window.dispatch_action(Box::new(crate::actions::ConfirmExport), cx);
            view.update(cx, |view, cx| {
                view.export_project_pages(Format::Pdf, true, cx)
            });
        });
        cx.run_until_parked();
        if cancel == "escape" {
            cx.simulate_keystrokes("escape");
        } else {
            cx.update(|window, cx| window.click(("message-box-action", 1usize), cx));
        }
        cx.run_until_parked();
        assert_eq!(std::fs::read(&target).unwrap(), previous);
        assert!(!folder.path().join(typed).exists());
        cx.update(|window, cx| {
            assert!(!window.has_active_prompt(), "no repeated export was queued");
            assert!(!view.read(cx).pages_ui.export_pending);
            assert_eq!(view.read(cx).editor.stamp(), original);
        });
    }

    // A deliberate Replace approves the actual normalized file, once.
    start_builtin_export(&view, cx, id);
    type_builtin_path(cx, &folder.path().join("output.wrong"));
    cx.update(|window, cx| window.click("file-prompt-ok", cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read(&target).unwrap(), previous);
    cx.update(|window, cx| window.click(("message-box-action", 0usize), cx));
    cx.run_until_parked();
    assert_ne!(std::fs::read(&target).unwrap(), previous);
    assert!(!folder.path().join("output.wrong").exists());
    cx.update(|window, cx| {
        assert!(!window.has_active_prompt());
        assert!(!view.read(cx).pages_ui.export_pending);
        assert_eq!(view.read(cx).editor.stamp(), original);
    });

    // An exact extension is already confirmed by the file chooser; do not ask twice.
    std::fs::write(&target, previous).unwrap();
    start_builtin_export(&view, cx, id);
    type_builtin_path(cx, &target);
    cx.update(|window, cx| window.click("file-prompt-ok", cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read(&target).unwrap(), previous);
    cx.update(|window, cx| {
        assert!(window.find("file-prompt-replace").visible());
        window.click("file-prompt-ok", cx);
    });
    cx.run_until_parked();
    assert_ne!(std::fs::read(&target).unwrap(), previous);
    assert!(!cx.did_prompt_for_new_path());
    cx.update(|window, cx| {
        assert!(!window.has_active_prompt());
        assert!(!view.read(cx).pages_ui.export_pending);
    });
}

#[gpui_kit::test]
fn normalized_png_zip_target_requires_replace_before_overwrite(cx: &mut TestAppContext) {
    normalized_export_target_requires_confirmation(cx, Some(Format::Png), 3);
}

#[gpui_kit::test]
fn normalized_pdf_target_requires_replace_before_overwrite(cx: &mut TestAppContext) {
    normalized_export_target_requires_confirmation(cx, Some(Format::Pdf), 5);
}

#[gpui_kit::test]
fn normalized_current_png_target_requires_replace_before_overwrite(cx: &mut TestAppContext) {
    normalized_export_target_requires_confirmation(cx, None, 3);
}

#[gpui_kit::test]
fn normalized_target_without_originating_window_fails_closed_and_releases_busy(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let target = folder.path().join("protected.zip");
    let chosen = folder.path().join("protected");
    let previous = b"Keep this file when replacement cannot be confirmed";
    std::fs::write(&target, previous).unwrap();
    let (view, cx) = open_project(cx, small_project(3));
    let output = target.clone();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| assert!(view.begin_file_export(cx)));
        let view = view.clone();
        cx.spawn(async move |cx| {
            let result = confirm_normalized_write_path(chosen, output, None, cx).await;
            view.update(cx, |view, cx| {
                assert!(
                    view.export_destination(Ok::<_, std::convert::Infallible>(result), cx)
                        .is_none()
                );
            });
        })
        .detach();
    });
    cx.run_until_parked();
    assert_eq!(std::fs::read(target).unwrap(), previous);
    cx.update(|window, cx| {
        assert!(!window.has_active_prompt());
        let view = view.read(cx);
        assert!(!view.pages_ui.export_pending);
        assert!(view.status.as_ref().unwrap().1);
    });
}

#[gpui_kit::test]
fn design_save_as_confirms_normalized_emu_target_without_changing_save_queue(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let target = folder.path().join("design.emu");
    let old_project = small_project(1);
    let project = small_project(3);
    let (workspace, cx) = crate::tests::open(cx, project.pages[0].doc.clone());
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let view = cx.update(|window, cx| {
        cx.set_global(crate::file_prompt::InAppFileDialog);
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(
                ProjectEditor::open(project.clone(), None).unwrap(),
                "Design save safety".into(),
                window,
                cx,
            );
            workspace.home_state.projects.catalog_root = Some(folder.path().join("catalog"));
        });
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    for (typed, cancel) in [("design", "escape"), ("design.wrong", "button")] {
        emulsion_io::project::write(&old_project, &target).unwrap();
        let previous = std::fs::read(&target).unwrap();
        let previous_path = cx.update(|_, cx| view.read(cx).editor.path.clone());
        cx.simulate_keystrokes("ctrl-shift-s");
        cx.run_until_parked();
        assert_builtin_prompt(cx);
        type_builtin_path(cx, &folder.path().join(typed));
        cx.update(|window, cx| window.click("file-prompt-ok", cx));
        cx.run_until_parked();
        assert_eq!(std::fs::read(&target).unwrap(), previous);
        cx.update(|window, cx| {
            assert!(window.find("message-box").visible());
            let view = view.read(cx);
            assert!(!view.history.save_busy);
            assert!(view.history.save_queued.is_none());
            assert!(
                !view.pages_ui.export_pending,
                "Save never borrows the export guard"
            );
        });
        if cancel == "escape" {
            cx.simulate_keystrokes("escape");
        } else {
            cx.update(|window, cx| window.click(("message-box-action", 1usize), cx));
        }
        cx.run_until_parked();
        assert_eq!(std::fs::read(&target).unwrap(), previous);
        assert!(!folder.path().join(typed).exists());
        cx.update(|window, cx| {
            assert!(!window.has_active_prompt());
            let view = view.read(cx);
            assert_eq!(view.editor.path, previous_path);
            assert_eq!(view.editor.stamp(), original);
            assert!(!view.history.save_busy);
            assert!(view.history.save_queued.is_none());
        });

        cx.simulate_keystrokes("ctrl-shift-s");
        cx.run_until_parked();
        assert_builtin_prompt(cx);
        type_builtin_path(cx, &folder.path().join(typed));
        cx.update(|window, cx| window.click("file-prompt-ok", cx));
        cx.run_until_parked();
        assert_eq!(std::fs::read(&target).unwrap(), previous);
        cx.update(|window, cx| window.click(("message-box-action", 0usize), cx));
        cx.run_until_parked();
        let saved = emulsion_io::project::read(&target).unwrap();
        assert_eq!(saved.pages.len(), project.pages.len());
        for (saved, expected) in saved.pages.iter().zip(&project.pages) {
            assert_eq!(saved.meta, expected.meta);
            assert_eq!(saved.doc, expected.doc);
        }
        assert_ne!(std::fs::read(&target).unwrap(), previous);
        assert!(!folder.path().join(typed).exists());
        cx.update(|window, cx| {
            assert!(!window.has_active_prompt());
            let view = view.read(cx);
            assert_eq!(view.editor.path.as_ref(), Some(&target));
            assert_eq!(view.editor.stamp(), original);
            assert!(!view.history.save_busy);
            assert!(view.history.save_queued.is_none());
            assert!(!view.pages_ui.export_pending);
        });
    }
}
