//! Image and Filter menus backed by the same editable effects as Properties.
use super::*;
use emulsion_filters::Filter;
use gpui_kit::base::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

pub(super) struct LastFilter(pub Filter);
impl Global for LastFilter {}

fn psd_background_candidate(doc: &Document, selected: Option<NodeId>) -> Option<NodeId> {
    selected.filter(|&id| doc.valid_psd_background(id))
}

/// Filter labels are stable catalogue identifiers; localize the new UI label
/// without changing command lookup or saved filter keys.
pub(super) fn filter_label(filter: &Filter) -> std::borrow::Cow<'static, str> {
    match filter {
        Filter::Invert => t!("editor.filters.invert"),
        _ => filter.label().into(),
    }
}

/// Filter menu groups: a catalog key and the filters it lists.
const FILTER_GROUPS: &[(&str, &[&str])] = &[
    (
        "editor.filters.group_blur",
        &["gaussian_blur", "box_blur", "motion_blur", "lens_blur"],
    ),
    (
        "editor.filters.group_sharpen",
        &["unsharp_mask", "smart_sharpen"],
    ),
    ("editor.filters.group_noise", &["add_noise", "reduce_noise"]),
    ("editor.filters.group_distort", &["pinch", "twirl", "wave"]),
    ("editor.filters.group_stylize", &["emboss", "find_edges"]),
    ("editor.filters.group_other", &["high_pass", "invert"]),
    (
        "editor.filters.group_enhance",
        &[
            "enhance",
            "structure",
            "atmosphere",
            "golden_hour",
            "dramatic",
        ],
    ),
    (
        "editor.filters.group_creative",
        &["glow", "orton", "sunrays"],
    ),
    ("editor.filters.group_portrait", &["skin_smooth"]),
];

impl EditorView {
    pub(super) fn effects_ready(&self) -> bool {
        !self.assistant.running
            && !self.frame_crop_active()
            && self.drag.is_none()
            && self.warp.is_none()
            && !self.editor.in_transaction()
    }

    pub(super) fn can_filter(&self) -> bool {
        self.effects_ready()
            && !self.tools.mask_edit_target.is_mask()
            && self.selected.is_some_and(|id| {
                self.editor.doc.locked_ancestor(id).is_none()
                    && self.editor.doc.node(id).is_some_and(|node| {
                        matches!(node.kind, NodeKind::Raster { .. } | NodeKind::Smart { .. })
                    })
            })
    }

    pub(crate) fn repeat_last_filter(&mut self, cx: &mut Context<Self>) {
        let Some(filter) = cx.try_global::<LastFilter>().map(|last| last.0.clone()) else {
            self.set_status(t!("editor.filters.choose_first"), false, cx);
            return;
        };
        self.apply_filter(filter, cx);
    }

