//! Authoring and slide navigation without contacting YouTube or starting a browser.
use super::*;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn design_video_authoring_and_presentation_navigation(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(800, 600));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(800, 600)).unwrap(),
                "Video slides".into(),
                window,
                cx,
            );
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-youtube-add", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-youtube-url", cx));
    cx.simulate_input("https://youtu.be/M7lc1UVf-VE?t=30");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let (first, id, original) = cx.update(|window, cx| {
        let e = view.read(cx);
        let id = e.selected.unwrap();
        assert_eq!(e.editor.doc.design.media[&id].start_seconds, 30);
        let result = (e.editor.active_page(), id, e.editor.doc.clone());
        window.click("design-youtube-edit", cx);
        result
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-youtube-url", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("https://example.com/not-youtube");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        assert!(window.find("design-youtube-url").visible());
        window.press("escape", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.add_project_page(false, cx);
            e.select_page(first, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-present-now", cx));
    cx.run_until_parked();
    // A finished animation must hold the slide until the presenter advances it.
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(20));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.active_page(), first);
        assert!(window.find("design-presentation").visible());
        assert!(window.try_find("presentation-player-status").is_none());
        assert!(window.find(("design-video-play", id as usize)).visible());
        assert!(window.find("presentation-fullscreen").visible());
        assert!(window.find("presentation-auto-advance").visible());
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("presentation-next", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_ne!(view.read(cx).editor.active_page(), first);
        assert!(
            window
                .try_find(("design-video-play", id as usize))
                .is_none()
        );
        window.click("presentation-prev", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.active_page(), first);
        assert_eq!(view.read(cx).editor.doc, original);
        window.press("space", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_ne!(view.read(cx).editor.active_page(), first);
        window.press("shift-space", cx);
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(20));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.active_page(), first);
        window.click("presentation-auto-advance", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_ne!(view.read(cx).editor.active_page(), first);
        window.click("presentation-auto-advance", cx);
        window.click("presentation-prev", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("presentation-exit", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("design-presentation").is_none());
        assert!(window.find("design-present-now").visible());
        assert_eq!(view.read(cx).editor.doc, original);
    });
}
