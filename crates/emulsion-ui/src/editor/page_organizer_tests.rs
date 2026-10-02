//! Real organizer interactions, including delayed native save/export choosers.
use super::*;
use crate::workspace::Workspace;
use ::core::prelude::v1::test;
use emulsion_core::{
    Node,
    command::Slot,
    project::{Project, ProjectEditor, ProjectKind},
};
use gpui_kit::InputEvent as _;
use gpui_kit::test::TestWindowExt;

fn page_document(index: u32) -> Document {
    let mut doc = Document::new(32 + index % 8 * 4, 24 + index % 8 * 2);
    Command::AddNode {
        node: Box::new(Node::path(
            0,
            format!("Artwork {index}"),
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                2., 3., 15., 12.,
            )),
            emulsion_raster::vector::PathStyle {
                fill: Some([30 + (index * 20 % 200) as u8, 80, 160, 255]),
                stroke: None,
                ..Default::default()
            },
            doc.width,
            doc.height,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    doc
}

fn open_organizer(
    cx: &mut TestAppContext,
    count: u32,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let mut session = ProjectEditor::new_project(ProjectKind::Design, page_document(1)).unwrap();
    for index in 2..=count {
        session
            .add_page(
                page_document(index),
                format!("Page {index}"),
                f64::from(index % 10),
            )
            .unwrap();
    }
    session.set_active_page(1).unwrap();
    // A newly opened fixture has no setup edits in chronological undo history.
    let session = ProjectEditor::open(session.snapshot().unwrap(), None).unwrap();
    let (workspace, cx) = crate::tests::open(cx, page_document(1));
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let view = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(session, "Organizer".into(), window, cx)
        });
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("project-page-organizer", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("page-organizer").visible());
        assert_eq!(view.read(cx).selected_project_pages(), vec![1]);
        assert!(!view.read(cx).editor.can_undo());
    });
    (workspace, view, cx)
}

fn layout(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PageId> {
    cx.update(|_, cx| {
        view.read(cx)
            .editor
            .page_list()
            .iter()
            .map(|p| p.id)
            .collect()
    })
}

fn click_page(cx: &mut VisualTestContext, id: PageId, modifiers: Modifiers) {
    let center = cx.update(|window, _| window.find(("organizer-page", id)).bounds().center());
    cx.simulate_click(center, modifiers);
    cx.run_until_parked();
}

fn assert_selection(view: &Entity<EditorView>, cx: &mut VisualTestContext, expected: &[PageId]) {
    cx.update(|_, cx| assert_eq!(view.read(cx).selected_project_pages(), expected));
}

fn assert_project_matches(actual: &Project, expected: &Project) {
    assert_eq!(actual.kind, expected.kind);
    assert_eq!(actual.active, expected.active);
    assert_eq!(actual.pages.len(), expected.pages.len());
    for (actual, expected) in actual.pages.iter().zip(&expected.pages) {
        assert_eq!(actual.meta, expected.meta);
        assert_eq!(actual.doc, expected.doc);
        assert_eq!(
            actual.graph.commits().count(),
            expected.graph.commits().count()
        );
    }
}

#[gpui_kit::test]
fn organizer_grid_plain_toggle_checkbox_and_shift_selection_do_not_edit(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 6);
    let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());
    click_page(cx, 2, Modifiers::none());
    assert_selection(&view, cx, &[2]);
    click_page(
        cx,
        4,
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    assert_selection(&view, cx, &[2, 4]);
    cx.update(|window, cx| window.click(("organizer-select", 2u64), cx));
    cx.run_until_parked();
    assert_selection(&view, cx, &[4]);
    cx.update(|window, cx| window.click(("organizer-select", 2u64), cx));
    cx.run_until_parked();
    assert_selection(&view, cx, &[2, 4]);
    click_page(
        cx,
        5,
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
    );
    assert_selection(&view, cx, &[2, 3, 4, 5]);
    click_page(
        cx,
        6,
        Modifiers {
            control: true,
            shift: true,
            ..Modifiers::none()
        },
    );
    assert_selection(&view, cx, &[2, 3, 4, 5, 6]);
    click_page(cx, 3, Modifiers::none());
    assert_selection(&view, cx, &[3]);
    cx.update(|window, cx| {
        assert_eq!(
            window.find(("organizer-select", 3u64)).label(),
            Some("Deselect page 3")
        );
        assert_eq!(
            window.find(("organizer-page", 3u64)).label(),
            Some("Page 3: Page 3, selected")
        );
        let view = view.read(cx);
        assert_eq!(
            view.editor.active_page(),
            1,
            "selection is independent from the editing page"
        );
        assert_eq!(view.editor.stamp(), stamp);
        assert!(!view.editor.can_undo());
    });
}

