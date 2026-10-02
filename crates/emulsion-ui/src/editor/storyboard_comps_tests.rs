//! Layer comps through the Panel inspector: save, apply, rename and delete,
//! each one Undo step; locked panels refuse.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::creation::{CanvasKind, CanvasSpec};
use gpui_kit::test::TestWindowExt;

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    [NodeId; 2],
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(size(px(1600.), px(3000.)));
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
    let layers = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let mut add = |name: &str| {
                e.execute(
                    Command::AddNode {
                        node: Box::new(Node::new(0, name, NodeKind::Fill { rgba: [200; 4] })),
                        slot: Slot::TOP,
                    },
                    cx,
                )
                .unwrap()
            };
            [add("Rain"), add("Sun")]
        })
    });
    settle(cx);
    (ws, view, layers, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.run_until_parked();
}

fn comps(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let e = e.read(cx);
        e.editor.storyboard().unwrap().panels[&e.editor.active_page()]
            .comps
            .iter()
            .map(|c| c.name.clone())
            .collect()
    })
}

fn visible(e: &Entity<EditorView>, id: NodeId, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| e.read(cx).editor.doc.node(id).unwrap().visible)
}

fn hide(e: &Entity<EditorView>, id: NodeId, hidden: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetVisible {
                    id,
                    visible: !hidden,
                },
                cx,
            );
        })
    });
}

fn undo(e: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    settle(cx);
}

#[gpui_kit::test]
fn comps_save_apply_rename_and_delete_each_in_one_step(cx: &mut TestAppContext) {
    let (_ws, e, [rain, sun], cx) = setup(cx);
    cx.update(|window, _| assert!(window.find("storyboard-layer-comps").visible()));
    // Save current as…: the dialog names the comp.
    hide(&e, sun, true, cx);
    cx.update(|window, cx| window.click("storyboard-comp-save", cx));
    settle(cx);
    cx.update(|window, cx| window.click("ok", cx));
    settle(cx);
    assert_eq!(comps(&e, cx), ["Comp 1"]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.rename_layer_comp("Comp 1", "Rainy", cx))
        })
    });
    assert_eq!(comps(&e, cx), ["Rainy"]);
    hide(&e, sun, false, cx);
    hide(&e, rain, true, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(e.save_layer_comp("Sunny", cx))));
    settle(cx);
    assert_eq!(comps(&e, cx), ["Rainy", "Sunny"]);
    // Apply shows and hides as saved; one Undo puts it back.
    cx.update(|window, cx| window.click(("storyboard-comp-apply", 0usize), cx));
    settle(cx);
    assert!(visible(&e, rain, cx) && !visible(&e, sun, cx));
    undo(&e, cx);
    assert!(!visible(&e, rain, cx) && visible(&e, sun, cx));
    // Rename, refusing a name in use.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(!e.rename_layer_comp("Rainy", "Sunny", cx));
            assert!(e.rename_layer_comp("Rainy", "Storm", cx));
        })
    });
    assert_eq!(comps(&e, cx), ["Storm", "Sunny"]);
    undo(&e, cx);
    assert_eq!(comps(&e, cx), ["Rainy", "Sunny"]);
    // Delete.
    cx.update(|window, cx| window.click(("storyboard-comp-delete", 1usize), cx));
    settle(cx);
    assert_eq!(comps(&e, cx), ["Rainy"]);
    undo(&e, cx);
    assert_eq!(comps(&e, cx), ["Rainy", "Sunny"]);
    // Locked panels refuse every change.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let panel = e.editor.active_page();
            assert!(e.edit_board(
                |b| {
                    b.panels.get_mut(&panel).unwrap().locked = true;
                    Ok(())
                },
                cx,
            ));
            assert!(!e.save_layer_comp("Night", cx));
            assert!(!e.apply_layer_comp("Rainy", cx));
            assert!(!e.rename_layer_comp("Rainy", "Dawn", cx));
            assert!(!e.delete_layer_comp("Rainy", cx));
        })
    });
    assert_eq!(comps(&e, cx), ["Rainy", "Sunny"]);
    assert!(!visible(&e, rain, cx));
}
