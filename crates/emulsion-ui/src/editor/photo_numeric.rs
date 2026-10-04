//! Photo's paint percentages and inline option values. These edit brush state,
//! never document transactions; layer opacity retains its existing undo path.
use super::tools::BrushSlot;
use super::*;
use gpui_kit::component::{Sizable, input::Escape};
use std::time::Duration;

const DIGIT_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq)]
enum PercentTarget {
    Opacity,
    Flow,
}

struct DigitSequence {
    first: u8,
    slot: BrushSlot,
    target: PercentTarget,
    at: Instant,
}

struct NumericEdit {
    key: SliderKey,
    slot: BrushSlot,
    input: Entity<InputState>,
    spec: (f32, f32, f32),
    _subscription: Subscription,
}

#[derive(Default)]
pub(crate) struct PhotoNumericState {
    digits: Option<DigitSequence>,
    edit: Option<NumericEdit>,
    watchers: Option<(Subscription, Subscription, Subscription, Subscription)>,
}

impl PhotoNumericState {
    fn percent(&mut self, digit: u8, slot: BrushSlot, target: PercentTarget, now: Instant) -> u8 {
        if let Some(previous) = self.digits.take()
            && previous.slot == slot
            && previous.target == target
            && now.saturating_duration_since(previous.at) < DIGIT_TIMEOUT
        {
            return previous.first * 10 + digit;
        }
        self.digits = Some(DigitSequence {
            first: digit,
            slot,
            target,
            at: now,
        });
        if digit == 0 { 100 } else { digit * 10 }
    }
}

impl EditorView {
    /// A tool change must also break a sequence when it switches away and back
    /// without another digit. Discard an unfinished value instead of applying
    /// it to the new slot.
    pub(crate) fn reset_photo_numeric(&mut self) {
        self.tools.photo_numeric.digits = None;
        self.tools.photo_numeric.edit = None;
    }

    pub(crate) fn clear_photo_numeric_sequence(&mut self) {
        self.tools.photo_numeric.digits = None;
    }