#[gpui_kit::test]
fn organizer_batch_duplicate_and_delete_each_take_one_undo(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 5);
    let original = cx.update(|_, cx| view.read(cx).editor.snapshot().unwrap());
    click_page(cx, 2, Modifiers::none());
    click_page(
        cx,
        4,
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    cx.update(|window, cx| window.click("organizer-duplicate", cx));
    cx.run_until_parked();
    assert_eq!(layout(&view, cx), vec![1, 2, 3, 4, 6, 7, 5]);
    assert_selection(&view, cx, &[6, 7]);
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert_eq!(v.editor.page(6).unwrap().doc, original.pages[1].doc);
        assert_eq!(v.editor.page(7).unwrap().doc, original.pages[3].doc);
    });
    cx.update(|window, cx| window.click("organizer-undo", cx));
    cx.run_until_parked();
    assert_eq!(layout(&view, cx), vec![1, 2, 3, 4, 5]);
    cx.update(|_, cx| assert!(!view.read(cx).editor.can_undo()));
    cx.update(|window, cx| window.click("organizer-redo", cx));
    cx.run_until_parked();
    assert_eq!(layout(&view, cx), vec![1, 2, 3, 4, 6, 7, 5]);
    click_page(cx, 6, Modifiers::none());
    click_page(
        cx,
        7,
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    let duplicated = cx.update(|_, cx| view.read(cx).editor.snapshot().unwrap());
    cx.update(|window, cx| window.click("organizer-delete", cx));
    cx.run_until_parked();
    assert_eq!(layout(&view, cx), vec![1, 2, 3, 4, 5]);
    cx.simulate_keystrokes("ctrl-z");
    cx.update(|_, cx| {
        assert_project_matches(&view.read(cx).editor.snapshot().unwrap(), &duplicated)
    });
    cx.simulate_keystrokes("ctrl-shift-z");
    assert_eq!(layout(&view, cx), vec![1, 2, 3, 4, 5]);
    cx.simulate_keystrokes("ctrl-z ctrl-z");
    cx.update(|_, cx| {
        assert_project_matches(&view.read(cx).editor.snapshot().unwrap(), &original);
        assert!(!view.read(cx).editor.can_undo());
    });
}

#[gpui_kit::test]
fn organizer_all_pages_delete_is_disabled_and_delete_keys_cannot_bypass_it(
    cx: &mut TestAppContext,
) {
    let (_, view, cx) = open_organizer(cx, 3);
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    cx.simulate_keystrokes("ctrl-a");
    assert_selection(&view, cx, &[1, 2, 3]);
    cx.update(|window, cx| {
        // Base Button does not expose a native disabled accessibility flag.
        // Verify that pointer activation is inert, including no error status
        // from accidentally calling the guarded delete operation.
        let status = view.read(cx).status.clone();
        window.click("organizer-delete", cx);
        assert_eq!(view.read(cx).status, status);
        assert_eq!(view.read(cx).editor.stamp(), original);
    });
    cx.simulate_keystrokes("delete backspace");
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert_eq!(v.editor.stamp(), original);
        assert_eq!(v.editor.page_list().len(), 3);
        assert!(!v.editor.can_undo());
    });
    cx.update(|window, cx| window.click("organizer-select-none", cx));
    cx.run_until_parked();
    for id in [
        "organizer-delete",
        "organizer-duplicate",
        "organizer-earlier",
        "organizer-later",
    ] {
        cx.update(|window, cx| {
            let status = view.read(cx).status.clone();
            window.click(id, cx);
            assert_eq!(view.read(cx).status, status, "{id} must not activate");
            assert_eq!(view.read(cx).editor.stamp(), original);
        });
    }
    cx.simulate_keystrokes("delete ctrl-d");
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.stamp(), original));
}

#[gpui_kit::test]
fn organizer_keyboard_navigation_and_reorder_preserve_active_page(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 6);
    click_page(cx, 2, Modifiers::none());
    cx.simulate_keystrokes("shift-right");
    assert_selection(&view, cx, &[2, 3]);
    cx.simulate_keystrokes("alt-right");
    assert_eq!(layout(&view, cx), vec![1, 4, 2, 3, 5, 6]);
    assert_selection(&view, cx, &[2, 3]);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.active_page(), 1));
    cx.simulate_keystrokes("alt-left");
    assert_eq!(layout(&view, cx), vec![1, 2, 3, 4, 5, 6]);
    cx.simulate_keystrokes("space");
    assert_selection(&view, cx, &[2]);
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| {
        assert!(window.try_find("page-organizer").is_none());
        assert_eq!(view.read(cx).editor.active_page(), 3);
        assert!(view.read(cx).canvas_focus.is_focused(window));
    });
}

