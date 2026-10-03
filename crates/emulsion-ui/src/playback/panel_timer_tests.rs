//! The Panel Timer: taps to durations, the review table, applying to the
//! selection or as new panels in one Undo step, and thumbnail sheets.
use super::*;
use crate::tests::open;
use core::prelude::v1::test;
use emulsion_core::project::ProjectKind;
use emulsion_core::storyboard::ThumbnailGrid;

fn storyboard(panels: usize) -> ProjectEditor {
    let mut p =
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(320, 180)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let items = (2..=panels)
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    p.insert_panels(Some(1), &blank, items, None).unwrap();
    p
}

fn ids(p: &ProjectEditor) -> Vec<PageId> {
    p.page_list().iter().map(|m| m.id).collect()
}

fn frames(p: &ProjectEditor) -> Vec<u32> {
    let board = p.storyboard().unwrap();
    ids(p).iter().map(|id| board.panels[id].frames).collect()
}

#[test]
fn taps_become_durations_on_the_running_time() {
    let rate = FrameRate::whole(24);
    let mut take = Take::new(Some(3));
    assert!(!take.tap(10.));
    assert_eq!(take.count(), 0, "the first tap starts the take");
    assert!(!take.tap(11.));
    // Cuts round on the running time, so the parts add up.
    assert!(!take.tap(11.52));
    assert!(take.tap(12.5), "the take ends after the last panel");
    assert_eq!(take.durations(rate), [24, 12, 24]);
    assert_eq!(take.durations(rate).iter().sum::<u32>(), 60);
    // Two taps in the same frame still leave every panel a frame.
    let mut quick = Take::new(None);
    for t in [0., 1., 1.001, 1.002] {
        quick.tap(t);
    }
    assert_eq!(quick.durations(rate), [24, 1, 1]);
    assert_eq!(quick.count(), 3);
}

#[test]
fn timings_apply_to_the_selection_as_one_undo_step() {
    let mut p = storyboard(4);
    let before = frames(&p);
    let ids = ids(&p);
    let target = TimerTarget::Panels(ids[1..].to_vec());
    // A partial take times the first panels only.
    let timed = apply_timings(&mut p, &target, &[10, 30]).unwrap();
    assert_eq!(timed, ids[1..3]);
    assert_eq!(frames(&p)[1..], [10, 30, 24]);
    assert!(p.undo());
    assert_eq!(frames(&p), before, "one Undo step restores every panel");
    assert!(apply_timings(&mut p, &target, &[]).is_err());
    assert!(apply_timings(&mut p, &target, &[0]).is_err());
}

#[test]
fn new_panels_go_after_the_selection_as_one_undo_step() {
    let mut p = storyboard(2);
    let made = apply_timings(&mut p, &TimerTarget::New { after: 1 }, &[12, 36, 6]).unwrap();
    assert_eq!(made.len(), 3);
    let order = ids(&p);
    assert_eq!(order[1..4], made[..], "after the selection, in order");
    assert_eq!(
        frames(&p),
        [p.storyboard().unwrap().panels[&1].frames, 12, 36, 6, 24]
    );
    let names: Vec<String> = p.page_list().iter().map(|m| m.name.clone()).collect();
    let unique: std::collections::HashSet<_> = names[1..4].iter().collect();
    assert_eq!(unique.len(), 3, "new panels get their own names: {names:?}");
    assert!(p.undo());
    assert_eq!(ids(&p).len(), 2);
}

fn setup(
    cx: &mut TestAppContext,
    project: ProjectEditor,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (editor, cx)
}

fn timer(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Entity<PanelTimer> {
    cx.update(|_, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            PanelTimer::new(e.read(cx), e.downgrade(), focus)
        })
    })
}

#[gpui_kit::test]
fn a_performance_is_reviewed_in_a_table_then_applied(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx, storyboard(4));
    let order: Vec<PageId> = cx.update(|_, cx| ids(&e.read(cx).editor));
    cx.update(|_, cx| e.update(cx, |e, _| e.set_board_selection(order[..3].to_vec())));
    let t = timer(&e, cx);
    cx.update(|window, cx| {
        t.update(cx, |t, cx| {
            assert_eq!(t.target(), TimerTarget::Panels(order[..3].to_vec()));
            t.sound = false;
            for now in [0., 0.5, 1.5, 2.] {
                t.tap(now, window, cx);
            }
            assert_eq!(t.phase, Phase::Review, "three taps time three panels");
            let old: Vec<_> = t.rows.iter().map(|(_, old, _)| *old).collect();
            assert_eq!(old[1..], [Some(24), Some(24)]);
            let typed = t.typed(cx).unwrap();
            assert_eq!(typed, [12, 24, 12]);
            // Durations can be corrected before applying.
            t.rows[2]
                .2
                .update(cx, |input, cx| input.set_value("30", window, cx));
            t.apply(window, cx);
        })
    });
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(frames(&e.editor)[..3], [12, 24, 30]);
        assert_eq!(e.board_selection(), order[..3]);
    });
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    cx.update(|_, cx| assert_eq!(frames(&e.read(cx).editor)[1..3], [24, 24]));
}

#[gpui_kit::test]
fn timing_a_thumbnail_sheet_converts_it_and_times_its_panels(cx: &mut TestAppContext) {
    let mut project = storyboard(2);
    project
        .edit_storyboard(|b| {
            b.panels.get_mut(&2).unwrap().thumbnails = Some(ThumbnailGrid::new(2, 1));
            Ok(())
        })
        .unwrap();
    let (e, cx) = setup(cx, project);
    cx.update(|_, cx| e.update(cx, |e, _| e.set_board_selection(vec![2])));
    let t = timer(&e, cx);
    cx.update(|window, cx| {
        t.update(cx, |t, cx| {
            assert_eq!(t.sheet, Some(2));
            assert!(t.selection.is_empty() && !t.on_selection);
            t.convert_sheet(cx);
            assert_eq!(t.selection.len(), 2, "the sheet's cells became panels");
            assert!(t.on_selection);
            t.sound = false;
            for now in [0., 1., 3.] {
                t.tap(now, window, cx);
            }
            t.apply(window, cx);
        })
    });
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(frames(&e.editor)[1..], [24, 48]);
    });
    // New panels in a take of their own.
    let t = timer(&e, cx);
    cx.update(|window, cx| {
        t.update(cx, |t, cx| {
            t.on_selection = false;
            t.sound = false;
            for now in [0., 0.25, 0.5] {
                t.tap(now, window, cx);
            }
            assert_eq!(t.phase, Phase::Timing, "new panels run until Esc");
            t.review(window, cx);
            assert_eq!(t.rows.len(), 2);
            t.apply(window, cx);
        })
    });
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.page_list().len(), 5);
        assert_eq!(frames(&e.editor)[3..], [6, 6]);
    });
}
