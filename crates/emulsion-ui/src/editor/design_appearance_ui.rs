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
enum Edit {
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
}
fn control(id: &'static str, label: impl Into<SharedString>) -> Button {
    Button::new(id)
        .label(label)
        .xsmall()
        .outline()
        .h(px(25.))
        .px(px(8.))
}
fn choice(
    id: &'static str,
    label: &str,
    state: &Entity<usize>,
    labels: &'static [&'static str],
    cx: &App,
) -> impl IntoElement {
    let current = *state.read(cx);
    let state = state.clone();
    control(
        id,
        format!("{label}: {}", labels[current.min(labels.len() - 1)]),
    )
    .dropdown_menu(move |mut menu, _, _| {
        for (i, label) in labels.iter().enumerate() {
            let state = state.clone();
            menu = menu.item(PopupMenuItem::new(*label).checked(i == current).on_click(
                move |_, window, cx| {
                    state.update(cx, |value, cx| {
                        *value = i;
                        cx.notify();
                    });
                    window.refresh();
                },
            ));
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
            PathPaint::LinearGradient { .. } => 2,
            PathPaint::RadialGradient { .. } => 3,
            PathPaint::Pattern { .. } => 1,
        }
    }
}
fn second_color(paint: PathPaint) -> [u8; 4] {
    match paint {
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
    /// A compact, wrapping row keeps formatting reachable without opening the
    /// full inspector. Every selection change derives fresh capabilities.
    pub(super) fn design_appearance_controls(
        &self,
        p: &Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_design() || self.previewing() {
            return None;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            return None;
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
        let mut row = div()
            .id("design-appearance-controls")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .px(px(12.))
            .py(px(5.))
            .flex_none()
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(if ids.len() == 1 {
                        "Appearance".into()
                    } else {
                        format!("{} objects", ids.len())
                    }),
            )
            .when(fill, |row| {
                row.child(
                    control(
                        "design-appearance-fill",
                        if paths {
                            "Fill / gradient…"
                        } else {
                            "Text / fill color…"
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
                    control("design-appearance-stroke", "Stroke…")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_appearance_dialog(Edit::Stroke, window, cx)
                        })),
                )
            })
            .child(
                control("design-appearance-opacity", "Opacity…")
                    .disabled(locked)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_appearance_dialog(Edit::Opacity, window, cx)
                    })),
            )
            .when(corners, |row| {
                row.child(
                    control("design-appearance-corners", "Corners…")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_appearance_dialog(Edit::Corners, window, cx)
                        })),
                )
            });
        if text.is_some() {
            row = row
                .child(
                    control("design-appearance-spacing", "Spacing…")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_appearance_dialog(Edit::Typography, window, cx)
                        })),
                )
                .child(
                    control("design-appearance-curve", "Curve…")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_appearance_dialog(Edit::Curve, window, cx)
                        })),
                )
                .child(
                    control("design-appearance-background", "Background…")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_appearance_dialog(Edit::Background, window, cx)
                        })),
                );
        }
        if let Some(id) = effect {
            row = row
                .child(
                    control("design-appearance-shadow", "Shadow…")
                        .disabled(locked)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_layer_effect_kind(id, "drop_shadow", window, cx)
                        })),
                )
                .when(text.is_some(), |row| {
                    row.child(
                        control("design-appearance-outline", "Outline…")
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_layer_effect_kind(id, "stroke", window, cx)
                            })),
                    )
                })
                .child(
                    control("design-appearance-effects", "Effects…")
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
                        control("design-appearance-group", "Group")
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
                        control("design-appearance-ungroup", "Ungroup")
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

    fn design_appearance_dialog(
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
        let angle = if let PathPaint::LinearGradient { angle, .. } = paint {
            angle
        } else {
            0.
        };
        let mut options: Vec<(&'static str, f32)> = Vec::new();
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
            Edit::Fill => options.push(("Gradient angle · degrees", angle)),
            Edit::Stroke => options.extend([
                ("Width · px", style.width),
                ("Gradient angle · degrees", angle),
            ]),
            Edit::Opacity => options.push(("Opacity · percent", first.opacity * 100.)),
            Edit::Corners => {
                if let NodeKind::Path { path, .. } = &first.kind
                    && let Some((_, _, _, _, r)) = ops::rectangle(path)
                {
                    options.push(("Corner radius · px", r as f32));
                }
            }
            Edit::Typography => {
                let Some(spec) = spec.as_ref() else { return };
                let selected = spec.style_at(range.as_ref().map_or(0, |r| r.start));
                options.extend([
                    ("Letter spacing · px", selected.letter_spacing),
                    ("Line height · multiple", spec.line_height),
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
                    ("Curve amount · percent", spec.warp.bend),
                    ("Horizontal distortion · percent", spec.warp.horizontal),
                    ("Vertical distortion · percent", spec.warp.vertical),
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
                    ("Horizontal padding · px", padding[0]),
                    ("Vertical padding · px", padding[1]),
                    ("Corner radius · px", radius),
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
        window.open_dialog(cx,move|dialog,_,cx|{
            let mut body=div().id("design-appearance-form").test_support().flex().flex_col().gap_3();
            let m=*mode.read(cx);
            match kind {
                Edit::Fill|Edit::Stroke=>{
                    body=body.child(choice("design-appearance-paint","Paint",&mode,if paths{&["None","Solid","Linear gradient","Radial gradient"]}else{&["None","Solid"]},cx));
                    if m>0 {body=body.child(ColorPicker::new(&pickers[0]).label(if m>1{"Start color"}else{"Color"}));}
                    if m>1 {body=body.child(ColorPicker::new(&pickers[1]).label("End color"));}
                    if stroke {body=body.child(choice("design-appearance-stroke-alignment","Alignment",&alignment,&["Inside","Center","Outside"],cx));}
                }
                Edit::Typography=>body=body.child(choice("design-appearance-text-alignment","Text alignment",&alignment,&["Left","Center","Right","Justify"],cx)),
                Edit::Curve=>body=body.child(choice("design-appearance-warp","Warp",&mode,&["None","Arc","Bulge","Flag"],cx)),
                Edit::Background=>{body=body.child(choice("design-appearance-backdrop-mode","Background",&mode,&["Remove","Enabled / refit"],cx));if m>0{body=body.child(ColorPicker::new(&pickers[0]).label("Background color"));}},
                _=>{}
            }
            for (i,(label,_)) in options.iter().enumerate() {
                if matches!(kind,Edit::Fill)&&m!=2 || matches!(kind,Edit::Stroke)&&i==1&&m!=2 {continue;}
                body=body.child(div().text_size(px(12.)).child(*label).child(Input::new(&inputs[i]).id(("design-appearance-input",i))));
            }
            if matches!(kind,Edit::Background) {body=body.child(div().text_size(px(11.)).child("Creates an editable vector background grouped with the text. Apply again to refit after editing text; ungroup to edit each object independently."));}
            if matches!(kind,Edit::Curve) {body=body.child(div().text_size(px(11.)).child("Text stays editable. Arc with a positive or negative amount curves the text in either direction; zero restores a straight baseline."));}
            let (inputs,pickers,mode,alignment,owner,ids,range)=(inputs.clone(),pickers.clone(),mode.clone(),alignment.clone(),owner.clone(),ids.clone(),range.clone());
            dialog.title(kind.title()).width(px(440.)).child(body).footer(crate::widgets::form_dialog_footer("Apply"))
                .on_ok(move|_,_,cx|{
                    let values:Vec<_>=inputs.iter().map(|input|input.read(cx).value().to_string()).collect();
                    let colors:Vec<_>=pickers.iter().map(|picker|picked(picker,cx)).collect();let mode=*mode.read(cx);let align=*alignment.read(cx);
                    owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket || this.selected_layer_roots()!=ids {this.set_status("The selection changed. Open appearance again.",true,cx);return false;}
                        let result=this.apply_design_appearance(kind,&ids,text,range.clone(),&values,&colors,mode,align);
                        match result {Ok((commands,select))=>{if commands.is_empty(){return true;}let applied=this.execute_layer_commands(kind.title(),commands,cx).is_some();if applied&&let Some(id)=select {this.set_layer_selection(vec![id],Some(id));cx.notify();}applied},Err(error)=>{this.set_status(error,true,cx);false}}
                    }).unwrap_or(false)
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
                let rgba = (mode != 0).then_some(colors[0]);
                if matches!(kind, Edit::Stroke) {
                    let width = number(0, 0., 1000.)?;
                    for id in ids {
                        let Some(NodeKind::Path { path, style, .. }) =
                            doc.node(*id).map(|n| &n.kind)
                        else {
                            return Err("Select vector shapes for a stroke.".into());
                        };
                        commands.push(Command::SetPath {
                            id: *id,
                            path: path.clone(),
                            style: PathStyle {
                                stroke: rgba,
                                stroke_paint: paint,
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
                        return Err("Select editable text.".into());
                    };
                    let mut spec = (**spec).clone();
                    spec.apply_style(range, |style| style.color = rgba.unwrap_or([0; 4]));
                    commands.push(Command::SetText {
                        id,
                        spec: Box::new(spec),
                    });
                } else {
                    commands = ops::fill(doc, ids, rgba, paint)?;
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
                let id = text.ok_or("Select editable text.")?;
                let Some(NodeKind::Text { spec, .. }) = doc.node(id).map(|n| &n.kind) else {
                    return Err("Missing text object".into());
                };
                let mut spec = (**spec).clone();
                if matches!(kind, Edit::Typography) {
                    let spacing = number(0, -50., 500.)?;
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