#[gpui_kit::test]
fn organizer_drag_reorders_selection_atomically_and_rejects_stale_or_foreign_payload(
    cx: &mut TestAppContext,
) {
    let (_, view, cx) = open_organizer(cx, 5);
    click_page(cx, 2, Modifiers::none());
    click_page(
        cx,
        4,
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    let stale = cx.update(|_, cx| DraggedPages {
        owner: view.entity_id().as_u64(),
        stamp: view.read(cx).editor.stamp(),
        ids: vec![2, 4],
    });
    cx.update(|window, cx| window.drag_to(("organizer-page", 2u64), ("organizer-page", 1u64), cx));
    cx.run_until_parked();
    assert_eq!(layout(&view, cx), vec![2, 4, 1, 3, 5]);
    assert_selection(&view, cx, &[2, 4]);
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            assert_eq!(v.editor.active_page(), 1);
            let moved = v.editor.stamp();
            v.drop_organizer_pages(&stale, 5, cx);
            assert_eq!(
                v.editor.stamp(),
                moved,
                "a source stamp from an older layout is rejected"
            );
            let foreign = DraggedPages {
                owner: stale.owner.wrapping_add(1),
                stamp: moved.clone(),
                ids: vec![2, 4],
            };
            v.drop_organizer_pages(&foreign, 5, cx);
            assert_eq!(
                v.editor.stamp(),
                moved,
                "another editor cannot reorder this project"
            );
        });
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(layout(&view, cx), vec![1, 2, 3, 4, 5]);
    cx.update(|_, cx| assert!(!view.read(cx).editor.can_undo()));
    cx.simulate_keystrokes("ctrl-y");
    assert_eq!(layout(&view, cx), vec![2, 4, 1, 3, 5]);
    cx.update(|window, cx| window.drag_to(("organizer-page", 2u64), "organizer-drop-end", cx));
    cx.run_until_parked();
    assert_eq!(layout(&view, cx), vec![1, 3, 5, 2, 4]);
}

#[gpui_kit::test]
fn organizer_escape_cancels_drag_before_closing_and_reopen_does_not_mutate(
    cx: &mut TestAppContext,
) {
    let (_, view, cx) = open_organizer(cx, 4);
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    let (from, to) = cx.update(|window, _| {
        (
            window.find(("organizer-page", 2u64)).bounds().center(),
            window.find(("organizer-page", 4u64)).bounds().center(),
        )
    });
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + point(px(20.), px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.update(|_, cx| assert!(cx.has_active_drag()));
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| {
        assert!(!cx.has_active_drag());
        assert!(window.find("page-organizer").visible());
    });
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| {
        assert!(window.try_find("page-organizer").is_none());
        assert_eq!(view.read(cx).editor.stamp(), original);
        assert!(view.read(cx).canvas_focus.is_focused(window));
        window.click("project-page-organizer", cx);
    });
    cx.run_until_parked();
    assert_selection(&view, cx, &[1]);
    click_page(cx, 3, Modifiers::none());
    cx.update(|window, cx| window.click("organizer-close", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("project-page-organizer", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.stamp(), original);
        assert!(!view.read(cx).editor.can_undo());
    });
    assert_selection(&view, cx, &[1]);
}

