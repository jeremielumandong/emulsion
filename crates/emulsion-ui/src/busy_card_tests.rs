use super::*;
use emulsion_ai::jobs::Job;
use gpui_kit::test::TestWindowExt;
use std::time::{Duration, Instant};

#[gpui_kit::test]
fn opening_shows_a_progress_card_until_the_file_is_ready(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            let mut busy =
                crate::busy_card::Busy::new("Opening photo.jpg").detail("JPG image · 2.1 MB");
            // Past the delay that keeps quick opens from flashing a card.
            busy.started = Instant::now() - Duration::from_secs(2);
            ws.busy = Some(busy);
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("busy-overlay").is_some());
        assert!(window.try_find("busy-card").is_some());
    });
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.busy = None;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("busy-overlay").is_none());
    });
}

#[gpui_kit::test]
fn ai_job_card_reports_progress_and_cancels(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let job = Job::new();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.watch_job(job.clone(), "Removing the background", cx);
            e.ai.job_busy.as_mut().unwrap().started = Instant::now() - Duration::from_secs(2);
            cx.notify();
        })
    });
    job.set_stage("finding the subject");
    job.progress(0.4);
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("ai-job-card").is_some());
    });
    cx.update(|window, cx| window.click("ai-job-cancel", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(job.cancelled());
        assert!(e.read(cx).ai.job.is_none());
        assert!(window.try_find("ai-job-card").is_none());
    });
}
