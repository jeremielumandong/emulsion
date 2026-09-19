//! Small square widgets in the Emulsion style.

use crate::theme::{MONO_FONT, Palette};
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
    let (bg, fg) = if primary {
        (p.ink, p.paper)
    } else {
        (p.soft_bg, p.ink)
    };
    let accent = p.accent;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .px(px(14.))
        .py(px(7.))
        .border_1()
        .border_color(p.ink)
        .bg(bg)
        .text_color(fg)
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| {
            s.bg(accent)
                .border_color(accent)
                .text_color(gpui_kit::white())
        })
        .child(text.into())
}

/// A small mono-label chip button.
pub fn chip(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    on: bool,
    p: &Palette,
) -> Stateful<Div> {
    let (bg, fg, border) = if on {
        (p.ink, p.paper, p.ink)
    } else {
        (p.soft_bg, p.ink, p.line)
    };
    let ink = p.ink;
    div()
        .id(id)
        .px(px(8.))
        .py(px(3.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .font_family(MONO_FONT)
        .text_size(px(10.))
        .cursor_pointer()
        .hover(move |s| s.border_color(ink))
        .child(text.into())
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
        .h(px(14.))
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
                .top(px(6.))
                .left_0()
                .right_0()
                .h(px(2.))
                .bg(p.line),
        )
        .child(
            div()
                .absolute()
                .top(px(6.))
                .left_0()
                .h(px(2.))
                .w(relative(v))
                .bg(p.accent),
        )
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(relative(v))
                .ml(px(-5.))
                .size(px(10.))
                .bg(p.panel)
                .border_1()
                .border_color(p.ink),
        )
}

/// Map a pointer x position to [0,1] along a track.
pub fn track_fraction(track: &TrackBounds, x: Pixels) -> Option<f32> {
    let b = track.get()?;
    let w = f32::from(b.size.width).max(1.0);
    Some(((f32::from(x - b.origin.x)) / w).clamp(0.0, 1.0))
}
