//! Develop controls share the persisted parameters and worker with Basic.
use super::*;
use emulsion_core::raw::DevelopParams;
use gpui_kit::component::{
    Disableable, Selectable, WindowExt,
    slider::{Slider, SliderEvent, SliderState},
};
#[derive(Clone, Copy)]
pub(super) enum Field {
    Basic(usize),
    Parametric(usize),
    Split(usize),
    Calibration(usize, usize),
    ShadowTint,
    GlobalGrade(usize),
    Blend,
    Balance,
    Detail(usize),
    Crop(usize),
    Straighten,
    Perspective(usize),
    Distortion,
    Aberration(usize),
    Kelvin,
    Curve(usize, usize),
    Hsl(usize, usize),
    Grade(usize, usize),
    Mask(usize, usize),
}
pub(super) fn assign(p: &mut DevelopParams, f: Field, v: f32) {
    match f {
        Field::Basic(i) => match i {
            0 => p.exposure = v,
            1 => p.contrast = v,
            2 => p.highlights = -v,
            3 => p.shadows = v,
            4 => p.blacks = v,
            5 => p.whites = v,
            6 => p.temperature = v,
            7 => p.tint = v,
            8 => p.saturation = v,
            9 => p.vibrance = v,
            10 => p.texture = v,
            11 => p.clarity = v,
            12 => p.dehaze = v,
            13 => p.vignette = v,
            14 => p.sharpening = v,
            15 => p.noise_reduction = v,
            _ => p.sensor_noise_reduction = v,
        },
        Field::Parametric(i) => {
            p.process_version = 2;
            p.parametric[i] = v;
        }
        Field::Split(i) => {
            p.process_version = 2;
            p.parametric_splits[i] = v;
        }
        Field::Calibration(i, j) => {
            p.process_version = 2;
            p.calibration[i][j] = v;
        }
        Field::ShadowTint => {
            p.process_version = 2;
            p.shadow_tint = v;
        }
        Field::GlobalGrade(i) => {
            p.process_version = 2;
            p.global_grading[i] = v;
        }
        Field::Blend => {
            p.process_version = 2;
            p.grading_blending = v;
        }
        Field::Balance => {
            p.process_version = 2;
            p.grading_balance = v;
        }
        Field::Detail(i) => {
            p.process_version = 2;
            match i {
                0 => p.sharpening = v,
                1 => p.sharpening_radius = v,
                2 => p.sharpening_detail = v,
                3 => p.sharpening_masking = v,
                4 => p.noise_reduction = v,
                5 => p.luminance_detail = v,
                6 => p.luminance_contrast = v,
                7 => p.color_noise_reduction = v,
                8 => p.color_noise_detail = v,
                9 => p.color_noise_smoothness = v,
                _ => p.sensor_noise_reduction = v,
            }
        }
        Field::Crop(i) => {
            let (lo, hi) = match i {
                0 => (0., p.crop[2] - 0.001),
                1 => (0., p.crop[3] - 0.001),
                2 => (p.crop[0] + 0.001, 1.),
                _ => (p.crop[1] + 0.001, 1.),
            };
            p.crop[i] = v.clamp(lo, hi);
        }
        Field::Straighten => p.straighten = v,
        Field::Perspective(i) => p.perspective[i] = v,
        Field::Distortion => p.distortion = v,
        Field::Aberration(i) => p.aberration[i] = v,
        Field::Kelvin => {
            p.kelvin = Some(v);
            p.wb_override = None;
            p.temperature = 0.;
        }
        Field::Curve(channel, i) => {
            materialize_curve(p, channel);
            p.smooth_point_curves[channel] = true;
            p.point_curves[channel].points[i][1] = v.clamp(0., 1.);
        }
        Field::Hsl(i, j) => p.hsl[i][j] = v,
        Field::Grade(i, j) => p.grading[i][j] = v,
        Field::Mask(i, j) => {
            let m = &mut p.masks[i];
            match j {
                0 => m.center[0] = v,
                1 => m.center[1] = v,
                2 => m.radius[0] = v,
                3 => m.radius[1] = v,
                4 => m.feather = v,
                5 => m.exposure = v,
                6 => m.saturation = v,
                _ => m.temperature = v,
            };
        }
    }
}
fn materialize_curve(p: &mut DevelopParams, channel: usize) {
    if p.point_curves[channel].len == 0 {
        p.smooth_point_curves[channel] = channel != 0 || p.smooth_curve;
        let points = if channel == 0 {
            (0..5)
                .map(|i| [i as f32 / 4., p.curve_output(i as f32 / 4.)])
                .collect()
        } else {
            vec![[0., 0.], [1., 1.]]
        };
        p.point_curves[channel] = emulsion_core::raw::PointCurve::try_from(points).unwrap();
    }
    if channel == 0 {
        p.tone_curve = DevelopParams::LINEAR_CURVE;
    }
}
impl Workspace {
    pub(super) fn library_advanced_panel(
        &mut self,
        path: PathBuf,
        params: DevelopParams,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = classic::palette(cx);
        let section = self.batch.develop.section;
        let mut panel = div().flex().flex_col().gap_1();
        let mut fields: Vec<(&str, Field, f32, f32, f32, f32)> = Vec::new();
        match section {
            1 => {
                panel = panel.child(label("Crop and geometry", &palette));
                for (tool, name) in [
                    (5, "Draw crop"),
                    (6, "Straighten line"),
                    (7, "Perspective guide"),
                ] {
                    panel = panel.child(
                        Button::new(("develop-geometry-tool", tool))
                            .label(name)
                            .small()
                            .outline()
                            .selected(self.batch.develop.canvas_tool == tool)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.develop.canvas_tool =
                                    if this.batch.develop.canvas_tool == tool {
                                        0
                                    } else {
                                        tool
                                    };
                                this.invalidate_library_preview();
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(
                    Button::new("develop-geometry-done")
                        .label("Done")
                        .small()
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch.develop.canvas_tool = 0;
                            this.invalidate_library_preview();
                            cx.notify();
                        })),
                );
                panel = panel.child(
                    Button::new("library-lens-auto")
                        .label("Match lens profile")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.library_match_lens(cx))),
                );
                if params.lens_profile.is_some() {
                    panel = panel.child(
                        Button::new("library-lens-clear")
                            .label("Remove measured profile")
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.library_adjust(
                                    DevelopParams {
                                        lens_profile: None,
                                        ..params
                                    },
                                    cx,
                                )
                            })),
                    );
                }
                for (i, name) in ["Left", "Top", "Right", "Bottom"].into_iter().enumerate() {
                    fields.push((name, Field::Crop(i), params.crop[i], 0., 1., 0.001));
                }
                fields.extend([
                    (
                        "Straighten °",
                        Field::Straighten,
                        params.straighten,
                        -45.,
                        45.,
                        0.1,
                    ),
                    (
                        "Horizontal",
                        Field::Perspective(0),
                        params.perspective[0],
                        -0.8,
                        0.8,
                        0.01,
                    ),
                    (
                        "Vertical",
                        Field::Perspective(1),
                        params.perspective[1],
                        -0.8,
                        0.8,
                        0.01,
                    ),
                    (
                        "Distortion",
                        Field::Distortion,
                        params.distortion,
                        -0.5,
                        0.5,
                        0.001,
                    ),
                    (
                        "Red fringe",
                        Field::Aberration(0),
                        params.aberration[0],
                        -0.05,
                        0.05,
                        0.0001,
                    ),
                    (
                        "Blue fringe",
                        Field::Aberration(1),
                        params.aberration[1],
                        -0.05,
                        0.05,
                        0.0001,
                    ),
                ]);
                let mut ratios = div().flex().flex_wrap().gap_1();
                for (i, (name, ratio)) in [
                    ("Original", 0.),
                    ("1:1", 1.),
                    ("4:3", 4. / 3.),
                    ("3:2", 1.5),
                    ("16:9", 16. / 9.),
                ]
                .into_iter()
                .enumerate()
                {
                    ratios = ratios.child(
                        Button::new(("library-crop-ratio", i))
                            .disabled(self.batch.develop.busy || self.batch.develop.preview_stale)
                            .label(name)
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some(source) = this.batch.develop.source.as_ref() else {
                                    return;
                                };
                                let mut next = params;
                                next.crop = [0., 0., 1., 1.];
                                if ratio > 0. {
                                    let ratio = if next.rotation % 2 == 1 {
                                        1. / ratio
                                    } else {
                                        ratio
                                    };
                                    // Recover the source aspect from the oriented preview, including camera EXIF orientation.
                                    let dimensions = this.batch.navigation.borrow().dimensions;
                                    let (mut w, mut h) = (dimensions.0 as f32, dimensions.1 as f32);
                                    if params.rotation % 2 == 1 && !this.batch.develop.before {
                                        std::mem::swap(&mut w, &mut h);
                                    }
                                    if this.batch.develop.canvas_tool == 0
                                        && !this.batch.develop.before
                                    {
                                        w /= (params.crop[2] - params.crop[0]).max(0.001);
                                        h /= (params.crop[3] - params.crop[1]).max(0.001);
                                    }
                                    let original = if w > 0. && h > 0. {
                                        w / h
                                    } else {
                                        source.info.width as f32 / source.info.height.max(1) as f32
                                    };
                                    if original > ratio {
                                        let width = ratio / original;
                                        next.crop = [(1. - width) / 2., 0., (1. + width) / 2., 1.];
                                    } else {
                                        let height = original / ratio;
                                        next.crop =
                                            [0., (1. - height) / 2., 1., (1. + height) / 2.];
                                    }
                                }
                                this.batch.develop.slider_key = None;
                                this.library_adjust(next, cx);
                            })),
                    );
                }
                panel = panel.child(ratios);
            }
            2 => {
                let channel = self.batch.develop.channel.min(3);
                let mut channels = div().flex().flex_wrap().gap_1();
                for (i, name) in ["RGB", "Red", "Green", "Blue"].into_iter().enumerate() {
                    channels = channels.child(
                        Button::new(("library-curve-channel", i))
                            .label(name)
                            .small()
                            .selected(channel == i)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.develop.channel = i;
                                this.batch.develop.slider_key = None;
                                cx.notify();
                            })),
                    );
                }
                panel = panel
                    .child(channels)
                    .child(self.library_curve_graph(params, cx))
                    .child(mono(
                        "Click to add · drag to move · right-click to remove",
                        10.,
                        palette.muted,
                    ));
                let mut current = params;
                materialize_curve(&mut current, channel);
                for i in 0..current.point_curves[channel].len as usize {
                    fields.push((
                        "Point output",
                        Field::Curve(channel, i),
                        current.point_curves[channel].points[i][1],
                        0.,
                        1.,
                        0.005,
                    ));
                }
                panel = panel.child(
                    Button::new("library-curve-reset")
                        .label("Reset channel")
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut p = params;
                            p.point_curves[channel] = Default::default();
                            if channel == 0 {
                                p.tone_curve = DevelopParams::LINEAR_CURVE;
                            }
                            this.batch.develop.slider_key = None;
                            this.library_adjust(p, cx);
                        })),
                );
            }
            3 => {
                let mut colors = div().flex().flex_wrap().gap_1();
                for (i, name) in [
                    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
                ]
                .into_iter()
                .enumerate()
                {
                    colors = colors.child(
                        Button::new(("library-mixer-color", i))
                            .label(name)
                            .small()
                            .ghost()
                            .selected(self.batch.develop.channel == i)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.develop.channel = i;
                                this.batch.develop.slider_key = None;
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(label("Color mixer", &palette)).child(colors);
                let i = self.batch.develop.channel.min(7);
                for (j, name) in ["Hue", "Saturation", "Luminance"].into_iter().enumerate() {
                    fields.push((name, Field::Hsl(i, j), params.hsl[i][j], -1., 1., 0.01));
                }
            }
            4 => {
                panel = panel.child(label("Color grading", &palette));
                for (i, name) in ["Shadows", "Midtones", "Highlights"]
                    .into_iter()
                    .enumerate()
                {
                    fields.extend([
                        (name, Field::Grade(i, 0), params.grading[i][0], 0., 360., 1.),
                        (
                            "Saturation",
                            Field::Grade(i, 1),
                            params.grading[i][1],
                            0.,
                            1.,
                            0.01,
                        ),
                        (
                            "Luminance",
                            Field::Grade(i, 2),
                            params.grading[i][2],
                            -1.,
                            1.,
                            0.01,
                        ),
                    ]);
                }
            }
            5 => {
                panel = panel.child(self.library_local_panel(cx));
                panel = panel.child(label("Local adjustments", &palette));
                let mut buttons = div().flex().flex_wrap().gap_1();
                for i in 0..8 {
                    buttons = buttons.child(
                        Button::new(("library-mask-slot", i))
                            .label(format!(
                                "{}{}",
                                i + 1,
                                if params.masks[i].enabled { " •" } else { "" }
                            ))
                            .small()
                            .ghost()
                            .selected(self.batch.develop.mask == i)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.develop.mask = i;
                                this.batch.develop.slider_key = None;
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(buttons);
                let i = self.batch.develop.mask.min(7);
                let m = params.masks[i];
                for (j, title, checked) in [
                    (0, "Enabled", m.enabled),
                    (1, "Linear gradient", m.linear),
                    (2, "Invert", m.inverted),
                ] {
                    panel = panel.child(
                        Checkbox::new(("library-mask-option", j as usize))
                            .label(title)
                            .checked(checked)
                            .on_change(cx.listener(move |this, value, _, cx| {
                                let mut next = params;
                                match j {
                                    0 => next.masks[i].enabled = *value,
                                    1 => next.masks[i].linear = *value,
                                    _ => next.masks[i].inverted = *value,
                                };
                                this.library_adjust(next, cx);
                            })),
                    );
                }
                fields.extend([
                    ("Center X", Field::Mask(i, 0), m.center[0], 0., 1., 0.01),
                    ("Center Y", Field::Mask(i, 1), m.center[1], 0., 1., 0.01),
                    ("Width", Field::Mask(i, 2), m.radius[0], 0.001, 2., 0.01),
                    ("Height", Field::Mask(i, 3), m.radius[1], 0.001, 2., 0.01),
                    ("Feather", Field::Mask(i, 4), m.feather, 0.001, 1., 0.01),
                    ("Exposure EV", Field::Mask(i, 5), m.exposure, -5., 5., 0.05),
                    ("Saturation", Field::Mask(i, 6), m.saturation, -1., 1., 0.01),
                    (
                        "Temperature",
                        Field::Mask(i, 7),
                        m.temperature,
                        -1.,
                        1.,
                        0.01,
                    ),
                ]);
            }
            9 => {
                let values = [
                    params.sharpening,
                    params.sharpening_radius,
                    params.sharpening_detail,
                    params.sharpening_masking,
                    params.noise_reduction,
                    params.luminance_detail,
                    params.luminance_contrast,
                    params.color_noise_reduction,
                    params.color_noise_detail,
                    params.color_noise_smoothness,
                    params.sensor_noise_reduction,
                ];
                for (i, name) in [
                    "Sharpening",
                    "Radius (pixels)",
                    "Detail",
                    "Edge masking",
                    "Luminance noise",
                    "Luminance detail",
                    "Luminance contrast",
                    "Color noise",
                    "Color detail",
                    "Color smoothness",
                    "Sensor denoise (RAW)",
                ]
                .into_iter()
                .enumerate()
                {
                    if i == 10 && !emulsion_io::photo_develop::is_raw_photo(&path) {
                        continue;
                    }
                    fields.push((
                        name,
                        Field::Detail(i),
                        values[i],
                        if i == 1 { 0.5 } else { 0. },
                        if i == 1 { 3. } else { 1. },
                        0.01,
                    ));
                }
            }
            10 => {
                fields.push((
                    "Shadow tint",
                    Field::ShadowTint,
                    params.shadow_tint,
                    -1.,
                    1.,
                    0.01,
                ));
                for (i, names) in [
                    ["Red primary hue", "Red primary saturation"],
                    ["Green primary hue", "Green primary saturation"],
                    ["Blue primary hue", "Blue primary saturation"],
                ]
                .into_iter()
                .enumerate()
                {
                    for (j, name) in names.into_iter().enumerate() {
                        fields.push((
                            name,
                            Field::Calibration(i, j),
                            params.calibration[i][j],
                            -1.,
                            1.,
                            0.01,
                        ));
                    }
                }
            }
            11 => {
                for (i, name) in ["Shadows", "Darks", "Lights", "Highlights"]
                    .into_iter()
                    .enumerate()
                {
                    fields.push((
                        name,
                        Field::Parametric(i),
                        params.parametric[i],
                        -1.,
                        1.,
                        0.01,
                    ));
                }
                for (i, name) in ["Shadow split", "Midtone split", "Highlight split"]
                    .into_iter()
                    .enumerate()
                {
                    fields.push((
                        name,
                        Field::Split(i),
                        params.parametric_splits[i],
                        0.01,
                        0.99,
                        0.01,
                    ));
                }
            }
            _ => {
                panel = panel.child(label("White balance", &palette));
                fields.push((
                    "Temperature K",
                    Field::Kelvin,
                    params.kelvin.unwrap_or(6504.),
                    2000.,
                    50000.,
                    50.,
                ));
                panel = panel.child(mono(
                    if params.kelvin.is_none() {
                        "As shot · move slider to set Kelvin"
                    } else {
                        "Custom illuminant"
                    },
                    11.,
                    palette.muted,
                ));
            }
        }
        if section == 4 {
            fields.extend([
                (
                    "Global hue",
                    Field::GlobalGrade(0),
                    params.global_grading[0],
                    0.,
                    360.,
                    1.,
                ),
                (
                    "Global saturation",
                    Field::GlobalGrade(1),
                    params.global_grading[1],
                    0.,
                    1.,
                    0.01,
                ),
                (
                    "Global luminance",
                    Field::GlobalGrade(2),
                    params.global_grading[2],
                    -1.,
                    1.,
                    0.01,
                ),
                (
                    "Blending",
                    Field::Blend,
                    params.grading_blending,
                    0.,
                    1.,
                    0.01,
                ),
                (
                    "Balance",
                    Field::Balance,
                    params.grading_balance,
                    -1.,
                    1.,
                    0.01,
                ),
            ]);
        }
        if self.batch.develop.slider_key.as_ref() != Some(&(path.clone(), params)) {
            self.batch.develop.sliders.clear();
            for &(_, field, value, min, max, step) in &fields {
                let slider = cx.new(|_| {
                    SliderState::new()
                        .max(max)
                        .min(min)
                        .step(step)
                        .default_value(value)
                });
                let sub = cx.subscribe(&slider, move |this, _, event, cx| {
                    if let SliderEvent::Release(_) = event {
                        this.batch.develop.gesture_active = false;
                        this.batch.develop.gesture_recorded = false;
                        return;
                    }
                    if let SliderEvent::Change(value) = event {
                        this.batch.develop.gesture_active = true;
                        let Some(path) = this
                            .batch
                            .current
                            .and_then(|i| this.batch.items.get(i))
                            .map(|i| i.path.clone())
                        else {
                            return;
                        };
                        let Some(mut p) = this.batch.develop.current_params(&path) else {
                            return;
                        };
                        assign(&mut p, field, value.end());
                        if p.validate().is_err() {
                            return;
                        }
                        this.batch.develop.slider_key = Some((path, p));
                        this.library_adjust(p, cx);
                    }
                });
                self.batch.develop.sliders.push((slider, sub));
            }
            self.batch.develop.slider_key = Some((path, params));
        }
        for (index, (name, field, value, min, max, step)) in fields.into_iter().enumerate() {
            panel = panel.child(
                div()
                    .id(("library-advanced-row", index))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(27.))
                    .child(
                        div()
                            .w(px(86.))
                            .flex_none()
                            .child(self.library_control_label(index, name, field, cx)),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Slider::new(&self.batch.develop.sliders[index].0)
                                .disabled(self.batch.develop.saving),
                        ),
                    )
                    .child(
                        self.library_numeric_control(index, name, field, value, min, max, step, cx),
                    ),
            );
        }
        panel.into_any_element()
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Keeps existing explicit workflow inputs together"
    )]
    pub(super) fn library_numeric_control(
        &self,
        index: usize,
        name: &str,
        field: Field,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = name.to_owned();
        div()
            .id(("develop-number-control", index))
            .flex()
            .gap_1()
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let delta = match event.keystroke.key.as_str() {
                    "left" | "down" => -step,
                    "right" | "up" => step,
                    _ => return,
                };
                if this.batch.develop.saving {
                    return;
                }
                let Some(path) = this
                    .batch
                    .current
                    .and_then(|i| this.batch.items.get(i))
                    .map(|i| i.path.clone())
                else {
                    return;
                };
                let Some(mut p) = this.batch.develop.current_params(&path) else {
                    return;
                };
                assign(&mut p, field, (value + delta).clamp(min, max));
                if p.validate().is_ok() {
                    this.library_adjust(p, cx);
                }
                cx.stop_propagation();
            }))
            .child(
                Button::new(("develop-number", index))
                    .label(format!("{value:.2}"))
                    .w(px(48.))
                    .px_0()
                    .text_size(px(10.))
                    .small()
                    .ghost()
                    .tooltip("Enter an exact value")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let Some(path) = this
                            .batch
                            .current
                            .and_then(|i| this.batch.items.get(i))
                            .map(|i| i.path.clone())
                        else {
                            return;
                        };
                        let input = cx.new(|cx| {
                            InputState::new(window, cx).default_value(format!("{value}"))
                        });
                        let owner = cx.weak_entity();
                        let title = name.clone();
                        window.open_dialog(cx, move |dialog, _, _| {
                            let input = input.clone();
                            let submitted = input.clone();
                            let owner = owner.clone();
                            let path = path.clone();
                            dialog
                                .title(title.clone())
                                .child(Input::new(&input))
                                .child(format!("Range: {min} to {max}"))
                                .footer(crate::widgets::form_dialog_footer("Apply"))
                                .on_ok(move |_, _, cx| {
                                    let Ok(v) = submitted.read(cx).value().parse::<f32>() else {
                                        return false;
                                    };
                                    if !v.is_finite() || !(min..=max).contains(&v) {
                                        return false;
                                    }
                                    owner
                                        .update(cx, |this, cx| {
                                            if this.batch.develop.saving
                                                || this
                                                    .batch
                                                    .current
                                                    .and_then(|i| this.batch.items.get(i))
                                                    .is_none_or(|i| i.path != path)
                                            {
                                                return false;
                                            }
                                            let Some(mut p) =
                                                this.batch.develop.current_params(&path)
                                            else {
                                                return false;
                                            };
                                            assign(&mut p, field, v);
                                            if p.validate().is_err() {
                                                return false;
                                            }
                                            this.batch.develop.gesture_active = false;
                                            this.batch.develop.gesture_recorded = false;
                                            this.library_adjust(p, cx);
                                            true
                                        })
                                        .unwrap_or(false)
                                })
                        });
                    })),
            )
            .child(
                Button::new(("develop-reset-control", index))
                    .label("↺")
                    .w(px(18.))
                    .px_0()
                    .small()
                    .ghost()
                    .tooltip("Reset this control")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library_reset_field(field, cx);
                    })),
            )
            .into_any_element()
    }
    pub(super) fn library_reset_field(&mut self, field: Field, cx: &mut Context<Self>) {
        if self.batch.develop.saving {
            return;
        }
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        let Some(mut p) = self.batch.develop.current_params(&path) else {
            return;
        };
        let default = match field {
            Field::Crop(i) => [0., 0., 1., 1.][i],
            Field::Kelvin => 6504.,
            Field::Split(i) => [0.25, 0.5, 0.75][i],
            Field::Blend => 0.5,
            Field::Detail(1) => 0.8,
            Field::Detail(2 | 5 | 8 | 9) => 0.5,
            Field::Mask(_, 0 | 1) => 0.5,
            Field::Mask(_, 2 | 3) => 0.25,
            Field::Mask(_, 4) => 0.5,
            Field::Curve(channel, i) => p.point_curves[channel].points[i][0],
            _ => 0.,
        };
        if matches!(field, Field::Kelvin) {
            p.kelvin = None;
            p.wb_override = None;
            p.temperature = 0.;
        } else {
            assign(&mut p, field, default);
        }
        if p.validate().is_ok() {
            self.library_adjust(p, cx);
        }
    }
    pub(super) fn library_control_label(
        &self,
        index: usize,
        name: &str,
        field: Field,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("develop-control-label", index))
            .text_size(px(10.))
            .text_color(classic::palette(cx).muted)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(name.to_owned())
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() == 2 {
                    this.library_reset_field(field, cx);
                }
            }))
            .into_any_element()
    }
    fn library_curve_graph(&self, params: DevelopParams, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let channel = self.batch.develop.channel.min(3);
        let mut params = params;
        materialize_curve(&mut params, channel);
        let track = self.batch.develop.curve_bounds.clone();
        let paint = track.clone();
        let line = p.line;
        let ink = p.ink;
        let histogram = self.batch.develop.histogram;
        let peak = histogram.iter().copied().max().unwrap_or(1).max(1) as f32;
        div()
            .id("library-curve-graph")
            .test_support()
            .w_full()
            .h(px(220.))
            .bg(p.stage)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let Some(bounds) = track.get() else {
                        return;
                    };
                    if this.batch.develop.saving {
                        return;
                    }
                    let x = (f32::from(event.position.x - bounds.origin.x)
                        / f32::from(bounds.size.width))
                    .clamp(0., 1.);
                    let y = 1.
                        - (f32::from(event.position.y - bounds.origin.y)
                            / f32::from(bounds.size.height))
                        .clamp(0., 1.);
                    let mut next = params;
                    next.smooth_point_curves[channel] = true;
                    this.batch.develop.gesture_active = true;
                    this.batch.develop.gesture_recorded = false;
                    let curve = &mut next.point_curves[channel];
                    let mut points = Vec::from(*curve);
                    let nearest = points
                        .iter()
                        .enumerate()
                        .min_by(|a, b| (a.1[0] - x).abs().total_cmp(&(b.1[0] - x).abs()))
                        .unwrap()
                        .0;
                    let index = if (points[nearest][0] - x).abs() < 0.04 || points.len() == 32 {
                        nearest
                    } else {
                        let at = points.partition_point(|p| p[0] < x);
                        points.insert(at, [x, y]);
                        at
                    };
                    points[index][1] = y;
                    *curve = emulsion_core::raw::PointCurve::try_from(points).unwrap();
                    this.batch.develop.curve_drag = Some(index);
                    this.batch.develop.slider_key = None;
                    this.library_adjust(next, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let Some(bounds) = this.batch.develop.curve_bounds.get() else {
                        return;
                    };
                    let x = (f32::from(event.position.x - bounds.origin.x)
                        / f32::from(bounds.size.width))
                    .clamp(0., 1.);
                    let mut next = params;
                    next.smooth_point_curves[channel] = true;
                    let mut points = Vec::from(next.point_curves[channel]);
                    if points.len() > 2 {
                        let index = points
                            .iter()
                            .enumerate()
                            .min_by(|a, b| (a.1[0] - x).abs().total_cmp(&(b.1[0] - x).abs()))
                            .unwrap()
                            .0;
                        points.remove(index);
                        next.point_curves[channel] =
                            emulsion_core::raw::PointCurve::try_from(points).unwrap();
                        this.batch.develop.slider_key = None;
                        this.library_adjust(next, cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.batch.develop.curve_drag.take().is_some() {
                        this.batch.develop.gesture_active = false;
                        this.batch.develop.gesture_recorded = false;
                        this.library_schedule_save(cx);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.batch.develop.curve_drag = None;
                    this.batch.develop.gesture_active = false;
                    this.batch.develop.gesture_recorded = false;
                    this.library_schedule_save(cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    this.batch.develop.curve_drag = None;
                    this.batch.develop.gesture_active = false;
                    this.batch.develop.gesture_recorded = false;
                    return;
                }
                let Some(i) = this.batch.develop.curve_drag else {
                    return;
                };
                let Some(bounds) = this.batch.develop.curve_bounds.get() else {
                    return;
                };
                let Some(mut p) = this
                    .batch
                    .current
                    .and_then(|i| this.batch.items.get(i))
                    .and_then(|i| this.batch.develop.current_params(&i.path))
                else {
                    return;
                };
                let y = 1.
                    - (f32::from(event.position.y - bounds.origin.y)
                        / f32::from(bounds.size.height))
                    .clamp(0., 1.);
                materialize_curve(&mut p, channel);
                p.smooth_point_curves[channel] = true;
                let x = (f32::from(event.position.x - bounds.origin.x)
                    / f32::from(bounds.size.width))
                .clamp(0., 1.);
                let curve = &mut p.point_curves[channel];
                if i >= curve.len as usize {
                    return;
                }
                let min = if i == 0 {
                    0.
                } else {
                    curve.points[i - 1][0] + 0.0001
                };
                let max = if i + 1 == curve.len as usize {
                    1.
                } else {
                    curve.points[i + 1][0] - 0.0001
                };
                if min <= max {
                    curve.points[i][0] = x.clamp(min, max);
                }
                curve.points[i][1] = y;
                this.batch.develop.slider_key = None;
                this.library_adjust(p, cx);
            }))
            .child(
                canvas(
                    move |bounds, _, _| paint.set(Some(bounds)),
                    move |bounds, _, window, _| {
                        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                        let at =
                            |x: f32, y: f32| bounds.origin + point(px(x * w), px((1. - y) * h));
                        let mut distribution = PathBuilder::fill();
                        distribution.move_to(at(0., 0.));
                        for (bin, count) in histogram.into_iter().enumerate() {
                            distribution.line_to(at(bin as f32 / 31., count as f32 / peak * 0.9));
                        }
                        distribution.line_to(at(1., 0.));
                        distribution.close();
                        if let Ok(path) = distribution.build() {
                            window.paint_path(path, ink.opacity(0.16));
                        }
                        for i in 1..4 {
                            let f = i as f32 / 4.;
                            window.paint_quad(fill(
                                Bounds::new(at(f, 1.), size(px(1.), px(h))),
                                line,
                            ));
                            window.paint_quad(fill(
                                Bounds::new(at(0., f), size(px(w), px(1.))),
                                line,
                            ));
                        }
                        let mut path = PathBuilder::stroke(px(2.));
                        path.move_to(at(0., params.point_curve_output(channel, 0.)));
                        for i in 1..=128 {
                            let x = i as f32 / 128.;
                            path.line_to(at(x, params.point_curve_output(channel, x)));
                        }
                        for p in &params.point_curves[channel].points
                            [..params.point_curves[channel].len as usize]
                        {
                            window.paint_quad(fill(
                                Bounds::new(
                                    at(p[0], p[1]) - point(px(3.), px(3.)),
                                    size(px(6.), px(6.)),
                                ),
                                ink,
                            ));
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, ink);
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}

impl Workspace {
    pub(super) fn library_history_panel(
        &self,
        path: PathBuf,
        params: DevelopParams,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = classic::palette(cx);
        let snapshot_path = path.clone();
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label("Snapshots", &p));
        panel = panel.child(
            Button::new("library-snapshot-save")
                .label("New snapshot")
                .small()
                .outline()
                .disabled(self.batch.develop.saving)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let path = snapshot_path.clone();
                    let Some(digest) = this.batch.develop.fingerprints.get(&path).cloned() else {
                        return;
                    };
                    let snapshots = this
                        .batch
                        .develop
                        .snapshots
                        .get(&path)
                        .cloned()
                        .unwrap_or_default();
                    let name = (1..)
                        .map(|i| format!("Snapshot {i}"))
                        .find(|n| !snapshots.contains_key(n))
                        .unwrap();
                    this.batch.develop.saving = true;
                    cx.spawn(async move |this, cx| {
                        let file = path.clone();
                        let title = name.clone();
                        let result = cx
                            .background_spawn(async move {
                                emulsion_io::raw_settings::save_snapshot(
                                    &file, &digest, &title, params,
                                )
                            })
                            .await;
                        this.update(cx, |this, cx| {
                            this.batch.develop.saving = false;
                            match result {
                                Ok(()) => {
                                    this.batch
                                        .develop
                                        .snapshots
                                        .entry(path)
                                        .or_default()
                                        .insert(name, params);
                                }
                                Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                            };
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                })),
        );
        if let Some(snapshots) = self.batch.develop.snapshots.get(&path) {
            for (index, (name, params)) in snapshots.iter().enumerate() {
                let params = *params;
                panel = panel.child(
                    Button::new(("library-snapshot", index))
                        .label(name.clone())
                        .small()
                        .ghost()
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.library_adjust(params, cx)),
                        ),
                );
            }
        }
        panel = panel.child(label("Saved history · newest first", &p));
        if let Some(history) = self.batch.develop.history.get(&path) {
            for (index, params) in history.iter().enumerate().rev().take(30) {
                let params = *params;
                panel = panel.child(
                    Button::new(("library-history-step", index))
                        .label(format!("Step {} · {:+.2} EV", index + 1, params.exposure))
                        .small()
                        .ghost()
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.library_adjust(params, cx)),
                        ),
                );
            }
        }
        panel.into_any_element()
    }
}

