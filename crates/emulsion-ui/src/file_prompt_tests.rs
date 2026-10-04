//! The in-app path dialog that stands in for a missing system file chooser.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::Document;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use gpui_kit::test::TestWindowExt;

/// The dialog most recently shown, for tests to read and type into.
pub(super) struct LastDialog(pub WeakEntity<PathDialog>);
impl Global for LastDialog {}

/// A workspace holding an unsaved storyboard, with the system chooser
/// reported missing so every path prompt falls back to [`PathDialog`].
fn storyboard(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<crate::editor::EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(size(px(1400.), px(1000.)));
    let editor = cx.update(|window, cx| {
        cx.set_global(InAppFileDialog);
        let project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (ws, editor, cx)
}

fn dialog(cx: &mut VisualTestContext) -> Entity<PathDialog> {
    cx.update(|window, cx| {
        assert!(window.has_active_prompt(), "the in-app path dialog is open");
        cx.global::<LastDialog>().0.upgrade().unwrap()
    })
}

fn value(dialog: &Entity<PathDialog>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| dialog.read(cx).text(cx))
}

fn type_path(dialog: &Entity<PathDialog>, path: &Path, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        dialog.update(cx, |d, cx| {
            d.set_text(path.to_string_lossy().into_owned(), window, cx)
        })
    });
    cx.run_until_parked();
}

fn press(key: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.press(key, cx));
    cx.run_until_parked();
}

fn closed(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, _| !window.has_active_prompt())
}

#[gpui_kit::test]
fn save_falls_back_to_the_in_app_dialog_and_writes_the_project(cx: &mut TestAppContext) {
    let (ws, editor, cx) = storyboard(cx);
    let folder = tempfile::tempdir().unwrap();
    // Saving registers recent projects; keep this fixture out of the Home catalog.
    cx.update(|_, cx| {
        ws.update(cx, |workspace, _| {
            workspace.home_state.projects.catalog_root = Some(folder.path().join("catalog"));
        });
    });
    press("ctrl-s", cx);
    assert!(
        !cx.did_prompt_for_new_path(),
        "the system chooser is skipped"
    );
    let d = dialog(cx);
    let suggested = value(&d, cx);
    assert!(suggested.ends_with("Board.emu"), "{suggested}");
    assert!(Path::new(&suggested).is_absolute(), "{suggested}");
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.find("file-prompt").visible());
    });

    // A missing folder is refused inline and the dialog stays.
    type_path(&d, &folder.path().join("nope").join("Board.emu"), cx);
    press("enter", cx);
    assert!(!closed(cx));
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.find("file-prompt-error").visible());
    });

    // A folder is not a file name.
    type_path(&d, folder.path(), cx);
    press("enter", cx);
    assert!(!closed(cx));
    assert!(cx.update(|_, cx| d.read(cx).error.is_some()));

    let path = folder.path().join("Board.emu");
    type_path(&d, &path, cx);
    press("enter", cx);
    assert!(closed(cx));
    cx.run_until_parked();
    assert!(path.is_file(), "Save wrote the .emu");
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.path.as_ref(), Some(&path)));

    // Save As onto the existing file asks before replacing it.
    press("ctrl-shift-s", cx);
    let d = dialog(cx);
    type_path(&d, &path, cx);
    std::fs::write(&path, b"old").unwrap();
    press("enter", cx);
    assert!(!closed(cx), "an existing file needs confirmation");
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.find("file-prompt-replace").visible());
    });
    press("enter", cx);
    assert!(closed(cx));
    cx.run_until_parked();
    assert_ne!(
        std::fs::read(&path).unwrap(),
        b"old",
        "the project replaced it"
    );
}

#[gpui_kit::test]
fn cancelling_the_in_app_save_dialog_does_nothing(cx: &mut TestAppContext) {
    let (_ws, editor, cx) = storyboard(cx);
    for key in ["escape", "cancel"] {
        press("ctrl-s", cx);
        let d = dialog(cx);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("Board.emu");
        type_path(&d, &path, cx);
        if key == "escape" {
            press("escape", cx);
        } else {
            cx.update(|window, cx| window.click("file-prompt-cancel", cx));
            cx.run_until_parked();
        }
        assert!(closed(cx), "{key}");
        assert!(!path.exists(), "{key}");
        cx.update(|_, cx| assert!(editor.read(cx).editor.path.is_none(), "{key}"));
    }
}

