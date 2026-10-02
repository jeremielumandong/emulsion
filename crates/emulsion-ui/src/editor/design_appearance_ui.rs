//! Selection-first Design formatting, using native editable document commands.
use super::*;
use emulsion_raster::vector::{PathPaint, PathStyle, StrokeAlignment};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::Button,
    color_picker::{ColorPicker, ColorPickerState},
    menu::{DropdownMenu, PopupMenuItem},
};
#[path = "design_appearance_ops.rs"]
mod ops;
#[cfg(test)]
#[path = "design_appearance_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum Edit {
    Fill,
    Stroke,
    Opacity,
    Corners,
    Typography,
    Curve,
    Background,
}
impl Edit {
    fn title(self) -> &'static str {
        match self {
            Self::Fill => "Object fill",
            Self::Stroke => "Object stroke",
            Self::Opacity => "Object opacity",
            Self::Corners => "Corner radius",
            Self::Typography => "Text spacing and alignment",
            Self::Curve => "Curve and warp text",
            Self::Background => "Text background",
        }
    }
    /// Dialog heading in the interface language; `title` stays the history label.
    fn display_title(self) -> String {
        match self {
            Self::Fill => t!("editor.design_appearance_ui.title_fill"),
            Self::Stroke => t!("editor.design_appearance_ui.title_stroke"),
            Self::Opacity => t!("editor.design_appearance_ui.title_opacity"),
            Self::Corners => t!("editor.design_appearance_ui.title_corners"),
            Self::Typography => t!("editor.design_appearance_ui.title_typography"),
            Self::Curve => t!("editor.design_appearance_ui.title_curve"),
            Self::Background => t!("editor.design_appearance_ui.title_background"),
        }
        .into_owned()
    }
}
fn control(id: &'static str, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .label(label)
        .xsmall()
        .outline()
        .flex_none()
        .h(px(25.))
        .px(px(8.))
}
fn choice(
    id: &'static str,
    label: &str,
    state: &Entity<usize>,
    labels: Vec<SharedString>,
    cx: &App,
) -> impl IntoElement {
    let current = *state.read(cx);
    let state = state.clone();
    control(
        id,
        t!(
            "editor.design_breakpoints_ui.label_value",
            label = label,
            value = labels[current.min(labels.len() - 1)]
        ),
    )
    .dropdown_menu(move |mut menu, _, _| {
        for (i, label) in labels.iter().enumerate() {
            let state = state.clone();
            menu = menu.item(
                PopupMenuItem::new(label.clone())
                    .checked(i == current)
                    .on_click(move |_, window, cx| {
                        state.update(cx, |value, cx| {
                            *value = i;
                            cx.notify();
                        });
                        window.refresh();
                    }),
            );
        }
        menu
    })
}
fn paint_mode(color: Option<[u8; 4]>, paint: PathPaint) -> usize {
    if color.is_none() {
        0
    } else {
        match paint {
            PathPaint::Solid => 1,
            PathPaint::LinearGradient { .. } | PathPaint::LinearStops { .. } => 2,
            PathPaint::RadialGradient { .. } | PathPaint::RadialStops { .. } => 3,
            PathPaint::Pattern { .. } => 1,
        }
    }
}
fn second_color(paint: PathPaint) -> [u8; 4] {
    match paint {
        PathPaint::LinearStops { stops, count, .. } | PathPaint::RadialStops { stops, count } => {
            stops[usize::from(count.clamp(1, 16)) - 1].color
        }
        PathPaint::LinearGradient { end, .. } | PathPaint::RadialGradient { end } => end,
        PathPaint::Pattern { secondary, .. } => secondary,
        _ => [255; 4],
    }
}
fn picked(state: &Entity<ColorPickerState>, cx: &App) -> [u8; 4] {
    let rgb = state
        .read(cx)
        .value()
        .unwrap_or_else(|| gpui_kit::gpui::rgb(0).into())
        .to_rgb();
    [rgb.r, rgb.g, rgb.b, rgb.a].map(|c| (c * 255.).round().clamp(0., 255.) as u8)
}

