//! Develop controls share the persisted parameters and worker with Basic.
use super::*;
use emulsion_core::raw::DevelopParams;
use gpui_kit::component::{
    Disableable, Selectable,
    slider::{Slider, SliderEvent, SliderState},
};
#[derive(Clone, Copy)]
enum Field {
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
fn assign(p: &mut DevelopParams, f: Field, v: f32) {
    match f {
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
        let palette = theme::palette(cx);
        let section = self.batch.develop.section;
        let mut panel = div().flex().flex_col().gap_3();
        let mut fields: Vec<(&str, Field, f32, f32, f32, f32)> = Vec::new();
        match section {
            1 => {
                panel = panel.child(label("Crop and geometry", &palette));
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
                                    let original =
                                        source.info.width as f32 / source.info.height.max(1) as f32;
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
                    if let SliderEvent::Change(value) = event {
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
        for (index, (name, _, value, _, _, _)) in fields.into_iter().enumerate() {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(mono(name, 11., palette.muted))
                            .child(mono(format!("{value:.3}"), 10., palette.ink)),
                    )
                    .child(
                        Slider::new(&self.batch.develop.sliders[index].0)
                            .disabled(self.batch.develop.saving),
                    ),
            );
        }
        panel.into_any_element()
    }
    fn library_curve_graph(&self, params: DevelopParams, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let channel = self.batch.develop.channel.min(3);
        let mut params = params;
        materialize_curve(&mut params, channel);
        let track = self.batch.develop.curve_bounds.clone();
        let paint = track.clone();
        let line = p.line;
        let ink = p.ink;
        div()
            .id("library-curve-graph")
            .test_support()
            .w_full()
            .h(px(160.))
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
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    this.batch.develop.curve_drag = None;
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
                        path.move_to(at(0., params.point_curves[channel].output(0.)));
                        for i in 1..=128 {
                            let x = i as f32 / 128.;
                            path.line_to(at(x, params.point_curves[channel].output(x)));
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
        let p = theme::palette(cx);
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
        let p = theme::palette(cx);
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
        let mut panel = div().flex().flex_col().gap_1();
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
                                    "Imported {} photos; {} missing. {}",
                                    report.imported,
                                    report.missing.len(),
                                    report.warnings.join(" ")
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
        let p = theme::palette(cx);
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
        let p = theme::palette(cx);
        let mut panel = div().flex().flex_col().gap_1();
        panel = panel.child(
            Button::new("library-preset-pack")
                .label("Import preset pack…")
                .small()
                .outline()
                .on_click(cx.listener(|_, _, _, cx| {
                    let rx = cx.prompt_for_paths(PathPromptOptions {
                        files: true,
                        directories: false,
                        multiple: true,
                        prompt: Some(
                            "Import Lightroom / VSCO .zip, .xmp or .lrtemplate presets".into(),
                        ),
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
                            this.batch.note = Some((
                                format!("Imported {imported} presets. {}", notes.join(" ")).into(),
                                imported == 0,
                            ));
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                })),
        );
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
