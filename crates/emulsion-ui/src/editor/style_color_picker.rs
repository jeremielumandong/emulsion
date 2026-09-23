//! Application-owned HSV, RGB, and approximate CMYK color selection, sharing the existing native color state.
use super::*;
use emulsion_raster::color::{cmyk_to_rgb, rgb_to_cmyk};
use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState};
use gpui_kit::component::{Sizable, button::Button};
use std::cell::Cell;

const FIELD_IDS: [&str; 11] = [
    "h",
    "s",
    "b",
    "r",
    "g",
    "b-channel",
    "alpha",
    "cyan",
    "magenta",
    "yellow",
    "black",
];
const FIELD_NAMES: [&str; 11] = [
    "Hue",
    "Saturation",
    "Brightness",
    "Red",
    "Green",
    "Blue",
    "Opacity",
    "Cyan %",
    "Magenta %",
    "Yellow %",
    "Black %",
];
const LIMITS: [f32; 11] = [
    360., 100., 100., 255., 255., 255., 100., 100., 100., 100., 100.,
];
const SWATCHES: [[u8; 3]; 12] = [
    [0, 0, 0],
    [255, 255, 255],
    [128, 128, 128],
    [255, 0, 0],
    [255, 128, 0],
    [255, 255, 0],
    [0, 180, 80],
    [0, 255, 255],
    [0, 100, 255],
    [90, 40, 200],
    [255, 0, 255],
    [140, 75, 35],
];
#[derive(Clone, Copy, PartialEq)]
enum DragPart {
    Sv,
    Hue,
}