    /// Apply the catalogue filter called `key`, for keyboard shortcuts.
    pub(crate) fn apply_filter_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(filter) = Filter::catalogue().into_iter().find(|f| f.key() == key) {
            self.apply_filter(filter, cx);
        }
    }

    pub(super) fn apply_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        if self.tools.mask_edit_target.is_mask() {
            self.set_status(t!("editor.filter_mask.no_filter_processing"), false, cx);
            return;
        }
        if !self.can_filter() {
            self.set_status(t!("editor.filters.select_layer_first"), false, cx);
            return;
        }
        let id = self.selected.unwrap();
        self.select_sidebar(SidebarTab::Properties, cx);
        self.add_filter(id, filter, cx);
        self.set_status(t!("editor.filters.applying"), false, cx);
    }

    /// `wide`: room for the app name beside its icon.
    pub(super) fn effect_menus(&self, p: &Palette, wide: bool, cx: &Context<Self>) -> AnyElement {
        let image_editor = cx.entity().downgrade();
        let filter_editor = image_editor.clone();
        div()
            .flex()
            .items_center()
            .gap_1()
            .flex_none()
            .child(self.app_menu(p, wide, cx))
            .child(self.file_menu(p, cx))
            .child(self.edit_menu(p, cx))
            .child(
                div()
                    .id("image-menu")
                    .when(!self.menu_visible("image"), |d| d.hidden())
                    .test_support()
                    .child(
                        Button::new("image-menu-button")
                            .label(super::menu_bar::menu_name("image"))
                            .small()
                            .ghost()
                            .text_color(p.ink)
                            .dropdown_menu(move |menu, window, cx| {
                                let Some(editor) = image_editor.upgrade() else {
                                    return menu;
                                };
                                let enabled = editor.read(cx).effects_ready();
                                let blend_space = editor.read(cx).editor.doc.blend_space;
                                let background = editor.read(cx).editor.doc.psd_background;
                                let background_candidate = psd_background_candidate(
                                    &editor.read(cx).editor.doc,
                                    editor.read(cx).selected,
                                );
                                let auto_ready = editor.read(cx).auto_correction_ready();
                                let auto_focus = editor.read(cx).canvas_focus.clone();
                                let adjustment_editor = image_editor.clone();
                                let blend_editor = image_editor.clone();
                                let menu =
                                    menu.action_context(auto_focus)
                                        .submenu(
                                            t!("window.adjustments"),
                                            window,
                                            cx,
                                            move |mut menu, _, _| {
                                                for adjustment in Adjustment::catalogue() {
                                                    let editor = adjustment_editor.clone();
                                                    let key = adjustment.key();
                                                    menu =
                                                        menu.item(
                                                            PopupMenuItem::new(adjustment.label())
                                                                .disabled(!enabled)
                                                                .on_click(move |_, window, cx| {
                                                                    editor
                                                        .update(cx, |view, cx| {
                                                            view.quick_adjust(key, cx);
                                                            view.restore_effect_focus(window, cx);
                                                        })
                                                        .ok();
                                                                }),
                                                        );
                                                }
                                                menu
                                            },
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new(t!("editor.filters.auto_tone"))
                                                .action(Box::new(crate::actions::AutoTone))
                                                .disabled(!auto_ready),
                                        )
                                        .item(
                                            PopupMenuItem::new(t!("editor.filters.auto_contrast"))
                                                .action(Box::new(crate::actions::AutoContrast))
                                                .disabled(!auto_ready),
                                        )
                                        .item(
                                            PopupMenuItem::new(t!("editor.filters.auto_color"))
                                                .action(Box::new(crate::actions::AutoColor))
                                                .disabled(!auto_ready),
                                        )
                                        .separator()
                                        .submenu(
                                            t!("editor.filters.blend_space"),
                                            window,
                                            cx,
                                            move |menu, _, _| {
                                                let mut menu = menu;
                                                for (space, label) in [
                                                    (
                                                        emulsion_raster::blend::BlendSpace::Srgb,
                                                        t!("editor.filters.blend_srgb"),
                                                    ),
                                                    (
                                                        emulsion_raster::blend::BlendSpace::Linear,
                                                        t!("editor.filters.blend_linear"),
                                                    ),
                                                    (
                                                        emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1,
                                                        t!("editor.filters.blend_photoshop_srgb_v1"),
                                                    ),
                                                ] {
                                                    let editor = blend_editor.clone();
                                                    menu =
                                                        menu.item(
                                                            PopupMenuItem::new(label)
                                                                .checked(blend_space == space)
                                                                .disabled(!enabled)
                                                                .on_click(move |_, window, cx| {
                                                                    editor
                                                        .update(cx, |view, cx| {
                                                            view.execute(
                                                                Command::SetBlendSpace { space },
                                                                cx,
                                                            );
                                                            view.restore_effect_focus(window, cx);
                                                        })
                                                        .ok();
                                                                }),
                                                        );
                                                }
                                                menu
                                            },
                                        )
                                        .separator();
                                let set_background_editor = image_editor.clone();
                                let clear_background_editor = image_editor.clone();
                                let menu = menu
                                    .item(
                                        PopupMenuItem::new(t!("editor.filters.set_psd_background"))
                                            .disabled(
                                                !enabled
                                                    || background_candidate.is_none()
                                                    || background_candidate == background,
                                            )
                                            .on_click(move |_, window, cx| {
                                                if let Some(id) = background_candidate {
                                                    set_background_editor
                                                        .update(cx, |view, cx| {
                                                            if view.effects_ready()
                                                                && view.editor.doc.valid_psd_background(id)
                                                            {
                                                                view.execute(
                                                                    Command::SetPsdBackground { id: Some(id) },
                                                                    cx,
                                                                );
                                                                view.restore_effect_focus(window, cx);
                                                            }
                                                        })
                                                        .ok();
                                                }
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("editor.filters.clear_psd_background"))
                                            .disabled(!enabled || background.is_none())
                                            .on_click(move |_, window, cx| {
                                                clear_background_editor
                                                    .update(cx, |view, cx| {
                                                        if view.effects_ready() {
                                                            view.execute(
                                                                Command::SetPsdBackground { id: None },
                                                                cx,
                                                            );
                                                            view.restore_effect_focus(window, cx);
                                                        }
                                                    })
                                                    .ok();
                                            }),
                                    )
                                    .separator();
                                let size_editor = image_editor.clone();
                                let canvas_editor = image_editor.clone();
                                menu.item(
                                    PopupMenuItem::new(t!("editor.filters.image_size"))
                                        .disabled(!enabled)
                                        .on_click(move |_, window, cx| {
                                            size_editor
                                                .update(cx, |view, cx| {
                                                    view.open_size_panel(
                                                        SizeMode::Image,
                                                        window,
                                                        cx,
                                                    )
                                                })
                                                .ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new(t!("editor.filters.canvas_size"))
                                        .disabled(!enabled)
                                        .on_click(move |_, window, cx| {
                                            canvas_editor
                                                .update(cx, |view, cx| {
                                                    view.open_size_panel(
                                                        SizeMode::Canvas,
                                                        window,
                                                        cx,
                                                    )
                                                })
                                                .ok();
                                        }),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .when(!self.menu_visible("layer"), |d| d.hidden())
                    .child(self.layer_menu_button(cx)),
            )
            .child(self.select_menu(p, cx))
            .child(
                div()
                    .id("filter-menu")
                    .when(!self.menu_visible("filter"), |d| d.hidden())
                    .test_support()
                    .child(
                        Button::new("filter-menu-button")
                            .label(super::menu_bar::menu_name("filter"))
                            .small()
                            .ghost()
                            .text_color(p.ink)
                            .dropdown_menu(move |menu, window, cx| {
                                let Some(editor) = filter_editor.upgrade() else {
                                    return menu;
                                };
                                let enabled = editor.read(cx).can_filter();
                                let last = cx
                                    .try_global::<LastFilter>()
                                    .map(|last| filter_label(&last.0));
                                let repeat_editor = filter_editor.clone();
                                let mut menu = menu
                                    .item(
                                        PopupMenuItem::new(
                                            last.as_ref()
                                                .map(|label| {
                                                    t!("editor.filters.repeat", label = label)
                                                })
                                                .unwrap_or_else(|| {
                                                    t!("editor.filters.repeat_last")
                                                }),
                                        )
                                        .disabled(!enabled || last.is_none())
                                        .on_click(
                                            move |_, window, cx| {
                                                repeat_editor
                                                    .update(cx, |view, cx| {
                                                        view.repeat_last_filter(cx);
                                                        view.restore_effect_focus(window, cx);
                                                    })
                                                    .ok();
                                            },
                                        ),
                                    )
                                    .separator();
                                for (group, keys) in FILTER_GROUPS {
                                    let group_editor = filter_editor.clone();
                                    menu = menu.submenu(
                                        t!(*group),
                                        window,
                                        cx,
                                        move |mut menu, _, _| {
                                            for filter in Filter::catalogue()
                                                .into_iter()
                                                .filter(|f| keys.contains(&f.key()))
                                            {
                                                let editor = group_editor.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::new(filter_label(&filter))
                                                        .disabled(!enabled)
                                                        .on_click(move |_, window, cx| {
                                                            editor
                                                                .update(cx, |view, cx| {
                                                                    view.apply_filter(
                                                                        filter.clone(),
                                                                        cx,
                                                                    );
                                                                    view.restore_effect_focus(
                                                                        window, cx,
                                                                    );
                                                                })
                                                                .ok();
                                                        }),
                                                );
                                            }
                                            menu
                                        },
                                    );
                                }
                                menu = menu.separator();
                                for filter in Filter::catalogue().into_iter().filter(|f| {
                                    matches!(
                                        f,
                                        Filter::LensCorrection { .. } | Filter::LensProfile { .. }
                                    )
                                }) {
                                    let editor = filter_editor.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(filter_label(&filter))
                                            .disabled(!enabled)
                                            .on_click(move |_, window, cx| {
                                                editor
                                                    .update(cx, |view, cx| {
                                                        view.apply_filter(filter.clone(), cx);
                                                        view.restore_effect_focus(window, cx);
                                                    })
                                                    .ok();
                                            }),
                                    );
                                }
                                let liquify_enabled = editor.read(cx).can_liquify();
                                let liquify_editor = filter_editor.clone();
                                menu.separator().item(
                                    PopupMenuItem::new(t!("editor.filters.liquify"))
                                        .disabled(!liquify_enabled)
                                        .on_click(move |_, window, cx| {
                                            liquify_editor
                                                .update(cx, |view, cx| {
                                                    if view.can_liquify() {
                                                        view.set_paint(PaintKind::Liquify, cx);
                                                        view.set_status(
                                                            t!("editor.filters.liquify_hint"),
                                                            false,
                                                            cx,
                                                        );
                                                        view.restore_effect_focus(window, cx);
                                                    }
                                                })
                                                .ok();
                                        }),
                                )
                            }),
                    ),
            )
            .child(self.view_menu(p, cx))
            .child(self.window_menu(p, cx))
            .child(
                Button::new("recipes-menu")
                    .when(!self.menu_visible("recipes"), |b| b.hidden())
                    .label(super::menu_bar::menu_name("recipes"))
                    .small()
                    .ghost()
                    .selected(self.sidebar_tab == SidebarTab::Recipes)
                    .text_color(p.ink)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.recipes.open = true;
                        this.select_sidebar(SidebarTab::Recipes, cx);
                        window.focus(&this.panel_focus, cx);
                    })),
            )
            .child(self.help_menu(p, cx))
            .into_any_element()
    }

    fn can_liquify(&self) -> bool {
        self.can_filter()
            && self
                .selected
                .and_then(|id| self.editor.doc.node(id))
                .is_some_and(|node| matches!(node.kind, NodeKind::Raster { .. }))
    }

    fn restore_effect_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.defer_in(window, |view, window, cx| {
            window.focus(&view.canvas_focus, cx)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn raster(id: NodeId, name: &str) -> Node {
        Node::raster(
            id,
            name,
            Arc::new(Raster::solid(16, 16, [0.2, 0.3, 0.4, 1.])),
            Placement::default(),
        )
    }

    #[test]
    fn background_menu_requires_selected_bottom_root_raster_without_clipping() {
        let mut doc = Document::new(16, 16);
        doc.nodes.push(raster(1, "Image"));
        doc.nodes.push(raster(2, "Upper"));
        let mut nested = raster(4, "Nested");
        nested.parent = Some(3);
        doc.nodes.extend([nested, Node::group(3, "Group")]);
        doc.next_id = 5;
        doc.validate().unwrap();
        assert_eq!(psd_background_candidate(&doc, Some(1)), Some(1));
        for selected in [None, Some(2), Some(3), Some(4), Some(99)] {
            assert_eq!(psd_background_candidate(&doc, selected), None);
        }
        doc.nodes[0].clip_to = Some(2);
        assert_eq!(psd_background_candidate(&doc, Some(1)), None);
        doc.nodes[0] = Node::group(1, "Bottom group");
        assert_eq!(psd_background_candidate(&doc, Some(1)), None);
    }

    #[test]
    fn background_menu_eligibility_does_not_infer_identity_from_layer_appearance() {
        let mut doc = Document::new(16, 16);
        let mut node = raster(1, "Background");
        node.locked = true;
        doc.nodes.push(node);
        for opacity in [1., 0.4, 0.] {
            doc.nodes[0].opacity = opacity;
            assert_eq!(psd_background_candidate(&doc, Some(1)), Some(1));
            assert_eq!(doc.psd_background, None);
        }
        doc.nodes[0].name = "Ordinary image".into();
        doc.nodes[0].locked = false;
        assert_eq!(psd_background_candidate(&doc, Some(1)), Some(1));
        assert_eq!(doc.psd_background, None);
    }
}
