//! Effects remain attached to their layer in the Layers list.
use super::styles_ui::effect_options as options_for;
use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(crate) fn toggle_layer_effect(
        &mut self,
        id: NodeId,
        effect_id: u64,
        cx: &mut Context<Self>,
    ) {
        if self.assistant.running
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some()
        {
            return;
        }
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let styles = node.styles.clone();
        let mut options = options_for(node);
        let Some(option) = options.iter_mut().find(|o| o.id == effect_id) else {
            return;
        };
        option.enabled = !option.enabled;
        self.execute(
            Command::SetLayerEffects {
                id,
                styles,
                options,
            },
            cx,
        );
    }

    pub(super) fn layer_effect_rows(
        &self,
        id: NodeId,
        depth: usize,
        p: &Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let node = self.editor.doc.node(id)?;
        if node.styles.is_empty() {
            return None;
        }
        let collapsed = self.layer_panel.effects_collapsed.contains(&id);
        let enabled = node.effects_enabled;
        let busy = self.assistant.running
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some();
        let disabled = busy || self.editor.doc.locked_ancestor(id).is_some();
        let mut rows = div()
            .id(("layer-effects", id))
            .flex()
            .flex_col()
            .flex_none()
            .pl(rems((depth as f32 + 1.) * 0.875))
            .text_xs()
            .text_color(p.muted);
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    Button::new(("effects-visible", id))
                        .icon(if enabled {
                            IconName::Eye
                        } else {
                            IconName::EyeOff
                        })
                        .xsmall()
                        .ghost()
                        .disabled(disabled)
                        .tooltip(if enabled {
                            "Hide layer effects"
                        } else {
                            "Show layer effects"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.panel_focus, cx);
                            this.execute(
                                Command::SetEffectsEnabled {
                                    id,
                                    enabled: !enabled,
                                },
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new(("effects-expand", id))
                        .icon(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .label("Effects")
                        .xsmall()
                        .ghost()
                        .tooltip(if collapsed {
                            "Expand layer effects"
                        } else {
                            "Collapse layer effects"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.panel_focus, cx);
                            if !this.layer_panel.effects_collapsed.remove(&id) {
                                this.layer_panel.effects_collapsed.insert(id);
                            }
                            cx.notify();
                        })),
                ),
        );
        if !collapsed {
            let options = options_for(node);
            for (style, option) in node.styles.iter().zip(options) {
                let effect_id = option.id;
                let label = style.label().to_string();
                let effect_enabled = option.enabled;
                let identity = format!("{id}-{effect_id}");
                rows = rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl_4()
                        .child(
                            Button::new(SharedString::from(format!("effect-visible-{identity}")))
                                .icon(if effect_enabled {
                                    IconName::Eye
                                } else {
                                    IconName::EyeOff
                                })
                                .xsmall()
                                .ghost()
                                .disabled(disabled)
                                .tooltip(format!(
                                    "{} {label}",
                                    if effect_enabled { "Hide" } else { "Show" }
                                ))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    window.focus(&this.panel_focus, cx);
                                    this.toggle_layer_effect(id, effect_id, cx);
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("effect-name-{identity}")))
                                .label(label)
                                .xsmall()
                                .ghost()
                                .disabled(busy)
                                .text_color(if enabled && effect_enabled {
                                    p.ink
                                } else {
                                    p.muted
                                })
                                .tooltip("Double-click to edit this effect")
                                .on_click(cx.listener(
                                    move |this, event: &ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        if event.click_count() >= 2 || event.click_count() == 0 {
                                            this.open_layer_effect(id, effect_id, window, cx);
                                        } else {
                                            this.select_layer_context(id, cx);
                                            window.focus(&this.panel_focus, cx);
                                        }
                                    },
                                )),
                        ),
                );
            }
        }
        Some(rows.into_any_element())
    }
}
