//! Advanced effect controls and staged color pickers owned by the style inspector.
use super::*;
use emulsion_core::style_options::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Disableable, Sizable};

// Keep native button behavior; observe a small layout wrapper in UI tests.
struct EffectControl<T> {
    id: gpui_kit::ElementId,
    inner: T,
}
fn effect_button(id: impl Into<gpui_kit::ElementId>) -> EffectControl<Button> {
    let id = id.into();
    EffectControl {
        id: id.clone(),
        inner: Button::new(SharedString::from(format!("effect-button:{id:?}"))),
    }
}
impl EffectControl<Button> {
    fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.inner = self.inner.label(label);
        self
    }
    fn small(mut self) -> Self {
        self.inner = self.inner.small();
        self
    }
    fn disabled(mut self, value: bool) -> Self {
        self.inner = self.inner.disabled(value);
        self
    }
    fn on_click(
        mut self,
        handler: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.inner = self.inner.on_click(handler);
        self
    }
    fn dropdown_menu(
        self,
        builder: impl Fn(
            gpui_kit::component::menu::PopupMenu,
            &mut Window,
            &mut Context<gpui_kit::component::menu::PopupMenu>,
        ) -> gpui_kit::component::menu::PopupMenu
        + 'static,
    ) -> EffectControl<impl IntoElement> {
        EffectControl {
            id: self.id,
            inner: self.inner.dropdown_menu(builder),
        }
    }
}
impl<T: IntoElement> EffectControl<T> {
    fn test_support(self) -> AnyElement {
        div()
            .id(self.id)
            .child(self.inner)
            .test_support()
            .into_any_element()
    }
}

