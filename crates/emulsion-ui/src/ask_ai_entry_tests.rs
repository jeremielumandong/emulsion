use super::*;
use crate::workspace::Screen;
use gpui_kit::test::TestWindowExt;

/// A first-run workspace: the Ask AI hint has not been closed yet.
fn first_run(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|_, cx| cx.global_mut::<AppSettings>().0.ai_hint_dismissed = false);
    cx.run_until_parked();
    (ws, cx)
}

#[gpui_kit::test]
fn ask_ai_hint_opens_the_ask_bar_and_stays_dismissed(cx: &mut TestAppContext) {
    let (ws, cx) = first_run(cx);
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    // Clicking the hint's button fails the test if the hint is missing.
    cx.update(|window, cx| window.click("ask-ai-hint-open", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(e.read(cx).ask.is_some());
        assert!(crate::app_state::settings(cx).ai_hint_dismissed);
        assert!(window.try_find("ask-ai-hint").is_none());
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(e.read(cx).ask.is_none());
        assert!(window.try_find("ask-ai-hint").is_none());
    });
}

#[gpui_kit::test]
fn ask_ai_button_toggles_the_ask_bar_and_hint_closes(cx: &mut TestAppContext) {
    let (ws, cx) = first_run(cx);
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ask-ai-hint-close", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(crate::app_state::settings(cx).ai_hint_dismissed);
        assert!(window.try_find("ask-ai-hint").is_none());
        assert!(e.read(cx).ask.is_none());
    });
    cx.update(|window, cx| window.click("ask-ai-button", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert!(e.read(cx).ask.is_some()));
    cx.update(|window, cx| window.click("ask-ai-button", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert!(e.read(cx).ask.is_none()));
}

#[gpui_kit::test]
fn f1_asks_and_ctrl_f_finds_layers(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo", "Sky"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.simulate_keystrokes("f1");
    cx.run_until_parked();
    cx.update(|_, cx| assert!(e.read(cx).ask.is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| assert!(e.read(cx).ask.is_none()));
    cx.simulate_keystrokes("alt-f1");
    cx.run_until_parked();
    cx.update(|_, cx| assert!(e.read(cx).ask.is_some()));
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    cx.simulate_input("sky");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(e.ask.is_none());
        assert_eq!(e.layer_panel.query, "sky");
        assert_eq!(e.filtered_layer_rows().len(), 1);
    });
}

#[gpui_kit::test]
fn library_assistant_shortcuts_and_button_share_host_without_switching_diagram(
    cx: &mut TestAppContext,
) {
    let diagram = emulsion_core::diagram::Builder::new(800, 600)
        .unwrap()
        .finish()
        .unwrap();
    let (ws, cx) = open(cx, diagram);
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| ws.set_screen(Screen::Batch, window, cx));
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let tabs = cx.update(|_, cx| ws.read(cx).tabs.len());
    cx.simulate_keystrokes("f1");
    cx.run_until_parked();
    let host = cx.update(|_, cx| {
        let ws = ws.read(cx);
        assert_eq!(ws.screen, Screen::Batch);
        assert!(editor.read(cx).ask.is_none());
        ws.batch.assistant_host.clone().unwrap()
    });
    cx.simulate_input("Help with these photos");
    cx.simulate_keystrokes("alt-f1");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let ws = ws.read(cx);
        assert_eq!(ws.screen, Screen::Batch);
        assert_eq!(ws.tabs.len(), tabs);
        assert_eq!(ws.batch.assistant_host.as_ref(), Some(&host));
        let assistant = host.read(cx);
        assert!(assistant.library_only);
        assert_eq!(
            assistant
                .ask
                .as_ref()
                .unwrap()
                .state
                .read(cx)
                .value()
                .as_ref(),
            "Help with these photos"
        );
        window.click("library-assistant", cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(host.read(cx).ask.is_none());
        assert!(host.read(cx).canvas_focus.is_focused(window));
        assert_eq!(ws.read(cx).screen, Screen::Batch);
        assert!(editor.read(cx).ask.is_none());
    });
    cx.simulate_keystrokes("alt-f1");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(host.read(cx).ask.is_some());
        assert_eq!(ws.read(cx).batch.assistant_host.as_ref(), Some(&host));
        ws.update(cx, |ws, cx| ws.activate_tab(0, window, cx));
    });
    cx.simulate_keystrokes("f1");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(ws.read(cx).screen, Screen::Editor);
        assert!(editor.read(cx).ask.is_some());
    });
}

#[gpui_kit::test]
fn library_assistant_shortcuts_work_without_a_document_tab(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.tabs.clear();
            ws.editor = None;
            ws.set_screen(Screen::Batch, window, cx);
        })
    });
    cx.run_until_parked();
    for key in ["f1", "alt-f1"] {
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
        cx.update(|_, cx| {
            let ws = ws.read(cx);
            assert_eq!(ws.screen, Screen::Batch);
            assert!(ws.tabs.is_empty());
            assert!(ws.editor.is_none());
            assert!(
                ws.batch
                    .assistant_host
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .ask
                    .is_some()
            );
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
}
