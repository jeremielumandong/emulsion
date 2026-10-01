//! Board view workflows through the real editor: grouping, selection, drag
//! and drop across scenes, locks, split/join/renumber, thumbnail sheets and
//! the panel clipboard between two open storyboards.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::Panel;
use gpui_kit::test::TestWindowExt;

fn storyboard(panels: usize, width: u32, height: u32) -> ProjectEditor {
    let mut p =
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(width, height)).unwrap();
    if panels > 1 {
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let items = (2..=panels)
            .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
            .collect();
        p.insert_panels(Some(1), &blank, items, None).unwrap();
    }
    p
}

fn install(
    ws: &Entity<Workspace>,
    project: ProjectEditor,
    name: &str,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, name.into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    editor
}

fn setup(
    cx: &mut TestAppContext,
    project: ProjectEditor,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let editor = install(&ws, project, "Board", cx);
    (ws, editor, cx)
}

fn layout(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PageId> {
    cx.update(|_, cx| e.read(cx).editor.page_list().iter().map(|m| m.id).collect())
}

fn scene_of(e: &Entity<EditorView>, id: PageId, cx: &mut VisualTestContext) -> GroupId {
    cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().panels[&id].scene)
}

fn open_board(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.click("storyboard-view-toggle", cx));
    cx.run_until_parked();
}

fn status(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        e.read(cx)
            .status
            .as_ref()
            .map(|(s, _)| s.to_string())
            .unwrap_or_default()
    })
}

#[gpui_kit::test]
fn board_groups_panels_and_selects_like_a_file_browser(cx: &mut TestAppContext) {
    let mut project = storyboard(4, 64, 36);
    let ids: Vec<_> = project.page_list().iter().map(|m| m.id).collect();
    project
        .edit_storyboard(|b| {
            b.split(&ids, ids[2], Level::Act, Some("Act Two"))
                .map(|_| ())
        })
        .unwrap();
    let (_ws, e, cx) = setup(cx, project);
    open_board(cx);
    let (first_scene, second_scene) = (scene_of(&e, ids[0], cx), scene_of(&e, ids[2], cx));
    let second_act = cx.update(|_, cx| {
        let board = e.read(cx).editor.storyboard().unwrap();
        board.sequences[&board.scenes[&second_scene].sequence].act
    });
    cx.update(|window, cx| {
        assert!(e.read(cx).board_open());
        assert!(window.find("storyboard-board").visible());
        assert!(window.find(("board-scene", first_scene)).visible());
        assert!(window.find(("board-scene", second_scene)).visible());
        // Two acts, so act headers show.
        assert!(window.find(("board-group", second_act)).visible());
        for id in &ids {
            assert!(window.find(("board-panel", *id)).visible());
        }
        window.click(("board-panel", ids[1]), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(e.read(cx).editor.active_page(), ids[1]);
        assert_eq!(e.read(cx).board_selection(), vec![ids[1]]);
        e.update(cx, |e, cx| {
            let shift = Modifiers {
                shift: true,
                ..Default::default()
            };
            e.board_click(ids[3], shift, 1, window, cx);
            assert_eq!(e.board_selection(), ids[1..].to_vec());
            let toggle = Modifiers::secondary_key();
            e.board_click(ids[2], toggle, 1, window, cx);
            assert_eq!(e.board_selection(), vec![ids[1], ids[3]]);
        });
    });
    cx.update(|window, cx| window.double_click(("board-panel", ids[2]), cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(!e.read(cx).board_open());
        assert_eq!(e.read(cx).editor.active_page(), ids[2]);
    });
}

#[gpui_kit::test]
fn dragging_panels_across_a_scene_boundary_joins_that_scene(cx: &mut TestAppContext) {
    let mut project = storyboard(4, 64, 36);
    let ids: Vec<_> = project.page_list().iter().map(|m| m.id).collect();
    project
        .edit_storyboard(|b| b.split(&ids, ids[2], Level::Scene, None).map(|_| ()))
        .unwrap();
    let (_ws, e, cx) = setup(cx, project);
    let (a, b) = (scene_of(&e, ids[0], cx), scene_of(&e, ids[2], cx));
    open_board(cx);
    cx.update(|window, cx| window.drag_to(("board-panel", ids[0]), ("board-panel", ids[3]), cx));
    cx.run_until_parked();
    assert_eq!(layout(&e, cx), vec![ids[1], ids[2], ids[0], ids[3]]);
    assert_eq!(scene_of(&e, ids[0], cx), b);
    assert_eq!(scene_of(&e, ids[1], cx), a);
    // Dropping on a scene appends to it, and is one Undo step.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.board_drop(vec![ids[3]], DropAt::SceneEnd(a), cx)
        })
    });
    assert_eq!(layout(&e, cx), vec![ids[1], ids[3], ids[2], ids[0]]);
    assert_eq!(scene_of(&e, ids[3], cx), a);
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(layout(&e, cx), vec![ids[1], ids[2], ids[0], ids[3]]);
    assert_eq!(scene_of(&e, ids[3], cx), b);
}