pub(crate) struct ColorDraft {
    pub state: Entity<ColorPickerState>,
    baseline: [u8; 4],
    value: [u8; 4],
    _subscription: Subscription,
}
pub(crate) fn effect_options(node: &Node) -> Vec<StyleOptions> {
    let mut options: Vec<_> = node
        .styles
        .iter()
        .enumerate()
        .map(|(i, s)| {
            node.style_options
                .get(i)
                .cloned()
                .unwrap_or_else(|| StyleOptions::for_style(s))
        })
        .collect();
    let mut used: std::collections::HashSet<_> = options
        .iter()
        .filter_map(|o| (o.id != 0).then_some(o.id))
        .collect();
    let mut seen = std::collections::HashSet::new();
    for option in &mut options {
        if option.id == 0 || !seen.insert(option.id) {
            option.id = (1..).find(|id| !used.contains(id)).expect("effect ID");
            used.insert(option.id);
            seen.insert(option.id);
        }
    }
    options
}
fn rgba_color(c: [u8; 4]) -> Hsla {
    gpui_kit::rgba(
        ((c[0] as u32) << 24) | ((c[1] as u32) << 16) | ((c[2] as u32) << 8) | c[3] as u32,
    )
    .into()
}
fn color_bytes(c: Hsla) -> [u8; 4] {
    let c = c.to_rgb();
    [c.r, c.g, c.b, c.a].map(|v| (v * 255.).round().clamp(0., 255.) as u8)
}
fn control_id(id: NodeId, effect: u64, name: &str) -> SharedString {
    format!("style-{id}-{effect}-{name}").into()
}
fn stops(style: &LayerStyle, options: &StyleOptions) -> Vec<GradientStop> {
    if !options.gradient.stops.is_empty() {
        return options.gradient.stops.clone();
    }
    let colors = style.colors();
    let first = colors.first().copied().unwrap_or([0; 3]);
    let last = colors.get(1).copied().unwrap_or([255; 3]);
    vec![
        GradientStop {
            position: 0.,
            color: [first[0], first[1], first[2], 255],
        },
        GradientStop {
            position: 1.,
            color: [last[0], last[1], last[2], 255],
        },
    ]
}
impl EditorView {
    pub(crate) fn sync_style_color_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((expanded_node, expanded_index)) = self.styles_ui.expanded else {
            self.styles_ui.colors.clear();
            return;
        };
        if self.selected != Some(expanded_node) {
            self.styles_ui.colors.clear();
            return;
        }
        let Some(node) = self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .cloned()
        else {
            self.styles_ui.colors.clear();
            return;
        };
        let options = effect_options(&node);
        let mut desired = Vec::new();
        for (i, style) in node.styles.iter().enumerate() {
            if i != expanded_index {
                continue;
            }
            let effect = options[i].id;
            let alpha = style
                .params()
                .into_iter()
                .find(|p| p.key == "opacity")
                .map_or(255, |p| (p.value * 2.55).round().clamp(0., 255.) as u8);
            for (slot, color) in style.colors().into_iter().enumerate() {
                desired.push((
                    (node.id, effect, slot),
                    [color[0], color[1], color[2], alpha],
                ));
            }
            if matches!(style, LayerStyle::GradientOverlay { .. })
                || options[i].fill == FillType::Gradient
            {
                for (slot, stop) in stops(style, &options[i]).into_iter().enumerate() {
                    desired.push(((node.id, effect, 100 + slot), stop.color));
                }
            }
        }
        self.styles_ui
            .colors
            .retain(|key, _| desired.iter().any(|(other, _)| key == other));
        for (key, value) in desired {
            if let Some(draft) = self.styles_ui.colors.get_mut(&key) {
                if draft.baseline != value {
                    draft.baseline = value;
                    draft.value = value;
                    if draft.state.read(cx).value().map(color_bytes) != Some(value) {
                        draft
                            .state
                            .update(cx, |s, cx| s.set_value(rgba_color(value), window, cx));
                    }
                }
                continue;
            }
            let state =
                cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgba_color(value)));
            let subscription =
                cx.subscribe(&state, move |this, _, event: &ColorPickerEvent, cx| {
                    if let ColorPickerEvent::Change(Some(color)) = event {
                        if let Some(draft) = this.styles_ui.colors.get_mut(&key) {
                            draft.value = color_bytes(*color);
                            cx.notify();
                        }
                        if this.styles_ui.dialog_for == Some(key.0) {
                            this.apply_style_color(key, cx);
                        }
                    }
                });
            self.styles_ui.colors.insert(
                key,
                ColorDraft {
                    state,
                    baseline: value,
                    value,
                    _subscription: subscription,
                },
            );
        }
    }
    fn apply_style_color(&mut self, key: (NodeId, u64, usize), cx: &mut Context<Self>) {
        let Some(value) = self.styles_ui.colors.get(&key).map(|d| d.value) else {
            return;
        };
        let Some(node) = self.editor.doc.node(key.0) else {
            return;
        };
        let mut options = effect_options(node);
        let Some(index) = options.iter().position(|o| o.id == key.1) else {
            return;
        };
        let mut styles = node.styles.clone();
        if key.2 >= 100 {
            let mut values = stops(&styles[index], &options[index]);
            if let Some(stop) = values.get_mut(key.2 - 100) {
                stop.color = value;
            }
            options[index].gradient.stops = values;
        } else {
            styles[index].set_color([value[0], value[1], value[2]], key.2 == 1);
            if self
                .styles_ui
                .colors
                .get(&key)
                .is_some_and(|draft| draft.baseline[3] != value[3])
            {
                styles[index].set_param("opacity", value[3] as f32 / 2.55);
            }
        }
        self.execute(
            Command::SetLayerEffects {
                id: key.0,
                styles,
                options,
            },
            cx,
        );
    }
    fn style_color_control(
        &self,
        id: NodeId,
        effect: u64,
        slot: usize,
        title: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = (id, effect, slot);
        let Some(draft) = self.styles_ui.colors.get(&key) else {
            return div().into_any_element();
        };
        let picker = if self.styles_ui.dialog_for == Some(id) {
            let title = title.to_string();
            Button::new(control_id(id, effect, &format!("color-open-{slot}")))
                .small()
                .label(title.clone())
                .child(div().size_5().rounded_sm().bg(rgba_color(draft.value)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_style_color_dialog(key, title.clone(), window, cx);
                }))
                .into_any_element()
        } else {
            ColorPicker::new(&draft.state)
                .small()
                .label(SharedString::from(title.to_string()))
                .into_any_element()
        };
        let mut row = div()
            .id(control_id(id, effect, &format!("color-{slot}")))
            .flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .child(
                div()
                    .id(control_id(id, effect, &format!("picker-{slot}")))
                    .flex_none()
                    .child(picker)
                    .test_support(),
            );
        if draft.value != draft.baseline && self.styles_ui.dialog_for != Some(id) {
            row = row
                .child(
                    effect_button(control_id(id, effect, &format!("color-apply-{slot}")))
                        .label("Apply color")
                        .small()
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.apply_style_color(key, cx)),
                        )
                        .test_support(),
                )
                .child(
                    effect_button(control_id(id, effect, &format!("color-cancel-{slot}")))
                        .label("Cancel")
                        .small()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(draft) = this.styles_ui.colors.get_mut(&key) {
                                draft.value = draft.baseline;
                                draft.state.update(cx, |s, cx| {
                                    s.set_value(rgba_color(draft.baseline), window, cx)
                                });
                                cx.notify();
                            }
                        }))
                        .test_support(),
                );
        }
        row.into_any_element()
    }
    pub(crate) fn update_style_option(
        &mut self,
        id: NodeId,
        index: usize,
        update: impl FnOnce(&mut StyleOptions),
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let styles = node.styles.clone();
        let mut options = effect_options(node);
        if let Some(option) = options.get_mut(index) {
            update(option);
            self.execute(
                Command::SetLayerEffects {
                    id,
                    styles,
                    options,
                },
                cx,
            );
        }
    }
    pub(crate) fn set_style_pattern(
        &mut self,
        id: NodeId,
        index: usize,
        image: Arc<PatternImage>,
        cx: &mut Context<Self>,
    ) {
        self.update_style_option(id, index, |o| o.pattern.image = Some(image), cx);
    }
    pub(crate) fn set_style_option_param(
        &mut self,
        id: NodeId,
        index: usize,
        key: &'static str,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        self.update_style_option(
            id,
            index,
            |o| match key {
                "spread" => o.spread = value,
                "choke" => o.choke = value,
                "noise" => o.noise = value,
                "altitude" => o.altitude = value,
                "soften" => o.soften = value,
                "highlight_opacity" => o.highlight_opacity = value,
                "shadow_opacity" => o.shadow_opacity = value,
                "texture_depth" => o.texture_depth = value,
                "gradient_scale" => o.gradient.scale = value,
                "gradient_angle" => o.gradient.angle = value,
                "gradient_x" => o.gradient.offset_x = value,
                "gradient_y" => o.gradient.offset_y = value,
                "pattern_scale" => o.pattern.scale = value,
                "pattern_x" => o.pattern.offset_x = value,
                "pattern_y" => o.pattern.offset_y = value,
                "pattern_angle" => o.pattern.angle = value,
                _ => {}
            },
            cx,
        );
    }
    pub(crate) fn set_style_stop_param(
        &mut self,
        id: NodeId,
        index: usize,
        stop: usize,
        opacity: bool,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(style) = self
            .editor
            .doc
            .node(id)
            .and_then(|n| n.styles.get(index))
            .cloned()
        else {
            return;
        };
        self.update_style_option(
            id,
            index,
            |o| {
                let mut values = stops(&style, o);
                if let Some(s) = values.get_mut(stop) {
                    if opacity {
                        s.color[3] = (value * 2.55).round().clamp(0., 255.) as u8;
                    } else {
                        s.position = (value / 100.).clamp(0., 1.);
                    }
                }
                o.gradient.stops = values;
            },
            cx,
        );
    }
    pub(crate) fn set_style_contour_param(
        &mut self,
        id: NodeId,
        index: usize,
        point: usize,
        y: bool,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        self.update_style_option(
            id,
            index,
            |o| {
                if let Some(p) = o.contour.get_mut(point) {
                    if y {
                        p.y = value / 100.;
                    } else {
                        p.x = value / 100.;
                    }
                }
            },
            cx,
        );
    }
    pub(crate) fn set_style_global_light(
        &mut self,
        altitude: bool,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        let mut light = self.editor.doc.global_light;
        if altitude {
            light.altitude = value;
        } else {
            light.angle = value;
        }
        self.execute(Command::SetGlobalLight { light }, cx);
    }
    fn effect_slider(
        &mut self,
        (id, index, key): (NodeId, usize, &'static str),
        title: &str,
        value: f32,
        range: (f32, f32, f32),
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.param_slider(
            SliderKey::StyleOption(id, index, key),
            title,
            format!("{value:.0}"),
            (value - range.0) / (range.1 - range.0),
            range,
            p,
            cx,
        )
        .into_any_element()
    }
    fn style_toggle(
        &self,
        (id, index, effect): (NodeId, usize, u64),
        key: &'static str,
        title: &str,
        value: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        effect_button(control_id(id, effect, key))
            .label(format!("{} {title}", if value { "✓" } else { "□" }))
            .small()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.update_style_option(
                    id,
                    index,
                    |o| match key {
                        "enabled" => o.enabled = !o.enabled,
                        "global-light" => o.use_global_light = !o.use_global_light,
                        "reverse" => o.gradient.reverse = !o.gradient.reverse,
                        "invert-contour" => o.invert_contour = !o.invert_contour,
                        "texture-invert" => o.texture_invert = !o.texture_invert,
                        _ => {}
                    },
                    cx,
                )
            }))
            .test_support()
            .into_any_element()
    }
    fn style_choice(
        &self,
        (id, index, effect): (NodeId, usize, u64),
        key: &'static str,
        title: &str,
        current: usize,
        labels: &[&'static str],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = cx.entity().downgrade();
        let choices = labels.to_vec();
        effect_button(control_id(id, effect, key))
            .label(if key.ends_with("preset") {
                format!("{title}…")
            } else {
                format!("{title}: {}", labels[current])
            })
            .small()
            .dropdown_menu(move |mut menu, _, _| {
                for (value, label) in choices.iter().enumerate() {
                    let weak = editor.clone();
                    menu = menu.item(
                        PopupMenuItem::new(*label)
                            .checked(value == current)
                            .on_click(move |_, _, cx| {
                                if let Some(editor) = weak.upgrade() {
                                    editor.update(cx, |this, cx| {
                                        this.set_style_choice(id, index, key, value, cx)
                                    });
                                }
                            }),
                    );
                }
                menu
            })
            .test_support()
            .into_any_element()
    }
    fn set_style_choice(
        &mut self,
        id: NodeId,
        index: usize,
        key: &'static str,
        value: usize,
        cx: &mut Context<Self>,
    ) {
        self.update_style_option(
            id,
            index,
            |o| match key {
                "fill" => o.fill = [FillType::Solid, FillType::Gradient, FillType::Pattern][value],
                "stroke-position" => {
                    o.stroke_position = [
                        StrokePosition::Outside,
                        StrokePosition::Inside,
                        StrokePosition::Center,
                    ][value]
                }
                "gradient-kind" => {
                    o.gradient.kind = [
                        GradientKind::Linear,
                        GradientKind::Radial,
                        GradientKind::Angle,
                        GradientKind::Reflected,
                        GradientKind::Diamond,
                    ][value]
                }
                "bevel-style" => {
                    o.bevel_style = [
                        BevelStyle::Inner,
                        BevelStyle::Outer,
                        BevelStyle::Emboss,
                        BevelStyle::Pillow,
                        BevelStyle::Stroke,
                    ][value]
                }
                "technique" => {
                    o.technique = [
                        Technique::Smooth,
                        Technique::ChiselHard,
                        Technique::ChiselSoft,
                    ][value]
                }
                "glow-source" => o.glow_source = [GlowSource::Edge, GlowSource::Center][value],
                "contour-preset" => {
                    o.contour = match value {
                        1 => vec![
                            ContourPoint { x: 0., y: 0. },
                            ContourPoint { x: 0.5, y: 1. },
                            ContourPoint { x: 1., y: 0. },
                        ],
                        2 => vec![
                            ContourPoint { x: 0., y: 0. },
                            ContourPoint { x: 0.25, y: 1. },
                            ContourPoint { x: 0.5, y: 0. },
                            ContourPoint { x: 0.75, y: 1. },
                            ContourPoint { x: 1., y: 0. },
                        ],
                        _ => vec![ContourPoint { x: 0., y: 0. }, ContourPoint { x: 1., y: 1. }],
                    }
                }
                "gradient-preset" => {
                    o.gradient.stops = match value {
                        1 => vec![
                            GradientStop {
                                position: 0.,
                                color: [255, 0, 0, 255],
                            },
                            GradientStop {
                                position: 0.5,
                                color: [255, 220, 0, 255],
                            },
                            GradientStop {
                                position: 1.,
                                color: [40, 40, 255, 255],
                            },
                        ],
                        2 => vec![
                            GradientStop {
                                position: 0.,
                                color: [0, 0, 0, 0],
                            },
                            GradientStop {
                                position: 1.,
                                color: [0, 0, 0, 255],
                            },
                        ],
                        _ => vec![
                            GradientStop {
                                position: 0.,
                                color: [0, 0, 0, 255],
                            },
                            GradientStop {
                                position: 1.,
                                color: [255, 255, 255, 255],
                            },
                        ],
                    }
                }
                _ => {}
            },
            cx,
        );
    }
    fn style_blend_choice(
        &self,
        (id, index, effect): (NodeId, usize, u64),
        key: &'static str,
        title: &str,
        value: BlendMode,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = cx.entity().downgrade();
        effect_button(control_id(id, effect, key))
            .label(format!("{title}: {}", value.label()))
            .small()
            .dropdown_menu(move |mut menu, _, _| {
                for mode in BlendMode::MENU.iter().flatten().copied() {
                    let weak = editor.clone();
                    menu = menu.item(
                        PopupMenuItem::new(mode.label())
                            .checked(mode == value)
                            .on_click(move |_, _, cx| {
                                if let Some(editor) = weak.upgrade() {
                                    editor.update(cx, |this, cx| {
                                        this.update_style_option(
                                            id,
                                            index,
                                            |o| match key {
                                                "highlight-blend" => o.highlight_blend = mode,
                                                "shadow-blend" => o.shadow_blend = mode,
                                                _ => o.blend = mode,
                                            },
                                            cx,
                                        )
                                    });
                                }
                            }),
                    );
                }
                menu
            })
            .test_support()
            .into_any_element()
    }
    pub(super) fn effect_controls(
        &mut self,
        (id, index): (NodeId, usize),
        style: &LayerStyle,
        option: &StyleOptions,
        count: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let effect = option.id;
        let in_dialog = self.styles_ui.dialog_for == Some(id);
        let expanded = in_dialog || self.styles_ui.expanded == Some((id, index));
        let mut views = vec![
            div()
                .flex()
                .items_center()
                .gap_1()
                .pt_2()
                .child(self.style_toggle(
                    (id, index, effect),
                    "enabled",
                    if option.enabled { "On" } else { "Off" },
                    option.enabled,
                    cx,
                ))
                .child(
                    effect_button(control_id(id, effect, "expand"))
                        .label(format!(
                            "{} {}",
                            if expanded { "▾" } else { "▸" },
                            style.label()
                        ))
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.styles_ui.expanded =
                                if this.styles_ui.expanded == Some((id, index)) {
                                    None
                                } else {
                                    Some((id, index))
                                };
                            cx.notify();
                        }))
                        .test_support(),
                )
                .into_any_element(),
        ];
        if !expanded {
            return views;
        }
        if in_dialog {
            views.clear();
        }
        views.push(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    effect_button(control_id(id, effect, "up"))
                        .label("Move up")
                        .small()
                        .disabled(index == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.move_style_effect(id, index, -1, cx)
                        }))
                        .test_support(),
                )
                .child(
                    effect_button(control_id(id, effect, "down"))
                        .label("Move down")
                        .small()
                        .disabled(index + 1 >= count)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.move_style_effect(id, index, 1, cx)
                        }))
                        .test_support(),
                )
                .child(
                    effect_button(control_id(id, effect, "remove"))
                        .label("Remove")
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_style_effect(id, index, cx)
                        }))
                        .test_support(),
                )
                .into_any_element(),
        );
        if !matches!(style, LayerStyle::BevelEmboss { .. }) {
            views.push(self.style_blend_choice(
                (id, index, effect),
                "blend",
                "Blend mode",
                option.blend,
                cx,
            ));
        }
        let show_colors = !(matches!(style, LayerStyle::GradientOverlay { .. })
            && !option.gradient.stops.is_empty()
            || matches!(style, LayerStyle::PatternOverlay { .. })
                && option.pattern.image.is_some()
            || matches!(
                style,
                LayerStyle::Stroke { .. }
                    | LayerStyle::OuterGlow { .. }
                    | LayerStyle::InnerGlow { .. }
            ) && option.fill != FillType::Solid);
        for slot in 0..if show_colors { style.colors().len() } else { 0 } {
            views.push(self.style_color_control(
                id,
                effect,
                slot,
                if slot == 0 { "Color" } else { "Second color" },
                cx,
            ));
        }
        for spec in style.params() {
            if option.use_global_light && spec.key == "angle" {
                continue;
            }
            views.push(
                self.param_slider(
                    SliderKey::Style(id, index, spec.key),
                    spec.label,
                    spec.display(),
                    (spec.value - spec.min) / (spec.max - spec.min).max(1e-6),
                    (spec.min, spec.max, spec.step),
                    p,
                    cx,
                )
                .into_any_element(),
            );
        }
        views.push(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    effect_button(("style-save-default", index))
                        .label("Save default")
                        .small()
                        .on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.save_style_default(id, index, cx)
                            }),
                        )
                        .test_support(),
                )
                .child(
                    effect_button(("style-reset-default", index))
                        .label("Reset default")
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.reset_style_default(id, index, cx)
                        }))
                        .test_support(),
                )
                .child(
                    effect_button(control_id(id, effect, "advanced"))
                        .label(if self.styles_ui.advanced == Some((id, index)) {
                            "Hide advanced"
                        } else {
                            "Advanced"
                        })
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.styles_ui.advanced =
                                if this.styles_ui.advanced == Some((id, index)) {
                                    None
                                } else {
                                    Some((id, index))
                                };
                            cx.notify();
                        }))
                        .test_support(),
                )
                .into_any_element(),
        );
        if self.styles_ui.advanced != Some((id, index)) {
            return views;
        }
        if matches!(
            style,
            LayerStyle::DropShadow { .. }
                | LayerStyle::InnerShadow { .. }
                | LayerStyle::BevelEmboss { .. }
        ) {
            views.push(self.style_toggle(
                (id, index, effect),
                "global-light",
                "Use global light",
                option.use_global_light,
                cx,
            ));
            if option.use_global_light {
                let light = self.editor.doc.global_light;
                views.push(
                    self.param_slider(
                        SliderKey::StyleGlobalLight(false),
                        "Global angle",
                        format!("{:.0}°", light.angle),
                        (light.angle + 180.) / 360.,
                        (-180., 180., 1.),
                        p,
                        cx,
                    )
                    .into_any_element(),
                );
                views.push(
                    self.param_slider(
                        SliderKey::StyleGlobalLight(true),
                        "Global altitude",
                        format!("{:.0}°", light.altitude),
                        light.altitude / 90.,
                        (0., 90., 1.),
                        p,
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        if matches!(
            style,
            LayerStyle::DropShadow { .. } | LayerStyle::OuterGlow { .. }
        ) {
            views.push(self.effect_slider(
                (id, index, "spread"),
                "Spread (%)",
                option.spread,
                (0., 100., 1.),
                p,
                cx,
            ));
        }
        if matches!(
            style,
            LayerStyle::InnerShadow { .. } | LayerStyle::InnerGlow { .. }
        ) {
            views.push(self.effect_slider(
                (id, index, "choke"),
                "Choke (%)",
                option.choke,
                (0., 100., 1.),
                p,
                cx,
            ));
        }
        if matches!(
            style,
            LayerStyle::DropShadow { .. }
                | LayerStyle::InnerShadow { .. }
                | LayerStyle::OuterGlow { .. }
                | LayerStyle::InnerGlow { .. }
                | LayerStyle::Satin { .. }
        ) {
            views.push(self.effect_slider(
                (id, index, "noise"),
                "Noise (%)",
                option.noise,
                (0., 100., 1.),
                p,
                cx,
            ));
        }
        if matches!(style, LayerStyle::Stroke { .. }) {
            views.push(self.style_choice(
                (id, index, effect),
                "stroke-position",
                "Position",
                option.stroke_position as usize,
                &["Outside", "Inside", "Center"],
                cx,
            ));
            views.push(self.style_choice(
                (id, index, effect),
                "fill",
                "Fill type",
                option.fill as usize,
                &["Color", "Gradient", "Pattern"],
                cx,
            ));
        }
        if matches!(style, LayerStyle::InnerGlow { .. }) {
            views.push(self.style_choice(
                (id, index, effect),
                "glow-source",
                "Source",
                option.glow_source as usize,
                &["Edge", "Center"],
                cx,
            ));
        }
        if matches!(
            style,
            LayerStyle::OuterGlow { .. } | LayerStyle::InnerGlow { .. }
        ) {
            views.push(self.style_choice(
                (id, index, effect),
                "fill",
                "Fill type",
                usize::from(option.fill == FillType::Gradient),
                &["Color", "Gradient"],
                cx,
            ));
        }
        if matches!(
            style,
            LayerStyle::InnerGlow { .. }
                | LayerStyle::OuterGlow { .. }
                | LayerStyle::BevelEmboss { .. }
        ) {
            views.push(self.style_choice(
                (id, index, effect),
                "technique",
                "Technique",
                option.technique as usize,
                &["Smooth", "Chisel hard", "Chisel soft"],
                cx,
            ));
        }
        if matches!(style, LayerStyle::BevelEmboss { .. }) {
            views.push(self.style_choice(
                (id, index, effect),
                "bevel-style",
                "Style",
                option.bevel_style as usize,
                &[
                    "Inner bevel",
                    "Outer bevel",
                    "Emboss",
                    "Pillow emboss",
                    "Stroke emboss",
                ],
                cx,
            ));
            if !option.use_global_light {
                views.push(self.effect_slider(
                    (id, index, "altitude"),
                    "Altitude (°)",
                    option.altitude,
                    (0., 90., 1.),
                    p,
                    cx,
                ));
            }
            views.push(self.effect_slider(
                (id, index, "soften"),
                "Soften (px)",
                option.soften,
                (0., 100., 1.),
                p,
                cx,
            ));
            views.push(self.style_blend_choice(
                (id, index, effect),
                "highlight-blend",
                "Highlight mode",
                option.highlight_blend,
                cx,
            ));
            views.push(self.effect_slider(
                (id, index, "highlight_opacity"),
                "Highlight opacity (%)",
                option.highlight_opacity,
                (0., 100., 1.),
                p,
                cx,
            ));
            views.push(self.style_blend_choice(
                (id, index, effect),
                "shadow-blend",
                "Shadow mode",
                option.shadow_blend,
                cx,
            ));
            views.push(self.effect_slider(
                (id, index, "shadow_opacity"),
                "Shadow opacity (%)",
                option.shadow_opacity,
                (0., 100., 1.),
                p,
                cx,
            ));
            views.push(self.effect_slider(
                (id, index, "texture_depth"),
                "Texture depth (%)",
                option.texture_depth,
                (-1000., 1000., 1.),
                p,
                cx,
            ));
            views.push(self.style_toggle(
                (id, index, effect),
                "texture-invert",
                "Invert texture",
                option.texture_invert,
                cx,
            ));
        }
        if matches!(style, LayerStyle::GradientOverlay { .. })
            || matches!(
                style,
                LayerStyle::Stroke { .. }
                    | LayerStyle::OuterGlow { .. }
                    | LayerStyle::InnerGlow { .. }
            ) && option.fill == FillType::Gradient
        {
            views.extend(self.gradient_controls(id, index, style, option, p, cx));
        }
        if matches!(
            style,
            LayerStyle::PatternOverlay { .. } | LayerStyle::BevelEmboss { .. }
        ) || matches!(style, LayerStyle::Stroke { .. }) && option.fill == FillType::Pattern
        {
            views.extend(self.pattern_controls(id, index, option, p, cx));
        }
        if matches!(
            style,
            LayerStyle::DropShadow { .. }
                | LayerStyle::InnerShadow { .. }
                | LayerStyle::OuterGlow { .. }
                | LayerStyle::InnerGlow { .. }
                | LayerStyle::Satin { .. }
                | LayerStyle::BevelEmboss { .. }
        ) {
            views.extend(self.contour_controls(id, index, option, p, cx));
        }
        views
    }
    fn move_style_effect(
        &mut self,
        id: NodeId,
        index: usize,
        direction: isize,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let target = index as isize + direction;
        if target < 0 || target >= node.styles.len() as isize {
            return;
        }
        let mut styles = node.styles.clone();
        let mut options = effect_options(node);
        styles.swap(index, target as usize);
        options.swap(index, target as usize);
        self.styles_ui.expanded = Some((id, target as usize));
        self.styles_ui.advanced = None;
        self.execute(
            Command::SetLayerEffects {
                id,
                styles,
                options,
            },
            cx,
        );
    }
    fn remove_style_effect(&mut self, id: NodeId, index: usize, cx: &mut Context<Self>) {
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        if index >= node.styles.len() {
            return;
        }
        let mut styles = node.styles.clone();
        let mut options = effect_options(node);
        styles.remove(index);
        options.remove(index);
        self.styles_ui.expanded = None;
        self.styles_ui.advanced = None;
        self.execute(
            Command::SetLayerEffects {
                id,
                styles,
                options,
            },
            cx,
        );
    }
    fn gradient_controls(
        &mut self,
        id: NodeId,
        index: usize,
        style: &LayerStyle,
        option: &StyleOptions,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let effect = option.id;
        let mut views = vec![
            label("Gradient", p).into_any_element(),
            self.style_choice(
                (id, index, effect),
                "gradient-kind",
                "Style",
                option.gradient.kind as usize,
                &["Linear", "Radial", "Angle", "Reflected", "Diamond"],
                cx,
            ),
            self.style_choice(
                (id, index, effect),
                "gradient-preset",
                "Preset",
                0,
                &["Black to white", "Spectrum", "Transparent to black"],
                cx,
            ),
            self.style_toggle(
                (id, index, effect),
                "reverse",
                "Reverse",
                option.gradient.reverse,
                cx,
            ),
        ];
        for (key, title, value, range) in [
            (
                "gradient_scale",
                "Scale (%)",
                option.gradient.scale,
                (1., 1000., 1.),
            ),
            (
                "gradient_x",
                "Horizontal offset (px)",
                option.gradient.offset_x,
                (-2000., 2000., 1.),
            ),
            (
                "gradient_y",
                "Vertical offset (px)",
                option.gradient.offset_y,
                (-2000., 2000., 1.),
            ),
        ] {
            views.push(self.effect_slider((id, index, key), title, value, range, p, cx));
        }
        let values = stops(style, option);
        if !matches!(style, LayerStyle::GradientOverlay { .. }) {
            views.push(self.effect_slider(
                (id, index, "gradient_angle"),
                "Gradient rotation (°)",
                option.gradient.angle,
                (-180., 180., 1.),
                p,
                cx,
            ));
        }
        for (stop, value) in values.iter().enumerate() {
            views.push(self.style_color_control(
                id,
                effect,
                100 + stop,
                &format!("Stop {}", stop + 1),
                cx,
            ));
            views.push(
                self.param_slider(
                    SliderKey::StyleStop(id, index, stop, false),
                    "Position",
                    format!("{:.0}%", value.position * 100.),
                    value.position,
                    (0., 100., 1.),
                    p,
                    cx,
                )
                .into_any_element(),
            );
            views.push(
                self.param_slider(
                    SliderKey::StyleStop(id, index, stop, true),
                    "Opacity",
                    format!("{:.0}%", value.color[3] as f32 / 2.55),
                    value.color[3] as f32 / 255.,
                    (0., 100., 1.),
                    p,
                    cx,
                )
                .into_any_element(),
            );
            views.push(
                effect_button(control_id(id, effect, &format!("stop-remove-{stop}")))
                    .label("Remove stop")
                    .small()
                    .disabled(values.len() <= 2)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(style) = this
                            .editor
                            .doc
                            .node(id)
                            .and_then(|n| n.styles.get(index))
                            .cloned()
                        else {
                            return;
                        };
                        this.update_style_option(
                            id,
                            index,
                            |o| {
                                let mut values = stops(&style, o);
                                if values.len() > 2 && stop < values.len() {
                                    values.remove(stop);
                                }
                                o.gradient.stops = values;
                            },
                            cx,
                        );
                    }))
                    .test_support()
                    .into_any_element(),
            );
        }
        views.push(
            effect_button(control_id(id, effect, "stop-add"))
                .label("Add color stop")
                .small()
                .disabled(values.len() >= 64)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let Some(style) = this
                        .editor
                        .doc
                        .node(id)
                        .and_then(|n| n.styles.get(index))
                        .cloned()
                    else {
                        return;
                    };
                    this.update_style_option(
                        id,
                        index,
                        |o| {
                            let mut values = stops(&style, o);
                            if values.len() < 64 {
                                values.push(GradientStop {
                                    position: 0.5,
                                    color: [255; 4],
                                });
                            }
                            o.gradient.stops = values;
                        },
                        cx,
                    );
                }))
                .test_support()
                .into_any_element(),
        );
        views
    }
    fn pattern_controls(
        &mut self,
        id: NodeId,
        index: usize,
        option: &StyleOptions,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let effect = option.id;
        let description = option
            .pattern
            .image
            .as_ref()
            .map(|i| format!("Embedded image · {} × {} px", i.width, i.height))
            .unwrap_or_else(|| "Built-in checker pattern".into());
        let mut views = vec![
            label("Pattern", p).into_any_element(),
            mono(description, 10., p.muted).into_any_element(),
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    effect_button(control_id(id, effect, "pattern-import"))
                        .label("Import pattern…")
                        .small()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.import_style_pattern(id, index, window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    effect_button(control_id(id, effect, "pattern-reset"))
                        .label("Use checker")
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_style_option(id, index, |o| o.pattern.image = None, cx)
                        }))
                        .test_support(),
                )
                .into_any_element(),
        ];
        for (key, title, value, range) in [
            (
                "pattern_scale",
                "Scale (%)",
                option.pattern.scale,
                (1., 1000., 1.),
            ),
            (
                "pattern_angle",
                "Rotation (°)",
                option.pattern.angle,
                (-180., 180., 1.),
            ),
            (
                "pattern_x",
                "Horizontal offset (px)",
                option.pattern.offset_x,
                (-2000., 2000., 1.),
            ),
            (
                "pattern_y",
                "Vertical offset (px)",
                option.pattern.offset_y,
                (-2000., 2000., 1.),
            ),
        ] {
            views.push(self.effect_slider((id, index, key), title, value, range, p, cx));
        }
        views
    }
    fn contour_controls(
        &mut self,
        id: NodeId,
        index: usize,
        option: &StyleOptions,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let effect = option.id;
        let mut views = vec![
            label("Contour", p).into_any_element(),
            self.style_choice(
                (id, index, effect),
                "contour-preset",
                "Preset",
                0,
                &["Linear", "Cone", "Double cone"],
                cx,
            ),
            self.style_toggle(
                (id, index, effect),
                "invert-contour",
                "Invert contour",
                option.invert_contour,
                cx,
            ),
        ];
        for (point, value) in option.contour.iter().enumerate() {
            views.push(mono(format!("Point {}", point + 1), 10., p.muted).into_any_element());
            for (y, title, value) in [(false, "Input", value.x), (true, "Output", value.y)] {
                views.push(
                    self.param_slider(
                        SliderKey::StyleContour(id, index, point, y),
                        title,
                        format!("{:.0}%", value * 100.),
                        value,
                        (0., 100., 1.),
                        p,
                        cx,
                    )
                    .into_any_element(),
                );
            }
            views.push(
                effect_button(control_id(id, effect, &format!("contour-remove-{point}")))
                    .label("Remove point")
                    .small()
                    .disabled(option.contour.len() <= 2)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_style_option(
                            id,
                            index,
                            |o| {
                                if o.contour.len() > 2 && point < o.contour.len() {
                                    o.contour.remove(point);
                                }
                            },
                            cx,
                        )
                    }))
                    .test_support()
                    .into_any_element(),
            );
        }
        views.push(
            effect_button(control_id(id, effect, "contour-add"))
                .label("Add contour point")
                .small()
                .disabled(option.contour.len() >= 64)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_style_option(
                        id,
                        index,
                        |o| {
                            if o.contour.is_empty() {
                                o.contour = vec![
                                    ContourPoint { x: 0., y: 0. },
                                    ContourPoint { x: 1., y: 1. },
                                ];
                            }
                            if o.contour.len() < 64 {
                                o.contour.push(ContourPoint { x: 0.5, y: 0.5 });
                            }
                        },
                        cx,
                    )
                }))
                .test_support()
                .into_any_element(),
        );
        views
    }
}
