//! Layer styles in the node panel: add, tune, recolour and remove.

use super::*;
use emulsion_core::style_options::StyleOptions;
use emulsion_core::styles::LayerStyle;
use emulsion_raster::composite::{BlendIfChannel, BlendRange, BlendingOptions, Knockout};
use gpui_kit::component::WindowExt;
#[path = "style_controls.rs"]
mod controls;
pub(crate) use controls::effect_options;
#[path = "style_color_dialog.rs"]
mod color_dialog;
#[path = "style_color_picker.rs"]
pub(super) mod color_picker;
#[path = "style_dialog.rs"]
mod dialog;

/// A joined triangle moves a cutoff; Alt/Option chooses one half for a fade.
/// Already separated halves remain independently draggable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BlendIfHandle {
    JoinedBlack,
    JoinedWhite,
    Half(usize),
}

impl BlendIfHandle {
    fn for_half(points: [f32; 4], index: usize, alt: bool) -> Self {
        let pair = index / 2 * 2;
        if alt || points[pair] != points[pair + 1] {
            Self::Half(index)
        } else if pair == 0 {
            Self::JoinedBlack
        } else {
            Self::JoinedWhite
        }
    }

    /// Unlike precise numeric entry, a pointer handle stops at its neighbor;
    /// dragging it across another handle must not move unrelated cutoffs.
    fn moved(self, mut points: [f32; 4], value: f32) -> [f32; 4] {
        if !value.is_finite() {
            return points;
        }
        match self {
            Self::JoinedBlack => {
                let value = value.clamp(0., points[2]);
                points[0] = value;
                points[1] = value;
            }
            Self::JoinedWhite => {
                let value = value.clamp(points[1], 1.);
                points[2] = value;
                points[3] = value;
            }
            Self::Half(index) if index < 4 => {
                let min = if index == 0 { 0. } else { points[index - 1] };
                let max = if index == 3 { 1. } else { points[index + 1] };
                points[index] = value.clamp(min, max);
            }
            Self::Half(_) => {}
        }
        points
    }
}

fn range_points(range: BlendRange) -> [f32; 4] {
    [range.black, range.black_fade, range.white_fade, range.white]
}

#[derive(Default)]
pub(crate) struct StylesUi {
    pub menu_for: Option<NodeId>,
    pub blend_if_open: bool,
    // Preserve the exact imported value at the pointer's starting position.
    // Reconstructing it from an absolute track can cross a half-step boundary
    // through f32 cancellation even on a mouse move with zero displacement.
    blend_if_origin: Option<(SliderKey, f32, Pixels)>,
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
    /// The dialog owns an outer transaction (possibly with a nested slider).
    /// Core history ends all transactions, so allowing document Undo/Redo here
    /// would leave an open dialog whose later Cancel cannot restore its preview.
    pub(super) fn style_dialog_blocks_history(&mut self, cx: &mut Context<Self>) -> bool {
        if self.styles_ui.dialog_for.is_none() {
            return false;
        }
        self.set_status(
            "Finish or cancel Layer Style before using document history.",
            false,
            cx,
        );
        true
    }

    fn style_action_ready(&mut self, cx: &mut Context<Self>) -> bool {
        if self.assistant.running
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some()
        {
            self.set_status(t!("editor.styles_ui.finish_edit"), false, cx);
            return false;
        }
        true
    }

