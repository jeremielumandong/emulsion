//! Review and change tracking through the real editor: review statuses,
//! notes and review layers as single Undo steps, the Board's review
//! filter, change marks against a board version and Next Change.
use super::tests::{layout, open_board, setup, storyboard};
use super::*;
use core::prelude::v1::test;
use emulsion_core::storyboard::ReviewStatus;
use emulsion_core::storyboard_changes::ChangeKind;

fn cards(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PageId> {
    cx.update(|_, cx| {
        e.read(cx)
            .board_rows(false)
            .into_iter()
            .flat_map(|row| match row {
                Row::Scene { cards, .. } => cards.into_iter().map(|c| c.id).collect(),
                Row::Group { .. } => Vec::new(),
            })
            .collect()
    })
}

#[gpui_kit::test]
fn review_status_notes_and_layers_are_one_undo_step_each(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, storyboard(3, 64, 36));
    let ids = layout(&e, cx);
    let review = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            e.read(cx).editor.storyboard().unwrap().panels[&ids[0]]
                .review
                .clone()
        })
    };
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_review_status(vec![ids[0]], ReviewStatus::NeedsChanges, cx);
            assert!(e.add_review_note_text(ids[0], "Bigger eyes", cx));
        })
    });
    assert_eq!(review(cx).status, ReviewStatus::NeedsChanges);
    assert_eq!(review(cx).notes[0].text, "Bigger eyes");
    // The Board filter shows only panels that need changes.
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            e.review_ui.filter =
                super::super::storyboard_review::BoardFilter::Status(ReviewStatus::NeedsChanges)
        })
    });
    assert_eq!(cards(&e, cx), vec![ids[0]]);
    cx.update(|_, cx| e.update(cx, |e, _| e.review_ui.filter = Default::default()));
    assert_eq!(cards(&e, cx).len(), 3);
    // One Undo step each.
    cx.update(|_, cx| e.update(cx, |e, _| assert!(e.editor.undo())));
    assert!(review(cx).notes.is_empty());
    cx.update(|_, cx| e.update(cx, |e, _| assert!(e.editor.undo())));
    assert!(review(cx).is_empty());
    // A review layer is one step too, and marked review.
    let layer = cx
        .update(|_, cx| e.update(cx, |e, cx| e.new_review_layer(cx)))
        .unwrap();
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            assert!(e.editor.doc.node(layer).unwrap().review);
            assert!(e.editor.undo());
            assert!(e.editor.doc.node(layer).is_none());
        })
    });
}

#[gpui_kit::test]
fn change_marks_follow_a_board_version_and_next_change_visits_them(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, storyboard(4, 64, 36));
    let ids = layout(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.create_board_version("Pass 1", cx));
            e.edit_board(
                |b| {
                    b.panels.get_mut(&ids[2]).unwrap().frames = 10;
                    Ok(())
                },
                cx,
            );
            e.select_page(ids[0], cx);
            e.toggle_change_marks(cx);
        })
    });
    open_board(cx);
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let computed = e.changes_now().unwrap();
            assert_eq!(computed.label, "Pass 1");
            assert_eq!(computed.of(ids[2]).unwrap().kind, ChangeKind::Changed);
            assert!(!computed.of(ids[1]).unwrap().is_change());
            // An outline and a badge on the changed panel only.
            let p = theme::palette(cx);
            e.refresh_change_marks(cx);
            assert_eq!(e.panel_marks(ids[2], false, &p).len(), 2);
            assert!(e.panel_marks(ids[1], false, &p).is_empty());
            e.step_change(true, cx);
            assert_eq!(e.editor.active_page(), ids[2]);
            assert_eq!(e.board_selection(), vec![ids[2]]);
            // Wrapping around comes back to it.
            e.step_change(true, cx);
            assert_eq!(e.editor.active_page(), ids[2]);
        })
    });
}
