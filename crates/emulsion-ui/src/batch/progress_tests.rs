use super::{doc, open};
use crate::workspace::Screen;
use gpui_kit::TestAppContext;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn batch_export_progress_stays_visible_and_stop_clears_it(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.screen = Screen::Batch;
            ws.batch.running = Some((0, 4));
            ws.batch.exporting = Some(format!("{}.jpg", "long-photo-name".repeat(30)).into());
            cx.notify();
        });
    });
    cx.run_until_parked();
    let initial_bounds = cx.update(|window, _| {
        let progress = window.find("batch-export-progress");
        assert!(progress.visible());
        assert!(window.find("batch-export-file").visible());
        assert!(window.find("batch-export-count").visible());
        assert!(window.find("batch-stop").visible());
        progress.bounds()
    });
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.batch.running = Some((2, 4));
            ws.batch.exporting = Some("photo-3.jpg".into());
            cx.notify();
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            window.find("batch-export-progress").bounds(),
            initial_bounds
        );
        let filename = window.find("batch-export-file").bounds();
        let count = window.find("batch-export-count").bounds();
        assert!(filename.right() <= count.left());
        window.click("batch-stop", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let batch = &ws.read(cx).batch;
        assert!(batch.running.is_none());
        assert!(batch.exporting.is_none());
        assert!(window.try_find("batch-export-progress").is_none());
        assert!(window.find("batch-run").visible());
    });
}
