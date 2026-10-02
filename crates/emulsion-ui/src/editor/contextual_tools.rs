//! A short list of next actions for the active canvas tool.
use super::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::{Disableable, Selectable, Sizable};

impl EditorView {
    pub(super) fn contextual_tool_actions(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut actions = Vec::new();
        if self.is_design() && self.tool == Tool::Move && self.warp.is_none() {
            return self.design_selection_actions(cx);
        }
        let ready = self.layer_menu_ready();
        let can_remove = ready
            && !self.generate.busy
            && self.pending_edit_job.is_none()
            && !self.tools.remove.running;
        let editable = self
            .selected
            .is_some_and(|id| self.editor.doc.locked_ancestor(id).is_none());
        let can_add_mask = ready
            && editable
            && self
                .selected
                .and_then(|id| self.editor.doc.node(id))
                .is_some_and(|node| node.mask.is_none());
        // Keep focus on the canvas so clicking Done does not first commit text
        // through blur, and keyboard shortcuts keep working after an action.
        let mut add = |id: &'static str,
                       label: std::borrow::Cow<'static, str>,
                       enabled: bool,
                       action: fn(&mut Self, &mut Context<Self>)| {
            actions.push(
                Button::new(id)
                    .small()
                    .label(label)
                    .selected(match id {
                        "context-shape-rectangle" => self.tools.shape == ShapeKind::Rect,
                        "context-shape-ellipse" => self.tools.shape == ShapeKind::Ellipse,
                        "context-gradient-linear" => !self.tools.radial,
                        "context-gradient-radial" => self.tools.radial,
                        "context-fill-contiguous" => self.tools.contiguous,
                        "context-heal-mode" => !self.tools.remove.enabled,
                        "context-remove-mode" => self.tools.remove.enabled,
                        "context-remove-after-stroke" => self.tools.remove.after_stroke,
                        _ => false,
                    })
                    .disabled(!enabled)
                    .capture_any_mouse_down(|event, window, _| {
                        if event.button == MouseButton::Left {
                            window.prevent_default();
                        }
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        action(this, cx);
                        window.focus(&this.canvas_focus, cx);
                    }))
                    .into_any_element(),
            );
        };
        if self.tools.quick_mask {
            add(
                "context-quick-mask-done",
                t!("editor.contextual_tools.quick_mask_done"),
                true,
                Self::toggle_quick_mask,
            );
            add(
                "context-invert-selection",
                t!("editor.contextual_tools.invert_selection"),
                true,
                Self::invert_selection,
            );
            add(
                "context-swap-colors",
                t!("editor.contextual_tools.swap_colors"),
                true,
                Self::swap_colors,
            );
            return actions;
        }
        let has_selection = self.editor.doc.selection.is_some();
        match self.tool {
            Tool::Hand | Tool::Zoom => {
                if self.tool == Tool::Zoom {
                    add(
                        "context-zoom-in",
                        t!("editor.contextual_tools.zoom_in"),
                        true,
                        |this, cx| this.zoom_step(true, cx),
                    );
                    add(
                        "context-zoom-out",
                        t!("editor.contextual_tools.zoom_out"),
                        true,
                        |this, cx| this.zoom_step(false, cx),
                    );
                }
                add(
                    "context-fit",
                    t!("editor.contextual_tools.fit"),
                    true,
                    Self::zoom_fit,
                );
                add("context-actual-size", "100%".into(), true, Self::zoom_100);
                if self.tool == Tool::Hand {
                    add(
                        "context-reset-view",
                        t!("editor.contextual_tools.reset_view"),
                        true,
                        |this, cx| this.rotate(0.0, cx),
                    );
                }
            }
            Tool::Move => {
                add(
                    "context-match-subject",
                    t!("editor.contextual_tools.match_subject"),
                    self.can_match_subject(),
                    Self::match_subject_stack,
                );
                if self.warp.is_some() {
                    add(
                        "context-warp-apply",
                        t!("editor.contextual_tools.warp_apply"),
                        true,
                        Self::finish_warp,
                    );
                    add(
                        "context-warp-cancel",
                        t!("editor.contextual_tools.warp_cancel"),
                        true,
                        Self::cancel_warp,
                    );
                } else {
                    add(
                        "context-transform",
                        t!("editor.contextual_tools.transform"),
                        ready && editable,
                        |this, cx| this.begin_transform_action("scale", cx),
                    );
                    add(
                        "context-subject",
                        t!("editor.contextual_tools.subject"),
                        true,
                        Self::select_subject,
                    );
                    add(
                        "context-remove-background",
                        t!("editor.contextual_tools.remove_background"),
                        true,
                        Self::remove_background,
                    );
                }
            }
            Tool::Select => {
                add(
                    "context-remove-selection",
                    t!("editor.contextual_tools.remove_selection"),
                    can_remove && has_selection,
                    Self::content_aware_fill,
                );
                add(
                    "context-subject",
                    t!("editor.contextual_tools.subject"),
                    true,
                    Self::select_subject,
                );
                add(
                    "context-invert-selection",
                    t!("editor.contextual_tools.invert_selection"),
                    has_selection,
                    Self::invert_selection,
                );
                add(
                    "context-selection-mask",
                    t!("editor.contextual_tools.selection_mask"),
                    has_selection && can_add_mask,
                    |this, cx| this.add_mask_inverted(false, cx),
                );
                add(
                    "context-deselect",
                    t!("editor.contextual_tools.deselect"),
                    has_selection,
                    Self::deselect,
                );
            }
            Tool::Mask => {
                add(
                    "context-add-mask",
                    t!("editor.contextual_tools.add_mask"),
                    can_add_mask,
                    |this, cx| this.add_mask_inverted(false, cx),
                );
            }
            Tool::Brush | Tool::Heal | Tool::Clone => {
                if self.tool == Tool::Heal {
                    add(
                        "context-heal-mode",
                        t!("editor.contextual_tools.heal_mode"),
                        ready,
                        |this, cx| this.set_remove_mode(false, cx),
                    );
                    add(
                        "context-remove-mode",
                        t!("editor.contextual_tools.remove_mode"),
                        ready,
                        |this, cx| this.set_remove_mode(true, cx),
                    );
                    if self.tools.remove.enabled {
                        add(
                            "context-remove-after-stroke",
                            t!("editor.contextual_tools.remove_after_stroke"),
                            ready,
                            |this, cx| {
                                this.tools.remove.after_stroke = !this.tools.remove.after_stroke;
                                cx.notify();
                            },
                        );
                        add(
                            "context-remove-apply",
                            t!("editor.contextual_tools.remove_apply"),
                            can_remove && self.remove_pending(),
                            Self::apply_remove,
                        );
                        add(
                            "context-remove-cancel",
                            t!("shell.cancel"),
                            self.remove_pending(),
                            |this, cx| {
                                this.cancel_remove(cx);
                            },
                        );
                    }
                }
                if self.brushy() || self.tools.paint == PaintKind::Liquify {
                    add(
                        "context-smaller-brush",
                        t!("editor.contextual_tools.smaller_brush"),
                        true,
                        |this, cx| this.brush_size(false, cx),
                    );
                    add(
                        "context-larger-brush",
                        t!("editor.contextual_tools.larger_brush"),
                        true,
                        |this, cx| this.brush_size(true, cx),
                    );
                }
                if self.brushy() {
                    add(
                        "context-brush-settings",
                        t!("editor.contextual_tools.brush_settings"),
                        true,
                        |this, cx| this.select_sidebar(SidebarTab::BrushSettings, cx),
                    );
                }
                if self.tool == Tool::Brush {
                    if self.tools.paint == PaintKind::Gradient {
                        add(
                            "context-gradient-linear",
                            t!("editor.contextual_tools.gradient_linear"),
                            true,
                            |this, cx| {
                                this.tools.radial = false;
                                cx.notify();
                            },
                        );
                        add(
                            "context-gradient-radial",
                            t!("editor.contextual_tools.gradient_radial"),
                            true,
                            |this, cx| {
                                this.tools.radial = true;
                                cx.notify();
                            },
                        );
                    } else if self.tools.paint == PaintKind::Bucket {
                        add(
                            "context-fill-contiguous",
                            t!("editor.contextual_tools.fill_contiguous"),
                            true,
                            |this, cx| {
                                this.tools.contiguous = !this.tools.contiguous;
                                cx.notify();
                            },
                        );
                    }
                    add(
                        "context-swap-colors",
                        t!("editor.contextual_tools.swap_colors"),
                        true,
                        Self::swap_colors,
                    );
                }
                if self.tool == Tool::Clone {
                    add(
                        "context-clone-source",
                        t!("editor.contextual_tools.clone_source"),
                        self.tools.clone_source.is_some(),
                        |this, cx| {
                            this.tools.clone_source = None;
                            this.tools.clone_offset = None;
                            cx.notify();
                        },
                    );
                }
            }
            Tool::Grade => {
                add(
                    "context-auto-tone",
                    t!("editor.contextual_tools.auto_tone"),
                    self.auto_correction_ready(),
                    |this, cx| this.auto_correct(emulsion_raster::auto::AutoCorrection::Tone, cx),
                );
                add(
                    "context-auto-contrast",
                    t!("editor.contextual_tools.auto_contrast"),
                    self.auto_correction_ready(),
                    |this, cx| {
                        this.auto_correct(emulsion_raster::auto::AutoCorrection::Contrast, cx)
                    },
                );
                add(
                    "context-auto-color",
                    t!("editor.contextual_tools.auto_color"),
                    self.auto_correction_ready(),
                    |this, cx| this.auto_correct(emulsion_raster::auto::AutoCorrection::Color, cx),
                );
                add(
                    "context-check-brightness",
                    t!("editor.contextual_tools.check_brightness"),
                    ready,
                    |this, cx| this.add_blending_check("brightness", cx),
                );
                add(
                    "context-check-saturation",
                    t!("editor.contextual_tools.check_saturation"),
                    ready,
                    |this, cx| this.add_blending_check("saturation", cx),
                );
                add(
                    "context-check-color",
                    t!("editor.contextual_tools.check_color"),
                    ready,
                    |this, cx| this.add_blending_check("color", cx),
                );
                add(
                    "context-hsl",
                    t!("editor.contextual_tools.hsl"),
                    true,
                    |this, cx| this.quick_adjust("hue_saturation", cx),
                );
                add(
                    "context-curves",
                    t!("editor.contextual_tools.curves"),
                    true,
                    |this, cx| this.quick_adjust("curves", cx),
                );
                add(
                    "context-adjustments",
                    t!("editor.contextual_tools.adjustments"),
                    true,
                    |this, cx| this.select_sidebar(SidebarTab::Adjustments, cx),
                );
            }
            Tool::Type => {
                if self.type_tool.field.is_some() {
                    add(
                        "context-text-done",
                        t!("design.background.done"),
                        true,
                        Self::close_text_field,
                    );
                }
                add(
                    "context-text-bold",
                    t!("editor.contextual_tools.text_bold"),
                    true,
                    |this, cx| this.restyle_text(|text| text.bold = !text.bold, cx),
                );
                add(
                    "context-text-italic",
                    t!("editor.contextual_tools.text_italic"),
                    true,
                    |this, cx| this.restyle_text(|text| text.italic = !text.italic, cx),
                );
                add(
                    "context-text-auto-width",
                    t!("editor.contextual_tools.text_auto_width"),
                    editable
                        && self
                            .text_target()
                            .is_some_and(|(_, spec)| spec.text_path.is_none()),
                    Self::auto_width_text,
                );
                add(
                    "context-text-properties",
                    t!("editor.contextual_tools.text_properties"),
                    true,
                    |this, cx| this.select_sidebar(SidebarTab::Properties, cx),
                );
            }
            Tool::Crop => {
                add(
                    "context-crop-apply",
                    t!("editor.contextual_tools.crop_apply"),
                    self.tools.crop.is_some() && self.tools.crop_options.valid,
                    Self::tool_commit,
                );
                add(
                    "context-crop-cancel",
                    t!("editor.contextual_tools.crop_cancel"),
                    self.tools.crop.is_some() || self.tools.straighten != 0.0,
                    |this, cx| {
                        this.tool_cancel(cx);
                    },
                );
            }
            Tool::Shape => {
                add(
                    "context-shape-rectangle",
                    t!("editor.contextual_tools.shape_rectangle"),
                    true,
                    |this, cx| {
                        this.tools.shape = ShapeKind::Rect;
                        cx.notify();
                    },
                );
                add(
                    "context-shape-ellipse",
                    t!("editor.contextual_tools.shape_ellipse"),
                    true,
                    |this, cx| {
                        this.tools.shape = ShapeKind::Ellipse;
                        cx.notify();
                    },
                );
                add(
                    "context-shape-properties",
                    t!("editor.contextual_tools.shape_properties"),
                    true,
                    |this, cx| this.select_sidebar(SidebarTab::Properties, cx),
                );
            }
            Tool::Pen => {
                let building = self.tools.pen.building.as_ref();
                let anchors = building.map_or(0, |path| path.anchors.len());
                add(
                    "context-pen-finish",
                    t!("editor.contextual_tools.pen_finish"),
                    anchors >= 2,
                    Self::pen_finish,
                );
                add(
                    "context-pen-close",
                    t!("editor.contextual_tools.pen_close"),
                    anchors >= 2,
                    |this, cx| {
                        if let Some(path) = &mut this.tools.pen.building {
                            path.closed = true;
                        }
                        this.pen_finish(cx);
                    },
                );
                add(
                    "context-path-selection",
                    t!("editor.contextual_tools.path_selection"),
                    anchors >= 3 || (building.is_none() && self.pen_target().is_some()),
                    Self::pen_to_selection,
                );
            }
            Tool::Eyedropper => {
                add(
                    "context-swap-colors",
                    t!("editor.contextual_tools.swap_colors"),
                    true,
                    Self::swap_colors,
                );
                add(
                    "context-default-colors",
                    t!("editor.contextual_tools.default_colors"),
                    true,
                    Self::default_colors,
                );
            }
        }
        if self.tool == Tool::Select {
            actions.push(
                Button::new("context-generative-fill")
                    .small()
                    .label(t!("editor.contextual_tools.generative_fill"))
                    .disabled(!can_remove || !has_selection)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_generative_fill(window, cx);
                    }))
                    .into_any_element(),
            );
        }
        actions
    }

    pub(super) fn design_selection_actions(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let ids = self.selected_layer_ids();
        let ready = self.layer_menu_ready()
            && !ids.is_empty()
            && ids
                .iter()
                .all(|id| self.editor.doc.locked_ancestor(*id).is_none());
        let mut actions = Vec::new();
        if let Some((_, spec)) = self.text_target().filter(|_| ids.len() == 1) {
            actions.push(
                Button::new("context-text-auto-width")
                    .small()
                    .label(t!("editor.contextual_tools.text_auto_width"))
                    .disabled(!ready || spec.text_path.is_some())
                    .on_click(cx.listener(|this, _, _, cx| this.auto_width_text(cx)))
                    .into_any_element(),
            );
        }
        actions.push(
            Button::new("context-design-properties")
                .small()
                .label(t!("window.properties"))
                .disabled(ids.is_empty())
                .on_click(
                    cx.listener(|this, _, _, cx| this.select_sidebar(SidebarTab::Properties, cx)),
                )
                .into_any_element(),
        );
        actions.push(
            Button::new("context-design-style")
                .small()
                .label(t!("editor.contextual_tools.design_style"))
                .disabled(!ready || ids.len() != 1)
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(id) = this.selected {
                        this.open_layer_styles_dialog(id, window, cx);
                    }
                }))
                .into_any_element(),
        );
        actions.push(
            Button::new("context-design-duplicate")
                .small()
                .label(t!("design.direct.duplicate"))
                .disabled(!ready)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.duplicate_selected(cx);
                    window.focus(&this.canvas_focus, cx);
                }))
                .into_any_element(),
        );
        if ids.len() > 1 {
            actions.push(
                Button::new("context-design-group")
                    .small()
                    .label(t!("design.direct.group"))
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.group_selected(cx);
                        window.focus(&this.canvas_focus, cx);
                    }))
                    .into_any_element(),
            );
        } else if self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .is_some_and(|n| n.is_group())
        {
            actions.push(
                Button::new("context-design-ungroup")
                    .small()
                    .label(t!("design.direct.ungroup"))
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.ungroup_selected(cx);
                        window.focus(&this.canvas_focus, cx);
                    }))
                    .into_any_element(),
            );
        }
        actions
    }
}