#[gpui_kit::test]
fn organizer_empty_export_never_opens_chooser_and_cancel_writes_nothing(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let (_, view, cx) = open_organizer(cx, 3);
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    cx.update(|window, cx| window.click("organizer-select-none", cx));
    cx.run_until_parked();
    for (index, format) in [Format::Png, Format::Pdf].into_iter().enumerate() {
        cx.update(|window, cx| {
            let status = view.read(cx).status.clone();
            window.click(("organizer-export", index), cx);
            assert_eq!(
                view.read(cx).status,
                status,
                "empty export button must not activate"
            );
            view.update(cx, |v, cx| {
                v.export_project_selection(format, Vec::new(), cx)
            });
        });
        cx.run_until_parked();
        assert!(!cx.did_prompt_for_new_path());
    }
    click_page(cx, 2, Modifiers::none());
    // Existing project path makes the chooser's default directory observable.
    cx.update(|_, cx| {
        view.update(cx, |v, _| {
            v.editor.path = Some(folder.path().join("source.emu"))
        })
    });
    for (index, format) in [Format::Png, Format::Pdf].into_iter().enumerate() {
        cx.update(|window, cx| window.click(("organizer-export", index), cx));
        cx.run_until_parked();
        assert!(cx.did_prompt_for_new_path());
        // Repeated clicks while a chooser is pending cannot enqueue another
        // destination prompt or accidentally produce a second export.
        cx.update(|window, cx| window.click(("organizer-export", index), cx));
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert!(v.pages_ui.export_pending);
                v.export_project_selection(format, vec![2], cx);
            });
        });
        cx.run_until_parked();
        cx.simulate_new_path_selection(|directory| {
            assert_eq!(directory, folder.path());
            None
        });
        cx.run_until_parked();
        assert!(!cx.did_prompt_for_new_path());
        cx.update(|_, cx| assert!(!view.read(cx).pages_ui.export_pending));
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    }
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.stamp(), original));
}

fn export_selection_snapshot_survives_chooser(
    cx: &mut TestAppContext,
    format: Format,
    index: usize,
) {
    let folder = tempfile::tempdir().unwrap();
    let extension = if format == Format::Png { "zip" } else { "pdf" };
    let expected_path = folder.path().join(format!("expected.{extension}"));
    let actual_path = folder.path().join(format!("actual.{extension}"));
    let (_, view, cx) = open_organizer(cx, 4);
    click_page(cx, 2, Modifiers::none());
    click_page(
        cx,
        4,
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    let source = cx.update(|_, cx| view.read(cx).editor.snapshot().unwrap());
    emulsion_io::project_export::write(&source, &[2, 4], format, false, &expected_path).unwrap();
    cx.update(|window, cx| window.click(("organizer-export", index), cx));
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    assert!(!actual_path.exists());
    // Selection, ordering, content, and even page existence change while the
    // native chooser is pending. Export must retain its click-time snapshot.
    click_page(cx, 1, Modifiers::none());
    cx.update(|window, cx| window.click("organizer-bleed", cx));
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.editor.move_pages(&[4], 0).unwrap();
            v.editor.remove_pages(&[2]).unwrap();
            v.select_page(4, cx);
            let node = v.editor.doc.nodes[0].id;
            v.execute(
                Command::SetOpacity {
                    id: node,
                    opacity: 0.2,
                },
                cx,
            );
            v.after_change(cx);
        })
    });
    cx.run_until_parked();
    let changed = cx.update(|_, cx| view.read(cx).editor.stamp());
    assert_eq!(layout(&view, cx), vec![4, 1, 3]);
    cx.simulate_new_path_selection(|_| Some(actual_path.clone()));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(&actual_path).unwrap(),
        std::fs::read(&expected_path).unwrap()
    );
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.stamp(),
            changed,
            "export never rolls the live editor back"
        );
        assert!(
            view.read(cx)
                .status
                .as_ref()
                .unwrap()
                .0
                .contains("Exported 2 page(s)")
        );
    });
}

#[gpui_kit::test]
fn organizer_png_selection_snapshot_survives_pending_chooser(cx: &mut TestAppContext) {
    export_selection_snapshot_survives_chooser(cx, Format::Png, 0);
}

#[gpui_kit::test]
fn organizer_pdf_selection_snapshot_survives_pending_chooser(cx: &mut TestAppContext) {
    export_selection_snapshot_survives_chooser(cx, Format::Pdf, 1);
}

