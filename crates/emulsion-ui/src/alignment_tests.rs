//! Alignment through the editor entry point, including its edit guards.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::command::{AlignTarget, Alignment};
use emulsion_core::{NodeId, NodeKind};
use emulsion_raster::select;
use gpui_kit::test::TestWindowExt;

fn artwork(grouped: bool) -> (Document, NodeId) {
    let mut d = Document::new(256, 192);
    let group = grouped.then(|| {
        Command::AddNode {
            node: Box::new(Node::group(0, "Artwork")),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap()
        .unwrap()
    });
    let mut selected = group;
    for (name, x, hidden) in [("A", 30., false), ("B", 70., true)] {
        if !grouped && hidden {
            continue;
        }
        let mut node = Node::raster(
            0,
            name,
            Arc::new(Raster::solid(20, 10, [1., 0., 0., 1.])),
            Placement::at(x, 40.),
        );
        node.visible = !hidden;
        let id = Command::AddNode {
            node: Box::new(node),
            slot: Slot::top_of(group),
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        selected.get_or_insert(id);
    }
    (d, selected.unwrap())
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
            e.set_tool(Tool::Move, cx);
        })
    });
    cx.run_until_parked();
    (e, cx)
}

fn placement(d: &Document, name: &str) -> Placement {
    match &d.nodes.iter().find(|n| n.name == name).unwrap().kind {
        NodeKind::Raster { placement, .. } => *placement,
        _ => panic!("expected pixel layer"),
    }
}

#[gpui_kit::test]
fn layer_alignment_is_one_undo_and_repeating_it_is_a_noop(cx: &mut TestAppContext) {
    let (d, id) = artwork(false);
    let before = d.clone();
    let (e, cx) = setup(cx, d, id);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.align_selected(Alignment::Left, AlignTarget::Canvas, cx);
            assert_eq!(placement(&e.editor.doc, "A").x, 0.);
            assert_eq!(placement(&e.editor.doc, "A").y, 40.);
            assert_eq!(e.selected, Some(id));
            assert_eq!(e.editor.history.len(), 1);
            let aligned = e.editor.doc.clone();
            e.align_selected(Alignment::Left, AlignTarget::Canvas, cx);
            assert_eq!(e.editor.doc, aligned);
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
            e.redo(cx);
            assert_eq!(e.editor.doc, aligned);
        })
    });
}

#[gpui_kit::test]
fn align_popup_opens_by_mouse_and_canvas_command_runs_by_keyboard(cx: &mut TestAppContext) {
    let (d, id) = artwork(false);
    let before = d.clone();
    let (e, cx) = setup(cx, d, id);
    cx.update(|window, cx| {
        assert!(window.find("move-align").visible());
        window.click("move-align", cx);
        let mut menu = window.within("popup-menu");
        assert_eq!(menu.find(0usize).label(), Some("Canvas"));
        assert_eq!(
            menu.find(1usize).label(),
            Some("Selection (make a selection first)")
        );
        // Popup rows expose selected metadata but not disabled metadata.
        // Verify that the unavailable target actually ignores a click.
        menu.click(1usize, cx);
        assert!(window.try_find("popup-menu").is_some());
        assert!(window.try_find("submenu").is_none());
        assert_eq!(
            e.read(cx).editor.doc,
            before,
            "opening the menu or clicking an unavailable target must not align"
        );
        assert_eq!(e.read(cx).editor.history.len(), 0);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        // Hovering establishes the chosen submenu independently of initial
        // pointer placement; Right then moves keyboard focus into it.
        window.within("popup-menu").hover(0usize, cx);
        // Hovering mounts a second popup-menu inside the submenu. Dispatch to
        // the focused window instead of looking up an ambiguous popup name.
        window.press("right", cx);
        assert_eq!(
            window.within("submenu").find(0usize).label(),
            Some("Align left")
        );
        assert_eq!(window.within("submenu").find(0usize).selected(), Some(true));
        window.press("enter", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("popup-menu").is_none());
        assert!(window.try_find("submenu").is_none());
        e.update(cx, |e, cx| {
            assert_eq!(placement(&e.editor.doc, "A").x, 0.);
            assert_eq!(placement(&e.editor.doc, "A").y, 40.);
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        });
    });
}

