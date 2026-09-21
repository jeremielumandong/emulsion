//! Layer selection and shared operations through real panel/canvas input.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::{NodeId, NodeKind};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton};

fn setup<'a>(
    cx: &'a mut TestAppContext,
    names: &[&str],
) -> (Entity<EditorView>, Vec<NodeId>, &'a mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(names, None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let ids = cx.update(|_, cx| e.read(cx).editor.doc.nodes.iter().map(|n| n.id).collect());
    cx.run_until_parked();
    (e, ids, cx)
}

fn click_row(cx: &mut VisualTestContext, id: NodeId, modifiers: Modifiers) {
    let point = cx.update(|window, _| window.find(("row", id)).bounds().center());
    cx.simulate_click(point, modifiers);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn range_toggle_duplicate_and_undo_use_the_layer_selection(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx, &["Bottom", "Middle", "Top"]);
    click_row(cx, ids[2], Modifiers::none());
    click_row(
        cx,
        ids[0],
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
    );
    cx.update(|_, cx| assert_eq!(e.read(cx).selected_layer_ids(), ids));
    click_row(
        cx,
        ids[1],
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    cx.update(|_, cx| assert_eq!(e.read(cx).selected_layer_ids(), vec![ids[0], ids[2]]));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-j"
    } else {
        "ctrl-j"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = e.read(cx);
        assert_eq!(editor.editor.doc.nodes.len(), 5);
        assert_eq!(editor.selected_layer_ids().len(), 2);
        assert_eq!(editor.editor.history.len(), 1);
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc.nodes.len(), 3));
}

#[gpui_kit::test]
fn shared_delete_is_atomic_when_one_layer_is_locked(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx, &["Unlocked", "Locked"]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.editor
                .execute(Command::SetLocked {
                    id: ids[1],
                    locked: true,
                })
                .unwrap();
            e.set_layer_selection(ids.clone(), Some(ids[0]));
            let before = e.editor.doc.clone();
            let history = e.editor.history.len();
            e.delete_selected(cx);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), history);
            assert!(e.status.as_ref().is_some_and(|(_, error)| *error));
        })
    });
}

#[gpui_kit::test]
fn group_uses_all_selected_layers_and_undo_restores_order(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx, &["Bottom", "Middle", "Top"]);
    click_row(cx, ids[2], Modifiers::none());
    click_row(
        cx,
        ids[1],
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-g"
    } else {
        "ctrl-g"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let group = e.selected.unwrap();
            assert!(e.editor.doc.node(group).unwrap().is_group());
            assert_eq!(e.editor.doc.children(Some(group)), ids[1..]);
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc.children(None), ids);
        })
    });
}

#[gpui_kit::test]
fn dragging_selected_layers_moves_together_and_escape_restores_both(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx, &["Bottom", "Top"]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_layer_selection(ids.clone(), Some(ids[1]));
            e.set_tool(Tool::Move, cx);
            e.snap = false;
            assert!(
                e.layer_outline().is_some(),
                "multiple selection has a visible canvas outline"
            );
        })
    });
    let (a, b) = cx.update(|_, cx| {
        let e = e.read(cx);
        (
            e.doc_to_window((50., 50.)).unwrap(),
            e.doc_to_window((70., 60.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        for id in &ids {
            let NodeKind::Raster { placement, .. } = &e.editor.doc.node(*id).unwrap().kind else {
                panic!()
            };
            assert_eq!((placement.x, placement.y), (20., 10.));
        }
    });
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        for id in &ids {
            let NodeKind::Raster { placement, .. } = &e.editor.doc.node(*id).unwrap().kind else {
                panic!()
            };
            assert_eq!((placement.x, placement.y), (0., 0.));
        }
        assert_eq!(e.editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn reordering_selected_block_keeps_order(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx, &["A", "B", "C", "D"]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_layer_selection(vec![ids[1], ids[2]], Some(ids[2]));
            e.ungroup_selected(cx);
            assert_eq!(e.selected_layer_ids(), vec![ids[1], ids[2]]);
            e.shift_selected(true, cx);
            assert_eq!(
                e.editor.doc.children(None),
                vec![ids[0], ids[3], ids[1], ids[2]]
            );
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc.children(None), ids);
        })
    });
}

#[gpui_kit::test]
fn editing_text_does_not_resurrect_an_old_multiple_layer_selection(cx: &mut TestAppContext) {
    use emulsion_core::text::TextSpec;
    let mut document = Document::new(240, 160);
    let mut ids = Vec::new();
    for (name, y) in [("First", 10.0), ("Second", 80.0)] {
        ids.push(
            Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    name,
                    TextSpec {
                        text: name.into(),
                        size: 20.0,
                        x: 10.0,
                        y,
                        ..Default::default()
                    },
                    240,
                    160,
                )),
                slot: Slot::TOP,
            }
            .apply(&mut document)
            .unwrap()
            .unwrap(),
        );
    }
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(ids.clone(), Some(ids[1]));
            e.set_tool(Tool::Type, cx);
        })
    });
    cx.run_until_parked();
    for (id, point) in [(ids[0], (20.0, 20.0)), (ids[1], (20.0, 90.0))] {
        cx.update(|window, cx| editor.update(cx, |e, cx| e.type_down(point, window, cx)));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).selected_layer_ids(), vec![id]));
    }
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.close_text_field(cx);
            e.delete_selected(cx);
            assert!(
                e.editor.doc.node(ids[0]).is_some(),
                "the previously selected layer must survive"
            );
            assert!(e.editor.doc.node(ids[1]).is_none());
        })
    });
}