pub(super) struct StyleColorPicker {
    state: Entity<ColorPickerState>,
    original: Hsla,
    hsv: [f32; 3],
    alpha: f32,
    cmyk: [f32; 4],
    fields: Vec<Entity<InputState>>,
    expected: Vec<String>,
    invalid: Option<usize>,
    hex_invalid: bool,
    drag: Option<DragPart>,
    sv_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    hue_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    sv_focus: FocusHandle,
    hue_focus: FocusHandle,
    pending_field: Option<usize>,
    _subscriptions: Vec<Subscription>,
}
impl StyleColorPicker {
    pub(super) fn new(
        state: Entity<ColorPickerState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        state.update(cx, |state, cx| state.sync_pending_value(window, cx));
        let original = state.read(cx).value().unwrap_or_else(gpui_kit::black);
        let rgb = original.to_rgb();
        let hsv = rgb_to_hsv([rgb.r, rgb.g, rgb.b], 0.);
        let cmyk = rgb_to_cmyk([rgb.r, rgb.g, rgb.b]);
        let values = field_values(hsv, [rgb.r, rgb.g, rgb.b], rgb.a, cmyk);
        let fields: Vec<_> = values
            .iter()
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v.clone())))
            .collect();
        let mut subscriptions = Vec::new();
        for (index, field) in fields.iter().enumerate() {
            subscriptions.push(cx.subscribe_in(
                field,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                        this.edit_field(index, window, cx);
                    }
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(
            &state,
            window,
            |this, _, event: &ColorPickerEvent, window, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    let rgb = color.to_rgb();
                    // Keep the user's ink separation when our own RGB preview
                    // comes back through the shared state (including alpha edits).
                    if !same_rgb(cmyk_to_rgb(this.cmyk), [rgb.r, rgb.g, rgb.b]) {
                        this.cmyk = rgb_to_cmyk([rgb.r, rgb.g, rgb.b]);
                    }
                    let next = rgb_to_hsv([rgb.r, rgb.g, rgb.b], this.hsv[0]);
                    this.hsv = if next[2] == 0. {
                        [this.hsv[0], this.hsv[1], 0.]
                    } else {
                        next
                    };
                    this.alpha = rgb.a;
                    let skip = this.pending_field.take();
                    this.sync_fields(skip, window, cx);
                    cx.notify();
                }
            },
        ));
        let hex = state.read(cx).hex_input().clone();
        subscriptions.push(cx.subscribe_in(
            &hex,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // State synchronization also writes this shared field. Only
                    // an active text edit may drive color back from its value.
                    if !input.read(cx).focus_handle(cx).is_focused(window) {
                        return;
                    }
                    let text = input.read(cx).value().to_string();
                    if let Some(color) = parse_hex(&text, this.alpha) {
                        this.hex_invalid = false;
                        let different = this
                            .state
                            .read(cx)
                            .value()
                            .is_none_or(|old| rgba_bytes(old) != rgba_bytes(color));
                        if different {
                            this.state.update(cx, |state, cx| {
                                state.update_color_preserving_hex(color, window, cx)
                            });
                        }
                    } else {
                        this.hex_invalid = true;
                    }
                    cx.notify();
                }
            },
        ));
        Self {
            state,
            original,
            hsv,
            alpha: rgb.a,
            cmyk,
            fields,
            expected: values,
            invalid: None,
            hex_invalid: false,
            drag: None,
            sv_bounds: Rc::new(Cell::new(None)),
            hue_bounds: Rc::new(Cell::new(None)),
            sv_focus: cx.focus_handle(),
            hue_focus: cx.focus_handle(),
            pending_field: None,
            _subscriptions: subscriptions,
        }
    }
    fn color(&self) -> Hsla {
        let [r, g, b] = hsv_to_rgb(self.hsv);
        Rgba {
            r,
            g,
            b,
            a: self.alpha,
        }
        .into()
    }
    fn sync_fields(&mut self, skip: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let rgb = hsv_to_rgb(self.hsv);
        let values = field_values(self.hsv, rgb, self.alpha, self.cmyk);
        for (index, value) in values.into_iter().enumerate() {
            if skip == Some(index)
                || self.fields[index]
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            {
                continue;
            }
            self.expected[index] = value.clone();
            if self.fields[index].read(cx).value().as_str() != value {
                self.fields[index].update(cx, |field, cx| field.set_value(value, window, cx));
            }
        }
    }
    fn publish(&mut self, skip: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if skip.is_none_or(|index| index < 6) {
            self.cmyk = rgb_to_cmyk(hsv_to_rgb(self.hsv));
        }
        self.invalid = None;
        self.hex_invalid = false;
        self.sync_fields(skip, window, cx);
        let color = self.color();
        self.pending_field = skip;
        self.state
            .update(cx, |state, cx| state.update_color(color, window, cx));
        cx.notify();
    }
    fn edit_field(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.fields[index].read(cx).value().to_string();
        if text == self.expected[index] {
            return;
        }
        let Some(value) = number(&text, LIMITS[index]) else {
            self.invalid = Some(index);
            cx.notify();
            return;
        };
        self.expected[index] = text;
        match index {
            0 => self.hsv[0] = value / 360.,
            1 => self.hsv[1] = value / 100.,
            2 => self.hsv[2] = value / 100.,
            3..=5 => {
                let mut rgb = hsv_to_rgb(self.hsv);
                rgb[index - 3] = value / 255.;
                self.hsv = rgb_to_hsv(rgb, self.hsv[0]);
            }
            6 => self.alpha = value / 100.,
            7..=10 => {
                self.cmyk[index - 7] = value / 100.;
                self.hsv = rgb_to_hsv(cmyk_to_rgb(self.cmyk), self.hsv[0]);
            }
            _ => unreachable!(),
        }
        self.publish(Some(index), window, cx);
    }
    pub(super) fn commit_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        for (index, limit) in LIMITS.iter().copied().enumerate() {
            if number(self.fields[index].read(cx).value().as_str(), limit).is_none() {
                self.invalid = Some(index);
                cx.notify();
                return false;
            }
        }
        let text = self.state.read(cx).hex_input().read(cx).value().to_string();
        let Some(color) = parse_hex(&text, self.alpha) else {
            self.hex_invalid = true;
            cx.notify();
            return false;
        };
        self.state
            .update(cx, |state, cx| state.update_color(color, window, cx));
        self.drag = None;
        true
    }
    fn pick(
        &mut self,
        part: DragPart,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = match part {
            DragPart::Sv => self.sv_bounds.get(),
            DragPart::Hue => self.hue_bounds.get(),
        };
        let Some(bounds) = bounds else {
            return;
        };
        let x =
            (f32::from(position.x - bounds.left()) / f32::from(bounds.size.width)).clamp(0., 1.);
        let y =
            (f32::from(position.y - bounds.top()) / f32::from(bounds.size.height)).clamp(0., 1.);
        match part {
            DragPart::Sv => {
                self.hsv[1] = x;
                self.hsv[2] = 1. - y;
            }
            DragPart::Hue => self.hsv[0] = y.min(0.999999),
        }
        self.publish(None, window, cx);
    }
    fn keyboard(
        &mut self,
        part: DragPart,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = if event.keystroke.modifiers.shift {
            0.1
        } else {
            0.01
        };
        let key = event.keystroke.key.as_str();
        match (part, key) {
            (DragPart::Sv, "left") => self.hsv[1] = (self.hsv[1] - step).max(0.),
            (DragPart::Sv, "right") => self.hsv[1] = (self.hsv[1] + step).min(1.),
            (DragPart::Sv, "up") => self.hsv[2] = (self.hsv[2] + step).min(1.),
            (DragPart::Sv, "down") => self.hsv[2] = (self.hsv[2] - step).max(0.),
            (DragPart::Hue, "up" | "left") => self.hsv[0] = (self.hsv[0] - step).rem_euclid(1.),
            (DragPart::Hue, "down" | "right") => self.hsv[0] = (self.hsv[0] + step).rem_euclid(1.),
            (_, "home") => match part {
                DragPart::Sv => {
                    self.hsv[1] = 0.;
                    self.hsv[2] = 1.;
                }
                DragPart::Hue => self.hsv[0] = 0.,
            },
            (_, "end") => match part {
                DragPart::Sv => {
                    self.hsv[1] = 1.;
                    self.hsv[2] = 0.;
                }
                DragPart::Hue => self.hsv[0] = 0.999999,
            },
            _ => return,
        }
        cx.stop_propagation();
        self.publish(None, window, cx);
    }
    fn field(&self, index: usize) -> impl IntoElement {
        div()
            .id(SharedString::from(format!(
                "style-color-{}",
                FIELD_IDS[index]
            )))
            .flex()
            .items_center()
            .gap_2()
            .child(div().w_20().text_xs().child(FIELD_NAMES[index]))
            .child(Input::new(&self.fields[index]).small().w_20())
            .test_support()
    }
}
impl Render for StyleColorPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.state
            .update(cx, |state, cx| state.sync_pending_value(window, cx));
        let p = theme::palette(cx);
        let hue: Hsla = {
            let [r, g, b] = hsv_to_rgb([self.hsv[0], 1., 1.]);
            Rgba { r, g, b, a: 1. }.into()
        };
        let sv_bounds = self.sv_bounds.clone();
        let hue_bounds = self.hue_bounds.clone();
        let weak_move = cx.weak_entity();
        let weak_up = cx.weak_entity();
        let sv = div()
            .id("style-color-sv")
            .relative()
            .size(px(280.))
            .flex_none()
            .track_focus(&self.sv_focus.clone().tab_stop(true))
            .border_1()
            .border_color(if self.sv_focus.is_focused(window) {
                p.accent
            } else {
                p.line
            })
            .bg(hue)
            .cursor_crosshair()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.sv_focus, cx);
                    this.drag = Some(DragPart::Sv);
                    this.pick(DragPart::Sv, event.position, window, cx);
                }),
            )
            .on_key_down(
                cx.listener(|this, event, window, cx| {
                    this.keyboard(DragPart::Sv, event, window, cx)
                }),
            )
            .child(div().absolute().size_full().bg(linear_gradient(
                90.,
                linear_color_stop(gpui_kit::white(), 0.),
                linear_color_stop(gpui_kit::white().opacity(0.), 1.),
            )))
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(gpui_kit::black().opacity(0.), 0.),
                linear_color_stop(gpui_kit::black(), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(relative(self.hsv[1]))
                    .top(relative(1. - self.hsv[2]))
                    .ml(px(-5.))
                    .mt(px(-5.))
                    .size(px(10.))
                    .rounded_full()
                    .border_2()
                    .border_color(gpui_kit::white())
                    .bg(gpui_kit::black().opacity(0.15)),
            )
            .child(
                canvas(
                    move |bounds, _, _| sv_bounds.set(Some(bounds)),
                    move |_, _, window, _| {
                        let weak = weak_move.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                weak.update(cx, |this, cx| {
                                    if let Some(part) = this.drag {
                                        if event.pressed_button == Some(MouseButton::Left) {
                                            this.pick(part, event.position, window, cx);
                                        } else {
                                            this.drag = None;
                                            cx.notify();
                                        }
                                    }
                                })
                                .ok();
                            }
                        });
                        let weak = weak_up.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && event.button == MouseButton::Left
                            {
                                weak.update(cx, |this, cx| {
                                    this.drag = None;
                                    cx.notify();
                                })
                                .ok();
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .test_support();
        let mut hue_strip = div()
            .id("style-color-hue")
            .relative()
            .w_6()
            .h(px(280.))
            .flex_none()
            .track_focus(&self.hue_focus.clone().tab_stop(true))
            .border_1()
            .border_color(if self.hue_focus.is_focused(window) {
                p.accent
            } else {
                p.line
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.hue_focus, cx);
                    this.drag = Some(DragPart::Hue);
                    this.pick(DragPart::Hue, event.position, window, cx);
                }),
            )
            .on_key_down(cx.listener(|this, event, window, cx| {
                this.keyboard(DragPart::Hue, event, window, cx)
            }));
        for index in 0..6 {
            let color = |h| {
                let [r, g, b] = hsv_to_rgb([h, 1., 1.]);
                Hsla::from(Rgba { r, g, b, a: 1. })
            };
            hue_strip = hue_strip.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(index as f32 / 6.))
                    .h(relative(1. / 6. + 0.002))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(color(index as f32 / 6.), 0.),
                        linear_color_stop(color((index + 1) as f32 / 6.), 1.),
                    )),
            );
        }
        hue_strip = hue_strip
            .child(
                div()
                    .absolute()
                    .left(px(-2.))
                    .right(px(-2.))
                    .top(relative(self.hsv[0]))
                    .h(px(4.))
                    .mt(px(-2.))
                    .border_1()
                    .border_color(gpui_kit::white())
                    .bg(gpui_kit::black()),
            )
            .child(
                canvas(
                    move |bounds, _, _| hue_bounds.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        let current = self.color();
        let previews = div()
            .flex()
            .gap_3()
            .child(
                div().flex().flex_col().gap_1().child("New").child(
                    div()
                        .id("style-color-current")
                        .w_16()
                        .h_10()
                        .bg(current)
                        .border_1()
                        .border_color(p.line)
                        .test_support(),
                ),
            )
            .child(
                div().flex().flex_col().gap_1().child("Current").child(
                    Button::new("style-color-old")
                        .label(" ")
                        .w_16()
                        .h_10()
                        .bg(self.original)
                        .on_click(cx.listener(|this, _, window, cx| {
                            window.focus(&this.sv_focus, cx);
                            let original = this.original;
                            this.state
                                .update(cx, |state, cx| state.update_color(original, window, cx));
                        })),
                ),
            );
        let inputs = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(previews)
            .children((0..7).map(|i| self.field(i)))
            .child(
                div()
                    .id("style-color-hex")
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w_20().text_xs().child("Hex"))
                    .child(Input::new(self.state.read(cx).hex_input()).small().w_20())
                    .test_support(),
            );
        let mut swatches = div().flex().gap_1();
        for (index, channels) in SWATCHES.iter().copied().enumerate() {
            let color = Rgba {
                r: channels[0] as f32 / 255.,
                g: channels[1] as f32 / 255.,
                b: channels[2] as f32 / 255.,
                a: 1.,
            };
            swatches = swatches.child(
                Button::new(SharedString::from(format!("style-color-swatch-{index}")))
                    .label(" ")
                    .w_6()
                    .h_6()
                    .bg(color)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.sv_focus, cx);
                        this.hsv = rgb_to_hsv([color.r, color.g, color.b], this.hsv[0]);
                        this.publish(None, window, cx);
                    })),
            );
        }
        let error = if let Some(index) = self.invalid {
            Some(format!(
                "Enter {} from 0 to {}.",
                FIELD_NAMES[index].to_lowercase(),
                LIMITS[index]
            ))
        } else if self.hex_invalid {
            Some("Enter a hex color such as #3399ff.".into())
        } else {
            None
        };
        div()
            .id("style-color-picker")
            .flex()
            .flex_col()
            .gap_3()
            .text_color(p.ink)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(sv)
                            .child(hue_strip.test_support()),
                    )
                    .child(inputs),
            )
            .child(swatches)
            .child(
                div()
                    .text_xs()
                    .child("CMYK (%) - approximate RGB conversion"),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(self.field(7))
                    .child(self.field(8)),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(self.field(9))
                    .child(self.field(10)),
            )
            .children(error.map(|error| div().text_xs().text_color(p.accent).child(error)))
            .test_support()
    }
}
fn number(text: &str, max: f32) -> Option<f32> {
    let value = text.trim().parse::<f32>().ok()?;
    (value.is_finite() && (0. ..=max).contains(&value)).then_some(value)
}
fn field_values(hsv: [f32; 3], rgb: [f32; 3], alpha: f32, cmyk: [f32; 4]) -> Vec<String> {
    [
        hsv[0] * 360.,
        hsv[1] * 100.,
        hsv[2] * 100.,
        rgb[0] * 255.,
        rgb[1] * 255.,
        rgb[2] * 255.,
        alpha * 100.,
        cmyk[0] * 100.,
        cmyk[1] * 100.,
        cmyk[2] * 100.,
        cmyk[3] * 100.,
    ]
    .map(|v| format!("{v:.0}"))
    .to_vec()
}
fn same_rgb(a: [f32; 3], b: [f32; 3]) -> bool {
    a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
}
fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h = h.rem_euclid(1.) * 6.;
    let f = h - h.floor();
    let p = v * (1. - s);
    let q = v * (1. - s * f);
    let t = v * (1. - s * (1. - f));
    match h as usize {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}