impl EditorView {
    /// Full-tools mode retains one stable, horizontally scrolling row.
    pub(super) fn design_appearance_controls(
        &self,
        p: &Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        self.design_appearance_controls_layout(p, cx, false)
    }

    /// Advanced controls wrap inside the basic editor's narrow Effects popup.
    pub(super) fn design_appearance_popup_controls(
        &self,
        p: &Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        self.design_appearance_controls_layout(p, cx, true)
    }

    fn design_appearance_controls_layout(
        &self,
        p: &Palette,
        cx: &Context<Self>,
        wrap: bool,
    ) -> Option<AnyElement> {
        if !self.is_design() || self.previewing() {
            return None;
        }
        let row = div()
            .id("design-appearance-controls")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .when(wrap, |row| row.w_full().min_h_9().flex_wrap().py_2())
            .when(!wrap, |row| row.h_9().overflow_x_scroll())
            .flex_none()
            .min_w_0()
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line);
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            return Some(
                row.child(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child(t!("editor.design_appearance_ui.select_hint").to_string()),
                )
                .into_any_element(),
            );
        }
        let nodes: Vec<_> = ids
            .iter()
            .filter_map(|id| self.editor.doc.node(*id))
            .collect();
        let locked = ids.iter().any(|id| {
            self.editor.doc.locked_ancestor(*id).is_some()
                || self.editor.doc.node(*id).is_some_and(|n| n.locks.pixels)
        });
        let paths = nodes
            .iter()
            .all(|n| matches!(n.kind, NodeKind::Path { .. }));
        let fill = nodes.iter().all(|n| {
            matches!(
                n.kind,
                NodeKind::Path { .. } | NodeKind::Text { .. } | NodeKind::Fill { .. }
            )
        });
        let corners = paths
            && nodes.iter().all(
                |n| matches!(&n.kind,NodeKind::Path{path,..} if ops::rectangle(path).is_some()),
            )
            && ids.iter().all(|id| {
                !self
                    .editor
                    .doc
                    .design
                    .frames
                    .values()
                    .any(|frame| frame.boundary == *id)
            });
        let text = (ids.len() == 1)
            .then(|| ops::text_backdrop(&self.editor.doc, ids[0]))
            .flatten()
            .map(|(text, _)| text);
        let effect = (ids.len() == 1).then_some(text.unwrap_or(ids[0]));
        let group = ids.len() > 1;
        let ungroup = ids.len() == 1 && nodes[0].is_group();
        let mut row = row
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(if ids.len() == 1 {
                        t!("editor.design_appearance_ui.appearance").into_owned()
                    } else {
                        t!("design.direct.objects", count = ids.len()).into_owned()
                    }),
            )
            .when(fill, |row| {
                row.child(
                    control(
                        "design-appearance-fill",
                        if paths {
                            t!("editor.design_appearance_ui.fill_gradient")
                        } else {
                            t!("editor.design_appearance_ui.text_fill_color")
                        },
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_appearance_dialog(Edit::Fill, window, cx)
                    })),
                )
            })
            .when(paths, |row| {
                row.child(
                    control(
                        "design-appearance-stroke",
                        t!("editor.design_appearance_ui.stroke"),
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_appearance_dialog(Edit::Stroke, window, cx)
                    })),
                )
            })
            .child(
                control(
                    "design-appearance-opacity",
                    t!("editor.design_appearance_ui.opacity"),
                )
                .disabled(locked)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.design_appearance_dialog(Edit::Opacity, window, cx)
                })),
            )
            .when(corners, |row| {
                row.child(
                    control(
                        "design-appearance-corners",
                        t!("editor.design_appearance_ui.corners"),
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_appearance_dialog(Edit::Corners, window, cx)
                    })),
                )
            });
        if text.is_some() {
            row = row
                .child(
                    control(
                        "design-appearance-spacing",
                        t!("editor.design_appearance_ui.spacing"),
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_appearance_dialog(Edit::Typography, window, cx)
                    })),
                )
                .child(
                    control(
                        "design-appearance-curve",
                        t!("editor.design_appearance_ui.curve"),
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_appearance_dialog(Edit::Curve, window, cx)
                    })),
                )
                .child(
                    control(
                        "design-appearance-background",
                        t!("editor.design_appearance_ui.background"),
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_appearance_dialog(Edit::Background, window, cx)
                    })),
                );
        }
        if let Some(id) = effect {
            row = row
                .child(
                    control(
                        "design-appearance-shadow",
                        t!("editor.design_appearance_ui.shadow"),
                    )
                    .disabled(locked)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_layer_effect_kind(id, "drop_shadow", window, cx)
                    })),
                )
                .when(text.is_some(), |row| {
                    row.child(
                        control(
                            "design-appearance-outline",
                            t!("editor.design_appearance_ui.outline"),
                        )
                        .disabled(locked)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_layer_effect_kind(id, "stroke", window, cx)
                            },
                        )),
                    )
                })
                .child(
                    control(
                        "design-appearance-effects",
                        t!("editor.design_appearance_ui.effects"),
                    )
                    .disabled(locked)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_layer_styles_dialog(id, window, cx)
                    })),
                );
        }
        Some(
            row.child(self.alignment_controls(p, cx))
                .when(group, |row| {
                    row.child(
                        control("design-appearance-group", t!("design.direct.group"))
                            .disabled(locked)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.prepare_page_action(cx) {
                                    this.group_selected(cx);
                                }
                            })),
                    )
                })
                .when(ungroup, |row| {
                    row.child(
                        control("design-appearance-ungroup", t!("design.direct.ungroup"))
                            .disabled(locked)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.prepare_page_action(cx) {
                                    this.ungroup_selected(cx);
                                }
                            })),
                    )
                })
                .into_any_element(),
        )
    }

    pub(super) fn design_appearance_dialog(
        &mut self,
        kind: Edit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        let Some(first) = ids.first().and_then(|id| self.editor.doc.node(*id)) else {
            return;
        };
        let text = if ids.len() == 1 {
            ops::text_backdrop(&self.editor.doc, ids[0]).map(|(text, _)| text)
        } else {
            None
        };
        let spec = text
            .and_then(|id| self.editor.doc.node(id))
            .and_then(|n| match &n.kind {
                NodeKind::Text { spec, .. } => Some((**spec).clone()),
                _ => None,
            });
        let range = if text == self.selected {
            self.text_style_range()
        } else {
            None
        };
        let style = match &first.kind {
            NodeKind::Path { style, .. } => *style,
            NodeKind::Text { spec, .. } => PathStyle {
                fill: Some(spec.style_at(range.as_ref().map_or(0, |r| r.start)).color),
                ..Default::default()
            },
            NodeKind::Fill { rgba } => PathStyle {
                fill: Some(*rgba),
                ..Default::default()
            },
            _ => Default::default(),
        };
        let stroke = matches!(kind, Edit::Stroke);
        let paint = if stroke {
            style.stroke_paint
        } else {
            style.fill_paint
        };
        let primary = if stroke { style.stroke } else { style.fill }.unwrap_or(self.tools.fg);
        let mut colors = vec![primary, second_color(paint)];
        let mut mode = paint_mode(if stroke { style.stroke } else { style.fill }, paint);
        let angle = paint.gradient_angle();
        let mut options: Vec<(String, f32)> = Vec::new();
        let mut alignment = match style.alignment {
            StrokeAlignment::Inside => 0,
            StrokeAlignment::Center => 1,
            StrokeAlignment::Outside => 2,
        };
        let paths = ids.iter().all(|id| {
            matches!(
                self.editor.doc.node(*id).map(|n| &n.kind),
                Some(NodeKind::Path { .. })
            )
        });
        match kind {
            Edit::Fill => options.push((
                t!("editor.design_appearance_ui.gradient_angle").into_owned(),
                angle,
            )),
            Edit::Stroke => options.extend([
                (
                    t!("editor.design_appearance_ui.width_px").into_owned(),
                    style.width,
                ),
                (
                    t!("editor.design_appearance_ui.gradient_angle").into_owned(),
                    angle,
                ),
            ]),
            Edit::Opacity => options.push((
                t!("editor.design_appearance_ui.opacity_percent").into_owned(),
                first.opacity * 100.,
            )),
            Edit::Corners => {
                if let NodeKind::Path { path, .. } = &first.kind
                    && let Some((_, _, _, _, r)) = ops::rectangle(path)
                {
                    options.push((
                        t!("editor.design_appearance_ui.corner_radius").into_owned(),
                        r as f32,
                    ));
                }
            }
            Edit::Typography => {
                let Some(spec) = spec.as_ref() else { return };
                let selected = spec.style_at(range.as_ref().map_or(0, |r| r.start));
                options.extend([
                    (
                        t!("editor.design_appearance_ui.letter_spacing").into_owned(),
                        selected.letter_spacing,
                    ),
                    (
                        t!("editor.design_appearance_ui.line_height").into_owned(),
                        spec.line_height,
                    ),
                ]);
                alignment = match spec.align {
                    emulsion_core::text::Align::Left => 0,
                    emulsion_core::text::Align::Center => 1,
                    emulsion_core::text::Align::Right => 2,
                    emulsion_core::text::Align::Justify => 3,
                };
            }
            Edit::Curve => {
                use emulsion_core::text_effects::WarpStyle;
                let Some(spec) = spec.as_ref() else { return };
                mode = match spec.warp.style {
                    WarpStyle::None => 0,
                    WarpStyle::Arc => 1,
                    WarpStyle::Bulge => 2,
                    WarpStyle::Flag => 3,
                };
                options.extend([
                    (
                        t!("editor.design_appearance_ui.curve_amount").into_owned(),
                        spec.warp.bend,
                    ),
                    (
                        t!("editor.design_appearance_ui.horizontal_distortion").into_owned(),
                        spec.warp.horizontal,
                    ),
                    (
                        t!("editor.design_appearance_ui.vertical_distortion").into_owned(),
                        spec.warp.vertical,
                    ),
                ]);
            }
            Edit::Background => {
                let Some((text, pair)) = ops::text_backdrop(&self.editor.doc, ids[0]) else {
                    return;
                };
                let mut padding = [16., 12.];
                let mut radius = 8.;
                colors[0] = [255, 235, 120, 255];
                mode = 1;
                if let Some((_, bg)) = pair
                    && let Some(NodeKind::Path { style, .. }) =
                        self.editor.doc.node(bg).map(|n| &n.kind)
                {
                    colors[0] = style.fill.unwrap_or([255, 235, 120, 255]);
                    if let Some((x, y, _, _, r)) =
                        ops::background_geometry(&self.editor.doc, text, bg)
                    {
                        radius = r as f32;
                        if let Some(spec) = spec.as_ref() {
                            let b = emulsion_core::text::layout(spec).bounds();
                            padding = [
                                (b.x as f64 - x).max(0.) as f32,
                                (b.y as f64 - y).max(0.) as f32,
                            ];
                        }
                    }
                }
                options.extend([
                    (
                        t!("editor.design_appearance_ui.horizontal_padding").into_owned(),
                        padding[0],
                    ),
                    (
                        t!("editor.design_appearance_ui.vertical_padding").into_owned(),
                        padding[1],
                    ),
                    (
                        t!("editor.design_appearance_ui.corner_radius").into_owned(),
                        radius,
                    ),
                ]);
            }
        }
        let inputs: Vec<_> = options
            .iter()
            .map(|(_, value)| {
                cx.new(|cx| InputState::new(window, cx).default_value(format!("{value:.2}")))
            })
            .collect();
        let pickers: Vec<_> = colors
            .into_iter()
            .map(|rgba| {
                cx.new(|cx| {
                    let [r, g, b, a] = rgba.map(|v| v as f32 / 255.);
                    ColorPickerState::new(window, cx).default_value(Rgba { r, g, b, a })
                })
            })
            .collect();
        let mode = cx.new(|_| mode);
        let alignment = cx.new(|_| alignment);
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, cx| {
            let mut body = div()
                .id("design-appearance-form")
                .test_support()
                .flex()
                .flex_col()
                .gap_3();
            let m = *mode.read(cx);
            match kind {
                Edit::Fill | Edit::Stroke => {
                    let mut paints = vec![
                        t!("editor.design_appearance_ui.none").into(),
                        t!("editor.design_appearance_ui.solid").into(),
                    ];
                    if paths {
                        paints.extend([
                            t!("editor.design_appearance_ui.linear_gradient").into(),
                            t!("editor.design_appearance_ui.radial_gradient").into(),
                        ]);
                    }
                    body = body.child(choice(
                        "design-appearance-paint",
                        &t!("editor.design_appearance_ui.paint"),
                        &mode,
                        paints,
                        cx,
                    ));
                    if m > 0 {
                        body = body.child(ColorPicker::new(&pickers[0]).label(if m > 1 {
                            t!("editor.design_appearance_ui.start_color")
                        } else {
                            t!("design.direct.color")
                        }));
                    }
                    if m > 1 {
                        body = body.child(
                            ColorPicker::new(&pickers[1])
                                .label(t!("editor.design_appearance_ui.end_color")),
                        );
                    }
                    if stroke {
                        body = body.child(choice(
                            "design-appearance-stroke-alignment",
                            &t!("editor.design_appearance_ui.alignment"),
                            &alignment,
                            vec![
                                t!("editor.design_appearance_ui.inside").into(),
                                t!("editor.design_appearance_ui.center").into(),
                                t!("editor.design_appearance_ui.outside").into(),
                            ],
                            cx,
                        ));
                    }
                }
                Edit::Typography => {
                    body = body.child(choice(
                        "design-appearance-text-alignment",
                        &t!("editor.design_appearance_ui.text_alignment"),
                        &alignment,
                        vec![
                            t!("editor.design_appearance_ui.left").into(),
                            t!("editor.design_appearance_ui.center").into(),
                            t!("editor.design_appearance_ui.right").into(),
                            t!("editor.design_appearance_ui.justify").into(),
                        ],
                        cx,
                    ))
                }
                Edit::Curve => {
                    body = body.child(choice(
                        "design-appearance-warp",
                        &t!("edit.warp"),
                        &mode,
                        vec![
                            t!("editor.design_appearance_ui.none").into(),
                            t!("editor.design_appearance_ui.arc").into(),
                            t!("editor.design_appearance_ui.bulge").into(),
                            t!("editor.design_appearance_ui.flag").into(),
                        ],
                        cx,
                    ))
                }
                Edit::Background => {
                    body = body.child(choice(
                        "design-appearance-backdrop-mode",
                        &t!("new_canvas.background"),
                        &mode,
                        vec![
                            t!("editor.design_appearance_ui.remove").into(),
                            t!("editor.design_appearance_ui.enabled_refit").into(),
                        ],
                        cx,
                    ));
                    if m > 0 {
                        body = body.child(
                            ColorPicker::new(&pickers[0])
                                .label(t!("editor.design_appearance_ui.background_color")),
                        );
                    }
                }
                _ => {}
            }
            for (i, (label, _)) in options.iter().enumerate() {
                if matches!(kind, Edit::Fill) && m != 2
                    || matches!(kind, Edit::Stroke) && i == 1 && m != 2
                {
                    continue;
                }
                body = body.child(
                    div()
                        .text_size(px(12.))
                        .child(label.clone())
                        .child(Input::new(&inputs[i]).id(("design-appearance-input", i))),
                );
            }
            if matches!(kind, Edit::Background) {
                body = body.child(
                    div()
                        .text_size(px(11.))
                        .child(t!("editor.design_appearance_ui.background_hint").to_string()),
                );
            }
            if matches!(kind, Edit::Curve) {
                body = body.child(
                    div()
                        .text_size(px(11.))
                        .child(t!("editor.design_appearance_ui.curve_hint").to_string()),
                );
            }
            let (inputs, pickers, mode, alignment, owner, ids, range) = (
                inputs.clone(),
                pickers.clone(),
                mode.clone(),
                alignment.clone(),
                owner.clone(),
                ids.clone(),
                range.clone(),
            );
            dialog
                .title(kind.display_title())
                .width(px(440.))
                .child(body)
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_appearance_ui.apply"
                )))
                .on_ok(move |_, _, cx| {
                    let values: Vec<_> = inputs
                        .iter()
                        .map(|input| input.read(cx).value().to_string())
                        .collect();
                    let colors: Vec<_> = pickers.iter().map(|picker| picked(picker, cx)).collect();
                    let mode = *mode.read(cx);
                    let align = *alignment.read(cx);
                    owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket || this.selected_layer_roots() != ids {
                                this.set_status(
                                    t!("editor.design_appearance_ui.selection_changed"),
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            let result = this.apply_design_appearance(
                                kind,
                                &ids,
                                text,
                                range.clone(),
                                &values,
                                &colors,
                                mode,
                                align,
                            );
                            match result {
                                Ok((commands, select)) => {
                                    if commands.is_empty() {
                                        return true;
                                    }
                                    let applied = this
                                        .execute_layer_commands(kind.title(), commands, cx)
                                        .is_some();
                                    if applied && let Some(id) = select {
                                        this.set_layer_selection(vec![id], Some(id));
                                        cx.notify();
                                    }
                                    applied
                                }
                                Err(error) => {
                                    this.set_status(error, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_design_appearance(
        &self,
        kind: Edit,
        ids: &[NodeId],
        text: Option<NodeId>,
        range: Option<std::ops::Range<usize>>,
        values: &[String],
        colors: &[[u8; 4]],
        mode: usize,
        align: usize,
    ) -> Result<(Vec<Command>, Option<NodeId>), String> {
        let number = |i: usize, min, max| {
            ops::number(values.get(i).ok_or("Missing appearance value")?, min, max)
        };
        let doc = &self.editor.doc;
        let mut commands = Vec::new();
        match kind {
            Edit::Fill | Edit::Stroke => {
                let index = usize::from(matches!(kind, Edit::Stroke));
                let paint = match mode {
                    2 => PathPaint::LinearGradient {
                        end: colors[1],
                        angle: number(index, -360., 360.)?,
                    },
                    3 => PathPaint::RadialGradient { end: colors[1] },
                    _ => PathPaint::Solid,
                };
                let preserve_stops = |previous: PathPaint| -> PathPaint {
                    if matches!(
                        previous,
                        PathPaint::LinearStops { .. } | PathPaint::RadialStops { .. }
                    ) && mode == if previous.is_radial() { 3 } else { 2 }
                    {
                        let mut stops = previous.gradient_stops(colors[0]).unwrap();
                        stops[0].color = colors[0];
                        stops.last_mut().unwrap().color = colors[1];
                        PathPaint::from_stops(&stops, mode == 3, paint.gradient_angle())
                            .unwrap_or(paint)
                    } else {
                        paint
                    }
                };
                let rgba = (mode != 0).then_some(colors[0]);
                if matches!(kind, Edit::Stroke) {
                    let width = number(0, 0., 1000.)?;
                    for id in ids {
                        let Some(NodeKind::Path { path, style, .. }) =
                            doc.node(*id).map(|n| &n.kind)
                        else {
                            return Err(
                                t!("editor.design_appearance_ui.select_vector_shapes").into_owned()
                            );
                        };
                        commands.push(Command::SetPath {
                            id: *id,
                            path: path.clone(),
                            style: PathStyle {
                                stroke: rgba,
                                stroke_paint: preserve_stops(style.stroke_paint),
                                width,
                                alignment: match align {
                                    0 => StrokeAlignment::Inside,
                                    2 => StrokeAlignment::Outside,
                                    _ => StrokeAlignment::Center,
                                },
                                ..*style
                            },
                        });
                    }
                } else if let (Some(id), Some(range)) = (text, range) {
                    let Some(NodeKind::Text { spec, .. }) = doc.node(id).map(|n| &n.kind) else {
                        return Err(
                            t!("editor.design_appearance_ui.select_editable_text").into_owned()
                        );
                    };
                    let mut spec = (**spec).clone();
                    spec.apply_style(range, |style| style.color = rgba.unwrap_or([0; 4]));
                    commands.push(Command::SetText {
                        id,
                        spec: Box::new(spec),
                    });
                } else {
                    commands = ops::fill(doc, ids, rgba, paint)?;
                    for command in &mut commands {
                        if let Command::SetPath { id, style, .. } = command
                            && let Some(NodeKind::Path { style: old, .. }) =
                                doc.node(*id).map(|n| &n.kind)
                        {
                            style.fill_paint = preserve_stops(old.fill_paint);
                        }
                    }
                }
            }
            Edit::Opacity => {
                let opacity = number(0, 0., 100.)? / 100.;
                commands = ids
                    .iter()
                    .map(|id| Command::SetOpacity { id: *id, opacity })
                    .collect();
            }
            Edit::Corners => {
                commands = ops::corners(doc, ids, number(0, 0., 100000.)?)?;
            }
            Edit::Typography | Edit::Curve => {
                let id = text.ok_or_else(|| {
                    t!("editor.design_appearance_ui.select_editable_text").into_owned()
                })?;
                let Some(NodeKind::Text { spec, .. }) = doc.node(id).map(|n| &n.kind) else {
                    return Err("Missing text object".into());
                };
                let mut spec = (**spec).clone();
                if matches!(kind, Edit::Typography) {
                    let spacing = number(0, -4000., 4000.)?;
                    spec.line_height = number(1, 0.5, 4.)?;
                    spec.align = match align {
                        1 => emulsion_core::text::Align::Center,
                        2 => emulsion_core::text::Align::Right,
                        3 => emulsion_core::text::Align::Justify,
                        _ => emulsion_core::text::Align::Left,
                    };
                    if range.is_none() {
                        spec.letter_spacing = spacing;
                    }
                    spec.apply_style(range.unwrap_or(0..spec.text.len()), |style| {
                        style.letter_spacing = spacing
                    });
                } else {
                    use emulsion_core::text_effects::{TextWarp, WarpStyle};
                    spec.warp = if mode == 0 {
                        TextWarp::default()
                    } else {
                        TextWarp {
                            style: match mode {
                                1 => WarpStyle::Arc,
                                2 => WarpStyle::Bulge,
                                3 => WarpStyle::Flag,
                                _ => WarpStyle::None,
                            },
                            bend: number(0, -100., 100.)?,
                            horizontal: number(1, -100., 100.)?,
                            vertical: number(2, -100., 100.)?,
                        }
                    };
                }
                commands.push(Command::SetText {
                    id,
                    spec: Box::new(spec),
                });
            }
            Edit::Background => {
                let (commands, id) = ops::background(
                    doc,
                    ids[0],
                    colors[0],
                    [number(0, 0., 1000.)?, number(1, 0., 1000.)?],
                    number(2, 0., 10000.)?,
                    mode == 0,
                )?;
                return Ok((commands, Some(id)));
            }
        }
        Ok((commands, None))
    }
}