#[gpui_kit::test]
fn group_alignment_moves_hidden_children_together_in_one_undo(cx: &mut TestAppContext) {
    let (d, id) = artwork(true);
    let before = d.clone();
    let (e, cx) = setup(cx, d, id);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.align_selected(Alignment::Left, AlignTarget::Canvas, cx);
            assert_eq!(placement(&e.editor.doc, "A").x, 0.);
            assert_eq!(placement(&e.editor.doc, "B").x, 40.);
            assert_eq!(placement(&e.editor.doc, "B").y, 40.);
            assert!(
                !e.editor
                    .doc
                    .nodes
                    .iter()
                    .find(|n| n.name == "B")
                    .unwrap()
                    .visible
            );
            assert_eq!(e.selected, Some(id));
            assert_eq!(e.editor.history.len(), 1);
            assert!(!e.editor.in_transaction());
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn aligning_to_selection_preserves_the_selection_and_undo_restores_artwork(
    cx: &mut TestAppContext,
) {
    let (mut d, id) = artwork(false);
    let selection = Arc::new(select::rect(256, 192, 100., 80., 60., 40.));
    d.selection = Some(selection.clone());
    let before = d.clone();
    let (e, cx) = setup(cx, d, id);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.align_selected(Alignment::HorizontalCenter, AlignTarget::Selection, cx);
            assert_eq!(placement(&e.editor.doc, "A").x, 120.);
            assert_eq!(placement(&e.editor.doc, "A").y, 40.);
            assert!(Arc::ptr_eq(
                e.editor.doc.selection.as_ref().unwrap(),
                &selection
            ));
            e.align_selected(Alignment::Bottom, AlignTarget::Selection, cx);
            assert_eq!(placement(&e.editor.doc, "A").y, 110.);
            assert_eq!(e.editor.history.len(), 2);
            assert!(Arc::ptr_eq(
                e.editor.doc.selection.as_ref().unwrap(),
                &selection
            ));
            e.undo(cx);
            assert_eq!(placement(&e.editor.doc, "A").y, 40.);
            assert_eq!(placement(&e.editor.doc, "A").x, 120.);
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn alignment_rejects_missing_targets_active_edits_warp_and_locked_children(
    cx: &mut TestAppContext,
) {
    let (d, group) = artwork(true);
    let before = d.clone();
    let child = d.nodes.iter().find(|n| n.name == "A").unwrap().id;
    let (e, cx) = setup(cx, d, group);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.selected = None;
            e.align_selected(Alignment::Left, AlignTarget::Canvas, cx);
            assert_eq!(e.editor.doc, before);
            e.selected = Some(group);
            e.align_selected(Alignment::Left, AlignTarget::Selection, cx);
            assert_eq!(
                e.editor.doc, before,
                "missing selection must not fall back to canvas"
            );
            assert_eq!(e.editor.history.len(), 0);

            e.editor.begin("Unfinished opacity edit");
            e.execute(
                Command::SetOpacity {
                    id: group,
                    opacity: 0.5,
                },
                cx,
            );
            let preview = e.editor.doc.clone();
            e.align_selected(Alignment::Left, AlignTarget::Canvas, cx);
            assert_eq!(e.editor.doc, preview);
            assert!(
                e.editor.in_transaction(),
                "alignment must not close another edit"
            );
            assert_eq!(e.editor.history.len(), 0);
            e.editor.cancel();

            e.selected = Some(child);
            e.start_warp(cx);
            let grid = e.warp.as_ref().unwrap().grid.clone();
            e.align_selected(Alignment::Left, AlignTarget::Canvas, cx);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.warp.as_ref().unwrap().grid, grid);
            assert_eq!(e.editor.history.len(), 0);
            e.cancel_warp(cx);

            e.execute(
                Command::SetLocked {
                    id: child,
                    locked: true,
                },
                cx,
            );
            e.selected = Some(group);
            let locked = e.editor.doc.clone();
            e.align_selected(Alignment::Left, AlignTarget::Canvas, cx);
            assert_eq!(e.editor.doc, locked);
            assert_eq!(
                e.editor.history.len(),
                1,
                "only the lock action belongs in history"
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}
