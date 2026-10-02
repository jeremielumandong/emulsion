//! Photo orientation and fine straightening, beside the Library preview.
use super::*;
use gpui_kit::component::{
    Disableable, Selectable,
    slider::{Slider, SliderEvent, SliderState},
};

#[derive(Default)]
pub(super) struct RotationControls {
    pub open: bool,
    slider: Option<(PathBuf, f32, Entity<SliderState>, Subscription)>,
}

impl Workspace {
    fn rotation_locked(&self) -> bool {
        self.batch.develop.saving || self.batch.mcp_busy || self.batch.running.is_some()
    }

    pub(super) fn library_rotate(&mut self, turns: Option<u8>, cx: &mut Context<Self>) {
        if self.rotation_locked() {
            return;
        }
        let Some(mut params) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .and_then(|item| self.batch.develop.current_params(&item.path))
        else {
            return;
        };
        match turns {
            Some(turns) => params.rotation = (params.rotation + turns % 4) % 4,
            None => {
                params.rotation = 0;
                params.straighten = 0.;
            }
        }
        self.batch.develop.canvas_tool = 0;
        self.batch.develop.canvas_points.clear();
        self.batch.develop.gesture_active = false;
        self.batch.develop.gesture_recorded = false;
        self.library_adjust(params, cx);
    }

    pub(super) fn library_rotation_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let params = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .and_then(|item| self.batch.develop.current_params(&item.path));
        let disabled = params.is_none() || self.rotation_locked();
        let open = self.batch.develop.rotation_controls.open;
        div()
            .id("library-rotation-controls")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .children(
                [
                    (3usize, t!("library.rotation.rotate_left"), "rotate-ccw"),
                    (1, t!("library.rotation.rotate_right"), "rotate-cw"),
                ]
                .into_iter()
                .map(|(turns, title, icon)| {
                    let title = SharedString::from(title);
                    Button::new(("library-rotate", turns))
                        .label(title.clone())
                        .small()
                        .ghost()
                        .disabled(disabled)
                        .accessibility_label(title.clone())
                        .tooltip(title)
                        .child(crate::editor::rail::tool_icon(icon).size(px(15.)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library_rotate(Some(turns as u8), cx)
                        }))
                }),
            )
            .child(
                Button::new("library-rotation-toggle")
                    .label(t!("library.rotation.straighten"))
                    .small()
                    .ghost()
                    .selected(open)
                    .disabled(disabled)
                    .tooltip(t!("library.rotation.straighten_tip"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.rotation_controls.open =
                            !this.batch.develop.rotation_controls.open;
                        if this.batch.develop.rotation_controls.open {
                            this.batch.develop.canvas_tool = 0;
                            this.batch.develop.canvas_points.clear();
                            this.batch.develop.detail_region = None;
                            this.batch.develop.before = false;
                            this.batch.navigation.borrow_mut().fit();
                            this.invalidate_library_preview();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(super) fn library_rotation_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.batch.develop.rotation_controls.open {
            return None;
        }
        let path = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))?
            .path
            .clone();
        let params = self.batch.develop.current_params(&path)?;
        let disabled = self.rotation_locked();
        let same = self
            .batch
            .develop
            .rotation_controls
            .slider
            .as_ref()
            .is_some_and(|(p, angle, _, _)| p == &path && *angle == params.straighten);
        if !same {
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(-45.)
                    .max(45.)
                    .step(0.1)
                    .default_value(params.straighten)
            });
            let owner_path = path.clone();
            let subscription = cx.subscribe(&slider, move |this, _, event, cx| {
                if let SliderEvent::Release(_) = event {
                    this.batch.develop.gesture_active = false;
                    this.batch.develop.gesture_recorded = false;
                }
                if this.rotation_locked()
                    || this
                        .batch
                        .current
                        .and_then(|i| this.batch.items.get(i))
                        .is_none_or(|i| i.path != owner_path)
                {
                    return;
                }
                if let SliderEvent::Change(value) = event {
                    let Some(mut params) = this.batch.develop.current_params(&owner_path) else {
                        return;
                    };
                    params.straighten = value.end();
                    if params.validate().is_err() {
                        return;
                    }
                    if let Some((_, angle, _, _)) = &mut this.batch.develop.rotation_controls.slider
                    {
                        *angle = params.straighten;
                    }
                    this.batch.develop.gesture_active = true;
                    this.library_adjust(params, cx);
                }
            });
            self.batch.develop.rotation_controls.slider =
                Some((path, params.straighten, slider, subscription));
        }
        let slider = self
            .batch
            .develop
            .rotation_controls
            .slider
            .as_ref()
            .unwrap()
            .2
            .clone();
        let p = classic::palette(cx);
        Some(
            div()
                .id("library-rotation-panel")
                .test_support()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .bg(p.panel)
                .border_b_1()
                .border_color(p.line)
                .child(label(t!("library.rotation.fine_angle"), &p))
                .child(
                    div()
                        .id("library-rotation-angle")
                        .test_support()
                        .w(px(200.))
                        .child(Slider::new(&slider).disabled(disabled)),
                )
                .child(self.library_numeric_control(
                    1000,
                    &t!("library.rotation.straighten_angle"),
                    advanced::Field::Straighten,
                    params.straighten,
                    -45.,
                    45.,
                    0.1,
                    cx,
                ))
                .child(
                    Button::new("library-rotation-reset")
                        .label(t!("library.rotation.reset"))
                        .small()
                        .ghost()
                        .tooltip(t!("library.rotation.reset_tip"))
                        .disabled(disabled || (params.rotation == 0 && params.straighten == 0.))
                        .on_click(cx.listener(|this, _, _, cx| this.library_rotate(None, cx))),
                )
                .child(
                    Button::new("library-rotation-done")
                        .label(t!("library.rotation.done"))
                        .small()
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch.develop.rotation_controls.open = false;
                            this.batch.develop.gesture_active = false;
                            this.batch.develop.gesture_recorded = false;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}
