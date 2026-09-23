use super::*;
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