fn rgb_to_hsv([r, g, b]: [f32; 3], fallback_hue: f32) -> [f32; 3] {
    let v = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = v - min;
    let h = if d < 1e-6 {
        fallback_hue
    } else if v == r {
        ((g - b) / d).rem_euclid(6.) / 6.
    } else if v == g {
        ((b - r) / d + 2.) / 6.
    } else {
        ((r - g) / d + 4.) / 6.
    };
    [h, if v == 0. { 0. } else { d / v }, v]
}
fn rgba_bytes(color: Hsla) -> [u8; 4] {
    let c = color.to_rgb();
    [c.r, c.g, c.b, c.a].map(|v| (v * 255.).round().clamp(0., 255.) as u8)
}
fn parse_hex(text: &str, alpha: f32) -> Option<Hsla> {
    let text = text.trim();
    let text = text.strip_prefix('#').unwrap_or(text);
    let expanded = if matches!(text.len(), 3 | 4) {
        text.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        text.to_string()
    };
    if !matches!(expanded.len(), 6 | 8) || !expanded.is_ascii() {
        return None;
    }
    let channel = |at| {
        u8::from_str_radix(&expanded[at..at + 2], 16)
            .ok()
            .map(|v| v as f32 / 255.)
    };
    Some(
        Rgba {
            r: channel(0)?,
            g: channel(2)?,
            b: channel(4)?,
            a: if expanded.len() == 8 {
                channel(6)?
            } else {
                alpha
            },
        }
        .into(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn hsv_roundtrips_rgb_and_retains_achromatic_hue() {
        for rgb in [
            [1., 0., 0.],
            [0.2, 0.8, 0.4],
            [0., 0., 1.],
            [1., 1., 1.],
            [0., 0., 0.],
        ] {
            let hsv = rgb_to_hsv(rgb, 0.7);
            let out = hsv_to_rgb(hsv);
            for i in 0..3 {
                assert!((out[i] - rgb[i]).abs() < 1e-5);
            }
        }
        assert_eq!(rgb_to_hsv([0.; 3], 0.7)[0], 0.7);
    }
    #[::core::prelude::v1::test]
    fn hex_and_numeric_validation_preserve_alpha() {
        assert!((parse_hex("#39f", 0.4).unwrap().to_rgb().a - 0.4).abs() < 1e-5);
        assert!(parse_hex("#12345", 1.).is_none());
        assert!(parse_hex("#ééé", 1.).is_none());
        assert!(number("NaN", 255.).is_none());
        assert!(number("256", 255.).is_none());
        assert_eq!(number("180", 360.), Some(180.));
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;

    fn open(cx: &mut TestAppContext) -> (Entity<StyleColorPicker>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
        });
        let slot = Rc::new(RefCell::new(None));
        let result = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let state = cx.new(|cx| {
                ColorPickerState::new(window, cx).default_value(Rgba {
                    r: 1.,
                    g: 0.,
                    b: 0.,
                    a: 0.4,
                })
            });
            let picker = cx.new(|cx| StyleColorPicker::new(state, window, cx));
            *slot.borrow_mut() = Some(picker.clone());
            Root::new(picker, window, cx)
        });
        cx.simulate_resize(size(px(650.), px(560.)));
        cx.update(|window, cx| window.render_frame(cx));
        let picker = result.borrow_mut().take().unwrap();
        (picker, cx)
    }
    #[gpui_kit::test]
    fn sv_and_hue_drag_keep_alpha_and_release_pointer(cx: &mut TestAppContext) {
        let (picker, cx) = open(cx);
        cx.update(|window, cx| {
            let bounds = window.find("style-color-sv").bounds();
            window.drag(
                bounds.center(),
                point(
                    bounds.left() + bounds.size.width * 0.8,
                    bounds.top() + bounds.size.height * 0.25,
                ),
                cx,
            );
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let state = picker.read(cx);
            assert!(state.drag.is_none());
            assert!((state.hsv[1] - 0.8).abs() < 0.02);
            assert!((state.hsv[2] - 0.75).abs() < 0.02);
            assert!((state.alpha - 0.4).abs() < 0.001);
            let before = state.color();
            window.hover("style-color-hue", cx);
            assert_eq!(picker.read(cx).color(), before);
            let bounds = window.find("style-color-hue").bounds();
            window.drag(
                bounds.center(),
                point(bounds.center().x, bounds.top() + bounds.size.height * 0.6),
                cx,
            );
            assert!((picker.read(cx).hsv[0] - 0.6).abs() < 0.02);
            assert!((picker.read(cx).alpha - 0.4).abs() < 0.001);
        });
    }
    #[gpui_kit::test]
    fn numeric_keyboard_swatch_and_invalid_input_use_shared_color_state(cx: &mut TestAppContext) {
        let (picker, cx) = open(cx);
        cx.update(|window, cx| {
            window.click("style-color-sv", cx);
            let before = picker.read(cx).hsv[1];
            window.press("right", cx);
            assert!(picker.read(cx).hsv[1] > before);
            let input = picker.read(cx).fields[0].clone();
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.press("ctrl-a", cx);
            window.input("120.5", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!((picker.read(cx).hsv[0] - 1. / 3.).abs() < 0.01);
            assert_eq!(picker.read(cx).fields[0].read(cx).value().as_str(), "120.5");
            window.click("style-color-swatch-9", cx);
            assert!((picker.read(cx).alpha - 0.4).abs() < 0.001);
            let input = picker.read(cx).fields[3].clone();
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.press("ctrl-a", cx);
            window.input("999", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!picker.update(cx, |picker, cx| picker.commit_pending(window, cx)));
            assert_eq!(picker.read(cx).invalid, Some(3));
        });
    }

    #[gpui_kit::test]
    fn cmyk_edits_keep_ink_separation_alpha_and_validate_percentages(cx: &mut TestAppContext) {
        let (picker, cx) = open(cx);
        // Equal C/M/Y with no K is deliberately different from canonical gray.
        for (index, text) in [(7, "50"), (8, "50"), (9, "50"), (10, "25")] {
            cx.update(|window, cx| {
                let input = picker.read(cx).fields[index].clone();
                window.focus(&input.read(cx).focus_handle(cx), cx);
                window.press("ctrl-a", cx);
                window.input(text, cx);
            });
            cx.run_until_parked();
        }
        cx.update(|window, cx| {
            let view = picker.read(cx);
            assert_eq!(view.cmyk, [0.5, 0.5, 0.5, 0.25]);
            assert_eq!(
                rgba_bytes(view.state.read(cx).value().unwrap()),
                [96, 96, 96, 102]
            );
            let input = view.fields[6].clone();
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.press("ctrl-a", cx);
            window.input("60", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(picker.read(cx).cmyk, [0.5, 0.5, 0.5, 0.25]);
            assert!((picker.read(cx).alpha - 0.6).abs() < 1e-5);
            let input = picker.read(cx).fields[10].clone();
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.press("ctrl-a", cx);
            window.input("101", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!picker.update(cx, |picker, cx| picker.commit_pending(window, cx)));
            assert_eq!(picker.read(cx).invalid, Some(10));
        });
    }

    #[gpui_kit::test]
    fn hex_changes_preview_before_enter_without_rewriting_text_or_alpha(cx: &mut TestAppContext) {
        let (picker, cx) = open(cx);
        cx.update(|window, cx| {
            let input = picker.read(cx).state.read(cx).hex_input().clone();
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.press("ctrl-a", cx);
            window.input("#22bb44", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let state = picker.read(cx).state.clone();
            assert_eq!(
                rgba_bytes(state.read(cx).value().unwrap()),
                [34, 187, 68, 102]
            );
            assert_eq!(
                state.read(cx).hex_input().read(cx).value().as_str(),
                "#22bb44"
            );
            window.press("enter", cx);
            assert_eq!(
                rgba_bytes(state.read(cx).value().unwrap()),
                [34, 187, 68, 102]
            );
        });
    }
}
