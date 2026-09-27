//! Shared appearance popover for Home and every editor workspace.
use crate::{app_state, theme};
use emulsion_io::settings::{Accent, Corners};
use gpui_kit::component::{
    Selectable, Sizable,
    button::{Button, ButtonVariants},
    popover::Popover,
};
use gpui_kit::*;

pub(crate) fn control(cx: &App) -> impl IntoElement {
    let p = theme::palette(cx);
    Popover::new("appearance")
        .trigger(
            Button::new("appearance-button")
                .label("●")
                .text_color(p.accent)
                .tooltip("Appearance: accent and corners")
                .xsmall()
                .ghost(),
        )
        .content(|_, _, cx| {
            let p = theme::palette(cx);
            let settings = app_state::settings(cx);
            let selected_accent = settings.accent;
            let selected_corners = settings.corners;
            let following = theme::following_omarchy(cx);
            div()
                .id("appearance-panel")
                .test_support()
                .w(px(232.))
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .text_size(px(11.))
                .text_color(p.muted)
                .child("Accent")
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .children(Accent::ALL.map(|accent| {
                            Button::new(("appearance-accent", accent as usize))
                                .label("●")
                                .tooltip(accent.label())
                                .small()
                                .ghost()
                                .text_color(rgb(accent.rgb()))
                                .selected(!following && selected_accent == accent)
                                .on_click(move |_, _, cx| theme::set_accent(accent, cx))
                        })),
                )
                .child("Theme")
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .children([(true, "Dark"), (false, "Light")].map(|(dark, label)| {
                            Button::new(("appearance-theme", dark as usize))
                                .label(label)
                                .small()
                                .ghost()
                                .selected(!following && p.dark == dark)
                                .on_click(move |_, _, cx| theme::set_dark(dark, cx))
                        })),
                )
                .child("Corners")
                .child(div().flex().gap_1().children(Corners::ALL.map(|corners| {
                    Button::new(("appearance-corners", corners as usize))
                        .label(corners.label())
                        .small()
                        .ghost()
                        .selected(selected_corners == corners)
                        .on_click(move |_, _, cx| theme::set_corners(corners, cx))
                })))
                .into_any_element()
        })
}
