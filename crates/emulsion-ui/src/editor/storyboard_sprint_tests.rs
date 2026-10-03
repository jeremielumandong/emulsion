//! Sketch Sprint timing on a fake clock, the time-lapse it writes, and
//! line mileage counted from real strokes on the Stage.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use gpui_kit::test::TestWindowExt;

fn plan(per_panel: f64, panels: usize, new_panels: bool) -> SprintPlan {
    SprintPlan {
        per_panel,
        panels,
        new_panels,
    }
}

#[test]
fn the_clock_moves_panels_on_and_ends_the_session() {
    let doc = Document::new(8, 8);
    let mut s = Sprint::new(plan(30., 3, true), 100., 1, doc.clone());
    assert_eq!(s.tick(110.), Tick::Running);
    assert_eq!(s.remaining(110.), 20.);
    assert_eq!(s.tick(130.), Tick::NextPanel);
    s.advance(130., 2, doc.clone());
    assert_eq!(s.tick(131.), Tick::Running);
    assert_eq!(s.remaining(131.), 29.);
    // Paused time does not count.
    s.pause(140.);
    assert_eq!(s.tick(500.), Tick::Running);
    assert_eq!(s.remaining(500.), 20.);
    s.resume(500.);
    assert_eq!(s.elapsed(510.), 50.);
    assert_eq!(s.tick(520.), Tick::NextPanel);
    s.advance(520., 3, doc.clone());
    assert_eq!(s.tick(549.), Tick::Running);
    assert_eq!(s.tick(550.), Tick::Finished);
    s.finish(555.);
    let summary = s.summary(900.);
    assert_eq!(summary.panels, 3);
    assert_eq!(summary.seconds, 90., "capped at the plan, frozen once done");
}

#[test]
fn strokes_are_recorded_with_their_time_but_not_while_paused() {
    let doc = Document::new(8, 8);
    let mut s = Sprint::new(plan(10., 2, true), 0., 1, doc.clone());
    let ink = Mileage::from_px(72., 72.);
    s.record(2., 1, ink, doc.clone());
    s.pause(3.);
    s.record(4., 1, ink, doc.clone());
    s.resume(5.);
    s.record(6., 1, ink, doc.clone());
    assert_eq!(s.strokes, 2);
    assert!((s.ink.mm - 50.8).abs() < 1e-9);
    let times: Vec<f64> = s.frames.iter().map(|f| f.at).collect();
    assert_eq!(times, vec![0., 2., 4.], "the start, then drawing time");
}

#[test]
fn time_lapses_speed_long_sessions_up() {
    assert_eq!(timelapse_speed(10.), 1.);
    assert_eq!(timelapse_speed(300.), 15.);
    let delays = timelapse_delays(&[(0., 1), (15., 1), (15.1, 1), (16., 2), (45., 2)], 15.);
    assert_eq!(delays, vec![1., 0.05, PANEL_HOLD, 1., HOLD_END]);
    let frames = timelapse_movie(&[(0., 1), (15., 1), (30., 1)], 15., 12.);
    // 1 s, 1 s and the hold, at 12 fps.
    assert_eq!(frames.len(), 42);
    assert_eq!(frames[0], 0);
    assert_eq!(frames[12], 1);
    assert_eq!(*frames.last().unwrap(), 2);
}

#[test]
fn a_time_lapse_gif_has_a_frame_per_recorded_moment() {
    let frames: Vec<SprintFrame> = (0..4u32)
        .map(|i| SprintFrame {
            panel: 1 + u64::from(i) / 2,
            at: f64::from(i) * 5.,
            doc: Document::new(16, 9),
        })
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sprint.gif");
    write_timelapse_gif(&frames, 20., &path).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)).unwrap();
    use image::AnimationDecoder;
    assert_eq!(decoder.into_frames().count(), 4);
}

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let editor = cx.update(|window, cx| {
        let board =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        ws.update(cx, |ws: &mut Workspace, cx| {
            ws.install_project(board, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    (editor, cx)
}

#[gpui_kit::test]
fn a_sprint_adds_panels_as_time_runs_out_and_records_strokes(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.start_sprint_at(plan(30., 2, true), 0., cx);
            let first = e.editor.active_page();
            assert_eq!(e.editor.page_list().len(), 2, "a fresh panel to start on");
            e.note_ink(144., cx);
            e.sprint_tick(10., cx);
            assert_eq!(e.editor.active_page(), first);
            e.sprint_tick(30., cx);
            assert_eq!(e.editor.page_list().len(), 3, "time up: the next panel");
            assert_ne!(e.editor.active_page(), first);
            e.sprint_tick(60., cx);
            let sprint = e.extras.sprint.as_ref().unwrap();
            assert!(sprint.finished.is_some());
            assert_eq!(sprint.panels.len(), 2);
            assert_eq!(sprint.strokes, 1);
            assert_eq!(sprint.frames.len(), 3);
            assert_eq!(e.editor.panel_mileage(first).px, 144.);
        })
    });
}

#[gpui_kit::test]
fn brush_strokes_on_the_stage_count_and_undo_takes_them_off(cx: &mut TestAppContext) {
    let (e, cx) = setup(cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(Tool::Brush, cx)));
    let at = |cx: &mut VisualTestContext, d| {
        cx.update(|_, cx| e.read(cx).doc_to_window(d).expect("canvas laid out"))
    };
    let (a, b) = (at(cx, (10., 18.)), at(cx, (50., 18.)));
    cx.update(|window, cx| window.drag(a, b, cx));
    cx.run_until_parked();
    let (page, after) = cx.update(|_, cx| {
        let e = e.read(cx);
        let page = e.editor.active_page();
        (page, e.editor.panel_mileage(page).px)
    });
    assert!((after - 40.).abs() < 4., "about the 40 px drawn: {after}");
    cx.update(|_, cx| e.update(cx, |e, _| assert!(e.editor.undo())));
    let undone = cx.update(|_, cx| e.read(cx).editor.panel_mileage(page).px);
    assert_eq!(undone, 0.);
}