#[gpui_kit::test]
fn organizer_layout_and_editable_artwork_survive_native_save_and_reopen(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("organized.emu");
    let reopen_path = folder.path().join("reopened.emu");
    let (workspace, view, cx) = open_organizer(cx, 4);
    // Saving also remembers the project in Home. Keep that catalog local to
    // this fixture so later Home tests do not inherit the saved project.
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, _| {
            workspace.home_state.projects.catalog_root = Some(folder.path().join("catalog"));
        });
    });
    click_page(cx, 2, Modifiers::none());
    click_page(
        cx,
        4,
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    cx.update(|window, cx| window.click("organizer-duplicate", cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("alt-left");
    let expected = cx.update(|_, cx| view.read(cx).editor.snapshot().unwrap());
    cx.simulate_keystrokes("ctrl-shift-s");
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_project_matches(&emulsion_io::project::read(&path).unwrap(), &expected);
    cx.update(|_, cx| assert!(!view.read(cx).has_unsaved_changes()));
    // A distinct path forces a real disk open, rather than selecting the
    // already-open tab and accidentally passing without deserialization.
    std::fs::copy(&path, &reopen_path).unwrap();
    cx.update(|window, cx| {
        workspace.update(cx, |w, cx| w.open_path(reopen_path.clone(), window, cx))
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let reopened = workspace.read(cx).editor.clone().unwrap();
        assert_ne!(view.entity_id(), reopened.entity_id());
        assert_project_matches(&reopened.read(cx).editor.snapshot().unwrap(), &expected);
        assert!(!reopened.read(cx).pages_ui.organizer.open);
        assert!(!reopened.read(cx).has_unsaved_changes());
        window.click("project-page-organizer", cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("page-organizer").visible()));
    let saved_reference = path.canonicalize().unwrap();
    let reopened_reference = reopen_path.canonicalize().unwrap();
    let private_catalog =
        emulsion_io::creative_library::load(&folder.path().join("catalog")).unwrap();
    assert!(
        private_catalog
            .projects
            .iter()
            .any(|project| project.path == saved_reference)
    );
    let shared_catalog =
        emulsion_io::creative_library::load(&emulsion_io::creative_library::root()).unwrap();
    assert!(
        shared_catalog
            .projects
            .iter()
            .all(|project| project.path != saved_reference && project.path != reopened_reference)
    );
}

#[gpui_kit::test]
fn organizer_narrow_viewports_keep_labeled_actions_and_tiles_reachable(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 6);
    for width in [320., 480., 800.] {
        cx.simulate_resize(size(px(width), px(900.)));
        cx.run_until_parked();
        cx.update(|window, _| {
            for (id, label) in [
                ("organizer-select-all", "Select all"),
                ("organizer-select-none", "Clear"),
                ("organizer-close", "Back to canvas"),
                ("organizer-duplicate", "Duplicate"),
                ("organizer-delete", "Delete"),
                ("organizer-earlier", "Move earlier"),
                ("organizer-later", "Move later"),
                ("organizer-undo", "Undo"),
                ("organizer-redo", "Redo"),
            ] {
                let button = window.find(id);
                let bounds = button.bounds();
                assert!(button.visible(), "{id} is hidden at {width}px");
                assert_eq!(button.label(), Some(label));
                assert!(
                    bounds.origin.x >= px(0.) && bounds.right() <= px(width),
                    "{id} exceeds {width}px: {bounds:?}"
                );
                assert!(
                    bounds.origin.y >= px(0.) && bounds.bottom() <= px(900.),
                    "{id} exceeds viewport height"
                );
                assert!(
                    bounds.size.height >= px(24.) && bounds.size.width >= px(24.),
                    "{id} is not a useful click target"
                );
            }
            for index in 0..2usize {
                let button = window.find(("organizer-export", index));
                assert!(button.visible());
                assert!(
                    button
                        .label()
                        .is_some_and(|label| label.contains("1 selected"))
                );
                assert!(button.bounds().right() <= px(width));
            }
            let tile = window.find(("organizer-page", 1u64));
            assert!(tile.visible());
            assert!(tile.bounds().right() <= px(width));
            assert!(tile.bounds().size.width >= px(160.));
        });
    }
    cx.update(|_, cx| assert!(!view.read(cx).editor.can_undo()));
}

#[gpui_kit::test]
fn organizer_duplicate_shortcut_does_not_dispatch_canvas_deselect(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 3);
    let original = cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            let selection = Arc::new(emulsion_raster::select::rect(
                v.editor.doc.width,
                v.editor.doc.height,
                2.,
                3.,
                10.,
                8.,
            ));
            v.execute(
                Command::SetSelection {
                    selection: Some(selection),
                },
                cx,
            );
            v.editor.doc.clone()
        })
    });
    cx.simulate_keystrokes("ctrl-d");
    assert_eq!(layout(&view, cx), vec![1, 4, 2, 3]);
    assert_selection(&view, cx, &[4]);
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert_eq!(v.editor.page(1).unwrap().doc, original);
        assert_eq!(v.editor.page(4).unwrap().doc, original);
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(layout(&view, cx), vec![1, 2, 3]);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
}

fn tab_to(cx: &mut VisualTestContext, id: &'static str) {
    cx.update(|window, cx| {
        for _ in 0..100 {
            window.focus_next(cx);
            window.render_frame(cx);
            if window.find(id).focused() == Some(true) {
                return;
            }
        }
        panic!("{id} is not keyboard-reachable");
    });
}

