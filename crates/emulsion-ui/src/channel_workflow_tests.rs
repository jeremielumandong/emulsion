use super::*;
use crate::editor::channels::ChannelView;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn channel_preview_is_per_document_and_never_changes_pixels_or_history(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    let source = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| window.click("dock-channels", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("channel-red", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = source.read(cx);
        assert_eq!(e.channels.view, ChannelView::Red);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
        assert!(e.canvas_focus.is_focused(window));
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(64, 64),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            )
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            ws.read(cx).editor.as_ref().unwrap().read(cx).channels.view,
            ChannelView::Rgb
        );
        source.update(cx, |e, cx| {
            e.select_channel(ChannelView::Rgb, cx);
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
        });
    });
}
