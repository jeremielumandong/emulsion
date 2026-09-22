//! Layer styles in the node panel: add, tune, recolour and remove.

use super::*;
use emulsion_core::style_options::StyleOptions;
use emulsion_core::styles::LayerStyle;
use emulsion_raster::composite::{BlendIfChannel, BlendingOptions, Knockout};
use gpui_kit::component::WindowExt;
#[path = "style_controls.rs"]
mod controls;
pub(crate) use controls::effect_options;
#[path = "style_color_dialog.rs"]
mod color_dialog;
#[path = "style_color_picker.rs"]
mod color_picker;
#[path = "style_dialog.rs"]
mod dialog;

#[derive(Default)]
pub(crate) struct StylesUi {
    pub menu_for: Option<NodeId>,
    pub blend_if_open: bool,
    pub expanded: Option<(NodeId, usize)>,
    pub advanced: Option<(NodeId, usize)>,
    pub colors: std::collections::HashMap<(NodeId, u64, usize), controls::ColorDraft>,
    pub dialog_for: Option<NodeId>,
    pub color_dialog_for: Option<(NodeId, u64, usize)>,
}

#[derive(Clone)]
struct StyleBundle {
    effects: Vec<LayerStyle>,
    options: Vec<StyleOptions>,
    effects_enabled: bool,
    blend: BlendMode,
    opacity: f32,
    blending: BlendingOptions,
}
impl StyleBundle {
    fn from_node(node: &Node) -> Self {
        Self {
            effects: node.styles.clone(),
            options: node.style_options.clone(),
            effects_enabled: node.effects_enabled,
            blend: node.blend,
            opacity: node.opacity,
            blending: node.blending,
        }
    }
    fn commands(&self, id: NodeId) -> Vec<Command> {
        vec![
            Command::SetEffectsEnabled {
                id,
                enabled: self.effects_enabled,
            },
            Command::SetLayerEffects {
                id,
                styles: self.effects.clone(),
                options: self.options.clone(),
            },
            Command::SetBlend {
                id,
                blend: self.blend,
            },
            Command::SetOpacity {
                id,
                opacity: self.opacity,
            },
            Command::SetBlendingOptions {
                id,
                options: self.blending,
            },
        ]
    }
}
#[derive(Clone)]
struct StyleClipboard(StyleBundle);
impl Global for StyleClipboard {}

impl EditorView {
    fn style_action_ready(&mut self, cx: &mut Context<Self>) -> bool {
        if self.assistant.running
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some()
        {
            self.set_status(
                "Finish the current edit before changing layer styles or applying a mask.",
                false,
                cx,
            );
            return false;
        }
        true
    }

    pub(crate) fn copy_layer_style(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.selected.and_then(|id| self.editor.doc.node(id)) else {
            return;
        };
        cx.set_global(StyleClipboard(StyleBundle::from_node(node)));
        self.set_status("Layer style and blending copied.", false, cx);
    }

    pub(crate) fn can_paste_layer_style(&self, cx: &App) -> bool {
        cx.try_global::<StyleClipboard>().is_some() && !self.selected_layer_ids().is_empty()
    }

    pub(crate) fn paste_layer_style(&mut self, cx: &mut Context<Self>) {
        if !self.style_action_ready(cx) {
            return;
        }
        let Some(StyleClipboard(styles)) = cx.try_global::<StyleClipboard>().cloned() else {
            return;
        };
        self.close_text_field(cx);
        let commands = self
            .selected_layer_ids()
            .into_iter()
            .flat_map(|id| styles.commands(id))
            .collect();
        self.execute_layer_commands("Paste layer style", commands, cx);
    }

    pub(crate) fn transfer_layer_style(
        &mut self,
        source: NodeId,
        target: NodeId,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        if source == target || !self.style_action_ready(cx) {
            return;
        }
        let Some(styles) = self.editor.doc.node(source).map(StyleBundle::from_node) else {
            return;
        };
        let mut commands = styles.commands(target);
        if !copy {
            commands.extend(
                StyleBundle {
                    effects: Vec::new(),
                    options: Vec::new(),
                    effects_enabled: true,
                    blend: BlendMode::Normal,
                    opacity: 1.0,
                    blending: BlendingOptions::default(),
                }
                .commands(source),
            );
        }
        self.close_text_field(cx);
        self.execute_layer_commands(
            if copy {
                "Copy layer style"
            } else {
                "Move layer style"
            },
            commands,
            cx,
        );
    }