fn activate_focused_button(cx: &mut VisualTestContext, key: &str) {
    cx.simulate_keystrokes(key);
    cx.update(|window, cx| {
        window.dispatch_event(
            KeyUpEvent {
                keystroke: Keystroke::parse(key).unwrap(),
            }
            .to_platform_input(),
            cx,
        );
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn organizer_tab_focused_buttons_activate_with_enter_and_space(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 3);
    tab_to(cx, "organizer-duplicate");
    activate_focused_button(cx, "enter");
    assert_eq!(layout(&view, cx), vec![1, 4, 2, 3]);
    assert_selection(&view, cx, &[4]);
    cx.update(|window, _| assert!(window.find("page-organizer").visible()));
    tab_to(cx, "organizer-select-none");
    activate_focused_button(cx, "space");
    assert_selection(&view, cx, &[]);
    cx.update(|window, _| assert!(window.find("page-organizer").visible()));
    tab_to(cx, "organizer-close");
    activate_focused_button(cx, "enter");
    cx.update(|window, cx| {
        assert!(window.try_find("page-organizer").is_none());
        assert!(view.read(cx).canvas_focus.is_focused(window));
    });
}

#[gpui_kit::test]
fn organizer_many_pages_finish_thumbnail_work_and_select_across_virtual_rows(
    cx: &mut TestAppContext,
) {
    // More pages than the thumbnail LRU capacity catches a strip which eagerly
    // rerenders every page forever instead of reaching a parked state.
    let (_, view, cx) = open_organizer(cx, 160);
    cx.update(|window, _| {
        assert!(window.find(("organizer-page", 1u64)).visible());
        assert!(window.try_find(("organizer-page", 160u64)).is_none());
    });
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.pages_ui
                .organizer
                .scroll
                .scroll_to_item(26, ScrollStrategy::Top);
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.find(("organizer-page", 160u64)).visible());
        assert!(window.try_find(("organizer-page", 1u64)).is_none());
    });
    click_page(
        cx,
        160,
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
    );
    assert_selection(&view, cx, &(1..=160).collect::<Vec<_>>());
    cx.update(|window, cx| window.click("organizer-close", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert!(!view.read(cx).editor.can_undo()));
}

#[gpui_kit::test]
fn organizer_thumbnail_lifecycle_rejects_prior_session_completion(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 3);
    let initial = cx.update(|_, cx| view.update(cx, |v, cx| v.page_thumbnail(1, cx).unwrap()));
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.set_visible(false, window, cx);
            assert!(v.page_thumbnail(1, cx).is_none());
            v.set_visible(true, window, cx);
            assert!(
                v.page_thumbnail(1, cx).is_none(),
                "hiding releases cached thumbnails"
            );
            // Queue the old landscape page, then replace the project before its
            // completion can install into the new portrait page with the same ID.
            let replacement =
                ProjectEditor::new_project(ProjectKind::Design, Document::new(12, 36)).unwrap();
            v.install_project_session(replacement, cx);
            v.open_page_organizer(window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            let image = v.page_thumbnail(1, cx).unwrap();
            assert!(!Arc::ptr_eq(&image, &initial));
            assert_eq!(image.size(0), size(DevicePixels(12), DevicePixels(36)));
            assert_eq!(v.editor.page_list().len(), 1);
            assert_eq!(v.selected_project_pages(), vec![1]);
        })
    });
}

#[gpui_kit::test]
fn organizer_open_dismisses_font_picker_without_editing_project(cx: &mut TestAppContext) {
    let (_, view, cx) = open_organizer(cx, 3);
    let original = cx.update(|_, cx| view.read(cx).editor.stamp());
    cx.update(|window, cx| window.click("organizer-close", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.set_tool(Tool::Type, cx);
            v.toggle_font_picker(
                Some(Bounds::new(
                    point(px(100.), px(100.)),
                    size(px(160.), px(28.)),
                )),
                window,
                cx,
            );
            assert!(v.type_tool.font_picker.is_some());
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("font-picker").visible());
        view.update(cx, |v, cx| v.open_page_organizer(window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("page-organizer").visible());
        assert!(window.try_find("font-picker").is_none());
        let v = view.read(cx);
        assert!(v.type_tool.font_picker.is_none());
        assert_eq!(v.editor.stamp(), original);
        assert!(!v.editor.can_undo());
    });
}
