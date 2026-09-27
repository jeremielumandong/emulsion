//! Authoring and slide navigation without contacting YouTube or starting a browser.
use super::*;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use gpui_kit::test::TestWindowExt;

#[cfg(target_os = "linux")]
fn changed_video_view_stops_safely(cx: &mut TestAppContext, rotate: bool) {
    let _helper = crate::web_player::test_helper(b"#!/bin/sh\nwhile read command; do :; done\n");
    let mut editor = emulsion_core::Editor::new(Document::new(800, 600), None);
    let id = emulsion_core::design::media::insert_youtube(
        &mut editor,
        "https://youtu.be/M7lc1UVf-VE",
        (80., 80.),
        (640., 360.),
    )
    .unwrap();
    let original = editor.doc.clone();
    let (ws, cx) = open(cx, Document::new(800, 600));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, editor.doc).unwrap(),
                "Player resize".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-present-now", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-video-play", id as usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("presentation-stop-video").visible());
        view.update(cx, |e, cx| {
            if rotate {
                e.view.rotation = 15.;
            } else {
                e.view.zoom = 0.1;
            }
            e.notify_canvas(cx);
        });
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(40));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-presentation").visible());
        assert!(window.try_find("presentation-stop-video").is_none());
        if !rotate {
            assert!(window.find("presentation-player-status").visible());
        }
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("presentation-exit", cx);
    });
    cx.run_until_parked();
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn shrinking_video_view_stops_playback_without_panicking(cx: &mut TestAppContext) {
    changed_video_view_stops_safely(cx, false);
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn rotating_video_view_stops_playback_without_panicking(cx: &mut TestAppContext) {
    changed_video_view_stops_safely(cx, true);
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn failed_video_helper_keeps_presentation_and_document_open(cx: &mut TestAppContext) {
    // Exercise the real subprocess/EOF path without depending on codecs or YouTube.
    let _helper = crate::web_player::test_helper(b"#!/bin/sh\nexit 1\n");
    let mut editor = emulsion_core::Editor::new(Document::new(800, 600), None);
    let id = emulsion_core::design::media::insert_youtube(
        &mut editor,
        "https://youtu.be/M7lc1UVf-VE",
        (80., 80.),
        (640., 360.),
    )
    .unwrap();
    let original = editor.doc.clone();
    let (ws, cx) = open(cx, Document::new(800, 600));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, editor.doc).unwrap(),
                "Player failure".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-present-now", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-video-play", id as usize), cx));
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(40));
        cx.run_until_parked();
        if cx.update(|_, cx| {
            view.read(cx)
                .status
                .as_ref()
                .is_some_and(|(_, error)| *error)
        }) {
            break;
        }
    }
    cx.update(|window, cx| {
        assert!(window.find("design-presentation").visible());
        assert!(window.find("presentation-player-status").visible());
        assert!(window.try_find("presentation-stop-video").is_none());
        assert!(window.find(("design-video-play", id as usize)).visible());
        assert_eq!(view.read(cx).editor.doc, original);
        assert!(view.read(cx).canvas_focus.is_focused(window));
        window.press("escape", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-present-now").visible());
        assert_eq!(view.read(cx).editor.doc, original);
    });
}

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