    pub(crate) fn can_apply_layer_mask(&self) -> bool {
        let ids = self.selected_layer_ids();
        !ids.is_empty()
            && ids.into_iter().all(|id| {
                self.editor.doc.node(id).is_some_and(|node| {
                    let locks = self.editor.doc.layer_locks(id);
                    node.mask.is_some()
                        && self.editor.doc.locked_ancestor(id).is_none()
                        && !locks.pixels
                        && !locks.transparency
                        && matches!(
                            node.kind,
                            NodeKind::Raster { .. }
                                | NodeKind::Smart { .. }
                                | NodeKind::Text { .. }
                                | NodeKind::Path { .. }
                                | NodeKind::Fill { .. }
                        )
                })
            })
    }

    pub(crate) fn apply_layer_mask(&mut self, cx: &mut Context<Self>) {
        if !self.style_action_ready(cx) {
            return;
        }
        if !self.can_apply_layer_mask() {
            self.set_status("Apply Layer Mask needs unlocked pixel, text, path, fill, or Smart content. Group and adjustment masks remain editable.", false, cx);
            return;
        }
        self.close_text_field(cx);
        let commands = self
            .selected_layer_ids()
            .into_iter()
            .filter(|id| {
                self.editor
                    .doc
                    .node(*id)
                    .is_some_and(|node| node.mask.is_some())
            })
            .map(|id| Command::ApplyLayerMask { id })
            .collect();
        if self
            .execute_layer_commands("Apply layer mask", commands, cx)
            .is_some()
        {
            self.tools.mask_edit = false;
        }
    }

    pub(crate) fn save_style_default(&mut self, id: NodeId, index: usize, cx: &mut Context<Self>) {
        let Some(style) = self
            .editor
            .doc
            .node(id)
            .and_then(|node| node.styles.get(index))
            .cloned()
        else {
            return;
        };
        crate::app_state::update_settings(cx, |settings| {
            settings
                .layer_style_defaults
                .retain(|saved| saved.key() != style.key());
            settings.layer_style_defaults.push(style);
        });
        if let Some(node) = self.editor.doc.node(id) {
            let options = controls::effect_options(node)[index].clone();
            let key = node.styles[index].key().to_string();
            crate::app_state::update_settings(cx, |settings| {
                settings.layer_style_option_defaults.insert(key, options);
            });
        }
        self.set_status("Effect saved as its default.", false, cx);
    }

    pub(crate) fn reset_style_default(&mut self, id: NodeId, index: usize, cx: &mut Context<Self>) {
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let Some(style) = node.styles.get(index) else {
            return;
        };
        let key = style.key();
        let Some(factory) = LayerStyle::catalogue()
            .into_iter()
            .find(|candidate| candidate.key() == key)
        else {
            return;
        };
        let mut styles = node.styles.clone();
        styles[index] = factory;
        let mut options = controls::effect_options(node);
        options[index] = StyleOptions::for_style(&styles[index]);
        self.execute(
            Command::SetLayerEffects {
                id,
                styles,
                options,
            },
            cx,
        );
        crate::app_state::update_settings(cx, |settings| {
            settings
                .layer_style_defaults
                .retain(|saved| saved.key() != key)
        });
        crate::app_state::update_settings(cx, |settings| {
            settings.layer_style_option_defaults.remove(key);
        });
    }

    pub(crate) fn set_blending(
        &mut self,
        id: NodeId,
        change: impl FnOnce(&mut BlendingOptions),
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let mut options = node.blending;
        change(&mut options);
        self.execute(Command::SetBlendingOptions { id, options }, cx);
    }

    pub(crate) fn set_blend_range(
        &mut self,
        id: NodeId,
        backdrop: bool,
        index: usize,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        self.set_blending(
            id,
            |options| {
                let range = if backdrop {
                    &mut options.blend_if.backdrop
                } else {
                    &mut options.blend_if.source
                };
                let mut points = [range.black, range.black_fade, range.white_fade, range.white];
                points[index] = value.clamp(0., 1.);
                for i in (0..index).rev() {
                    points[i] = points[i].min(points[i + 1]);
                }
                for i in index + 1..4 {
                    points[i] = points[i].max(points[i - 1]);
                }
                [range.black, range.black_fade, range.white_fade, range.white] = points;
            },
            cx,
        );
    }

