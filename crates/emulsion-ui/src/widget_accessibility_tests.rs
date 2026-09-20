use super::*;
use core::prelude::v1::test;

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