impl Workspace {
    fn library_match_lens(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        let Some(params) = self.batch.develop.current_params(&path) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let file = path.clone();
            let result = cx
                .background_spawn(async move { emulsion_io::photo_develop::matched_lens(&file) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(profile) => {
                        if this
                            .batch
                            .current
                            .and_then(|i| this.batch.items.get(i))
                            .map(|i| &i.path)
                            == Some(&path)
                            && this.batch.develop.current_params(&path) == Some(params)
                        {
                            this.library_adjust(
                                DevelopParams {
                                    lens_profile: Some(profile),
                                    ..params
                                },
                                cx,
                            );
                        }
                    }
                    Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Workspace {
    pub(super) fn library_output_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let settings = &self.batch.output_settings;
        let mut panel =
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(mono("Long edge · no enlargement", 10., p.muted));
        let mut sizes = div().flex().flex_wrap().gap_1();
        for (i, (name, value)) in [
            ("Original", 0),
            ("2048", 2048),
            ("3840", 3840),
            ("6000", 6000),
        ]
        .into_iter()
        .enumerate()
        {
            sizes = sizes.child(
                Button::new(("library-output-size", i))
                    .label(name)
                    .small()
                    .ghost()
                    .selected(settings.long_edge == value)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.output_settings.long_edge = value;
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(sizes);
        let mut spaces = div().flex().flex_wrap().gap_1();
        for (i, (title, space)) in [
            ("sRGB", emulsion_io::photo_color::Space::Srgb),
            ("Adobe RGB", emulsion_io::photo_color::Space::AdobeRgb),
            ("ProPhoto RGB", emulsion_io::photo_color::Space::ProPhoto),
        ]
        .into_iter()
        .enumerate()
        {
            spaces = spaces.child(
                Button::new(("library-output-color", i))
                    .label(title)
                    .small()
                    .ghost()
                    .selected(settings.color_space == space)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.output_settings.color_space = space;
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(label("Output color space", &p)).child(spaces);
        let mut quality = div().flex().flex_wrap().gap_1();
        for (i, value) in [80, 92, 100].into_iter().enumerate() {
            quality = quality.child(
                Button::new(("library-output-quality", i))
                    .label(format!("JPEG {value}%"))
                    .small()
                    .ghost()
                    .selected(settings.jpeg_quality == value)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.output_settings.jpeg_quality = value;
                        cx.notify();
                    })),
            );
        }
        let mut policies = div().flex().flex_wrap().gap_1();
        for (i, (name, policy)) in [
            ("No metadata", emulsion_io::photo_metadata::Policy::None),
            ("Copyright", emulsion_io::photo_metadata::Policy::Copyright),
            ("Camera", emulsion_io::photo_metadata::Policy::Camera),
            (
                "Camera + GPS",
                emulsion_io::photo_metadata::Policy::CameraAndLocation,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            policies = policies.child(
                Button::new(("library-output-metadata", i))
                    .label(name)
                    .small()
                    .selected(self.batch.output_settings.metadata == policy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.output_settings.metadata = policy;
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(label("Embedded metadata", &p)).child(policies);
        panel = panel.child(quality);
        panel = panel.child(
            Checkbox::new("library-output-sharpen")
                .label("Output sharpening")
                .checked(settings.sharpening > 0.)
                .on_change(cx.listener(|this, checked, _, cx| {
                    this.batch.output_settings.sharpening = if *checked { 0.35 } else { 0. };
                    cx.notify();
                })),
        );
        panel = panel.child(
            Button::new("library-watermark")
                .label(if settings.watermark.is_some() {
                    "Remove watermark"
                } else {
                    "Add image watermark…"
                })
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.batch.output_settings.watermark.take().is_some() {
                        cx.notify();
                        return;
                    }
                    let rx = cx.prompt_for_paths(PathPromptOptions {
                        files: true,
                        directories: false,
                        multiple: false,
                        prompt: Some("Choose watermark image".into()),
                    });
                    cx.spawn(async move |this, cx| {
                        if let Ok(Ok(Some(paths))) = rx.await {
                            this.update(cx, |this, cx| {
                                this.batch.output_settings.watermark = paths.into_iter().next();
                                cx.notify();
                            })
                            .ok();
                        }
                    })
                    .detach();
                })),
        );
        panel = panel.child(
            Button::new("library-publish-config")
                .label("Load WebDAV destination…")
                .small()
                .outline()
                .on_click(cx.listener(|_this, _, _, cx| {
                    let picker = cx.prompt_for_paths(PathPromptOptions {
                        files: true,
                        directories: false,
                        multiple: false,
                        prompt: Some(
                            "Choose publish destination JSON (url, authorization_env)".into(),
                        ),
                    });
                    cx.spawn(async move |this, cx| {
                        let Ok(Ok(Some(paths))) = picker.await else {
                            return;
                        };
                        let Some(path) = paths.into_iter().next() else {
                            return;
                        };
                        let result = cx
                            .background_spawn(async move {
                                emulsion_io::photo_publish::Destination::load(&path)
                            })
                            .await;
                        this.update(cx, |this, cx| {
                            match result {
                                Ok(destination) => {
                                    this.batch.output_settings.publish = Some(destination);
                                    this.batch.note = Some((
                                        "Publishing enabled for subsequent exports".into(),
                                        false,
                                    ));
                                }
                                Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                            };
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                })),
        );
        if let Some(destination) = &self.batch.output_settings.publish {
            panel = panel
                .child(mono(format!("Publish: {}", destination.url), 10., p.muted))
                .child(
                    Button::new("library-publish-disable")
                        .label("Disable publishing")
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch.output_settings.publish = None;
                            cx.notify();
                        })),
                );
        }
        for (save, title) in [
            (true, "Save export preset…"),
            (false, "Load export preset…"),
        ] {
            panel = panel.child(
                Button::new(if save {
                    "library-output-save"
                } else {
                    "library-output-load"
                })
                .label(title)
                .small()
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| this.library_output_preset(save, cx))),
            );
        }
        panel
            .child(mono(
                "Serial numbers and maker notes are never exported. GPS is opt-in.",
                10.,
                p.muted,
            ))
            .into_any_element()
    }
    fn library_output_preset(&mut self, save: bool, cx: &mut Context<Self>) {
        let settings = self.batch.output_settings.clone();
        let pick = if save {
            let rx = cx.prompt_for_new_path(
                self.batch.out_dir.as_deref().unwrap_or(Path::new(".")),
                Some("export-preset.json"),
            );
            cx.spawn(async move |_, _| rx.await.ok().and_then(|r| r.ok()).flatten())
        } else {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Load export preset".into()),
            });
            cx.spawn(async move |_, _| {
                rx.await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
                    .and_then(|v| v.into_iter().next())
            })
        };
        cx.spawn(async move |this, cx| {
            let Some(path) = pick.await else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    if save {
                        emulsion_io::photo_export::save_preset(&path, &settings).map(|_| None)
                    } else {
                        emulsion_io::photo_export::load_preset(&path).map(Some)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Some(s)) => this.batch.output_settings = s,
                    Ok(None) => this.batch.note = Some(("Export preset saved".into(), false)),
                    Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Workspace {
    pub(super) fn library_catalog_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div().flex().flex_col().gap_1().child(
            Button::new("library-relink-root")
                .label("Relink folder root…")
                .small()
                .ghost()
                .on_click(
                    cx.listener(|this, _, window, cx| this.library_relink_root_dialog(window, cx)),
                ),
        );
        panel = panel.child(
            Button::new("library-maintain-cache")
                .label("Manage preview storage")
                .small()
                .ghost()
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.spawn(async move |this, cx| {
                        let result = cx
                            .background_spawn(async { emulsion_io::thumb::maintain_cache() })
                            .await;
                        this.update(cx, |this, cx| {
                            this.batch.note = Some(match result {
                                Ok(r) => (
                                    format!(
                                        "Preview storage: {} files, {:.1} MiB; reclaimed {:.1} MiB",
                                        r.files,
                                        r.bytes as f64 / 1048576.,
                                        r.reclaimed_bytes as f64 / 1048576.
                                    )
                                    .into(),
                                    false,
                                ),
                                Err(e) => (e.to_string().into(), true),
                            });
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                })),
        );
        for (i, title) in [
            "Create virtual copy",
            "Stack selection",
            "Unstack selection",
            "Save filters as smart collection",
            "Back up library + files…",
            "Import Lightroom catalog…",
            "Restore catalog backup…",
        ]
        .into_iter()
        .enumerate()
        {
            panel = panel.child(
                Button::new(("library-catalog-action", i))
                    .label(title)
                    .small()
                    .ghost()
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.library_catalog_action(i, cx)),
                    ),
            );
        }
        panel = panel
            .child(
                Checkbox::new("library-collapse-stacks")
                    .label("Collapse stacks")
                    .checked(self.batch.library.collapse_stacks)
                    .on_change(cx.listener(|this, value, _, cx| {
                        this.batch.library.collapse_stacks = *value;
                        this.library_show(cx);
                    })),
            )
            .child(
                Checkbox::new("library-deduplicate")
                    .label("Skip duplicate imports")
                    .checked(self.batch.library.deduplicate)
                    .on_change(cx.listener(|this, value, _, cx| {
                        this.batch.library.deduplicate = *value;
                        cx.notify();
                    })),
            );
        for (index, asset) in self
            .batch
            .library
            .catalog
            .assets
            .iter()
            .filter(|a| {
                a.kind == emulsion_io::creative_library::AssetKind::Image && !a.path.is_file()
            })
            .take(10)
            .enumerate()
        {
            let old = asset.path.clone();
            panel = panel.child(
                Button::new(("library-relink-missing", index))
                    .label(format!("Locate {}…", asset.name))
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let old = old.clone();
                        let rx = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Locate the original photo".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = rx.await
                                && let Some(new) = paths.into_iter().next()
                            {
                                this.update(cx, |this, cx| {
                                    this.library_edit(
                                        move |c| emulsion_io::photo_catalog::relink(c, &old, &new),
                                        cx,
                                    )
                                })
                                .ok();
                            }
                        })
                        .detach();
                    })),
            );
        }
        panel.into_any_element()
    }
    fn library_catalog_action(&mut self, action: usize, cx: &mut Context<Self>) {
        use emulsion_io::{creative_library as catalog, photo_catalog};
        let paths = self.library_paths();
        if action == 6 {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Restore Emulsion catalog backup".into()),
            });
            cx.spawn(async move |this, cx| {
                let Ok(Ok(Some(paths))) = rx.await else {
                    return;
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                this.update(cx, |this, cx| {
                    this.library_edit(
                        move |c| {
                            emulsion_io::photo_catalog::restore(
                                c,
                                &path,
                                &catalog::root().join("backups"),
                            )
                        },
                        cx,
                    )
                })
                .ok();
            })
            .detach();
            return;
        }
        if action == 5 {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Import Lightroom catalog (.lrcat) or handoff (.emulr.json)".into()),
            });
            cx.spawn(async move |this, cx| {
                let Ok(Ok(Some(paths))) = rx.await else {
                    return;
                };
                let Some(file) = paths.into_iter().next() else {
                    return;
                };
                let result = cx
                    .background_spawn(async move {
                        catalog::update(&catalog::root(), |c| {
                            emulsion_io::lightroom_catalog::import(&file, c)
                        })
                    })
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok((c, report)) => {
                            this.batch.library.catalog = c;
                            this.batch.library.source_paths = None;
                            this.library_show(cx);
                            this.batch.note = Some((
                                format!(
                                    "Imported {} photos, {} keyword assignments, {} labels, {} collections and {} histories; {} offline. {}",
                                    report.imported, report.keywords, report.color_labels, report.collections, report.histories,
                                    report.missing.len(),
                                    format_args!("{}{}",report.warnings.iter().take(3).cloned().collect::<Vec<_>>().join(" "),if report.warnings.len()>3{format!(" ({} more compatibility notes available through MCP.)",report.warnings.len()-3)}else{String::new()})
                                )
                                .into(),
                                false,
                            ));
                        }
                        Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                    };
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        if action == 4 {
            let catalog = self.batch.library.catalog.clone();
            let rx =
                cx.prompt_for_new_path(&catalog::root(), Some("photo-library-backup.emulibrary"));
            cx.spawn(async move|this,cx|{let Ok(Ok(Some(file)))=rx.await else{return;};let result=cx.background_spawn(async move{photo_catalog::backup(&catalog,&file)}).await;this.update(cx,|this,cx|{this.batch.note=Some(match result{Ok(())=>("Portable backup saved with originals, sidecars, virtual copies, masks and presets.".into(),false),Err(e)=>(e.to_string().into(),true)});cx.notify();}).ok();}).detach();
            return;
        }
        if action == 0 {
            let Some(path) = self
                .batch
                .current
                .and_then(|i| self.batch.items.get(i))
                .map(|i| i.path.clone())
            else {
                return;
            };
            let Some(params) = self.batch.develop.current_params(&path) else {
                return;
            };
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_spawn(async move {
                        let copy = emulsion_io::photo_develop::create_virtual(
                            &path,
                            params,
                            &catalog::root().join("virtual-copies"),
                        )?;
                        catalog::update(&catalog::root(), |c| {
                            c.add_asset(copy, catalog::AssetKind::Image)?;
                            Ok(())
                        })
                    })
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok((c, _)) => {
                            this.batch.library.catalog = c;
                            this.batch.library.source_paths = None;
                            this.batch.library.collection = None;
                            this.library_show(cx);
                        }
                        Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                    };
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        let state = &self.batch.library;
        let rule = photo_catalog::SmartRule {
            minimum_rating: state.rating,
            color_label: state.color_label,
            flagged: state.flagged,
            rejected: state.rejected,
            raw_only: state.raw_only,
            keyword: state
                .search
                .as_ref()
                .map(|s| s.read(cx).value().to_string())
                .unwrap_or_default(),
        };
        self.library_edit(
            move |c| {
                match action {
                    1 => photo_catalog::stack(c, &paths)?,
                    2 => photo_catalog::unstack(c, &paths),
                    3 => {
                        let id = c.add_collection(
                            format!("Smart collection {}", c.photos.smart.len() + 1),
                            vec![],
                        )?;
                        c.photos.smart.insert(id, rule);
                    }
                    _ => {}
                }
                Ok(())
            },
            cx,
        );
    }
}