#[gpui_kit::test]
fn locked_panels_refuse_drawing_and_delete_until_unlocked(cx: &mut TestAppContext) {
    let project = storyboard(2, 64, 36);
    let ids: Vec<_> = project.page_list().iter().map(|m| m.id).collect();
    let (_ws, e, cx) = setup(cx, project);
    open_board(cx);
    cx.update(|window, cx| window.click(("board-panel", ids[0]), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("board-lock", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(e.read(cx).editor.storyboard().unwrap().panels[&ids[0]].locked);
        window.click("board-delete", cx);
    });
    cx.run_until_parked();
    assert_eq!(layout(&e, cx), ids);
    assert!(status(&e, cx).contains("locked"));
    // Back on the Stage: a banner explains, and drawing is refused.
    open_board(cx);
    let before = cx.update(|_, cx| {
        e.update(cx, |e, cx| e.set_tool(Tool::Brush, cx));
        e.read(cx).editor.doc.clone()
    });
    cx.update(|window, cx| {
        assert!(window.find("storyboard-lock-banner").visible());
        let canvas = window.find("editor-canvas-column").bounds();
        let center = canvas.center();
        window.drag(center, center + point(px(40.), px(20.)), cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(e.read(cx).editor.doc, before);
        assert!(e.read(cx).editor.is_read_only());
    });
    assert!(status(&e, cx).contains("drawing and edits are off"));
    cx.update(|window, cx| window.click("storyboard-stage-unlock", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!e.read(cx).active_panel_locked());
        assert!(window.try_find("storyboard-lock-banner").is_none());
    });
    // Unlocked, the Board deletes it with the keyboard.
    open_board(cx);
    cx.update(|window, cx| {
        window.click(("board-panel", ids[0]), cx);
        window.press("delete", cx);
    });
    cx.run_until_parked();
    assert_eq!(layout(&e, cx), vec![ids[1]]);
}

#[gpui_kit::test]
fn split_join_and_renumber_from_the_board(cx: &mut TestAppContext) {
    let project = storyboard(3, 64, 36);
    let ids: Vec<_> = project.page_list().iter().map(|m| m.id).collect();
    let (_ws, e, cx) = setup(cx, project);
    open_board(cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.board_split(ids[1], Level::Scene, cx)));
    let (a, b) = (scene_of(&e, ids[0], cx), scene_of(&e, ids[1], cx));
    assert_ne!(a, b);
    assert_eq!(scene_of(&e, ids[2], cx), b);
    cx.update(|_, cx| {
        assert_eq!(
            e.read(cx).editor.storyboard().unwrap().scenes[&b].name,
            "1A"
        )
    });
    cx.update(|window, cx| window.click("board-renumber", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = &e.read(cx).editor;
        let names: Vec<_> = editor.page_list().iter().map(|m| m.name.clone()).collect();
        assert_eq!(names, ["Panel 1", "Panel 1", "Panel 2"]);
        assert_eq!(editor.storyboard().unwrap().scenes[&b].name, "2");
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.board_join(b, cx)));
    assert_eq!(scene_of(&e, ids[2], cx), a);
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(scene_of(&e, ids[2], cx), b);
}