    fn watch_photo_numeric_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tools.photo_numeric.watchers.is_some() {
            return;
        }
        let blur = cx.on_focus_out(&self.canvas_focus, window, |this, _, _, _| {
            this.tools.photo_numeric.digits = None;
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.reset_photo_numeric();
                cx.notify();
            }
        });
        // Resolve actions as well as digits: a customized number key can
        // mean Undo/Zoom instead of setting a paint percentage.
        let keys = cx.observe_keystrokes(|this, event, _, _| {
            use crate::actions::*;
            let numeric = event.action.as_ref().is_some_and(|action| {
                let action = action.as_any();
                macro_rules! is_percent {
                    ($($kind:ty),* $(,)?) => { $(action.is::<$kind>())||* };
                }
                is_percent!(
                    Opacity10, Opacity20, Opacity30, Opacity40, Opacity50, Opacity60, Opacity70,
                    Opacity80, Opacity90, Opacity100, Flow10, Flow20, Flow30, Flow40, Flow50,
                    Flow60, Flow70, Flow80, Flow90, Flow100,
                )
            });
            if !numeric {
                this.clear_photo_numeric_sequence();
            }
        });
        // Raw canvas callbacks (for example the RAW split view's Home key)
        // can consume an event before observers run. Clear unrelated keys up
        // front as well, without consuming the event or changing any binding.
        let owner = cx.weak_entity();
        let raw_keys = cx.intercept_keystrokes(move |event, _, cx| {
            let modifiers = event.keystroke.modifiers;
            let key = event.keystroke.key.as_str();
            let numeric = key.len() == 1
                && (key.as_bytes()[0].is_ascii_digit() || "!@#$%^&*()".contains(key));
            if modifiers.control
                || modifiers.platform
                || modifiers.alt
                || modifiers.function
                || !numeric
            {
                owner
                    .update(cx, |this, _| this.clear_photo_numeric_sequence())
                    .ok();
            }
        });
        self.tools.photo_numeric.watchers = Some((blur, activation, keys, raw_keys));
    }

    /// Canvas-only dispatch, including defensive checks for remapped actions.
    /// Heal/Remove have no opacity option and must never affect layer opacity.
    pub(crate) fn photo_opacity_shortcut(
        &mut self,
        percent: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_photo_workflow() {
            self.reset_photo_numeric();
            self.opacity_shortcut(percent, cx);
            return;
        }
        if !self.photo_numeric_canvas_ready(window) {
            self.tools.photo_numeric.digits = None;
            return;
        }
        if self.tool == Tool::Heal {
            self.tools.photo_numeric.digits = None;
            return;
        }
        if matches!(self.tool, Tool::Brush | Tool::Clone | Tool::Mask) {
            self.photo_percent_digit(percent, PercentTarget::Opacity, window, cx);
        } else {
            self.tools.photo_numeric.digits = None;
            self.opacity_shortcut(percent, cx);
        }
    }

    pub(crate) fn photo_flow_shortcut(
        &mut self,
        percent: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_photo_workflow()
            || !self.photo_numeric_canvas_ready(window)
            || !self.brushy()
            || self.tool == Tool::Heal
        {
            self.tools.photo_numeric.digits = None;
            return;
        }
        self.photo_percent_digit(percent, PercentTarget::Flow, window, cx);
    }

    fn photo_numeric_canvas_ready(&self, window: &Window) -> bool {
        self.canvas_focus.is_focused(window)
            && self.type_tool.field.is_none()
            && self.tools.photo_numeric.edit.is_none()
            && self.drag.is_none()
            && !self.editor.in_transaction()
            && !self.frame_crop_active()
            && !self.motion.presenting
            && !self.responsive_preview_active()
    }

    fn photo_percent_digit(
        &mut self,
        percent: u8,
        target: PercentTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = BrushSlot::of(self.tool, self.tools.paint) else {
            return;
        };
        self.watch_photo_numeric_focus(window, cx);
        let digit = if percent == 100 { 0 } else { percent / 10 };
        if digit > 9 {
            return;
        }
        // The raster engine and stored brush catalog support a 1% minimum.
        // Report the effective value rather than displaying an inert 0%.
        let value = self
            .tools
            .photo_numeric
            .percent(digit, slot, target, Instant::now())
            .max(1);
        match target {
            PercentTarget::Opacity => self.tools.brush.opacity = f32::from(value) / 100.,
            PercentTarget::Flow => self.tools.brush.flow = f32::from(value) / 100.,
        }
        self.remember_active_brush(cx);
        let label = match target {
            PercentTarget::Opacity => t!("editor.tools.opacity"),
            PercentTarget::Flow => t!("editor.tools.flow"),
        };
        self.set_status(format!("{label}: {value}%"), false, cx);
        cx.notify();
    }

    pub(super) fn photo_numeric_option(&self, key: SliderKey) -> bool {
        self.is_photo_workflow()
            && BrushSlot::of(self.tool, self.tools.paint).is_some()
            && matches!(
                key,
                SliderKey::ToolSize
                    | SliderKey::ToolHardness
                    | SliderKey::ToolOpacity
                    | SliderKey::ToolFlow
            )
    }

    fn start_photo_numeric(
        &mut self,
        key: SliderKey,
        spec: (f32, f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.photo_numeric_option(key) || self.drag.is_some() || self.editor.in_transaction() {
            return;
        }
        let Some(slot) = BrushSlot::of(self.tool, self.tools.paint) else {
            return;
        };
        // Exact entry follows the raster engine and stored brush range, while
        // the pointer track keeps its finer nonlinear 1–500 px scale.
        let spec = if key == SliderKey::ToolSize {
            (1., 1000., 1.)
        } else {
            spec
        };
        self.reset_photo_numeric();
        self.watch_photo_numeric_focus(window, cx);
        let brush = self.tools.brush;
        let value = match key {
            SliderKey::ToolSize => brush.size,
            SliderKey::ToolHardness => brush.hardness * 100.,
            SliderKey::ToolOpacity => brush.opacity * 100.,
            SliderKey::ToolFlow => brush.flow * 100.,
            _ => return,
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(format!("{value:.0}")));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if !this
                    .tools
                    .photo_numeric
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.input == *input)
                {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => this.apply_photo_numeric(window, cx),
                    InputEvent::Blur => {
                        this.tools.photo_numeric.edit = None;
                        cx.notify();
                    }
                    _ => {}
                }
            },
        );
        self.tools.photo_numeric.edit = Some(NumericEdit {
            key,
            slot,
            input: input.clone(),
            spec,
            _subscription: subscription,
        });
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        cx.notify();
    }

    fn apply_photo_numeric(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.tools.photo_numeric.edit.as_ref() else {
            return;
        };
        if !self.photo_numeric_option(edit.key)
            || BrushSlot::of(self.tool, self.tools.paint) != Some(edit.slot)
            || self.drag.is_some()
            || self.editor.in_transaction()
        {
            self.reset_photo_numeric();
            cx.notify();
            return;
        }
        let text = edit.input.read(cx).value();
        let Some(value) = parse_numeric(&text, edit.spec) else {
            self.set_status(
                t!(
                    "editor.brush_studio.enter_range",
                    min = edit.spec.0,
                    max = edit.spec.1
                ),
                true,
                cx,
            );
            return;
        };
        let key = edit.key;
        self.tools.photo_numeric.edit = None;
        // Size is already in pixels. Routing it through apply_slider would
        // square the value a second time; the pointer track remains nonlinear.
        match key {
            SliderKey::ToolSize => self.tools.brush.size = value,
            SliderKey::ToolHardness => self.tools.brush.hardness = value / 100.,
            SliderKey::ToolOpacity => self.tools.brush.opacity = value / 100.,
            SliderKey::ToolFlow => self.tools.brush.flow = value / 100.,
            _ => return,
        }
        self.remember_active_brush(cx);
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }

    pub(super) fn photo_numeric_value(
        &mut self,
        key: SliderKey,
        name: &str,
        display: String,
        spec: (f32, f32, f32),
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(edit) = self
            .tools
            .photo_numeric
            .edit
            .as_ref()
            .filter(|edit| edit.key == key)
        {
            return div()
                .id(SharedString::from(format!("photo-value-edit-{key:?}")))
                .test_support()
                .w(rems(3.5))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.tools.photo_numeric.edit = None;
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &Escape, window, cx| {
                    this.tools.photo_numeric.edit = None;
                    window.focus(&this.canvas_focus, cx);
                    cx.notify();
                }))
                .child(Input::new(&edit.input).small().w_full())
                .into_any_element();
        }
        div()
            .id(SharedString::from(format!("photo-value-{key:?}")))
            .test_support()
            .w(rems(3.5))
            .text_color(p.ink)
            .cursor(CursorStyle::IBeam)
            .tab_index(0)
            .role(Role::Button)
            .aria_label(format!("{name}: {display}"))
            .focus_visible(|s| s.bg(p.accent.opacity(0.2)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.start_photo_numeric(key, spec, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space")
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                    && !event.keystroke.modifiers.alt
                {
                    this.start_photo_numeric(key, spec, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(display)
            .into_any_element()
    }
}

fn parse_numeric(text: &str, (min, max, step): (f32, f32, f32)) -> Option<f32> {
    let value = text.trim().parse::<f32>().ok()?;
    (value.is_finite() && (min..=max).contains(&value)).then(|| snap(value, step).clamp(min, max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_raster::Placement;
    use gpui_kit::{TestAppContext, VisualTestContext, test::TestWindowExt};

    fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        let mut doc = Document::new(256, 192);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Layer",
                Arc::new(Raster::solid(256, 192, [0.2, 0.3, 0.4, 1.])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let (ws, cx) = crate::tests::open(cx, doc);
        let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_paint(PaintKind::Brush, cx);
                window.focus(&editor.canvas_focus, cx);
            })
        });
        cx.run_until_parked();
        (editor, cx)
    }

    fn press(keys: &str, cx: &mut VisualTestContext) {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
    }

    fn enter_value(key: &'static str, value: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.click(key, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(value);
        cx.run_until_parked();
    }

    #[test]
    fn numeric_sequence_timeout_and_target_boundaries() {
        let mut state = PhotoNumericState::default();
        let slot = BrushSlot::Paint(PaintKind::Brush);
        let at = Instant::now();
        assert_eq!(state.percent(4, slot, PercentTarget::Opacity, at), 40);
        assert_eq!(
            state.percent(
                5,
                slot,
                PercentTarget::Opacity,
                at + Duration::from_millis(999)
            ),
            45
        );
        assert_eq!(state.percent(0, slot, PercentTarget::Opacity, at), 100);
        assert_eq!(state.percent(0, slot, PercentTarget::Opacity, at), 0);
        assert_eq!(state.percent(4, slot, PercentTarget::Opacity, at), 40);
        assert_eq!(
            state.percent(5, slot, PercentTarget::Opacity, at + DIGIT_TIMEOUT),
            50
        );
        assert_eq!(
            state.percent(6, slot, PercentTarget::Flow, at + DIGIT_TIMEOUT),
            60
        );
        assert_eq!(
            state.percent(7, BrushSlot::Clone, PercentTarget::Flow, at + DIGIT_TIMEOUT),
            70
        );
    }

    #[test]
    fn numeric_input_rejects_nonfinite_and_out_of_range() {
        for value in ["", "abc", "NaN", "inf", "-inf", "1e999"] {
            assert_eq!(parse_numeric(value, (1., 500., 1.)), None, "{value}");
        }
        assert_eq!(parse_numeric("83", (1., 500., 1.)), Some(83.));
        assert_eq!(parse_numeric("900", (1., 500., 1.)), None);
        assert_eq!(parse_numeric("0", (1., 500., 1.)), None);
        assert_eq!(parse_numeric("-5", (0., 100., 1.)), None);
        assert_eq!(parse_numeric(" 45.6 ", (0., 100., 1.)), Some(46.));
    }

    #[gpui_kit::test]
    fn photo_numeric_digits_flow_and_zero_do_not_edit_document(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        press("4 5", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.45));
        press("0", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 1.));
        press("0", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.01));
        press("shift-4 shift-5", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.flow, 0.45));
        press("shift-0 shift-0", cx);
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.tools.brush.flow, 0.01);
            assert_eq!(editor.editor.doc, before);
            assert!(editor.editor.history.is_empty());
            assert!(!editor.editor.in_transaction());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_timeout_tools_and_slots_reset_sequences(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        press("4", cx);
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                editor.tools.photo_numeric.digits.as_mut().unwrap().at -= DIGIT_TIMEOUT;
            })
        });
        press("5", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.5));
        press("e 6 b 7", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.7));
        press("e", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.6));
        press("b v b 8", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.8));
        // Switching from opacity to flow cannot combine the two values.
        press("shift-3", cx);
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.tools.brush.opacity, 0.8);
            assert_eq!(editor.tools.brush.flow, 0.3);
            assert!(editor.editor.history.is_empty());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_layer_rename_and_canvas_text_own_digits(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        press("4", cx);
        cx.update(|window, cx| editor.update(cx, |editor, cx| editor.rename_layer(window, cx)));
        cx.run_until_parked();
        press("5 shift-3", cx);
        cx.update(|window, cx| {
            assert_eq!(editor.read(cx).tools.brush.opacity, 0.4);
            assert!(editor.read(cx).tools.photo_numeric.digits.is_none());
            editor.read(cx).canvas_focus.clone().focus(window, cx);
        });
        cx.run_until_parked();
        press("5", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.5));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_tool(Tool::Type, cx);
                editor.type_down((30., 30.), window, cx);
            })
        });
        cx.run_until_parked();
        let (opacity, flow) = cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.type_tool.field.is_some());
            (editor.tools.brush.opacity, editor.tools.brush.flow)
        });
        press("4 5 shift-3", cx);
        cx.simulate_input("45");
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.type_tool.field.is_some());
            assert_eq!(editor.tools.brush.opacity, opacity);
            assert_eq!(editor.tools.brush.flow, flow);
            assert!(editor.editor.doc.nodes.iter().all(|n| n.opacity == 1.));
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_heal_remove_layers_and_other_workspaces_are_isolated(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        press("j 4 5 shift-6", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, before));
        cx.update(|_, cx| editor.update(cx, |editor, cx| editor.set_remove_mode(true, cx)));
        press("4 5 shift-6", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, before));
        press("v 4 5", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).editor.doc.nodes[0].opacity, 0.5);
            assert!(!editor.read(cx).editor.in_transaction());
        });
        press("ctrl-z", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc.nodes[0].opacity, 0.4));
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.draw_mode = true;
                editor.set_paint(PaintKind::Brush, cx);
                editor.tools.brush.flow = 0.9;
            })
        });
        cx.run_until_parked();
        press("4 5 shift-6", cx);
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.tools.brush.opacity, 0.5);
            assert_eq!(editor.tools.brush.flow, 0.9);
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_modified_keys_and_active_transactions_do_not_change_percentages(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx);
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                editor.tools.brush.opacity = 0.73;
                editor.tools.brush.flow = 0.81;
            })
        });
        press("ctrl-4 alt-5 ctrl-shift-6", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.opacity, 0.73);
            assert_eq!(editor.read(cx).tools.brush.flow, 0.81);
            editor.update(cx, |editor, _| editor.editor.begin("Existing gesture"));
        });
        press("4 shift-5", cx);
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.tools.brush.opacity, 0.73);
            assert_eq!(editor.tools.brush.flow, 0.81);
            assert!(editor.editor.in_transaction());
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                editor.editor.cancel();
            })
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_undo_strokes_sliders_and_other_keys_break_sequences(cx: &mut TestAppContext) {
        for compact in [false, true] {
            let (editor, cx) = setup(cx);
            cx.simulate_resize(size(px(1280.), px(900.)));
            cx.update(|window, cx| {
                cx.global_mut::<crate::app_state::AppSettings>()
                    .0
                    .compact_chrome = compact;
                window.refresh();
            });
            cx.run_until_parked();
            press("4 ctrl-z 5", cx);
            cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.5));
            press("b 4 ] 5", cx);
            cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.5));
            press("b 4", cx);
            cx.update(|window, cx| window.click("canvas", cx));
            cx.run_until_parked();
            let history = cx.update(|_, cx| editor.read(cx).editor.history.len());
            press("5", cx);
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.tools.brush.opacity, 0.5);
                assert_eq!(editor.editor.history.len(), history);
                assert!(!editor.editor.in_transaction());
            });
            press("b 4", cx);
            cx.update(|window, cx| window.click("ToolOpacity", cx));
            cx.run_until_parked();
            cx.update(|window, cx| editor.read(cx).canvas_focus.clone().focus(window, cx));
            press("5", cx);
            cx.update(|_, cx| {
                assert_eq!(editor.read(cx).tools.brush.opacity, 0.5);
                assert!(!editor.read(cx).editor.in_transaction());
            });
        }
    }

    #[gpui_kit::test]
    fn photo_numeric_remapped_digit_action_breaks_sequence(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        cx.update(|_, cx| {
            cx.bind_keys([
                KeyBinding::new("6", crate::actions::Zoom100, Some("Canvas")),
                KeyBinding::new("7", crate::actions::Undo, Some("Canvas")),
            ])
        });
        press("4 6 5", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.5));
        press("b 4 7 5", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.opacity, 0.5);
            assert!(!editor.read(cx).editor.in_transaction());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_inline_values_apply_cancel_validate_and_preserve_history(
        cx: &mut TestAppContext,
    ) {
        for compact in [false, true] {
            let (editor, cx) = setup(cx);
            cx.simulate_resize(size(px(1280.), px(900.)));
            cx.update(|window, cx| {
                cx.global_mut::<crate::app_state::AppSettings>()
                    .0
                    .compact_chrome = compact;
                window.refresh();
            });
            cx.run_until_parked();
            let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
            enter_value("photo-value-ToolSize", "83", cx);
            press("enter", cx);
            cx.update(|window, cx| {
                assert_eq!(editor.read(cx).tools.brush.size, 83.);
                assert!(editor.read(cx).canvas_focus.is_focused(window));
            });
            enter_value("photo-value-ToolSize", "200", cx);
            press("escape", cx);
            cx.update(|window, cx| {
                assert_eq!(editor.read(cx).tools.brush.size, 83.);
                assert!(editor.read(cx).tools.photo_numeric.edit.is_none());
                assert!(editor.read(cx).canvas_focus.is_focused(window));
            });
            enter_value("photo-value-ToolOpacity", "37", cx);
            press("enter", cx);
            enter_value("photo-value-ToolFlow", "0", cx);
            press("enter", cx);
            cx.update(|_, cx| assert!(editor.read(cx).tools.photo_numeric.edit.is_some()));
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input("1");
            press("enter", cx);
            enter_value("photo-value-ToolHardness", "200", cx);
            press("enter", cx);
            cx.update(|_, cx| assert!(editor.read(cx).tools.photo_numeric.edit.is_some()));
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input("100");
            press("enter", cx);
            enter_value("photo-value-ToolSize", "NaN", cx);
            press("enter", cx);
            cx.update(|_, cx| assert!(editor.read(cx).tools.photo_numeric.edit.is_some()));
            press("escape", cx);
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert_eq!(editor.tools.brush.size, 83.);
                assert_eq!(editor.tools.brush.opacity, 0.37);
                assert_eq!(editor.tools.brush.flow, 0.01);
                assert_eq!(editor.tools.brush.hardness, 1.);
                assert_eq!(editor.editor.doc, before);
                assert!(editor.editor.history.is_empty());
                assert!(!editor.editor.in_transaction());
            });
            press("e b", cx);
            cx.update(|_, cx| {
                assert_eq!(editor.read(cx).tools.brush.size, 83.);
                assert_eq!(editor.read(cx).tools.brush.opacity, 0.37);
                assert_eq!(editor.read(cx).tools.brush.flow, 0.01);
            });
        }
    }

    #[gpui_kit::test]
    fn photo_numeric_large_size_round_trip_obeys_engine_range(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        enter_value("photo-value-ToolSize", "900", cx);
        press("enter", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.size, 900.));
        cx.update(|window, cx| window.click("photo-value-ToolSize", cx));
        cx.run_until_parked();
        // Match native Return: simulate_keystrokes adds an IME newline after
        // a propagated Enter, erasing this single-line field's selected value
        // before its queued PressEnter event. Native Return does not do that.
        cx.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let editor = editor.read(cx);
            assert_eq!(editor.tools.brush.size, 900.);
            assert!(editor.tools.photo_numeric.edit.is_none());
            assert!(editor.canvas_focus.is_focused(window));
        });
        enter_value("photo-value-ToolSize", "1200", cx);
        press("enter", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.size, 900.);
            assert!(editor.read(cx).tools.photo_numeric.edit.is_some());
            assert!(editor.read(cx).editor.history.is_empty());
            assert!(!editor.read(cx).editor.in_transaction());
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("1000");
        press("enter", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.size, 1000.));
    }

    #[gpui_kit::test]
    fn photo_numeric_compact_overflow_resize_keeps_draft_safe(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        cx.simulate_resize(size(px(640.), px(700.)));
        cx.update(|window, cx| {
            cx.global_mut::<crate::app_state::AppSettings>()
                .0
                .compact_chrome = true;
            window.refresh();
        });
        cx.run_until_parked();
        let before = cx.update(|_, cx| editor.read(cx).tools.brush.flow);
        cx.update(|window, cx| window.click("tool-options-more", cx));
        cx.run_until_parked();
        enter_value("photo-value-ToolFlow", "34", cx);
        cx.simulate_resize(size(px(1280.), px(900.)));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.flow, before));
        press("escape", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.flow, before);
            assert!(editor.read(cx).tools.photo_numeric.edit.is_none());
            assert!(!editor.read(cx).editor.in_transaction());
        });
        // Close any still-open overflow and then reopen the moved inline field.
        press("escape", cx);
        enter_value("photo-value-ToolFlow", "34", cx);
        press("enter", cx);
        cx.update(|window, cx| {
            assert_eq!(editor.read(cx).tools.brush.flow, 0.34);
            assert!(editor.read(cx).canvas_focus.is_focused(window));
            assert!(editor.read(cx).editor.history.is_empty());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_tip_panel_does_not_mount_a_second_flow_editor(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        cx.simulate_resize(size(px(1280.), px(900.)));
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.brush_settings_section = super::super::tools::BrushSettingsSection::Tip;
                editor.show_brush_settings(cx);
            })
        });
        cx.run_until_parked();
        enter_value("photo-value-ToolFlow", "34", cx);
        cx.update(|_, cx| assert!(editor.read(cx).tools.photo_numeric.edit.is_some()));
        press("enter", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.flow, 0.34);
            assert!(editor.read(cx).editor.history.is_empty());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_native_symbols_drive_flow_and_preserve_opacity(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        press("4 5", cx);
        // Native Linux reports these printed symbols without Shift.
        press("$ %", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.flow, 0.45);
            assert_eq!(editor.read(cx).tools.brush.opacity, 0.45);
        });
        press(") )", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.flow, 0.01));
        press("! @", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.flow, 0.12));
    }

    #[gpui_kit::test]
    fn photo_numeric_supported_values_reach_strokes_and_persisted_memory(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        let id = cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert!(editor.apply_preset_named("Chalk", cx));
                editor.tools.brush = emulsion_raster::paint::Brush {
                    size: 32.,
                    hardness: 1.,
                    ..Default::default()
                };
                editor.tools.quick_shape = false;
                editor.set_fg([255, 0, 0, 255], cx);
                editor.presets.current_id.clone().unwrap()
            })
        });
        press("4 5 shift-5", cx);
        let saved = emulsion_io::brush_library::load().unwrap();
        let memory = &saved.tool_memory("paint", &id).unwrap().brush;
        assert_eq!(memory.opacity, 0.45);
        assert_eq!(memory.flow, 0.5);
        assert_eq!(memory.size, 32.);
        let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        let at = cx.update(|_, cx| editor.read(cx).doc_to_window((128., 96.)).unwrap());
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            let NodeKind::Raster { raster, .. } = &editor.editor.doc.nodes[0].kind else {
                panic!("raster")
            };
            let NodeKind::Raster {
                raster: original, ..
            } = &before.nodes[0].kind
            else {
                panic!("raster")
            };
            assert_ne!(raster.get(128, 96), original.get(128, 96));
            assert_eq!(editor.tools.brush, editor.tools.brush.sanitized());
            assert_eq!(editor.editor.history.len(), 1);
            assert!(!editor.editor.in_transaction());
        });
        press("ctrl-z", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, before));
        press("0 0 shift-0 shift-0", cx);
        let saved = emulsion_io::brush_library::load().unwrap();
        let memory = &saved.tool_memory("paint", &id).unwrap().brush;
        assert_eq!(memory.opacity, 0.01);
        assert_eq!(memory.flow, 0.01);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush, *memory));
    }

    #[gpui_kit::test]
    fn photo_numeric_design_storyboard_and_diagram_remain_single_digit(cx: &mut TestAppContext) {
        use emulsion_core::project::{ProjectEditor, ProjectKind};
        let (workspace, cx) = crate::tests::open(cx, Document::new(256, 192));
        for kind in [
            ProjectKind::Design,
            ProjectKind::Storyboard,
            ProjectKind::Diagram,
        ] {
            let project = ProjectEditor::new_project(kind, Document::new(256, 192)).unwrap();
            let editor = cx.update(|window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.install_project(project, "Other numeric workflow".into(), window, cx);
                });
                let editor = workspace.read(cx).editor.clone().unwrap();
                editor.update(cx, |editor, cx| {
                    editor.set_paint(PaintKind::Brush, cx);
                    editor.tools.brush.flow = 0.81;
                    editor.tools.brush.hardness = 0.61;
                    window.focus(&editor.canvas_focus, cx);
                });
                editor
            });
            cx.run_until_parked();
            press("4 5 shift-6 $ % { }", cx);
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert!(!editor.is_photo_workflow());
                assert_eq!(editor.tools.brush.opacity, 0.5, "{kind:?}");
                assert_eq!(editor.tools.brush.flow, 0.81, "{kind:?}");
                assert_eq!(editor.tools.brush.hardness, 0.61, "{kind:?}");
                assert!(editor.editor.history.is_empty());
                assert!(!editor.editor.in_transaction());
            });
        }
    }

    #[gpui_kit::test]
    fn photo_numeric_window_deactivation_discards_sequences_and_drafts(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        press("4", cx);
        cx.deactivate_window();
        cx.run_until_parked();
        cx.update(|_, cx| assert!(editor.read(cx).tools.photo_numeric.digits.is_none()));
        cx.update(|window, cx| {
            window.activate_window();
            editor.read(cx).canvas_focus.clone().focus(window, cx);
        });
        press("5", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.opacity, 0.5));
        enter_value("photo-value-ToolOpacity", "12", cx);
        cx.deactivate_window();
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(editor.read(cx).tools.photo_numeric.edit.is_none());
            assert_eq!(editor.read(cx).tools.brush.opacity, 0.5);
            assert!(editor.read(cx).editor.history.is_empty());
            assert!(!editor.read(cx).editor.in_transaction());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_values_activate_by_enter_and_space_after_tab_traversal(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx);
        cx.simulate_resize(size(px(1280.), px(900.)));
        cx.run_until_parked();
        for key in ["enter", "space"] {
            // Establish focus on the preceding slider, then traverse to its
            // numeric button without clicking the value itself.
            cx.update(|window, cx| window.click("ToolSize", cx));
            cx.run_until_parked();
            cx.update(|window, cx| window.focus_next(cx));
            cx.run_until_parked();
            press(key, cx);
            cx.update(|_, cx| {
                assert_eq!(
                    editor
                        .read(cx)
                        .tools
                        .photo_numeric
                        .edit
                        .as_ref()
                        .map(|e| e.key),
                    Some(SliderKey::ToolSize)
                );
            });
            cx.simulate_input("83");
            press("enter", cx);
            cx.update(|window, cx| {
                assert_eq!(editor.read(cx).tools.brush.size, 83.);
                assert!(editor.read(cx).canvas_focus.is_focused(window));
            });
        }
    }

    #[gpui_kit::test]
    fn photo_numeric_native_braces_step_hardness_without_editing_artwork(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        cx.update(|_, cx| editor.update(cx, |editor, _| editor.tools.brush.hardness = 0.6));
        press("}", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.hardness, 0.75));
        press("{ {", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.hardness, 0.25);
            assert!(editor.read(cx).editor.history.is_empty());
            assert!(!editor.read(cx).editor.in_transaction());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_bracket_limit_matches_photo_engine_without_changing_paint(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx);
        cx.update(|_, cx| editor.update(cx, |editor, _| editor.tools.brush.size = 950.));
        press("] ]", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.brush.size, 1000.));
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.draw_mode = true;
                editor.tools.brush.size = 1950.;
                cx.notify();
            })
        });
        cx.run_until_parked();
        press("] ]", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.size, 2000.);
            assert!(editor.read(cx).editor.history.is_empty());
        });
    }

    #[gpui_kit::test]
    fn photo_numeric_outside_click_and_tool_switch_discard_drafts(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx);
        let opacity = cx.update(|_, cx| editor.read(cx).tools.brush.opacity);
        enter_value("photo-value-ToolOpacity", "12", cx);
        // Switching to another option discards the first draft, without
        // stealing focus from the newly opened input in its queued Blur.
        enter_value("photo-value-ToolFlow", "34", cx);
        press("enter", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.opacity, opacity);
            assert_eq!(editor.read(cx).tools.brush.flow, 0.34);
        });
        enter_value("photo-value-ToolOpacity", "12", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_paint(PaintKind::Eraser, cx);
                window.focus(&editor.canvas_focus, cx);
            })
        });
        cx.run_until_parked();
        press("enter", cx);
        press("b", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tools.brush.opacity, opacity);
            assert!(editor.read(cx).tools.photo_numeric.edit.is_none());
            assert!(editor.read(cx).editor.history.is_empty());
            assert!(!editor.read(cx).editor.in_transaction());
        });
    }
}