    pub(crate) fn copy_layer_style(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.selected.and_then(|id| self.editor.doc.node(id)) else {
            return;
        };
        cx.set_global(StyleClipboard(StyleBundle::from_node(node)));
        self.set_status(t!("editor.styles_ui.copied"), false, cx);
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
            self.set_status(t!("editor.styles_ui.mask_needs"), false, cx);
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
            && self.tools.mask_edit_target == MaskEditTarget::RasterMask
        {
            self.set_mask_edit_target(MaskEditTarget::Content, cx);
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
        self.set_status(t!("editor.styles_ui.default_saved"), false, cx);
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
        if options != node.blending {
            self.execute(Command::SetBlendingOptions { id, options }, cx);
        }
    }

    pub(crate) fn set_blend_range(
        &mut self,
        id: NodeId,
        backdrop: bool,
        index: usize,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        if index >= 4 || !value.is_finite() {
            return;
        }
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

    #[allow(clippy::too_many_arguments)]
    fn blend_if_handle_down(
        &mut self,
        id: NodeId,
        backdrop: bool,
        index: usize,
        track: &TrackBounds,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.styles_ui.dialog_for != Some(id)
            || self.selected != Some(id)
            || self.drag.is_some()
            || self.assistant.running
            || self.editor.doc.locked_ancestor(id).is_some()
        {
            return;
        }
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let Some(mut bounds) = track.get().filter(|bounds| bounds.size.width > px(0.)) else {
            return;
        };
        let points = range_points(if backdrop {
            node.blending.blend_if.backdrop
        } else {
            node.blending.blend_if.source
        });
        let handle = BlendIfHandle::for_half(points, index, event.modifiers.alt);
        let key = SliderKey::BlendIfHandle(id, backdrop, handle);
        self.styles_ui.blend_if_origin = Some((key, points[index], event.position.x));
        // Preserve the grab offset so clicking either half never jumps a value.
        bounds.origin.x = event.position.x - bounds.size.width * points[index];
        let track = Rc::new(std::cell::Cell::new(Some(bounds)));
        self.close_text_field(cx);
        self.clear_photo_numeric_sequence();
        self.editor.begin("Blend If");
        self.drag = Some(Drag::Slider {
            key,
            track,
            min: 0.,
            max: 255.,
            step: 1.,
            vertical: false,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn apply_blend_if_pointer(
        &mut self,
        key: SliderKey,
        x: Pixels,
        value: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        let SliderKey::BlendIfHandle(id, backdrop, handle) = key else {
            return false;
        };
        let value = self
            .styles_ui
            .blend_if_origin
            .filter(|(origin_key, _, origin_x)| {
                *origin_key == key
                    && matches!(self.drag, Some(Drag::Slider { key: active, .. }) if active == key)
                    && x == *origin_x
            })
            .map_or(value, |(_, origin, _)| origin);
        self.set_blend_if_handle(id, backdrop, handle, value, cx);
        true
    }

    pub(super) fn set_blend_if_handle(
        &mut self,
        id: NodeId,
        backdrop: bool,
        handle: BlendIfHandle,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        if self.styles_ui.dialog_for != Some(id) || self.selected != Some(id) {
            return;
        }
        self.set_blending(
            id,
            |options| {
                let range = if backdrop {
                    &mut options.blend_if.backdrop
                } else {
                    &mut options.blend_if.source
                };
                [range.black, range.black_fade, range.white_fade, range.white] =
                    handle.moved(range_points(*range), value);
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
                .child(label(t!("editor.style_dialog.title"), p))
                .child(mono(t!("editor.styles_ui.one_window"), 10., p.muted))
                .when_some(id, |body, id| {
                    body.child(
                        chip("open-layer-style", t!("editor.styles_ui.open"), false, p)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                // Opening replaces this pointer-focused chip with
                                // live controls. Give the dialog a persistent
                                // Photo return target before it captures focus.
                                if this.is_photo_workflow()
                                    && !window.has_active_dialog(cx)
                                    && !window.has_active_prompt()
                                    && window.focused_input(cx).is_none()
                                {
                                    window.focus(&this.panel_focus, cx);
                                }
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
                    .child(label(t!("editor.style_dialog.blending_options"), p))
                    .child(div().flex_1())
                    .child(
                        chip(
                            "layer-blending-close",
                            t!("editor.styles_ui.close"),
                            false,
                            p,
                        )
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
                .child(label(t!("editor.styles_ui.select_layer"), p))
                .test_support()
                .into_any_element();
        };
        let id = n.id;
        body = body.child(label(n.name.clone(), p));
        if self.editor.doc.locked_ancestor(id).is_some() {
            return body
                .child(label(t!("editor.styles_ui.locked"), p))
                .test_support()
                .into_any_element();
        }
        let blend_open = self.menu == Some(Menu::Blend);
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label(t!("editor.styles_ui.blend_mode"), p))
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
            &t!("editor.styles_ui.opacity"),
            format!("{:.0}%", n.opacity * 100.),
            n.opacity,
            (0., 100., 1.),
            p,
            cx,
        ));
        body = body
            .child(self.param_slider(
                SliderKey::FillOpacity(id),
                &t!("editor.styles_ui.fill"),
                format!("{:.0}%", n.blending.fill_opacity * 100.),
                n.blending.fill_opacity,
                (0., 100., 1.),
                p,
                cx,
            ))
            .child(mono(t!("editor.styles_ui.fill_hint"), 10., p.muted));
        let mut channels = div()
            .flex()
            .items_center()
            .gap_2()
            .child(label(t!("editor.styles_ui.channels"), p));
        for (index, name) in [
            t!("editor.styles_ui.red"),
            t!("editor.styles_ui.green"),
            t!("editor.styles_ui.blue"),
        ]
        .into_iter()
        .enumerate()
        {
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
            .child(label(t!("editor.styles_ui.knockout"), p));
        for (index, (value, name)) in [
            (Knockout::None, t!("editor.styles_ui.knockout_none")),
            (Knockout::Shallow, t!("editor.styles_ui.knockout_shallow")),
            (Knockout::Deep, t!("editor.styles_ui.knockout_deep")),
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
                t!("editor.styles_ui.interior_as_group"),
                n.blending.blend_interior_effects_as_group,
            ),
            (
                t!("editor.styles_ui.clipped_as_group"),
                n.blending.blend_clipped_layers_as_group,
            ),
            (
                t!("editor.styles_ui.transparency_shapes"),
                n.blending.transparency_shapes_layer,
            ),
            (
                t!("editor.styles_ui.mask_hides_effects"),
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
                t!("editor.styles_ui.blend_if"),
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
                (BlendIfChannel::Gray, t!("editor.styles_ui.gray")),
                (BlendIfChannel::Red, t!("editor.styles_ui.red")),
                (BlendIfChannel::Green, t!("editor.styles_ui.green")),
                (BlendIfChannel::Blue, t!("editor.styles_ui.blue")),
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
                (
                    false,
                    t!("editor.styles_ui.this_layer"),
                    n.blending.blend_if.source,
                ),
                (
                    true,
                    t!("editor.styles_ui.underlying"),
                    n.blending.blend_if.backdrop,
                ),
            ] {
                body = body.child(label(title, p));
                let points = range_points(range);
                let track = TrackBounds::default();
                let measure = track.clone();
                let mut gradient = div()
                    .id(("blend-if-gradient", usize::from(backdrop)))
                    .relative()
                    .w_full()
                    .h(px(32.))
                    .child(
                        canvas(
                            move |bounds, _, _| measure.set(Some(bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top_0()
                            .h(px(18.))
                            .border_1()
                            .border_color(p.line)
                            .bg(linear_gradient(
                                90.,
                                linear_color_stop(gpui_kit::black(), 0.),
                                linear_color_stop(gpui_kit::white(), 1.),
                            )),
                    )
                    .test_support();
                for (index, value) in points.into_iter().enumerate() {
                    let left = index % 2 == 0;
                    let color = if index < 2 {
                        gpui_kit::black()
                    } else {
                        gpui_kit::white()
                    };
                    let track = track.clone();
                    let outline = p.muted;
                    gradient = gradient.child(
                        div()
                            .id(format!("blend-if-handle-{backdrop}-{index}"))
                            .absolute()
                            .left(relative(value))
                            .ml(if left { px(-8.) } else { px(0.) })
                            .top(px(18.))
                            .w(px(8.))
                            .h(px(14.))
                            .cursor(CursorStyle::ResizeLeftRight)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event, window, cx| {
                                    this.blend_if_handle_down(
                                        id, backdrop, index, &track, event, window, cx,
                                    )
                                }),
                            )
                            .child(
                                canvas(
                                    |bounds, _, _| bounds,
                                    move |bounds, _, window, _| {
                                        let x = if left { bounds.right() } else { bounds.left() };
                                        let outer =
                                            if left { bounds.left() } else { bounds.right() };
                                        let mut path = PathBuilder::fill();
                                        path.move_to(point(x, bounds.top()));
                                        path.line_to(point(outer, bounds.bottom()));
                                        path.line_to(point(x, bounds.bottom()));
                                        path.close();
                                        if let Ok(path) = path.build() {
                                            window.paint_path(path, color);
                                        }
                                        let mut edge = PathBuilder::stroke(px(1.));
                                        edge.move_to(point(x, bounds.top()));
                                        edge.line_to(point(outer, bounds.bottom()));
                                        edge.line_to(point(x, bounds.bottom()));
                                        edge.close();
                                        if let Ok(path) = edge.build() {
                                            window.paint_path(path, outline);
                                        }
                                    },
                                )
                                .size_full(),
                            )
                            .test_support(),
                    );
                }
                body = body.child(div().px(px(8.)).child(gradient)).child(mono(
                    "Drag a triangle; Alt/Option-drag a half to split the fade.",
                    10.,
                    p.muted,
                ));
                for (index, (name, value)) in [
                    (t!("editor.styles_ui.black_cutoff"), range.black),
                    (t!("editor.styles_ui.black_fade_end"), range.black_fade),
                    (t!("editor.styles_ui.white_fade_start"), range.white_fade),
                    (t!("editor.styles_ui.white_cutoff"), range.white),
                ]
                .into_iter()
                .enumerate()
                {
                    body = body.child(self.param_slider(
                        SliderKey::BlendRange(id, backdrop, index),
                        &name,
                        format!("{:.0}", value * 255.),
                        value,
                        (0., 255., 1.),
                        p,
                        cx,
                    ));
                }
            }
            body = body.child(
                chip(
                    "blend-if-reset",
                    t!("editor.styles_ui.reset_blend_if"),
                    false,
                    p,
                )
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
        body.child(mono(t!("editor.styles_ui.preview_hint"), 10., p.muted))
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
                .child(label(t!("editor.styles_ui.styles"), p))
                .child(div().flex_1())
                .when(!show_catalogue, |row| {
                    row.child(
                        chip(
                            "style-add",
                            t!("editor.styles_ui.add_style"),
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
            v.push(mono(t!("editor.styles_ui.empty"), 10., p.muted).into_any_element());
        }

        v
    }
}

#[cfg(test)]
#[path = "blend_if_handle_tests.rs"]
mod blend_if_handle_tests;
