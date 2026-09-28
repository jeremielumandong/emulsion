use super::*;
use core::prelude::v1::test;

#[gpui_kit::test]
fn command_toolbar_wraps_with_many_external_tab_stops_and_skips_disabled(cx: &mut TestAppContext) {
    use gpui_kit::component::{Disableable as _, Root, button::Button};
    use gpui_kit::test::TestWindowExt;
    struct ToolbarHarness;
    impl Render for ToolbarHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .child(
                    command_bar("commands", "Commands")
                        .child(Button::new("first").label("First"))
                        .child(
                            Button::new("disabled-command")
                                .label("Disabled")
                                .disabled(true),
                        )
                        .child(Button::new("last").label("Last")),
                )
                .children((0usize..150).map(|i| {
                    div()
                        .id(("external", i))
                        .focusable()
                        .tab_index(0)
                        .size(px(1.))
                }))
        }
    }
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| ToolbarHarness);
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.focus_next(cx);
        window.render_frame(cx);
        assert_eq!(window.find("first").focused(), Some(true));
        window.press("left", cx);
        assert_eq!(window.find("last").focused(), Some(true));
        window.press("right", cx);
        assert_eq!(window.find("first").focused(), Some(true));
        window.press("right", cx);
        assert_eq!(window.find("last").focused(), Some(true));
    });
}

struct Controls {
    clicks: Rc<[Cell<usize>; 4]>,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = crate::theme::dark();
        let handler = |index: usize| {
            let clicks = self.clicks.clone();
            move |_: &ClickEvent, _: &mut Window, _: &mut App| {
                clicks[index].set(clicks[index].get() + 1);
            }
        };
        div()
            .flex()
            .flex_col()
            .w(px(200.))
            .child(chip_action("disabled", "Unavailable", false, false, &p, handler(0)).h(px(40.)))
            .child(
                button("button", "Button", false, &p)
                    .h(px(40.))
                    .on_click(handler(1)),
            )
            .child(
                chip("chip", "Chip", false, &p)
                    .h(px(40.))
                    .on_click(handler(2)),
            )
            .child(chip_action("action", "Action", false, true, &p, handler(3)).h(px(40.)))
    }
}

#[gpui_kit::test]
fn controls_tab_past_disabled_action_and_activate_once_per_key(cx: &mut TestAppContext) {
    let clicks = Rc::new(std::array::from_fn(|_| Cell::new(0)));
    let window: AnyWindowHandle = cx
        .add_window({
            let clicks = clicks.clone();
            move |_, _| Controls { clicks }
        })
        .into();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    for index in 1..4 {
        cx.update_window(window, |_, window, cx| {
            window.focus_next(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
        for key in ["enter", "space"] {
            // GPUI's keystroke helper only dispatches KeyDown. Clickable
            // controls activate on release, matching physical key presses.
            cx.simulate_keystrokes(window, key);
            cx.update_window(window, |_, window, cx| {
                window.dispatch_event(
                    KeyUpEvent {
                        keystroke: Keystroke::parse(key).unwrap(),
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        }
        assert_eq!(
            clicks[index].get(),
            2,
            "Enter and Space activate exactly once"
        );
        assert_eq!(clicks[0].get(), 0, "disabled action is skipped");
    }
    cx.update_window(window, |_, window, cx| {
        let position = point(px(20.), px(20.));
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::none(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    assert_eq!(clicks[0].get(), 0, "disabled action has no mouse listener");
}
