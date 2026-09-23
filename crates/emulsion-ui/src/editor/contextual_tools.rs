//! A short list of next actions for the active canvas tool.
use super::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::{Disableable, Selectable, Sizable};

impl EditorView {
    pub(super) fn contextual_tool_actions(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut actions = Vec::new();
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
                       label: &'static str,
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
                "Finish Quick Mask",
                true,
                Self::toggle_quick_mask,
            );
            add(
                "context-invert-selection",
                "Invert selection",
                true,
                Self::invert_selection,
            );
            add(
                "context-swap-colors",
                "Swap colors",
                true,
                Self::swap_colors,
            );
            return actions;
        }
        let has_selection = self.editor.doc.selection.is_some();
        match self.tool {
            Tool::Hand | Tool::Zoom => {
                if self.tool == Tool::Zoom {
                    add("context-zoom-in", "Zoom in", true, |this, cx| {
                        this.zoom_step(true, cx)
                    });
                    add("context-zoom-out", "Zoom out", true, |this, cx| {
                        this.zoom_step(false, cx)
                    });
                }
                add("context-fit", "Fit image", true, Self::zoom_fit);
                add("context-actual-size", "100%", true, Self::zoom_100);
                if self.tool == Tool::Hand {
                    add("context-reset-view", "Reset rotation", true, |this, cx| {
                        this.rotate(0.0, cx)
                    });
                }
            }
            Tool::Move => {
                add(
                    "context-match-subject",
                    "Set up subject match",
                    self.can_match_subject(),
                    Self::match_subject_stack,
                );
                if self.warp.is_some() {
                    add("context-warp-apply", "Apply warp", true, Self::finish_warp);
                    add(
                        "context-warp-cancel",
                        "Cancel warp",
                        true,
                        Self::cancel_warp,
                    );
                } else {
                    add(
                        "context-transform",
                        "Transform",
                        ready && editable,
                        |this, cx| this.begin_transform_action("scale", cx),
                    );
                    add(
                        "context-subject",
                        "Select subject",
                        true,
                        Self::select_subject,
                    );
                    add(
                        "context-remove-background",
                        "Remove background",
                        true,
                        Self::remove_background,
                    );
                }
            }
            Tool::Select => {
                add(
                    "context-remove-selection",
                    "Remove selection",
                    can_remove && has_selection,
                    Self::content_aware_fill,
                );
                add(
                    "context-subject",
                    "Select subject",
                    true,
                    Self::select_subject,
                );
                add(
                    "context-invert-selection",
                    "Invert selection",
                    has_selection,
                    Self::invert_selection,
                );
                add(
                    "context-selection-mask",
                    "Create layer mask",
                    has_selection && can_add_mask,
                    |this, cx| this.add_mask_inverted(false, cx),
                );
                add(
                    "context-deselect",
                    "Deselect",
                    has_selection,
                    Self::deselect,
                );
            }
            Tool::Mask => {
                add(
                    "context-add-mask",
                    "Add layer mask",
                    can_add_mask,
                    |this, cx| this.add_mask_inverted(false, cx),
                );
            }
            Tool::Brush | Tool::Heal | Tool::Clone => {
                if self.tool == Tool::Heal {
                    add("context-heal-mode", "Heal", ready, |this, cx| {
                        this.set_remove_mode(false, cx)
                    });
                    add("context-remove-mode", "Remove", ready, |this, cx| {
                        this.set_remove_mode(true, cx)
                    });
                    if self.tools.remove.enabled {
                        add(
                            "context-remove-after-stroke",
                            "Remove after each stroke",
                            ready,
                            |this, cx| {
                                this.tools.remove.after_stroke = !this.tools.remove.after_stroke;
                                cx.notify();
                            },
                        );
                        add(
                            "context-remove-apply",
                            "Remove now",
                            can_remove && self.remove_pending(),
                            Self::apply_remove,
                        );
                        add(
                            "context-remove-cancel",
                            "Cancel",
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
                        "Smaller brush",
                        true,
                        |this, cx| this.brush_size(false, cx),
                    );
                    add("context-larger-brush", "Larger brush", true, |this, cx| {
                        this.brush_size(true, cx)
                    });
                }
                if self.brushy() {
                    add(
                        "context-brush-settings",
                        "Brush settings",
                        true,
                        |this, cx| this.select_sidebar(SidebarTab::BrushSettings, cx),
                    );
                }
                if self.tool == Tool::Brush {
                    if self.tools.paint == PaintKind::Gradient {
                        add("context-gradient-linear", "Linear", true, |this, cx| {
                            this.tools.radial = false;
                            cx.notify();
                        });
                        add("context-gradient-radial", "Radial", true, |this, cx| {
                            this.tools.radial = true;
                            cx.notify();
                        });
                    } else if self.tools.paint == PaintKind::Bucket {
                        add(
                            "context-fill-contiguous",
                            "Contiguous fill",
                            true,
                            |this, cx| {
                                this.tools.contiguous = !this.tools.contiguous;
                                cx.notify();
                            },
                        );
                    }
                    add(
                        "context-swap-colors",
                        "Swap colors",
                        true,
                        Self::swap_colors,
                    );
                }
                if self.tool == Tool::Clone {
                    add(
                        "context-clone-source",
                        "Reset source",
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
                    "Auto Tone",
                    self.auto_correction_ready(),
                    |this, cx| this.auto_correct(emulsion_raster::auto::AutoCorrection::Tone, cx),
                );
                add(
                    "context-auto-contrast",
                    "Auto Contrast",
                    self.auto_correction_ready(),
                    |this, cx| {
                        this.auto_correct(emulsion_raster::auto::AutoCorrection::Contrast, cx)
                    },
                );
                add(
                    "context-auto-color",
                    "Auto Color",
                    self.auto_correction_ready(),
                    |this, cx| this.auto_correct(emulsion_raster::auto::AutoCorrection::Color, cx),
                );
                add(
                    "context-check-brightness",
                    "Check brightness",
                    ready,
                    |this, cx| this.add_blending_check("brightness", cx),
                );
                add(
                    "context-check-saturation",
                    "Check saturation",
                    ready,
                    |this, cx| this.add_blending_check("saturation", cx),
                );
                add("context-check-color", "Check color", ready, |this, cx| {
                    this.add_blending_check("color", cx)
                });
                add("context-hsl", "Hue / Saturation", true, |this, cx| {
                    this.quick_adjust("hue_saturation", cx)
                });
                add("context-curves", "Curves", true, |this, cx| {
                    this.quick_adjust("curves", cx)
                });
                add(
                    "context-adjustments",
                    "All adjustments",
                    true,
                    |this, cx| this.select_sidebar(SidebarTab::Adjustments, cx),
                );
            }
            Tool::Type => {
                if self.type_tool.field.is_some() {
                    add("context-text-done", "Done", true, Self::close_text_field);
                }
                add("context-text-bold", "Toggle bold", true, |this, cx| {
                    this.restyle_text(|text| text.bold = !text.bold, cx)
                });
                add("context-text-italic", "Toggle italic", true, |this, cx| {
                    this.restyle_text(|text| text.italic = !text.italic, cx)
                });
                add(
                    "context-text-properties",
                    "Character / Paragraph",
                    true,
                    |this, cx| this.select_sidebar(SidebarTab::Properties, cx),
                );
            }
            Tool::Crop => {
                add(
                    "context-crop-apply",
                    "Apply crop",
                    self.tools.crop.is_some() && self.tools.crop_options.valid,
                    Self::tool_commit,
                );
                add(
                    "context-crop-cancel",
                    "Cancel crop",
                    self.tools.crop.is_some() || self.tools.straighten != 0.0,
                    |this, cx| {
                        this.tool_cancel(cx);
                    },
                );
            }
            Tool::Shape => {
                add("context-shape-rectangle", "Rectangle", true, |this, cx| {
                    this.tools.shape = ShapeKind::Rect;
                    cx.notify();
                });
                add("context-shape-ellipse", "Ellipse", true, |this, cx| {
                    this.tools.shape = ShapeKind::Ellipse;
                    cx.notify();
                });
                add(
                    "context-shape-properties",
                    "Shape properties",
                    true,
                    |this, cx| this.select_sidebar(SidebarTab::Properties, cx),
                );
            }
            Tool::Pen => {
                let building = self.tools.pen.building.as_ref();
                let anchors = building.map_or(0, |path| path.anchors.len());
                add(
                    "context-pen-finish",
                    "Finish path",
                    anchors >= 2,
                    Self::pen_finish,
                );
                add(
                    "context-pen-close",
                    "Close path",
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
                    "Make selection",
                    anchors >= 3 || (building.is_none() && self.pen_target().is_some()),
                    Self::pen_to_selection,
                );
            }
            Tool::Eyedropper => {
                add(
                    "context-swap-colors",
                    "Swap colors",
                    true,
                    Self::swap_colors,
                );
                add(
                    "context-default-colors",
                    "Default colors",
                    true,
                    Self::default_colors,
                );
            }
        }
        if self.tool == Tool::Select {
            actions.push(
                Button::new("context-generative-fill")
                    .small()
                    .label("Generative fill")
                    .disabled(!can_remove || !has_selection)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_generative_fill(window, cx);
                    }))
                    .into_any_element(),
            );
        }
        actions
    }
}