    pub(crate) fn open_blending_options(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.doc.node(id).is_none() {
            return;
        }
        self.set_layer_selection(vec![id], Some(id));
        self.styles_ui.menu_for = None;
        self.styles_ui.expanded = None;
        self.open_layer_styles_dialog(id, window, cx);
        self.select_sidebar(SidebarTab::BlendingOptions, cx);
    }

    pub(super) fn blending_options_panel(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.styles_ui.dialog_for.is_none() {
            let id = self.selected;
            return div()
                .id("layer-blending-panel")
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(label("Layer Style", p))
                .child(mono(
                    "Edit blending and effects in one window.",
                    10.,
                    p.muted,
                ))
                .when_some(id, |body, id| {
                    body.child(
                        chip("open-layer-style", "Open Layer Style…", false, p)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_blending_options(id, window, cx)
                            }))
                            .test_support(),
                    )
                })
                .test_support()
                .into_any_element();
        }
        let mut body = div()
            .id("layer-blending-panel")
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label("Blending Options", p))
                    .child(div().flex_1())
                    .child(
                        chip("layer-blending-close", "Close", false, p)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.styles_ui.dialog_for.is_some() {
                                    this.close_style_dialog(true, cx);
                                    window.close_dialog(cx);
                                    return;
                                }
                                this.select_sidebar(SidebarTab::History, cx);
                                window.focus(&this.panel_focus, cx);
                            }))
                            .test_support(),
                    ),
            );
        let Some(n) = self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .cloned()
        else {
            return body
                .child(label("Select a layer to edit its blending options.", p))
                .test_support()
                .into_any_element();
        };
        let id = n.id;
        body = body.child(label(n.name.clone(), p));
        if self.editor.doc.locked_ancestor(id).is_some() {
            return body
                .child(label(
                    "This layer or its group is locked. Unlock it to edit blending options.",
                    p,
                ))
                .test_support()
                .into_any_element();
        }
        let blend_open = self.menu == Some(Menu::Blend);
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Blend mode", p))
                .child(
                    chip("layer-blend", n.blend.label(), blend_open, p)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.menu = if this.menu == Some(Menu::Blend) {
                                None
                            } else {
                                Some(Menu::Blend)
                            };
                            cx.notify();
                        }))
                        .test_support(),
                ),
        );
        if blend_open {
            let modes = n
                .is_group()
                .then_some(BlendMode::PassThrough)
                .into_iter()
                .chain(BlendMode::MENU.iter().flatten().copied())
                .map(|m| (SharedString::from(m.label()), MenuAction::Blend(id, m)))
                .collect();
            body = body.child(self.menu_list("blending-mode-menu", modes, p, cx));
        }
        body = body.child(self.param_slider(
            SliderKey::Opacity(id),
            "Opacity",
            format!("{:.0}%", n.opacity * 100.),
            n.opacity,
            (0., 100., 1.),
            p,
            cx,
        ));
        body = body
            .child(self.param_slider(
                SliderKey::FillOpacity(id),
                "Fill",
                format!("{:.0}%", n.blending.fill_opacity * 100.),
                n.blending.fill_opacity,
                (0., 100., 1.),
                p,
                cx,
            ))
            .child(mono(
                "Fill changes layer content while preserving its effects.",
                10.,
                p.muted,
            ));
        let mut channels = div()
            .flex()
            .items_center()
            .gap_2()
            .child(label("Channels", p));
        for (index, name) in ["Red", "Green", "Blue"].into_iter().enumerate() {
            channels = channels.child(
                chip(
                    ("blend-channel", index),
                    name,
                    n.blending.channels[index],
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_blending(
                        id,
                        |options| options.channels[index] = !options.channels[index],
                        cx,
                    )
                }))
                .test_support(),
            );
        }
        body = body.child(channels);
        let mut knockout = div()
            .flex()
            .items_center()
            .gap_2()
            .child(label("Knockout", p));
        for (index, (value, name)) in [
            (Knockout::None, "None"),
            (Knockout::Shallow, "Shallow"),
            (Knockout::Deep, "Deep"),
        ]
        .into_iter()
        .enumerate()
        {
            knockout = knockout.child(
                chip(
                    ("blend-knockout", index),
                    name,
                    n.blending.knockout == value,
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_blending(id, |options| options.knockout = value, cx)
                }))
                .test_support(),
            );
        }
        body = body.child(knockout);
        for (index, (name, enabled)) in [
            (
                "Blend interior effects as group",
                n.blending.blend_interior_effects_as_group,
            ),
            (
                "Blend clipped layers as group",
                n.blending.blend_clipped_layers_as_group,
            ),
            (
                "Transparency shapes layer",
                n.blending.transparency_shapes_layer,
            ),
            (
                "Layer mask hides effects",
                n.blending.layer_mask_hides_effects,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            body = body.child(
                chip(("advanced-blend-toggle", index), name, enabled, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_blending(
                            id,
                            |options| match index {
                                0 => {
                                    options.blend_interior_effects_as_group =
                                        !options.blend_interior_effects_as_group
                                }
                                1 => {
                                    options.blend_clipped_layers_as_group =
                                        !options.blend_clipped_layers_as_group
                                }
                                2 => {
                                    options.transparency_shapes_layer =
                                        !options.transparency_shapes_layer
                                }
                                _ => {
                                    options.layer_mask_hides_effects =
                                        !options.layer_mask_hides_effects
                                }
                            },
                            cx,
                        )
                    }))
                    .test_support(),
            );
        }
        body = body.child(
            chip(
                "blend-if-toggle",
                "Blend If",
                self.styles_ui.blend_if_open,
                p,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.styles_ui.blend_if_open = !this.styles_ui.blend_if_open;
                cx.notify();
            }))
            .test_support(),
        );
        if self.styles_ui.blend_if_open {
            let mut channels = div().flex().flex_wrap().gap_1();
            for (index, (channel, name)) in [
                (BlendIfChannel::Gray, "Gray"),
                (BlendIfChannel::Red, "Red"),
                (BlendIfChannel::Green, "Green"),
                (BlendIfChannel::Blue, "Blue"),
            ]
            .into_iter()
            .enumerate()
            {
                channels = channels.child(
                    chip(
                        ("blend-if-channel", index),
                        name,
                        n.blending.blend_if.channel == channel,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_blending(id, |options| options.blend_if.channel = channel, cx)
                    }))
                    .test_support(),
                );
            }
            body = body.child(channels);
            for (backdrop, title, range) in [
                (false, "This layer", n.blending.blend_if.source),
                (true, "Underlying layers", n.blending.blend_if.backdrop),
            ] {
                body = body.child(label(title, p));
                let points = [range.black, range.black_fade, range.white_fade, range.white];
                let mut gradient = div()
                    .id(("blend-if-gradient", usize::from(backdrop)))
                    .relative()
                    .w_full()
                    .h(px(30.))
                    .border_1()
                    .border_color(p.line)
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(gpui_kit::black(), 0.),
                        linear_color_stop(gpui_kit::white(), 1.),
                    ))
                    .test_support();
                for (index, point) in points.into_iter().enumerate() {
                    let upper = index == 1 || index == 2;
                    gradient = gradient.child(
                        div()
                            .id(format!("blend-if-handle-{backdrop}-{index}"))
                            .absolute()
                            .left(relative(point))
                            .ml(px(-4.))
                            .top(if upper { px(2.) } else { px(17.) })
                            .w(px(8.))
                            .h(px(11.))
                            .border_1()
                            .border_color(if upper { p.accent } else { p.ink })
                            .bg(p.panel)
                            .test_support(),
                    );
                }
                body = body.child(gradient).child(mono(
                    "Outer handles set cutoffs; inner handles set the split fade.",
                    10.,
                    p.muted,
                ));
                for (index, (name, value)) in [
                    ("Black cutoff", range.black),
                    ("Black fade end", range.black_fade),
                    ("White fade start", range.white_fade),
                    ("White cutoff", range.white),
                ]
                .into_iter()
                .enumerate()
                {
                    body = body.child(self.param_slider(
                        SliderKey::BlendRange(id, backdrop, index),
                        name,
                        format!("{:.0}", value * 255.),
                        value,
                        (0., 255., 1.),
                        p,
                        cx,
                    ));
                }
            }
            body = body.child(
                chip("blend-if-reset", "Reset Blend If", false, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_blending(id, |options| options.blend_if = Default::default(), cx)
                    }))
                    .test_support(),
            );
        }
        if self.styles_ui.dialog_for.is_none()
            && matches!(
                n.kind,
                NodeKind::Raster { .. }
                    | NodeKind::Smart { .. }
                    | NodeKind::Path { .. }
                    | NodeKind::Text { .. }
                    | NodeKind::Fill { .. }
                    | NodeKind::Group { .. }
            )
        {
            body = body.children(self.styles_panel_with_catalogue(id, &n.styles, true, p, cx));
        }
        body.child(mono(
            "Preview updates immediately. OK keeps changes; Cancel restores them.",
            10.,
            p.muted,
        ))
        .test_support()
        .into_any_element()
    }

    fn set_styles(&mut self, id: NodeId, styles: Vec<LayerStyle>, cx: &mut Context<Self>) {
        self.execute(Command::SetStyles { id, styles }, cx);
    }

    pub fn add_style(&mut self, id: NodeId, mut s: LayerStyle, cx: &mut Context<Self>) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let saved = crate::app_state::settings(cx)
            .layer_style_defaults
            .iter()
            .find(|saved| saved.key() == s.key())
            .cloned();
        if let Some(saved) = &saved {
            s = saved.clone();
        }
        // New effects take the foreground colour only without a custom default.
        if saved.is_none()
            && !matches!(
                s,
                LayerStyle::DropShadow { .. }
                    | LayerStyle::InnerShadow { .. }
                    | LayerStyle::BevelEmboss { .. }
                    | LayerStyle::Satin { .. }
            )
        {
            let fg = self.tools.fg;
            s.set_color([fg[0], fg[1], fg[2]], false);
        }
        let mut styles = n.styles.clone();
        let mut options = controls::effect_options(n);
        let mut option = crate::app_state::settings(cx)
            .layer_style_option_defaults
            .get(s.key())
            .cloned()
            .unwrap_or_else(|| StyleOptions::for_style(&s));
        option.id = (1..)
            .find(|id| options.iter().all(|o| o.id != *id))
            .expect("effect ID");
        options.push(option);
        styles.push(s);
        self.styles_ui.expanded = Some((id, styles.len() - 1));
        self.execute(
            Command::SetLayerEffects {
                id,
                styles,
                options,
            },
            cx,
        );
        self.styles_ui.menu_for = None;
    }

    pub(crate) fn set_style_param(
        &mut self,
        id: NodeId,
        idx: usize,
        key: &'static str,
        v: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let mut styles = n.styles.clone();
        if let Some(s) = styles.get_mut(idx)
            && s.set_param(key, v)
        {
            self.set_styles(id, styles, cx);
        }
    }

    pub(crate) fn styles_panel(
        &mut self,
        id: NodeId,
        styles: &[LayerStyle],
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.styles_panel_with_catalogue(id, styles, false, p, cx)
    }

    fn styles_panel_with_catalogue(
        &mut self,
        id: NodeId,
        styles: &[LayerStyle],
        show_catalogue: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        v.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pt(px(6.))
                .child(label("Styles", p))
                .child(div().flex_1())
                .when(!show_catalogue, |row| {
                    row.child(
                        chip(
                            "style-add",
                            "+ style",
                            self.styles_ui.menu_for == Some(id),
                            p,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.styles_ui.menu_for = if this.styles_ui.menu_for == Some(id) {
                                None
                            } else {
                                Some(id)
                            };
                            cx.notify();
                        }))
                        .test_support(),
                    )
                })
                .into_any_element(),
        );
        if show_catalogue || self.styles_ui.menu_for == Some(id) {
            let mut menu = div().flex().flex_wrap().gap(px(4.));
            for (i, s) in LayerStyle::catalogue().into_iter().enumerate() {
                let text = s.label();
                menu = menu.child(
                    chip(("style-kind", i), text, false, p)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.add_style(id, s.clone(), cx)),
                        )
                        .test_support(),
                );
            }
            v.push(menu.into_any_element());
        }
        let options = self
            .editor
            .doc
            .node(id)
            .map(controls::effect_options)
            .unwrap_or_default();
        for (index, style) in styles.iter().enumerate() {
            let option = options
                .get(index)
                .cloned()
                .unwrap_or_else(|| StyleOptions::for_style(style));
            v.extend(self.effect_controls((id, index), style, &option, styles.len(), p, cx));
        }
        if styles.is_empty() {
            v.push(
                mono(
                    "Add an effect, then expand it to edit its settings.",
                    10.,
                    p.muted,
                )
                .into_any_element(),
            );
        }

        v
    }
}