#[gpui_kit::test]
fn captions_csv_export_uses_the_in_app_dialog(cx: &mut TestAppContext) {
    let (_ws, editor, cx) = storyboard(cx);
    cx.update(|_, cx| editor.update(cx, |e, cx| e.storyboard_csv(cx)));
    cx.run_until_parked();
    let d = dialog(cx);
    assert!(value(&d, cx).ends_with("Board.csv"));
    let folder = tempfile::tempdir().unwrap();
    // The export still fixes the extension after the dialog answers.
    type_path(&d, &folder.path().join("captions"), cx);
    press("enter", cx);
    assert!(closed(cx));
    cx.run_until_parked();
    let csv = folder.path().join("captions.csv");
    assert!(
        csv.is_file(),
        "{:?}",
        std::fs::read_dir(folder.path())
            .unwrap()
            .collect::<Vec<_>>()
    );
}

#[gpui_kit::test]
fn open_falls_back_with_a_folder_listing(cx: &mut TestAppContext) {
    let (ws, _editor, cx) = storyboard(cx);
    let folder = tempfile::tempdir().unwrap();
    let image = folder.path().join("Artwork.png");
    image::RgbaImage::from_pixel(32, 24, image::Rgba([40, 80, 120, 255]))
        .save(&image)
        .unwrap();
    std::fs::create_dir(folder.path().join("sub")).unwrap();
    press("ctrl-o", cx);
    assert!(!cx.did_prompt_for_paths(), "the system chooser is skipped");
    let d = dialog(cx);

    // Missing paths are refused inline.
    type_path(&d, &folder.path().join("missing.png"), cx);
    press("enter", cx);
    assert!(!closed(cx));
    assert!(cx.update(|_, cx| d.read(cx).error.is_some()));

    // A folder opens into its listing rather than answering.
    type_path(&d, folder.path(), cx);
    press("enter", cx);
    assert!(!closed(cx));
    let listed = cx.update(|_, cx| d.read(cx).listing.clone().unwrap());
    assert_eq!(listed.0, folder.path());
    assert_eq!(
        listed.1,
        vec![(folder.path().join("sub"), true), (image.clone(), false)],
        "folders first"
    );

    // Picking the file (".." is row 0, "sub" row 1) selects it; Enter opens.
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.click(("file-prompt-entry", 2usize), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| d.read(cx).selected.clone()),
        vec![image.clone()]
    );
    press("enter", cx);
    assert!(closed(cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let ws = ws.read(cx);
        let opened = ws.editor.as_ref().unwrap().read(cx);
        assert_eq!(opened.source.as_ref(), Some(&image));
    });
}

#[gpui_kit::test]
fn folder_prompts_only_accept_folders(cx: &mut TestAppContext) {
    let (_ws, _editor, cx) = storyboard(cx);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("note.txt");
    std::fs::write(&file, "x").unwrap();
    let answer = cx.update(|_, cx| {
        cx.prompt_open_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        })
    });
    cx.run_until_parked();
    let d = dialog(cx);
    type_path(&d, folder.path(), cx);
    let listed = cx.update(|_, cx| d.read(cx).listing.clone().unwrap());
    assert!(listed.1.is_empty(), "files are hidden: {listed:?}");
    type_path(&d, &file, cx);
    press("enter", cx);
    assert!(!closed(cx), "a file is refused");
    type_path(&d, folder.path(), cx);
    press("enter", cx);
    assert!(closed(cx));
    let mut answer = answer;
    assert_eq!(
        answer.try_recv().unwrap().unwrap().unwrap(),
        Some(vec![folder.path().to_path_buf()])
    );
}

/// Every path prompt must go through [`FilePrompts`], or it silently does
/// nothing where the system file chooser is missing.
#[test]
fn all_path_prompts_use_the_fallback_wrapper() {
    fn scan(dir: &Path, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, hits);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|n| n != "file_prompt.rs")
            {
                let text = std::fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    // Spelled in pieces so this check does not match itself.
                    let needles = [
                        concat!(".prompt_for", "_new_path("),
                        concat!(".prompt_for", "_paths("),
                    ];
                    if needles.iter().any(|needle| line.contains(needle)) {
                        hits.push(format!("{}:{}", path.display(), n + 1));
                    }
                }
            }
        }
    }
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut hits = Vec::new();
    for krate in std::fs::read_dir(crates).unwrap().flatten() {
        let src = krate.path().join("src");
        if src.is_dir() {
            scan(&src, &mut hits);
        }
    }
    assert!(
        hits.is_empty(),
        "use crate::file_prompt::FilePrompts (prompt_save_path / prompt_open_paths) instead of \
         GPUI's prompts, which fail silently without xdg-desktop-portal: {hits:#?}"
    );
}
