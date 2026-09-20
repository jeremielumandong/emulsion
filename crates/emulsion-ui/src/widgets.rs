//! Small square widgets in the Emulsion style.

use crate::theme::{MONO_FONT, Palette};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;

/// Monospace metadata text.
pub fn mono(text: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    div()
        .font_family(MONO_FONT)
        .text_size(px(size))
        .text_color(color)
        .child(text.into())
}

/// Uppercase section label.
pub fn label(text: impl Into<SharedString>, p: &Palette) -> Div {
    let t: SharedString = text.into();
    mono(t.to_uppercase(), 9.5, p.muted)
}

/// A square text button.
pub fn button(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    primary: bool,
    p: &Palette,
) -> Stateful<Div> {
    let text = text.into();
    let (bg, fg) = if primary {
        (p.ink, p.paper)
    } else {
        (p.soft_bg, p.ink)
    };
    let accent = p.accent;
    let accent_fg = p.accent_fg;
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(text.clone())
        .focusable()
        .tab_index(0)
        .focus_visible(move |s| s.border_color(accent).bg(accent).text_color(accent_fg))
        .flex()
        .items_center()
        .justify_center()
        .px(px(14.))
        .py(px(7.))
        .min_h(px(32.))
        .min_w(px(32.))
        .border_1()
        .border_color(p.ink)
        .bg(bg)
        .text_color(fg)
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(accent).border_color(accent).text_color(accent_fg))
        .child(text)
}

/// A small mono-label chip button.
pub fn chip(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    on: bool,
    p: &Palette,
) -> Stateful<Div> {
    chip_base(id, text, on, true, p)
}

fn chip_base(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    on: bool,
    enabled: bool,
    p: &Palette,
) -> Stateful<Div> {
    let text = text.into();
    let (bg, fg, border) = if on {
        (p.ink, p.paper, p.ink)
    } else {
        (p.soft_bg, p.ink, p.line)
    };
    let ink = p.ink;
    let accent = p.accent;
    let accent_fg = p.accent_fg;
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(text.clone())
        // `on` is also used for emphasized actions, so it cannot safely be
        // announced as a toggle state. Toggle callers may add aria_toggled.
        .when(enabled, |d| {
            d.focusable()
                .tab_index(0)
                .focus_visible(move |s| s.border_color(accent).bg(accent).text_color(accent_fg))
                .cursor_pointer()
                .hover(move |s| s.border_color(ink))
        })
        .flex()
        .items_center()
        .justify_center()
        .min_h(px(24.))
        .min_w(px(24.))
        .px(px(8.))
        .py(px(3.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .font_family(MONO_FONT)
        .text_size(px(10.))
        .when(!enabled, |d| {
            d.bg(p.paper)
                .text_color(p.muted)
                .border_color(p.line)
                .cursor(CursorStyle::Arrow)
                // GPUI currently has no disabled-state accessibility API.
                .aria_description("Unavailable")
        })
        .child(text)
}

/// A prerequisite-dependent action. Disabled actions have no mouse, keyboard,
/// or accessibility click handler, and do not enter the tab order. Attach a
/// tooltip explaining the missing prerequisite at the call site.
pub fn chip_action(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    on: bool,
    enabled: bool,
    p: &Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    chip_base(id, text, on, enabled, p).when(enabled, |d| d.on_click(on_click))
}

/// Attach a plain-text tooltip to an element, for a first-time user who
/// wants to know what a chip does before clicking it.
pub fn tip<E: InteractiveElement>(el: E, text: &'static str) -> E {
    let mut el = el;
    el.interactivity()
        .tooltip(move |w, cx| gpui_kit::component::tooltip::Tooltip::new(text).build(w, cx));
    el
}

/// Shared slot for a slider's track bounds, filled during layout.
pub type TrackBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

/// A square slider: 2 px track, accent fill, square thumb. `value` is the
/// normalised position in [0,1]. Pointer handling is done by the owner,
/// which knows how to map the position back to a value.
pub fn slider(
    id: impl Into<ElementId>,
    value: f32,
    track: TrackBounds,
    p: &Palette,
    on_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let v = value.clamp(0.0, 1.0);
    let t2 = track.clone();
    div()
        .id(id)
        .relative()
        .h(px(24.))
        .w_full()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(MouseButton::Left, on_down)
        .child(
            canvas(move |b, _, _| t2.set(Some(b)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .top(px(11.))
                .left_0()
                .right_0()
                .h(px(2.))
                .bg(p.line),
        )
        .child(
            div()
                .absolute()
                .top(px(11.))
                .left_0()
                .h(px(2.))
                .w(relative(v))
                .bg(p.accent),
        )
        .child(
            div()
                .absolute()
                .top(px(7.))
                .left(relative(v))
                .ml(px(-5.))
                .size(px(10.))
                .bg(p.panel)
                .border_1()
                .border_color(p.ink),
        )
}

/// A vertical slider (Procreate's side sliders): full height of its box,
/// filled from the bottom. `value` is the normalised position in [0,1].
pub fn vslider(
    id: impl Into<ElementId>,
    value: f32,
    track: TrackBounds,
    p: &Palette,
    on_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let v = value.clamp(0.0, 1.0);
    let t2 = track.clone();
    div()
        .id(id)
        .relative()
        .w(px(24.))
        .h_full()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(MouseButton::Left, on_down)
        .child(
            canvas(move |b, _, _| t2.set(Some(b)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .left(px(11.))
                .top_0()
                .bottom_0()
                .w(px(2.))
                .bg(p.line),
        )
        .child(
            div()
                .absolute()
                .left(px(11.))
                .bottom_0()
                .w(px(2.))
                .h(relative(v))
                .bg(p.accent),
        )
        .child(
            div()
                .absolute()
                .left(px(6.))
                .bottom(relative(v))
                .mb(px(-6.))
                .size(px(12.))
                .bg(p.panel)
                .border_1()
                .border_color(p.ink),
        )
}

/// Map a pointer y position to [0,1] up a vertical track (bottom = 0).
pub fn track_fraction_v(track: &TrackBounds, y: Pixels) -> Option<f32> {
    let b = track.get()?;
    let h = f32::from(b.size.height).max(1.0);
    Some((1.0 - f32::from(y - b.origin.y) / h).clamp(0.0, 1.0))
}

/// Map a pointer x position to [0,1] along a track.
pub fn track_fraction(track: &TrackBounds, x: Pixels) -> Option<f32> {
    let b = track.get()?;
    let w = f32::from(b.size.width).max(1.0);
    Some(((f32::from(x - b.origin.x)) / w).clamp(0.0, 1.0))
}

#[cfg(test)]
#[path = "widget_accessibility_tests.rs"]
mod accessibility_tests;
