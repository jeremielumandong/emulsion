use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use ::core::prelude::v1::test;
use emulsion_core::creation::{CanvasKind, CanvasSpec};
use gpui_kit::test::TestWindowExt;

/// A two-panel storyboard in its workspace with a "Hero" layer selected on
/// panel 1 and the Library panel showing.
fn storyboard(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(size(px(1600.), px(2400.)));
    let project = CanvasSpec {
        name: "Board".into(),
        kind: CanvasKind::Storyboard,
        width: 64.,
        height: 36.,
        pages: 2,
        ..Default::default()
    }
    .create_project()
    .unwrap();
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.select_page(1, cx);
            let hero = v
                .editor
                .execute(Command::AddNode {
                    node: Box::new(Node::new(
                        0,
                        "Hero",
                        NodeKind::Fill {
                            rgba: [200, 40, 40, 255],
                        },
                    )),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap();
            v.after_change(cx);
            v.set_layer_selection(vec![hero], Some(hero));
        })
    });
    settle(cx);
    cx.update(|window, cx| window.click("sidebar-storyboard-library", cx));
    settle(cx);
    cx.update(|window, _| assert!(window.find("storyboard-library").visible()));
    (ws, view, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.run_until_parked();
}

fn names(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        view.read(cx)
            .editor
            .doc
            .nodes
            .iter()
            .map(|n| n.name.clone())
            .collect()
    })
}

fn library(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u64, String)> {
    cx.update(|_, cx| {
        view.read(cx)
            .editor
            .storyboard()
            .unwrap()
            .library
            .items
            .iter()
            .map(|i| (i.id, i.name.clone()))
            .collect()
    })
}

fn undo(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| view.update(cx, |v, cx| v.undo(cx)));
    settle(cx);
}

#[gpui_kit::test]
fn project_library_items_add_place_and_delete_as_single_undo_steps(cx: &mut TestAppContext) {
    let (_ws, view, cx) = storyboard(cx);
    let added = cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.library_add(
                Scope::Project,
                ItemKind::Layers,
                "Hero",
                &["character".into()],
                cx,
            )
        })
    });
    assert!(added);
    let added = cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.library_add(Scope::Project, ItemKind::Panel, "Opening", &[], cx)
        })
    });
    assert!(added);
    settle(cx);
    assert_eq!(
        library(&view, cx),
        [(1, "Hero".to_string()), (2, "Opening".to_string())]
    );

    // Clicking the card places the layers on the active panel.
    cx.update(|_, cx| view.update(cx, |v, cx| v.select_page(2, cx)));
    settle(cx);
    assert!(!names(&view, cx).contains(&"Hero".to_string()));
    cx.update(|window, cx| window.click("storyboard-library-project-1", cx));
    settle(cx);
    assert_eq!(names(&view, cx).last().unwrap(), "Hero");
    undo(&view, cx);
    assert!(!names(&view, cx).contains(&"Hero".to_string()));

    // Dropping a panel item on a Board panel adds the panel after it.
    cx.update(|_, cx| view.update(cx, |v, cx| v.library_place(Scope::Project, 2, Some(1), cx)));
    settle(cx);
    let order = cx.update(|_, cx| {
        let e = &view.read(cx).editor;
        (
            e.page_list().iter().map(|m| m.id).collect::<Vec<_>>(),
            e.active_page(),
        )
    });
    assert_eq!(order.0.len(), 3);
    assert_eq!(order.0[1], order.1);
    assert!(names(&view, cx).contains(&"Hero".to_string()));
    undo(&view, cx);
    assert_eq!(cx.update(|_, cx| view.read(cx).editor.page_list().len()), 2);

    // Deleting is undoable too, and a bad item changes nothing.
    cx.update(|_, cx| view.update(cx, |v, cx| v.library_delete(Scope::Project, 1, cx)));
    settle(cx);
    assert_eq!(library(&view, cx).len(), 1);
    let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());
    cx.update(|_, cx| view.update(cx, |v, cx| v.library_place(Scope::Project, 1, None, cx)));
    cx.update(|_, cx| view.update(cx, |v, cx| v.library_delete(Scope::Project, 1, cx)));
    assert_eq!(cx.update(|_, cx| view.read(cx).editor.stamp()), stamp);
    undo(&view, cx);
    assert_eq!(library(&view, cx).len(), 2);

    // Search filters the cards.
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.storyboard_library
                .search
                .clone()
                .unwrap()
                .update(cx, |s, cx| s.set_value("charac", window, cx))
        })
    });
    settle(cx);
    cx.update(|window, _| {
        assert!(window.find("storyboard-library-project-1").visible());
        assert!(window.try_find("storyboard-library-project-2").is_none());
    });
}

