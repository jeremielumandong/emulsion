//! Crop constraints and their retained toolbar controls.
use super::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CropMode {
    #[default]
    Free,
    FixedRatio,
    FixedSize,
    Original,
    Ratio(u32, u32),
}

impl CropMode {
    fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::FixedRatio => "Fixed ratio",
            Self::FixedSize => "Fixed size",
            Self::Original => "Original ratio",
            Self::Ratio(1, 1) => "1:1",
            Self::Ratio(4, 3) => "4:3",
            Self::Ratio(3, 2) => "3:2",
            Self::Ratio(16, 9) => "16:9",
            _ => "Ratio",
        }
    }
}

pub(crate) struct CropOptions {
    pub mode: CropMode,
    pub ratio: (f64, f64),
    pub size: (f64, f64),
    pub valid: bool,
    fields: Option<CropFields>,
}

struct CropFields {
    width: Entity<InputState>,
    height: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Default for CropOptions {
    fn default() -> Self {
        Self {
            mode: CropMode::Free,
            ratio: (1., 1.),
            size: (256., 256.),
            valid: true,
            fields: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CropConstraint {
    Free,
    Ratio(f64),
    Size(f64, f64),
}

/// Preview and mouse-up use the same rectangle, including pixel rounding.
pub(crate) fn crop_rect(
    start: (f64, f64),
    end: (f64, f64),
    centered: bool,
    constraint: CropConstraint,
    shift: bool,
) -> (f64, f64, f64, f64) {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let multiplier = if centered { 2. } else { 1. };
    let (mut width, mut height) = (dx.abs() * multiplier, dy.abs() * multiplier);
    let constraint = match constraint {
        CropConstraint::Free if shift => CropConstraint::Ratio(1.),
        other => other,
    };
    match constraint {
        CropConstraint::Free => {}
        CropConstraint::Ratio(ratio) => {
            if width > height * ratio {
                height = width / ratio;
            } else {
                width = height * ratio;
            }
        }
        CropConstraint::Size(w, h) => (width, height) = (w, h),
    }
    let (width, height) = (width.round(), height.round());
    let (x, y) = if centered {
        (start.0 - width / 2., start.1 - height / 2.)
    } else {
        (
            start.0 - if dx < 0. { width } else { 0. },
            start.1 - if dy < 0. { height } else { 0. },
        )
    };
    (x.round(), y.round(), width, height)
}

impl EditorView {
    pub(crate) fn crop_constraint(&self) -> CropConstraint {
        let options = &self.tools.crop_options;
        match options.mode {
            CropMode::Free => CropConstraint::Free,
            CropMode::FixedRatio => CropConstraint::Ratio(options.ratio.0 / options.ratio.1),
            CropMode::FixedSize => CropConstraint::Size(options.size.0, options.size.1),
            CropMode::Original => {
                CropConstraint::Ratio(self.editor.doc.width as f64 / self.editor.doc.height as f64)
            }
            CropMode::Ratio(w, h) => CropConstraint::Ratio(w as f64 / h as f64),
        }
    }

    fn refresh_crop_constraint(&mut self, cx: &mut Context<Self>) {
        if self.tools.crop_options.valid
            && let Some((x, y, w, h)) = self.tools.crop
        {
            let centered = self.tools.crop_centered;
            let constraint = self.crop_constraint();
            // Changing presets keeps the current width. Repeated W/H edits
            // must not progressively grow the crop from its previous height.
            let next_height = match constraint {
                CropConstraint::Ratio(ratio) => w / ratio,
                _ => h,
            };
            let start = if centered {
                (x + w / 2., y + h / 2.)
            } else {
                (x, y)
            };
            self.tools.crop = Some(crop_rect(
                start,
                if centered {
                    (start.0 + w / 2., start.1 + next_height / 2.)
                } else {
                    (start.0 + w, start.1 + next_height)
                },
                centered,
                constraint,
                false,
            ));
        }
        cx.notify();
    }

    pub(crate) fn set_crop_mode(
        &mut self,
        mode: CropMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tools.crop_options.mode = mode;
        self.tools.crop_options.valid = true;
        self.tools.crop_options.fields = None;
        if matches!(mode, CropMode::FixedRatio | CropMode::FixedSize) {
            let values = if mode == CropMode::FixedRatio {
                self.tools.crop_options.ratio
            } else {
                self.tools.crop_options.size
            };
            let width =
                cx.new(|cx| InputState::new(window, cx).default_value(values.0.to_string()));
            let height =
                cx.new(|cx| InputState::new(window, cx).default_value(values.1.to_string()));
            let mut subscriptions = Vec::new();
            for field in [&width, &height] {
                subscriptions.push(cx.subscribe_in(
                    field,
                    window,
                    |this, _, event: &InputEvent, window, cx| match event {
                        InputEvent::PressEnter { .. } => {
                            if this.tools.crop_options.valid {
                                window.focus(&this.canvas_focus, cx);
                                this.tool_commit(cx);
                            }
                        }
                        InputEvent::Change => this.read_crop_fields(cx),
                        _ => {}
                    },
                ));
            }
            self.tools.crop_options.fields = Some(CropFields {
                width,
                height,
                _subscriptions: subscriptions,
            });
        }
        self.refresh_crop_constraint(cx);
        // Popup dismissal restores its trigger after the selection handler.
        // Once it has closed, Enter should apply the pending canvas crop.
        cx.defer_in(window, |this, window, cx| {
            window.focus(&this.canvas_focus, cx);
        });
    }

    fn read_crop_fields(&mut self, cx: &mut Context<Self>) {
        let options = &mut self.tools.crop_options;
        let Some(fields) = &options.fields else {
            return;
        };
        let parse = |field: &Entity<InputState>| {
            field
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| {
                    v.is_finite() && *v > 0. && *v <= emulsion_core::document::MAX_SIDE as f64
                })
        };
        let values = parse(&fields.width).zip(parse(&fields.height));
        options.valid = values.is_some_and(|(w, h)| {
            if options.mode == CropMode::FixedSize {
                w.fract() == 0.
                    && h.fract() == 0.
                    && emulsion_io::import::check_size(w as u32, h as u32).is_ok()
            } else {
                let ratio = w / h;
                let limit = emulsion_core::document::MAX_SIDE as f64;
                ratio.is_finite() && (1. / limit..=limit).contains(&ratio)
            }
        });
        if options.valid {
            let values = values.unwrap();
            if options.mode == CropMode::FixedRatio {
                options.ratio = values;
            } else {
                options.size = values;
            }
        }
        self.refresh_crop_constraint(cx);
    }

    pub(crate) fn crop_options(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let editor = cx.entity().downgrade();
        let mode = self.tools.crop_options.mode;
        let mut row = div().flex().items_center().gap_2().flex_none().child(
            div().id("crop-mode").test_support().child(
                Button::new("crop-mode-button")
                    .label(format!("{} ▾", mode.label()))
                    .small()
                    .rounded_none()
                    .bg(p.soft_bg)
                    .text_color(p.ink)
                    .border_color(p.line)
                    .dropdown_menu(move |mut menu, _, _| {
                        for choice in [
                            CropMode::Free,
                            CropMode::FixedRatio,
                            CropMode::FixedSize,
                            CropMode::Original,
                            CropMode::Ratio(1, 1),
                            CropMode::Ratio(4, 3),
                            CropMode::Ratio(3, 2),
                            CropMode::Ratio(16, 9),
                        ] {
                            let editor = editor.clone();
                            menu = menu.item(
                                PopupMenuItem::new(choice.label())
                                    .checked(choice == mode)
                                    .on_click(move |_, window, cx| {
                                        editor
                                            .update(cx, |this, cx| {
                                                this.set_crop_mode(choice, window, cx)
                                            })
                                            .ok();
                                    }),
                            );
                        }
                        menu
                    }),
            ),
        );
        if let Some(fields) = &self.tools.crop_options.fields {
            row = row
                .child("W")
                .child(
                    div().id("crop-width-field").test_support().w_16().child(
                        Input::new(&fields.width)
                            .id("crop-width")
                            .aria_label("Crop width")
                            .small(),
                    ),
                )
                .child("H")
                .child(
                    div().id("crop-height-field").test_support().w_16().child(
                        Input::new(&fields.height)
                            .id("crop-height")
                            .aria_label("Crop height")
                            .small(),
                    ),
                );
            if mode == CropMode::FixedSize {
                row = row.child("px");
            }
            if !self.tools.crop_options.valid {
                row = row.child(div().text_color(cx.theme().danger).child(
                    if mode == CropMode::FixedSize {
                        "Enter valid whole-pixel dimensions"
                    } else {
                        "Enter positive ratio values"
                    },
                ));
            }
        }
        row.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn crop_ratio_preserves_drag_direction_and_pixel_precision() {
        for (w, h) in [(1., 1.), (4., 3.), (3., 2.), (16., 9.)] {
            for (sx, sy) in [(1., 1.), (-1., 1.), (1., -1.), (-1., -1.)] {
                let (x, y, width, height) = crop_rect(
                    (100., 100.),
                    (100. + sx * 63., 100. + sy * 29.),
                    false,
                    CropConstraint::Ratio(w / h),
                    false,
                );
                assert!((width / (w / h) - height).abs() <= 1.);
                assert_eq!(x, if sx < 0. { 100. - width } else { 100. });
                assert_eq!(y, if sy < 0. { 100. - height } else { 100. });
            }
        }
    }

    #[test]
    fn crop_fixed_size_center_and_shift_obey_selected_constraint() {
        assert_eq!(
            crop_rect(
                (100., 100.),
                (110., 120.),
                true,
                CropConstraint::Size(80., 40.),
                true
            ),
            (60., 80., 80., 40.)
        );
        assert_eq!(
            crop_rect(
                (100., 100.),
                (110., 120.),
                false,
                CropConstraint::Free,
                true
            ),
            (100., 100., 20., 20.)
        );
        assert_eq!(
            crop_rect(
                (100., 100.),
                (110., 120.),
                true,
                CropConstraint::Ratio(2.),
                true
            ),
            (60., 80., 80., 40.)
        );
    }
}