#[gpui_kit::test]
fn thumbnail_sheets_show_frames_on_the_stage_and_convert_to_panels(cx: &mut TestAppContext) {
    let project = storyboard(1, 640, 360);
    let sheet = project.active_page();
    let (_ws, e, cx) = setup(cx, project);
    open_board(cx);
    cx.update(|window, cx| {
        e.update(cx, |e, cx| e.board_sheet_dialog(sheet, window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        let grid = e.editor.storyboard().unwrap().panels[&sheet].thumbnails;
        assert_eq!(grid.map(|g| (g.columns, g.rows)), Some((3, 3)));
        assert_eq!(e.thumbnail_sheet_frames().len(), 9);
        // Sheets have no screen time.
        assert_eq!(e.editor.storyboard().unwrap().total_frames(), 0);
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.board_convert_sheet(sheet, cx)));
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.page_list().len(), 9);
        assert!(e.thumbnail_sheet_frames().is_empty());
        assert_eq!(e.board_selection().len(), 9);
        assert!(
            e.editor
                .storyboard()
                .unwrap()
                .panels
                .values()
                .all(|p| p.thumbnails.is_none())
        );
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(layout(&e, cx), vec![sheet]);
}

#[gpui_kit::test]
fn copied_scenes_paste_into_another_open_storyboard(cx: &mut TestAppContext) {
    let mut source = storyboard(3, 64, 36);
    let ids: Vec<_> = source.page_list().iter().map(|m| m.id).collect();
    source
        .edit_storyboard(|b| {
            b.split(&ids, ids[1], Level::Scene, Some("Chase"))
                .map(|_| ())
        })
        .unwrap();
    let (ws, a, cx) = setup(cx, source);
    open_board(cx);
    cx.update(|window, cx| {
        window.click(("board-panel", ids[1]), cx);
        a.update(cx, |e, cx| {
            let shift = Modifiers {
                shift: true,
                ..Default::default()
            };
            e.board_click(ids[2], shift, 1, window, cx)
        });
        window.press("ctrl-c", cx);
    });
    cx.run_until_parked();
    assert_eq!(status(&a, cx), "Copied 1 scene.");
    let b = install(&ws, storyboard(1, 128, 72), "Second", cx);
    assert_ne!(a, b);
    open_board(cx);
    let only = layout(&b, cx)[0];
    cx.update(|window, cx| {
        window.click(("board-panel", only), cx);
        window.press("ctrl-v", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = &b.read(cx).editor;
        let board = editor.storyboard().unwrap();
        assert_eq!(editor.page_list().len(), 3);
        let outline = board.outline(&editor.page_list().iter().map(|m| m.id).collect::<Vec<_>>());
        assert_eq!(outline.len(), 2);
        assert_eq!(board.scenes[&outline[1].scene].name, "Chase");
        // Other resolutions are fitted to this storyboard's camera.
        let pasted = editor.page(outline[1].panels[0]).unwrap();
        assert_eq!((pasted.doc.width, pasted.doc.height), (128, 72));
    });
    // On the Stage the same shortcuts stay with layers: no panels appear.
    open_board(cx);
    cx.update(|window, cx| {
        let canvas_focus = b.read(cx).canvas_focus.clone();
        window.focus(&canvas_focus, cx);
        window.press("ctrl-v", cx);
    });
    cx.run_until_parked();
    assert_eq!(layout(&b, cx).len(), 3);
    // Undo removes the whole paste in one step.
    cx.update(|_, cx| b.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(layout(&b, cx), vec![only]);
}
