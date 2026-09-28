//! Canvas gestures commit one immutable edit asset and one history step.
use super::*;
use emulsion_core::develop_edits::{Component, LocalEdits, Mask, Operation, Shape, Spot, SpotMode};
use gpui_kit::component::{Selectable, WindowExt};
impl Workspace {
    pub(super) fn library_edit_set(&self) -> Result<LocalEdits, String> {
        let params = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .and_then(|i| self.batch.develop.current_params(&i.path));
        match params.and_then(|p| p.local_edits) {
            Some(d) => emulsion_io::develop_edits::load(&d).map_err(|e| e.to_string()),
            None => Ok(LocalEdits::default()),
        }
    }
    pub(super) fn library_commit_edit_set(&mut self, edits: LocalEdits, cx: &mut Context<Self>) {
        let Some(mut params) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .and_then(|i| self.batch.develop.current_params(&i.path))
        else {
            return;
        };
        match emulsion_io::develop_edits::store(&edits) {
            Ok(d) => {
                params.local_edits = Some(d);
                self.library_adjust(params, cx);
            }
            Err(e) => {
                self.batch.note = Some((e.to_string().into(), true));
                cx.notify();
            }
        }
    }
    pub(super) fn library_local_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let edits = match self.library_edit_set() {
            Ok(v) => v,
            Err(e) => return mono(e, 11., p.accent).into_any_element(),
        };
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label("Local adjustments", &p));
        let mut tools = div().flex().flex_wrap().gap_1();
        for (tool, name) in [
            (1, "Brush"),
            (2, "Erase"),
            (3, "Heal"),
            (4, "Clone"),
            (8, "Radial"),
            (9, "Linear"),
        ] {
            tools = tools.child(
                Button::new(("develop-canvas-tool", tool))
                    .label(name)
                    .small()
                    .ghost()
                    .selected(self.batch.develop.canvas_tool == tool)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.develop.canvas_tool = if this.batch.develop.canvas_tool == tool {
                            0
                        } else {
                            tool
                        };
                        this.invalidate_library_preview();
                        cx.notify();
                    })),
            );
        }
        panel=panel.child(tools).child(mono("Drag on the original view. Alt-click sets the heal/clone source. O shows mask overlay.",10.,p.muted));
        for (id, title, shape) in [
            (
                0,
                "Luminance range",
                Shape::Luminance {
                    range: [0.25, 0.75],
                    feather: 0.5,
                },
            ),
            (
                1,
                "Color range",
                Shape::Color {
                    rgb: [0.5, 0.25, 0.1],
                    tolerance: 0.3,
                    feather: 0.5,
                },
            ),
        ] {
            panel = panel.child(
                Button::new(("develop-new-range", id as usize))
                    .label(title)
                    .small()
                    .outline()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library_add_component(title, shape.clone(), cx);
                    })),
            );
        }
        panel = panel.child(
            Checkbox::new("develop-mask-intersect")
                .label("Intersect next component")
                .checked(self.batch.develop.mask_intersect)
                .on_change(cx.listener(|this, value, _, cx| {
                    this.batch.develop.mask_intersect = *value;
                    cx.notify();
                })),
        );
        panel = panel.child(
            Button::new("develop-mask-use-ai")
                .label("Add selected AI mask")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    let digest = this
                        .batch
                        .current
                        .and_then(|i| this.batch.items.get(i))
                        .and_then(|i| this.batch.develop.current_params(&i.path))
                        .and_then(|p| p.masks[this.batch.develop.mask.min(7)].bitmap);
                    if let Some(digest) = digest {
                        this.library_add_component(
                            "AI mask",
                            Shape::Bitmap {
                                digest,
                                inverted: false,
                            },
                            cx,
                        );
                    }
                })),
        );
        panel = panel.child(
            Button::new("develop-new-mask")
                .label("New brush mask")
                .small()
                .outline()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.batch.develop.active_mask = None;
                    this.batch.develop.canvas_tool = 1;
                    this.invalidate_library_preview();
                    cx.notify();
                })),
        );
        panel = panel.child(
            Checkbox::new("develop-mask-overlay")
                .label("Show mask overlay")
                .checked(self.batch.develop.mask_overlay)
                .on_change(cx.listener(|this, v, _, cx| {
                    this.batch.develop.mask_overlay = *v;
                    this.invalidate_library_preview();
                    cx.notify();
                })),
        );
        panel =
            panel.child(
                div()
                    .flex()
                    .gap_1()
                    .child(mono(
                        format!(
                            "Brush size {:.1}%",
                            brush_radius(self.batch.develop.brush_radius) * 100.
                        ),
                        10.,
                        p.muted,
                    ))
                    .children([(0, 0.5, "−"), (1, 2., "+")].into_iter().map(
                        |(id, factor, title)| {
                            Button::new(("develop-brush-size", id as usize))
                                .label(title)
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.batch.develop.brush_radius =
                                        (brush_radius(this.batch.develop.brush_radius) * factor)
                                            .clamp(0.002, 0.5);
                                    cx.notify();
                                }))
                        },
                    )),
            );
        for mask in edits.masks {
            let id = mask.id;
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .border_t_1()
                    .border_color(p.line)
                    .child(
                        Button::new(("develop-mask-select", id as usize))
                            .label(mask.name.clone())
                            .small()
                            .ghost()
                            .selected(self.batch.develop.active_mask == Some(id))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.develop.active_mask = Some(id);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                Checkbox::new(("develop-mask-enabled", id as usize))
                                    .label("Enabled")
                                    .checked(mask.enabled)
                                    .on_change(cx.listener(move |this, v, _, cx| {
                                        if let Ok(mut edits) = this.library_edit_set() {
                                            if let Some(m) =
                                                edits.masks.iter_mut().find(|m| m.id == id)
                                            {
                                                m.enabled = *v;
                                            }
                                            this.library_commit_edit_set(edits, cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new(("develop-mask-rename", id as usize))
                                    .label("Edit mask…")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.library_mask_properties(id, window, cx)
                                    })),
                            )
                            .child(
                                Button::new(("develop-mask-delete", id as usize))
                                    .label("Delete")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Ok(mut edits) = this.library_edit_set() {
                                            edits.masks.retain(|m| m.id != id);
                                            this.library_commit_edit_set(edits, cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(mono(
                                format!("Exposure {:+.2} EV", mask.exposure),
                                11.,
                                p.ink,
                            ))
                            .children([(0, -0.25, "−"), (1, 0.25, "+")].into_iter().map(
                                |(index, delta, title)| {
                                    Button::new((
                                        "develop-mask-exposure",
                                        (id as usize) * 2 + index,
                                    ))
                                    .label(title)
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if let Ok(mut edits) = this.library_edit_set() {
                                                if let Some(m) =
                                                    edits.masks.iter_mut().find(|m| m.id == id)
                                                {
                                                    m.exposure =
                                                        (m.exposure + delta).clamp(-5., 5.);
                                                }
                                                this.library_commit_edit_set(edits, cx);
                                            }
                                        },
                                    ))
                                },
                            )),
                    ),
            );
        }
        panel = panel.child(
            Checkbox::new("develop-dust-visualization")
                .label("Visualize spots")
                .checked(self.batch.develop.dust_visualization)
                .on_change(cx.listener(|this, value, _, cx| {
                    this.batch.develop.dust_visualization = *value;
                    this.invalidate_library_preview();
                    cx.notify();
                })),
        );
        for spot in edits.spots {
            let id = spot.id;
            panel = panel.child(
                Button::new(("develop-spot-edit", id as usize))
                    .label(format!("Edit {:?} spot {id}…", spot.mode))
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.library_spot_dialog(id, window, cx)
                    })),
            );
            panel = panel.child(
                Button::new(("develop-spot-delete", id as usize))
                    .label(format!("Remove {:?} spot {id}", spot.mode))
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Ok(mut edits) = this.library_edit_set() {
                            edits.spots.retain(|s| s.id != id);
                            this.library_commit_edit_set(edits, cx);
                        }
                    })),
            );
        }
        panel.into_any_element()
    }
    fn library_spot_dialog(&self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::WindowExt;
        let Ok(edits) = self.library_edit_set() else {
            return;
        };
        let Some(spot) = edits.spots.iter().find(|s| s.id == id) else {
            return;
        };
        let inputs: Vec<_> = [spot.radius, spot.feather, spot.opacity]
            .into_iter()
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v.to_string())))
            .collect();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let inputs = inputs.clone();
            let owner = owner.clone();
            let mut d = dialog.title("Spot settings");
            for (title, input) in [
                "Radius (greater than 0, up to 1)",
                "Feather (0–1)",
                "Opacity (0–1)",
            ]
            .into_iter()
            .zip(&inputs)
            {
                d = d.child(title).child(Input::new(input));
            }
            d.footer(crate::widgets::form_dialog_footer("Apply"))
                .on_ok(move |_, _, cx| {
                    let values: Option<Vec<f32>> = inputs
                        .iter()
                        .map(|i| i.read(cx).value().parse().ok())
                        .collect();
                    let Some(v) = values else {
                        return false;
                    };
                    owner
                        .update(cx, |this, cx| {
                            let Ok(mut edits) = this.library_edit_set() else {
                                return false;
                            };
                            let Some(s) = edits.spots.iter_mut().find(|s| s.id == id) else {
                                return false;
                            };
                            s.radius = v[0];
                            s.feather = v[1];
                            s.opacity = v[2];
                            if edits.validate().is_err() {
                                return false;
                            }
                            this.library_commit_edit_set(edits, cx);
                            true
                        })
                        .unwrap_or(false)
                })
        });
    }
    fn library_add_component(&mut self, title: &str, shape: Shape, cx: &mut Context<Self>) {
        if let Ok(mut edits) = self.library_edit_set() {
            if let Some(mask) = self
                .batch
                .develop
                .active_mask
                .and_then(|id| edits.masks.iter_mut().find(|m| m.id == id))
            {
                mask.components.push(Component {
                    operation: if self.batch.develop.mask_intersect {
                        Operation::Intersect
                    } else {
                        Operation::Add
                    },
                    shape,
                });
            } else {
                let id = edits.masks.iter().map(|m| m.id).max().unwrap_or(0) + 1;
                edits.masks.push(new_mask(id, title.into(), shape));
                self.batch.develop.active_mask = Some(id);
            }
            self.library_commit_edit_set(edits, cx);
        }
    }
    fn library_mask_properties(&self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        let Ok(edits) = self.library_edit_set() else {
            return;
        };
        let Some(mask) = edits.masks.iter().find(|m| m.id == id) else {
            return;
        };
        let mut labels = vec![
            "Name",
            "Exposure EV (−5 to 5)",
            "Contrast (−1 to 1)",
            "Saturation (−1 to 1)",
            "Temperature (−1 to 1)",
            "Tint (−1 to 1)",
        ];
        let mut values = vec![
            mask.name.clone(),
            mask.exposure.to_string(),
            mask.contrast.to_string(),
            mask.saturation.to_string(),
            mask.temperature.to_string(),
            mask.tint.to_string(),
        ];
        if let Some(c) = mask.components.last() {
            match &c.shape {
                Shape::Luminance { range, feather } => {
                    labels.extend([
                        "Minimum luminance (0 to 1)",
                        "Maximum luminance (0 to 1)",
                        "Feather (0 to 1)",
                    ]);
                    values.extend([range[0], range[1], *feather].map(|v| v.to_string()));
                }
                Shape::Color {
                    rgb,
                    tolerance,
                    feather,
                } => {
                    labels.extend([
                        "Red (linear 0 to 1)",
                        "Green (linear 0 to 1)",
                        "Blue (linear 0 to 1)",
                        "Color tolerance (0 to 1)",
                        "Feather (0 to 1)",
                    ]);
                    values.extend(
                        [rgb[0], rgb[1], rgb[2], *tolerance, *feather].map(|v| v.to_string()),
                    );
                }
                _ => {}
            }
        }
        let inputs: Vec<_> = values
            .into_iter()
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)))
            .collect();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let inputs = inputs.clone();
            let submitted = inputs.clone();
            let owner = owner.clone();
            let path = path.clone();
            dialog
                .title("Mask settings")
                .child(
                    div()
                        .id("develop-mask-form")
                        .max_h(px(500.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(
                            labels
                                .iter()
                                .zip(inputs)
                                .map(|(name, input)| div().child(*name).child(Input::new(&input))),
                        ),
                )
                .footer(crate::widgets::form_dialog_footer("Apply"))
                .on_ok(move |_, _, cx| {
                    let name = submitted[0].read(cx).value().to_string();
                    let parsed: Result<Vec<f32>, _> = submitted[1..]
                        .iter()
                        .map(|i| i.read(cx).value().parse::<f32>())
                        .collect();
                    let Ok(v) = parsed else {
                        return false;
                    };
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
                            let Ok(mut edits) = this.library_edit_set() else {
                                return false;
                            };
                            let Some(m) = edits.masks.iter_mut().find(|m| m.id == id) else {
                                return false;
                            };
                            m.name = name;
                            m.exposure = v[0];
                            m.contrast = v[1];
                            m.saturation = v[2];
                            m.temperature = v[3];
                            m.tint = v[4];
                            if let Some(c) = m.components.last_mut() {
                                match &mut c.shape {
                                    Shape::Luminance { range, feather } if v.len() == 8 => {
                                        *range = [v[5], v[6]];
                                        *feather = v[7];
                                    }
                                    Shape::Color {
                                        rgb,
                                        tolerance,
                                        feather,
                                    } if v.len() == 10 => {
                                        *rgb = [v[5], v[6], v[7]];
                                        *tolerance = v[8];
                                        *feather = v[9];
                                    }
                                    _ => {}
                                }
                            }
                            if edits.validate().is_err() {
                                return false;
                            }
                            this.library_commit_edit_set(edits, cx);
                            true
                        })
                        .unwrap_or(false)
                })
        });
    }
    pub(super) fn library_canvas_point(&self, position: Point<Pixels>) -> Option<[f32; 2]> {
        let nav = self.batch.navigation.borrow();
        let bounds = nav.bounds?;
        let (x, y) = nav.view.screen_to_doc(
            (f32::from(position.x) as f64, f32::from(position.y) as f64),
            &bounds,
        );
        if self.batch.develop.canvas_tool != 5
            && (x < 0. || y < 0. || x > nav.dimensions.0 as f64 || y > nav.dimensions.1 as f64)
        {
            return None;
        }
        Some([
            (x as f32 / nav.dimensions.0.max(1) as f32).clamp(0., 1.),
            (y as f32 / nav.dimensions.1.max(1) as f32).clamp(0., 1.),
        ])
    }
    pub(super) fn library_canvas_down(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.batch.develop.canvas_tool == 0 {
            return false;
        }
        if self.batch.develop.saving || self.batch.develop.preview_stale || self.batch.develop.busy
        {
            return true;
        }
        if let Some(point) = self.library_canvas_point(event.position) {
            if event.modifiers.alt {
                self.batch.develop.clone_source = Some(point);
            } else {
                let mut start = point;
                if self.batch.develop.canvas_tool == 5
                    && let Some(params) = self
                        .batch
                        .current
                        .and_then(|i| self.batch.items.get(i))
                        .and_then(|i| self.batch.develop.current_params(&i.path))
                {
                    let [l, t, r, b] = params.crop;
                    for (corner, opposite) in [
                        ([l, t], [r, b]),
                        ([r, t], [l, b]),
                        ([r, b], [l, t]),
                        ([l, b], [r, t]),
                    ] {
                        if (corner[0] - point[0]).abs() < 0.03
                            && (corner[1] - point[1]).abs() < 0.03
                        {
                            start = opposite;
                            break;
                        }
                    }
                }
                self.batch.develop.canvas_points = vec![start, point];
            }
            cx.notify();
        }
        true
    }
    pub(super) fn library_canvas_move(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.batch.develop.canvas_tool == 0 {
            return false;
        }
        if event.pressed_button == Some(MouseButton::Left)
            && !self.batch.develop.canvas_points.is_empty()
            && self.batch.develop.canvas_points.len() < 4096
        {
            if let Some(point) = self.library_canvas_point(event.position) {
                self.batch.develop.canvas_points.push(point);
                cx.notify();
            }
        }
        true
    }
    pub(super) fn library_canvas_up(&mut self, cx: &mut Context<Self>) {
        let points = std::mem::take(&mut self.batch.develop.canvas_points);
        let (Some(start), Some(end)) = (points.first().copied(), points.last().copied()) else {
            return;
        };
        let tool = self.batch.develop.canvas_tool;
        let radius = brush_radius(self.batch.develop.brush_radius);
        if tool >= 5 && tool <= 7 {
            let Some(mut p) = self
                .batch
                .current
                .and_then(|i| self.batch.items.get(i))
                .and_then(|i| self.batch.develop.current_params(&i.path))
            else {
                return;
            };
            match tool {
                5 => {
                    let crop = [
                        start[0].min(end[0]),
                        start[1].min(end[1]),
                        start[0].max(end[0]),
                        start[1].max(end[1]),
                    ];
                    if crop[2] - crop[0] > 0.001 && crop[3] - crop[1] > 0.001 {
                        p.crop = crop;
                    }
                }
                6 => {
                    let nav = self.batch.navigation.borrow();
                    p.straighten = -((end[1] - start[1]) * nav.dimensions.1 as f32)
                        .atan2((end[0] - start[0]) * nav.dimensions.0 as f32)
                        .to_degrees();
                    p.straighten = ((p.straighten + 45.).rem_euclid(90.) - 45.).clamp(-45., 45.);
                }
                _ => {
                    let d = [end[0] - start[0], end[1] - start[1]];
                    if d[1].abs() > d[0].abs() {
                        p.perspective[0] = (-d[0] / d[1]).clamp(-0.8, 0.8);
                    } else if d[0].abs() > 0.001 {
                        p.perspective[1] = (-d[1] / d[0]).clamp(-0.8, 0.8);
                    }
                }
            }
            if p.validate().is_ok() {
                self.library_adjust(p, cx);
            }
            return;
        }
        let Ok(mut edits) = self.library_edit_set() else {
            return;
        };
        if tool == 3 || tool == 4 {
            let source = self
                .batch
                .develop
                .clone_source
                .unwrap_or([(start[0] - 0.1).max(0.), start[1]]);
            if let Some(existing) = edits.spots.iter_mut().find(|s| {
                ((s.source[0] - start[0]).powi(2) + (s.source[1] - start[1]).powi(2)).sqrt() < 0.02
            }) {
                existing.source = end;
            } else if let Some(existing) = edits.spots.iter_mut().find(|s| {
                ((s.target[0] - start[0]).powi(2) + (s.target[1] - start[1]).powi(2)).sqrt() < 0.02
            }) {
                let delta = [end[0] - existing.target[0], end[1] - existing.target[1]];
                for p in &mut existing.stroke {
                    p[0] = (p[0] + delta[0]).clamp(0., 1.);
                    p[1] = (p[1] + delta[1]).clamp(0., 1.);
                }
                existing.target = end;
            } else {
                let id = edits.spots.iter().map(|s| s.id).max().unwrap_or(0) + 1;
                edits.spots.push(Spot {
                    id,
                    source,
                    target: start,
                    stroke: if points.len() > 2 {
                        let mut stroke: Vec<_> = points
                            .iter()
                            .step_by(points.len().div_ceil(511))
                            .copied()
                            .collect();
                        if stroke.last() != Some(&end) {
                            stroke.push(end);
                        }
                        stroke
                    } else {
                        vec![]
                    },
                    radius,
                    feather: 0.5,
                    opacity: 1.,
                    mode: if tool == 3 {
                        SpotMode::Heal
                    } else {
                        SpotMode::Clone
                    },
                });
            }
        } else {
            let shape = match tool {
                8 => Shape::Radial {
                    center: start,
                    radius: [
                        (end[0] - start[0]).abs().max(0.002),
                        (end[1] - start[1]).abs().max(0.002),
                    ],
                    feather: 0.5,
                },
                9 if start != end => Shape::Linear { start, end },
                _ => Shape::Brush {
                    points,
                    radius,
                    feather: 0.5,
                },
            };
            if let Some(mask) = self
                .batch
                .develop
                .active_mask
                .and_then(|id| edits.masks.iter_mut().find(|m| m.id == id))
            {
                mask.components.push(Component {
                    operation: if tool == 2 {
                        Operation::Subtract
                    } else if self.batch.develop.mask_intersect {
                        Operation::Intersect
                    } else {
                        Operation::Add
                    },
                    shape,
                });
            } else if tool != 2 {
                let id = edits.masks.iter().map(|m| m.id).max().unwrap_or(0) + 1;
                edits.masks.push(new_mask(id, format!("Mask {id}"), shape));
                self.batch.develop.active_mask = Some(id);
            }
        }
        self.library_commit_edit_set(edits, cx);
    }
}
fn new_mask(id: u32, name: String, shape: Shape) -> Mask {
    Mask {
        id,
        name,
        enabled: true,
        components: vec![Component {
            operation: Operation::Add,
            shape,
        }],
        exposure: 0.,
        contrast: 0.,
        saturation: 0.,
        temperature: 0.,
        tint: 0.,
    }
}

fn brush_radius(value: f32) -> f32 {
    if value == 0. { 0.03 } else { value }
}
