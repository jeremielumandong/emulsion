//! The experimental layout switch must apply immediately and preserve editing state.
use super::*;
use crate::app_state;
use crate::workspace::Screen;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, WindowHandle, WindowOptions};

// These tests check the same process-local settings file. Serialize their writes;
// the full UI suite also runs serially because other settings/catalog tests share it.
static SETTINGS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
const SWITCH: &str = "experimental-layout-reuse";

fn show_settings(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(actions::ShowSettings), cx));
    cx.run_until_parked();
    cx.update(|window, _| {
        let switch = window.find(SWITCH);
        assert!(switch.visible());
        assert_eq!(switch.label(), Some("Reuse interface layout"));
    });
}

fn show_editor(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(actions::ShowEditor), cx));
    cx.run_until_parked();
}

fn click_switch(cx: &mut VisualTestContext) {
    let position = cx.update(|window, _| {
        window.activate_window();
        window.find(SWITCH).bounds().center()
    });
    cx.simulate_click(position, Modifiers::none());
    cx.run_until_parked();
}

fn assert_switch(cx: &mut VisualTestContext, enabled: bool) {
    cx.update(|window, cx| {
        let switch = window.find(SWITCH);
        assert_eq!(switch.checked(), Some(enabled));
        assert_eq!(switch.label(), Some("Reuse interface layout"));
        assert_eq!(app_state::layout_reuse_enabled(cx), enabled);
        assert_eq!(app_state::settings(cx).experimental_layout_reuse, enabled);
        assert_eq!(
            window.layout_reuse_stats().retained_nodes > 0,
            enabled,
            "the switch must affect the current window's next completed frame"
        );
    });
    assert_eq!(
        Settings::load().experimental_layout_reuse,
        enabled,
        "the choice must survive a later launch"
    );
}

fn activate_key(cx: &mut VisualTestContext, key: &str) {
    let keystroke = Keystroke::parse(key).unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn layout_setting_applies_both_ways_and_preserves_document_undo(cx: &mut TestAppContext) {
    let _guard = SETTINGS_TEST_LOCK.lock().unwrap();
    let original = doc(&["Photo"], None);
    let (workspace, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-shift-n"
    } else {
        "ctrl-shift-n"
    });
    let edited = cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.editor.doc.nodes.len(), 2);
        assert_eq!(editor.editor.history.len(), 1);
        editor.editor.doc.clone()
    });
    let initial = cx.update(|_, cx| app_state::layout_reuse_enabled(cx));
    show_settings(cx);
    for enabled in [!initial, initial] {
        click_switch(cx);
        assert_switch(cx, enabled);
        show_editor(cx);
        cx.update(|window, cx| {
            assert_eq!(workspace.read(cx).screen, Screen::Editor);
            assert_eq!(workspace.read(cx).editor.as_ref(), Some(&editor));
            assert_eq!(editor.read(cx).editor.doc, edited);
            assert_eq!(editor.read(cx).editor.history.len(), 1);
            assert_eq!(window.layout_reuse_stats().retained_nodes > 0, enabled);
        });
        show_settings(cx);
        assert_switch(cx, enabled);
    }
    show_editor(cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    });
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-shift-z"
    } else {
        "ctrl-shift-z"
    });
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, edited));
}

#[gpui_kit::test]
fn layout_setting_supports_keyboard_activation_after_pointer_focus(cx: &mut TestAppContext) {
    let _guard = SETTINGS_TEST_LOCK.lock().unwrap();
    let (_, cx) = open(cx, doc(&["Photo"], None));
    let initial = cx.update(|_, cx| app_state::layout_reuse_enabled(cx));
    show_settings(cx);
    click_switch(cx);
    assert_switch(cx, !initial);
    cx.update(|window, _| assert_eq!(window.find(SWITCH).focused(), Some(true)));
    activate_key(cx, "space");
    assert_switch(cx, initial);
    activate_key(cx, "enter");
    assert_switch(cx, !initial);
}

fn add_workspace_window(cx: &mut VisualTestContext) -> WindowHandle<Root> {
    let handle = cx.update(|_, cx| {
        cx.open_window(WindowOptions::default(), |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            cx.new(|cx| Root::new(workspace, window, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    handle
}

fn window_retains_layout(handle: WindowHandle<Root>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| {
        handle
            .update(cx, |_, window, _| {
                window.layout_reuse_stats().retained_nodes > 0
            })
            .unwrap()
    })
}

#[gpui_kit::test]
fn layout_setting_updates_existing_windows_and_is_inherited_by_new_windows(
    cx: &mut TestAppContext,
) {
    let _guard = SETTINGS_TEST_LOCK.lock().unwrap();
    let (_, cx) = open(cx, doc(&["Photo"], None));
    let initial = cx.update(|_, cx| app_state::layout_reuse_enabled(cx));
    let second = add_workspace_window(cx);
    assert_eq!(window_retains_layout(second, cx), initial);
    show_settings(cx);
    click_switch(cx);
    assert_switch(cx, !initial);
    assert_eq!(
        window_retains_layout(second, cx),
        !initial,
        "changing the setting must refresh other existing windows"
    );
    let third = add_workspace_window(cx);
    assert_eq!(
        window_retains_layout(third, cx),
        !initial,
        "new windows must inherit the session choice rather than reapplying startup environment"
    );
    click_switch(cx);
    assert_switch(cx, initial);
    assert_eq!(window_retains_layout(second, cx), initial);
    assert_eq!(window_retains_layout(third, cx), initial);
}
