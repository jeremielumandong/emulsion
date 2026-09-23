//! Pointer and keyboard movement through the same actions as the canvas.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::{NodeId, NodeKind};
use emulsion_raster::select;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton, Pixels, Point};

fn artwork() -> (Document, NodeId) {
    let mut d = Document::new(256, 192);
    let group = Command::AddNode {
        node: Box::new(Node::group(0, "Artwork")),
        slot: Slot::TOP,
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    for (name, x) in [("A", 40.), ("B", 90.)] {
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                name,
                Arc::new(Raster::solid(32, 24, [1., 0., 0., 1.])),
                Placement::at(x, 50.),
            )),
            slot: Slot::top_of(Some(group)),
        }
        .apply(&mut d)
        .unwrap();
    }
    Command::SetMask {
        id: group,
        mask: Some(Arc::new(select::rect(256, 192, 30., 40., 110., 50.))),
    }
    .apply(&mut d)
    .unwrap();
    (d, group)
}

fn setup(
    cx: &mut TestAppContext,
    d: Document,
    selected: NodeId,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, d);
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(selected);
            e.snap = false;
            e.set_tool(Tool::Move, cx);
        })
    });
    cx.run_until_parked();
    (e, cx)
}

fn at(e: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) -> Point<Pixels> {
    cx.update(|_, cx| e.read(cx).doc_to_window(p).unwrap())
}

fn placement(d: &Document, name: &str) -> Placement {
    match &d.nodes.iter().find(|n| n.name == name).unwrap().kind {
        NodeKind::Raster { placement, .. } => *placement,
        _ => panic!("pixel layer"),
    }
}

#[gpui_kit::test]
fn group_drag_moves_children_and_mask_as_one_undo_step(cx: &mut TestAppContext) {
    let (d, group) = artwork();
    let before = d.clone();
    let (e, cx) = setup(cx, d, group);
    let (a, b) = (at(&e, cx, (60., 60.)), at(&e, cx, (80., 75.)));
    cx.update(|window, cx| window.drag(a, b, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(
                (
                    placement(&e.editor.doc, "A").x,
                    placement(&e.editor.doc, "A").y
                ),
                (60., 65.)
            );
            assert_eq!(placement(&e.editor.doc, "B").x, 110.);
            let mask = e.editor.doc.node(group).unwrap().mask.as_ref().unwrap();
            assert_eq!(mask.get(31, 41), 0);
            assert_eq!(mask.get(51, 56), 255);
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
            e.redo(cx);
            assert_eq!(placement(&e.editor.doc, "B").x, 110.);
        })
    });
}

#[gpui_kit::test]
fn move_back_and_escape_restore_original_mask_and_unsafe_move_is_rejected(cx: &mut TestAppContext) {
    let (d, group) = artwork();
    let before = d.clone();
    let (e, cx) = setup(cx, d, group);
    let a = at(&e, cx, (60., 60.));
    let outside = at(&e, cx, (5., 5.));
    let b = at(&e, cx, (75., 75.));
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(
            e.read(cx).editor.doc,
            before,
            "moves that clip a document mask must be rejected"
        )
    });
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_move(a, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(a, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(e.read(cx).editor.doc, before);
        assert_eq!(e.read(cx).editor.history.len(), 0);
    });
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(e.read(cx).editor.doc, before);
        assert_eq!(e.read(cx).editor.history.len(), 0);
        assert!(!e.read(cx).editor.in_transaction());
    });
}

#[gpui_kit::test]
fn secondary_button_and_tool_switch_do_not_leave_move_transaction_open(cx: &mut TestAppContext) {
    let (d, group) = artwork();
    let (e, cx) = setup(cx, d, group);
    let a = at(&e, cx, (60., 60.));
    let b = at(&e, cx, (75., 75.));
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_down(b, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_up(b, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert!(!e.read(cx).editor.in_transaction());
        assert_eq!(e.read(cx).editor.history.len(), 1);
    });
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_keystrokes("b");
    cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert!(!e.read(cx).editor.in_transaction());
        assert_eq!(e.read(cx).tool, Tool::Brush);
    });
}

#[gpui_kit::test]
fn arrow_nudges_use_document_pixels_and_respect_locked_descendants(cx: &mut TestAppContext) {
    let (d, group) = artwork();
    let (e, cx) = setup(cx, d, group);
    let a = at(&e, cx, (60., 60.));
    cx.simulate_click(a, Modifiers::none());
    cx.simulate_keystrokes("right shift-down");
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(
                (
                    placement(&e.editor.doc, "A").x,
                    placement(&e.editor.doc, "A").y
                ),
                (41., 60.)
            );
            assert_eq!(e.editor.history.len(), 2);
            e.undo(cx);
            assert_eq!(placement(&e.editor.doc, "A").y, 50.);
            let child = e.editor.doc.children(Some(group))[0];
            e.execute(
                Command::SetLocked {
                    id: child,
                    locked: true,
                },
                cx,
            );
        })
    });
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    cx.simulate_keystrokes("left shift-up");
    let b = at(&e, cx, (100., 100.));
    cx.update(|window, cx| window.drag(a, b, cx));
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, before));
}

#[gpui_kit::test]
fn move_does_not_restore_preview_over_a_newer_edit(cx: &mut TestAppContext) {
    let (d, group) = artwork();
    let (e, cx) = setup(cx, d, group);
    let a = at(&e, cx, (60., 60.));
    let b = at(&e, cx, (80., 60.));
    cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::none());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetOpacity {
                    id: group,
                    opacity: 0.5,
                },
                cx,
            );
        })
    });
    let expected = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    cx.simulate_mouse_move(a, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(a, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(e.read(cx).editor.doc, expected);
        assert!(!e.read(cx).editor.in_transaction());
    });
}

#[gpui_kit::test]
fn arrows_inside_text_fields_do_not_nudge_artwork(cx: &mut TestAppContext) {
    let (d, group) = artwork();
    let (e, cx) = setup(cx, d, group);
    // Ctrl-F (Ask) gives a real text field focus while Move remains the active tool.
    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_keystrokes("a b c");
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    cx.simulate_keystrokes("left shift-right up shift-down");
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, before));
}

#[gpui_kit::test]
fn completely_masked_group_can_still_be_moved(cx: &mut TestAppContext) {
    let (mut d, group) = artwork();
    Command::SetMask {
        id: group,
        mask: Some(Arc::new(emulsion_raster::Mask::empty(256, 192, 0))),
    }
    .apply(&mut d)
    .unwrap();
    let (e, cx) = setup(cx, d, group);
    let a = at(&e, cx, (60., 60.));
    cx.simulate_click(a, Modifiers::none());
    cx.simulate_keystrokes("right");
    cx.update(|_, cx| assert_eq!(placement(&e.read(cx).editor.doc, "A").x, 41.));
}