impl Workspace {
    pub(crate) fn library_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.batch.assistant_host.is_none() {
            let workspace = cx.weak_entity();
            let host = cx.new(|cx| {
                let mut editor = crate::editor::EditorView::new(
                    Document::new(1, 1),
                    None,
                    None,
                    None,
                    "Library".into(),
                    cx,
                );
                editor.library_only = true;
                editor.library_workspace = Some(workspace);
                editor
            });
            self.batch.assistant_observer = Some(cx.observe(&host, |_, _, cx| cx.notify()));
            self.batch.assistant_host = Some(host);
        }
        self.batch
            .assistant_host
            .as_ref()
            .unwrap()
            .update(cx, |host, cx| host.open_ask(window, cx));
        cx.notify();
    }
    pub(super) fn library_assistant_surface(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = classic::palette(cx);
        self.batch.assistant_host.as_ref().map(|host| {
            host.update(cx, |host, cx| {
                div()
                    .flex()
                    .flex_col()
                    .children(host.ask_bar(&p, cx))
                    .children(host.assistant_dock(&p, cx))
                    .children(
                        host.status
                            .as_ref()
                            .map(|(message, _)| mono(message.clone(), 11., p.muted)),
                    )
                    .into_any_element()
            })
        })
    }
}

impl Workspace {
    pub(super) fn library_preset_bank(
        &self,
        params: emulsion_core::raw::DevelopParams,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = classic::palette(cx);
        let mut panel = div().flex().flex_col().gap_1();
        panel = panel.child(
            Button::new("library-preset-pack")
                .label("Import preset pack…")
                .small()
                .outline()
                .on_click(cx.listener(|this, _, _, cx| this.import_library_preset_pack(cx))),
        );
        panel = panel.child(self.library_preset_notes(
            &self.batch.develop.preset_import_notes,
            true,
            cx,
        ));
        let mut list = div()
            .id("library-imported-presets")
            .max_h(px(130.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for (index, file) in self.batch.develop.preset_files.iter().enumerate() {
            let file = file.clone();
            let name = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let name = name.get(13..).unwrap_or(&name).to_string();
            let path = self
                .batch
                .current
                .and_then(|i| self.batch.items.get(i))
                .map(|i| i.path.clone());
            list = list.child(
                Button::new(("library-imported-preset", index))
                    .label(name)
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let file = file.clone();
                        let path = path.clone();
                        cx.spawn(async move |this, cx| {
                            let result = cx
                                .background_spawn(async move {
                                    emulsion_io::lightroom_presets::load(&file, params)
                                })
                                .await;
                            this.update(cx, |this, cx| {
                                match result {
                                    Ok(report) => {
                                        if this
                                            .batch
                                            .current
                                            .and_then(|i| this.batch.items.get(i))
                                            .map(|i| &i.path)
                                            == path.as_ref()
                                            && path
                                                .as_ref()
                                                .and_then(|p| this.batch.develop.current_params(p))
                                                == Some(params)
                                        {
                                            this.library_adjust(report.params, cx);
                                            this.batch.develop.preset_report = Some(report);
                                        }
                                    }
                                    Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                                };
                                cx.notify();
                            })
                            .ok();
                        })
                        .detach();
                    })),
            );
        }
        panel
            .child(mono("Imported presets", 10., p.muted))
            .child(list)
            .into_any_element()
    }
}

impl Workspace {
    pub(crate) fn import_library_preset_pack(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import Lightroom / VSCO .zip, .xmp or .lrtemplate presets".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let results = cx
                .background_spawn(async move {
                    paths
                        .into_iter()
                        .map(|p| emulsion_io::lightroom_presets::install(&p))
                        .collect::<Vec<_>>()
                })
                .await;
            let files = cx
                .background_spawn(async { emulsion_io::lightroom_presets::installed() })
                .await;
            this.update(cx, |this, cx| {
                let mut notes = Vec::new();
                let mut imported = 0;
                for result in results {
                    match result {
                        Ok(report) => {
                            imported += report.files.len();
                            notes.extend(report.warnings);
                        }
                        Err(e) => notes.push(e.to_string()),
                    }
                }
                this.batch.develop.preset_files = files;
                this.record_preset_import(imported, notes, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