#[gpui_kit::test]
fn personal_library_items_reach_other_storyboards(cx: &mut TestAppContext) {
    let (_ws, view, cx) = storyboard(cx);
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.library_add(Scope::Personal, ItemKind::Layers, "Shared hero", &[], cx)
        })
    });
    settle(cx);
    let id = cx.update(|_, cx| {
        personal::items(&view.read(cx).creative.catalog)
            .find(|(a, _)| a.name == "Shared hero")
            .map(|(a, _)| a.id)
            .unwrap()
    });
    // Not a project edit: the project library stays empty.
    assert!(library(&view, cx).is_empty());
    // The drawing loads for its thumbnail, then places like any other.
    settle(cx);
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.library_place(Scope::Personal, id, Some(2), cx)
        })
    });
    settle(cx);
    assert_eq!(cx.update(|_, cx| view.read(cx).editor.active_page()), 2);
    // The layers keep their names; the item's name is the library's.
    assert_eq!(names(&view, cx).last().unwrap(), "Hero");
    undo(&view, cx);
    assert!(!names(&view, cx).contains(&"Hero".to_string()));
    cx.update(|_, cx| view.update(cx, |v, cx| v.library_delete(Scope::Personal, id, cx)));
    settle(cx);
    assert!(cx.update(|_, cx| {
        personal::items(&view.read(cx).creative.catalog).all(|(a, _)| a.id != id)
    }));
}

#[gpui_kit::test]
fn storyboard_templates_start_unsaved_copies_from_new_canvas(cx: &mut TestAppContext) {
    let (ws, view, cx) = storyboard(cx);
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.library_add(Scope::Project, ItemKind::Panel, "Opening", &[], cx);
            v.editor
                .edit_storyboard(|b| {
                    b.settings.frame_rate = emulsion_core::storyboard::FrameRate::whole(25);
                    b.smart_add_layers = vec!["Hero".into()];
                    Ok(())
                })
                .unwrap();
            v.save_storyboard_template("Pilot template".into(), vec!["tv".into()], cx);
        })
    });
    settle(cx);
    let (id, source) = cx.update(|_, cx| {
        let catalog = &ws.read(cx).home_state.projects.catalog;
        let asset = catalog
            .assets
            .iter()
            .find(|a| a.kind == library::AssetKind::StoryboardTemplate)
            .expect("template installed and published to the workspace");
        (asset.id, view.read(cx).editor.snapshot().unwrap())
    });
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_new_canvas_kind(CanvasKind::Storyboard, window, cx)
        })
    });
    settle(cx);
    cx.update(|window, cx| window.click("new-canvas-templates", cx));
    settle(cx);
    cx.update(|window, cx| {
        window.click(SharedString::from(format!("new-template-local-{id}")), cx)
    });
    settle(cx);
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    settle(cx);
    cx.update(|_, cx| {
        let tab = ws.read(cx).editor.clone().unwrap();
        assert_ne!(tab.entity_id(), view.entity_id());
        let e = tab.read(cx);
        assert_eq!(e.name, "Pilot template");
        assert!(e.editor.path.is_none());
        assert!(e.has_unsaved_changes());
        assert!(!e.editor.can_undo());
        let board = e.editor.storyboard().unwrap();
        assert_eq!(board.settings, source.storyboard.as_ref().unwrap().settings);
        assert_eq!(board.smart_add_layers, ["Hero"]);
        assert_eq!(board.library.items[0].name, "Opening");
        assert_eq!(e.editor.page_list().len(), source.pages.len());
        assert!(e.draw_mode);
    });
}
