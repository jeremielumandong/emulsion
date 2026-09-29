//! Photo's independent canvas flyouts. Editing uses the same document commands
//! and slider transactions as the dock; thumbnails are bounded background work.
use super::*;
use emulsion_raster::paint::{Brush, DualBlend};
type PreviewBrush = (Brush, Option<Brush>, DualBlend);
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::collections::HashSet;

#[derive(Clone, Default, PartialEq)]
enum BrushFilter {
    #[default]
    All,
    Pinned,
    Recent,
    User,
    Set(String),
}

#[derive(Default)]
pub(crate) struct PhotoPanelState {
    closed: HashSet<&'static str>,
    search: Option<(Entity<InputState>, Subscription)>,
    pub composer: Option<(Entity<InputState>, Subscription)>,
    previews: HashMap<String, Arc<RenderImage>>,
    preview_revision: Option<(u64, bool)>,
    pending: HashSet<String>,
    preview_busy: bool,
    page: usize,
    filter: BrushFilter,
    fields: HashMap<&'static str, (Entity<InputState>, Subscription)>,
    active: Option<(PreviewBrush, Arc<RenderImage>)>,
    active_pending: Option<PreviewBrush>,
}

impl EditorView {
    pub(super) fn shared_panel_mode(&self) -> bool {
        !self.is_design() && !self.is_diagram() && !self.library_only
    }

    pub(crate) fn assistant_in_panel(&self) -> bool {
        self.sidebar_tab == SidebarTab::Assistant
            || (self.sidebar_layout.flyout_open
                && self.sidebar_layout.flyout_tab == SidebarTab::Assistant)
    }

    pub(super) fn photo_section(
        &mut self,
        id: &'static str,
        title: &str,
        content: AnyElement,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = !self.sidebar_layout.photo.closed.contains(id);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(p.line)
            .child(
                Button::new(id)
                    .label(format!("{} {title}", if open { "▾" } else { "▸" }))
                    .small()
                    .ghost()
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.sidebar_layout.photo.closed.remove(id) {
                            this.sidebar_layout.photo.closed.insert(id);
                        }
                        cx.notify();
                    })),
            )
            .when(open, |section| section.child(content))
            .into_any_element()
    }

    pub(super) fn photo_properties(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut panel = div()
            .id("sidebar-properties-content")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .text_size(px(12.));
        // Shape tool defaults or the selected vector shape's fill, stroke,
        // size and alignment, as in the dock inspector.
        let shape = self.shape_properties(window, cx);
        let has_shape = shape.is_some();
        panel = panel.children(shape);
        let Some(node) = self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .cloned()
        else {
            if has_shape {
                return panel.into_any_element();
            }
            return panel
                .child("Select a layer to see its properties.")
                .into_any_element();
        };
        if matches!(node.kind, NodeKind::Text { .. })
            && let Some(text) = self.text_properties(window, cx)
        {
            return panel.child(text).into_any_element();
        }
        let id = node.id;
        panel = panel.child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(rail::tool_icon("image").size(px(14.)))
                .child(div().flex_1().min_w_0().truncate().child(node.name.clone())),
        );
        let thumbnail = self
            .nav_thumb(cx)
            .map(|(image, _)| img(image).size_full().object_fit(ObjectFit::Contain));
        let zoom = div()
            .flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .w(px(84.))
                    .h(px(64.))
                    .flex_none()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(p.soft_bg)
                    .border_1()
                    .border_color(p.line)
                    .children(thumbnail),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child("Zoom")
                            .child(mono(format!("{:.0}%", self.view.zoom * 100.), 11., p.ink)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                Button::new("photo-zoom-fit")
                                    .label("Fit")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| this.zoom_fit(cx))),
                            )
                            .child(
                                Button::new("photo-zoom-100")
                                    .label("100%")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| this.zoom_100(cx))),
                            )
                            .child(
                                Button::new("photo-zoom-out")
                                    .label("−")
                                    .accessibility_label("Zoom out")
                                    .xsmall()
                                    .ghost()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.zoom_step(false, cx)),
                                    ),
                            )
                            .child(
                                Button::new("photo-zoom-in")
                                    .label("+")
                                    .accessibility_label("Zoom in")
                                    .xsmall()
                                    .ghost()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.zoom_step(true, cx)),
                                    ),
                            ),
                    ),
            );
        panel = panel.child(zoom);
        self.sync_transform_fields(window, cx);
        panel = panel.child(
            div()
                .grid()
                .grid_cols(2)
                .gap_2()
                .children(self.photo_transform_fields(p, cx)),
        );
        panel = panel.child(
            div()
                .flex()
                .gap_1()
                .justify_end()
                .child(
                    Button::new("photo-flip-horizontal")
                        .accessibility_label("Flip horizontal")
                        .tooltip("Flip horizontal")
                        .small()
                        .outline()
                        .child(rail::tool_icon("flip-horizontal").size(px(13.)))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.flip_transform_selection(true, cx)),
                        ),
                )
                .child(
                    Button::new("photo-flip-vertical")
                        .accessibility_label("Flip vertical")
                        .tooltip("Flip vertical")
                        .small()
                        .outline()
                        .child(rail::tool_icon("flip-vertical").size(px(13.)))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.flip_transform_selection(false, cx)),
                        ),
                ),
        );
        let align = self.alignment_controls(p, cx);
        panel = panel.child(self.photo_section("photo-align", "Align & distribute", align, p, cx));
        let owner = cx.weak_entity();
        let group = node.is_group();
        let blend = Button::new("photo-blend")
            .label(node.blend.label())
            .small()
            .outline()
            .w_full()
            .justify_start()
            .dropdown_menu(move |mut menu, _, _| {
                for mode in std::iter::once(BlendMode::PassThrough)
                    .filter(|_| group)
                    .chain(BlendMode::MENU.iter().flatten().copied())
                {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(mode.label()).on_click(move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                this.execute(Command::SetBlend { id, blend: mode }, cx)
                            })
                            .ok();
                    }));
                }
                menu
            });
        let blending = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(blend)
            .child(self.photo_slider(
                SliderKey::PhotoOpacity(id),
                "Opacity",
                format!("{:.0}%", node.opacity * 100.),
                node.opacity,
                (0., 100., 1.),
                p,
                cx,
            ))
            .child(self.photo_slider(
                SliderKey::PhotoFillOpacity(id),
                "Fill",
                format!("{:.0}%", node.blending.fill_opacity * 100.),
                node.blending.fill_opacity,
                (0., 100., 1.),
                p,
                cx,
            ))
            .into_any_element();
        panel = panel.child(self.photo_section("photo-blending", "Blending", blending, p, cx));
        let mask = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(mono(
                if node.mask.is_some() {
                    "Layer mask"
                } else {
                    "No mask"
                },
                11.,
                p.muted,
            ))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_1()
                    .child(
                        Button::new("mask-add")
                            .label(if node.mask.is_some() {
                                "Edit mask"
                            } else {
                                "Add mask"
                            })
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                if this.editor.doc.node(id).is_some_and(|n| n.mask.is_some()) {
                                    this.set_tool(Tool::Mask, cx);
                                } else {
                                    this.add_mask_inverted(event.modifiers().alt, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("photo-mask-invert")
                            .label("Invert")
                            .small()
                            .outline()
                            .disabled(node.mask.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.invert_mask(cx))),
                    )
                    .child(
                        Button::new("photo-mask-selection")
                            .label("To selection")
                            .small()
                            .outline()
                            .disabled(node.mask.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.mask_to_selection(cx))),
                    )
                    .child(
                        Button::new("photo-mask-feather")
                            .label("Feather 6 px")
                            .small()
                            .outline()
                            .disabled(node.mask.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.feather_mask(6., cx))),
                    ),
            )
            .into_any_element();
        panel = panel.child(self.photo_section("photo-mask", "Layer mask", mask, p, cx));
        let actions = div()
            .grid()
            .grid_cols(2)
            .gap_1()
            .child(
                Button::new("photo-select-subject")
                    .label("Select subject")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.select_subject(cx))),
            )
            .child(
                Button::new("photo-remove-background")
                    .label("Remove background")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.remove_background(cx))),
            )
            .child(
                Button::new("photo-auto-tone")
                    .label("Auto tone")
                    .small()
                    .outline()
                    .disabled(!self.auto_correction_ready())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.auto_correct(emulsion_raster::auto::AutoCorrection::Tone, cx)
                    })),
            )
            .child(
                Button::new("photo-adjustment")
                    .label("Adjustments…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.select_sidebar(SidebarTab::Adjustments, cx)
                    })),
            )
            .into_any_element();
        panel = panel.child(self.photo_section("photo-actions", "Quick actions", actions, p, cx));
        // Smart filters and adjustment parameters remain reachable without
        // crowding the everyday geometry/blending controls.
        if matches!(node.kind, NodeKind::Smart { .. } | NodeKind::Adjust(_)) {
            let open = self
                .sidebar_layout
                .photo
                .closed
                .contains("photo-layer-details");
            panel = panel.child(
                Button::new("photo-layer-details")
                    .label(if open {
                        "▾ Layer controls"
                    } else {
                        "▸ Layer controls"
                    })
                    .small()
                    .ghost()
                    .justify_start()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this
                            .sidebar_layout
                            .photo
                            .closed
                            .remove("photo-layer-details")
                        {
                            this.sidebar_layout
                                .photo
                                .closed
                                .insert("photo-layer-details");
                        }
                        cx.notify();
                    })),
            );
            if open {
                match &node.kind {
                    NodeKind::Smart {
                        filters,
                        filter_styles,
                        ..
                    } => {
                        panel = panel.children(self.smart_panel(id, filters, filter_styles, p, cx))
                    }
                    NodeKind::Adjust(adjustment) => {
                        panel = panel.children(self.adjust_extras(id, adjustment, p, cx));
                        for param in self.adjust_visible_params(adjustment) {
                            let norm = (param.value - param.min) / (param.max - param.min);
                            panel = panel.child(self.photo_slider(
                                SliderKey::Param(id, param.key),
                                param.label,
                                param.display(),
                                norm,
                                (param.min, param.max, param.step),
                                p,
                                cx,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        panel.into_any_element()
    }

    pub(super) fn photo_brushes(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.presets.library.is_none() {
            self.prepare_presets(cx);
        }
        if self.sidebar_layout.photo.search.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("Search brushes"));
            let sub = cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.sidebar_layout.photo.page = 0;
                    cx.notify();
                }
            });
            self.sidebar_layout.photo.search = Some((state, sub));
        }
        let search = self.sidebar_layout.photo.search.as_ref().unwrap().0.clone();
        let query = search.read(cx).value().to_lowercase();
        let catalog = &self.presets.library.as_ref().unwrap().read(cx).catalog;
        let dual = self
            .presets
            .current_id
            .as_ref()
            .and_then(|id| catalog.brush(id))
            .map(|b| (b.secondary, b.combine_mode))
            .unwrap_or_default();
        let preview_id = self.presets.current_id.clone();
        let revision = (catalog.revision, p.dark);
        let dark = p.dark;
        let filter = self.sidebar_layout.photo.filter.clone();
        let mut filters = vec![
            (BrushFilter::All, "All brushes".to_string()),
            (BrushFilter::Pinned, "Pinned".into()),
            (BrushFilter::Recent, "Recent".into()),
            (BrushFilter::User, "My brushes".into()),
        ];
        filters.extend(
            catalog
                .sets
                .iter()
                .map(|set| (BrushFilter::Set(set.id.clone()), set.name.clone())),
        );
        let filter_label = filters
            .iter()
            .find(|(key, _)| *key == filter)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| "All brushes".into());
        let mut matching: Vec<_> = catalog
            .brushes
            .iter()
            .filter(|brush| {
                let included = match &filter {
                    BrushFilter::All => true,
                    BrushFilter::Pinned => catalog.pinned.contains(&brush.id),
                    BrushFilter::Recent => catalog.recent.contains(&brush.id),
                    BrushFilter::User => catalog
                        .sets
                        .iter()
                        .any(|set| set.id == brush.set_id && !set.builtin),
                    BrushFilter::Set(id) => &brush.set_id == id,
                };
                included && (query.is_empty() || brush.name.to_lowercase().contains(&query))
            })
            .collect();
        if filter == BrushFilter::Recent {
            matching.sort_by_key(|brush| {
                catalog
                    .recent
                    .iter()
                    .position(|id| id == &brush.id)
                    .unwrap_or(usize::MAX)
            });
        }
        let owner = cx.weak_entity();
        let filter_button = Button::new("photo-brush-filter")
            .label(filter_label)
            .small()
            .outline()
            .w_full()
            .justify_start()
            .dropdown_menu(move |mut menu, _, _| {
                for (key, label) in &filters {
                    let owner = owner.clone();
                    let key = key.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(key == filter)
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.sidebar_layout.photo.filter = key.clone();
                                        this.sidebar_layout.photo.page = 0;
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            });
        let count = matching.len();
        const PAGE: usize = 12;
        self.sidebar_layout.photo.page = self
            .sidebar_layout
            .photo
            .page
            .min(count.saturating_sub(1) / PAGE);
        let page = self.sidebar_layout.photo.page;
        let brushes: Vec<_> = matching
            .into_iter()
            .skip(page * PAGE)
            .take(PAGE)
            .cloned()
            .collect();
        if self.sidebar_layout.photo.preview_revision != Some(revision) {
            self.sidebar_layout.photo.previews.clear();
            self.sidebar_layout.photo.active = None;
            self.sidebar_layout.photo.pending.clear();
            self.sidebar_layout.photo.preview_revision = Some(revision);
        }
        let missing: Vec<_> = brushes
            .iter()
            .filter(|b| {
                !self.sidebar_layout.photo.previews.contains_key(&b.id)
                    && !self.sidebar_layout.photo.pending.contains(&b.id)
            })
            .take(if self.sidebar_layout.photo.preview_busy {
                0
            } else {
                12
            })
            .cloned()
            .collect();
        if !missing.is_empty() {
            self.sidebar_layout.photo.preview_busy = true;
            for b in &missing {
                self.sidebar_layout.photo.pending.insert(b.id.clone());
            }
            cx.spawn(async move |this, cx| {
                let images = cx
                    .background_spawn(async move {
                        missing
                            .into_iter()
                            .map(|b| {
                                (
                                    b.id,
                                    super::brush_library_ui::stroke_preview_on(
                                        b.brush,
                                        b.secondary,
                                        b.combine_mode,
                                        dark,
                                    ),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .await;
                this.update(cx, |this, cx| {
                    let state = &mut this.sidebar_layout.photo;
                    state.preview_busy = false;
                    if state.preview_revision != Some(revision) {
                        cx.notify();
                        return;
                    }
                    if state.previews.len() >= 48 {
                        state.previews.clear();
                    }
                    for (id, image) in images {
                        state.pending.remove(&id);
                        state.previews.insert(id, image);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let b = self.tools.brush;
        let preview_brush = (b, dual.0, dual.1);
        if self
            .sidebar_layout
            .photo
            .active
            .as_ref()
            .is_none_or(|(brush, _)| *brush != preview_brush)
            && self.sidebar_layout.photo.active_pending.is_none()
        {
            self.sidebar_layout.photo.active_pending = Some(preview_brush);
            cx.spawn(async move |this, cx| {
                let image = cx
                    .background_spawn(async move {
                        super::brush_library_ui::stroke_preview_on(b, dual.0, dual.1, dark)
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.sidebar_layout.photo.active_pending = None;
                    if this.tools.brush == b
                        && this.presets.current_id == preview_id
                        && this.sidebar_layout.photo.preview_revision == Some(revision)
                    {
                        this.sidebar_layout.photo.active = Some((preview_brush, image));
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let mut panel = div()
            .id("photo-brushes-content")
            .test_support()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(mono(
                self.presets
                    .current
                    .clone()
                    .unwrap_or_else(|| "Custom brush".into()),
                11.,
                p.muted,
            ))
            .child(
                div()
                    .w_full()
                    .h(px(82.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(p.soft_bg)
                    .children(self.sidebar_layout.photo.active.as_ref().map(|(_, image)| {
                        img(image.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                    })),
            );
        for (key, name, display, norm, spec) in [
            (
                SliderKey::PhotoBrushSize,
                "Size",
                format!("{:.0} px", b.size),
                ((b.size - 1.) / 499.).clamp(0., 1.).sqrt(),
                (1., 500., 1.),
            ),
            (
                SliderKey::PhotoBrushHardness,
                "Hardness",
                format!("{:.0}%", b.hardness * 100.),
                b.hardness,
                (0., 100., 1.),
            ),
            (
                SliderKey::PhotoBrushOpacity,
                "Opacity",
                format!("{:.0}%", b.opacity * 100.),
                b.opacity,
                (1., 100., 1.),
            ),
            (
                SliderKey::PhotoBrushFlow,
                "Flow",
                format!("{:.0}%", b.flow * 100.),
                b.flow,
                (1., 100., 1.),
            ),
        ] {
            panel = panel.child(self.photo_slider(key, name, display, norm, spec, p, cx));
        }
        let extras = self.photo_brush_fields(p, window, cx);
        panel = panel.child(extras).child(
            div().flex().flex_wrap().gap_1().children(
                [
                    (0, "Pressure → size", b.size_pressure > 0.),
                    (1, "Pressure → flow", b.flow_pressure > 0.),
                    (2, "Tilt → angle", b.tilt > 0.),
                ]
                .into_iter()
                .map(|(index, title, on)| {
                    Button::new(("photo-brush-pressure", index as usize))
                        .label(title)
                        .small()
                        .outline()
                        .when(on, |b| b.bg(p.accent.opacity(0.12)).text_color(p.accent))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            match index {
                                0 => this.tools.brush.size_pressure = if on { 0. } else { 1. },
                                1 => this.tools.brush.flow_pressure = if on { 0. } else { 1. },
                                _ => this.tools.brush.tilt = if on { 0. } else { 1. },
                            }
                            cx.notify();
                        }))
                }),
            ),
        );
        let mut presets = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&search).id("photo-brush-search").small()),
                    )
                    .child(
                        Button::new("photo-brush-new")
                            .label("+ New")
                            .tooltip("Save current brush as a new preset")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.save_preset(cx))),
                    ),
            )
            .child(filter_button)
            .child(mono(format!("{count} brushes"), 10., p.muted));
        let mut grid = div().grid().grid_cols(3).gap_1();
        for brush in brushes {
            let selected = self.presets.current_id.as_ref() == Some(&brush.id);
            let image = self.sidebar_layout.photo.previews.get(&brush.id).cloned();
            let id = brush.id.clone();
            grid = grid.child(
                Button::new(SharedString::from(format!("photo-brush-{}", brush.id)))
                    .accessibility_label(brush.name.clone())
                    .tooltip(brush.name.clone())
                    .outline()
                    .h(px(86.))
                    .min_w_0()
                    .p_1()
                    .when(selected, |b| {
                        b.border_color(p.accent).bg(p.accent.opacity(0.1))
                    })
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .h(px(38.))
                                    .w_full()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .children(image.map(|image| {
                                        img(image).size_full().object_fit(ObjectFit::Contain)
                                    })),
                            )
                            .child(div().text_size(px(10.)).truncate().child(brush.name)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_tool(Tool::Brush, cx);
                        this.apply_brush_id(&id, cx);
                    })),
            );
        }
        presets = presets
            .child(grid)
            .when(count == 0, |p| p.child("No matching brushes."))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Button::new("photo-brush-prev")
                            .label("Previous")
                            .small()
                            .ghost()
                            .disabled(page == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_layout.photo.page =
                                    this.sidebar_layout.photo.page.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(mono(
                        format!("{} / {}", page + 1, count.div_ceil(PAGE).max(1)),
                        10.,
                        p.muted,
                    ))
                    .child(
                        Button::new("photo-brush-next")
                            .label("Next")
                            .small()
                            .ghost()
                            .disabled((page + 1) * PAGE >= count)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_layout.photo.page += 1;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                Button::new("photo-brush-library")
                    .label("Brush library & advanced settings…")
                    .small()
                    .outline()
                    .w_full()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_brush_workspace(window, cx)),
                    ),
            );
        panel
            .child(self.photo_section(
                "photo-brush-presets",
                "Presets",
                presets.into_any_element(),
                p,
                cx,
            ))
            .into_any_element()
    }
}

impl EditorView {
    fn photo_brush_fields(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let b = self.tools.brush;
        let mut fields = div().grid().grid_cols(2).gap_1();
        for (key, title, value, suffix) in [
            ("spacing", "Spacing", b.spacing * 100., "%"),
            ("smoothing", "Smoothing", b.stabilizer * 100., "%"),
            ("angle", "Angle", b.angle, "°"),
            ("roundness", "Roundness", b.roundness * 100., "%"),
        ] {
            let text = format!("{value:.0}");
            if !self.sidebar_layout.photo.fields.contains_key(key) {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(text.clone()));
                let sub = cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event: &InputEvent, window, cx| {
                        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            return;
                        }
                        let value = input.read(cx).value().trim().parse::<f32>();
                        if let Ok(value) = value
                            && value.is_finite()
                        {
                            let b = &mut this.tools.brush;
                            match key {
                                "spacing" => b.spacing = value.clamp(2., 200.) / 100.,
                                "smoothing" => b.stabilizer = value.clamp(0., 100.) / 100.,
                                "angle" => b.angle = value.rem_euclid(360.),
                                _ => b.roundness = value.clamp(5., 100.) / 100.,
                            }
                        } else {
                            this.set_status(
                                "Enter a finite number for the brush setting.",
                                true,
                                cx,
                            );
                        }
                        let b = this.tools.brush;
                        let actual = match key {
                            "spacing" => b.spacing * 100.,
                            "smoothing" => b.stabilizer * 100.,
                            "angle" => b.angle,
                            _ => b.roundness * 100.,
                        };
                        input.update(cx, |input, cx| {
                            input.set_value(format!("{actual:.0}"), window, cx)
                        });
                        cx.notify();
                    },
                );
                self.sidebar_layout.photo.fields.insert(key, (input, sub));
            }
            let input = self.sidebar_layout.photo.fields[key].0.clone();
            if !input.read(cx).focus_handle(cx).is_focused(window)
                && input.read(cx).value().as_ref() != text
            {
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
            let focus = input.read(cx).focus_handle(cx);
            fields = fields.child(
                div()
                    .id(SharedString::from(format!("photo-brush-{key}")))
                    .test_support()
                    .min_w_0()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&focus, cx)
                    })
                    .child(
                        Input::new(&input)
                            .aria_label(title)
                            .small()
                            .h(px(26.))
                            .prefix(div().text_size(px(11.)).text_color(p.muted).child(title))
                            .suffix(mono(suffix, 10., p.muted))
                            .font_family(MONO_FONT)
                            .text_size(px(11.))
                            .text_align(TextAlign::Right),
                    ),
            );
        }
        fields.into_any_element()
    }

    pub(super) fn photo_history(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let count = self.editor.history.len();
        let mut body = div()
            .id("sidebar-history-content")
            .test_support()
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("history-undo")
                            .label("Undo")
                            .small()
                            .ghost()
                            .disabled(!self.editor.can_undo())
                            .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                    )
                    .child(
                        Button::new("history-redo")
                            .label("Redo")
                            .small()
                            .ghost()
                            .disabled(!self.editor.can_redo())
                            .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                    )
                    .child(
                        Button::new("history-versions")
                            .label("Versions…")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.open_history(cx))),
                    ),
            )
            .child(
                Button::new("history-initial")
                    .label(if count == 0 {
                        "Current state"
                    } else {
                        "Earlier state"
                    })
                    .small()
                    .ghost()
                    .justify_start()
                    .w_full()
                    .when(count == 0, |b| b.bg(p.soft_bg))
                    .on_click(cx.listener(move |this, _, _, cx| this.undo_to(count, cx))),
            );
        for (index, step) in self
            .editor
            .history
            .steps()
            .enumerate()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            body = body.child(
                Button::new(("history-step", step.revision_before))
                    .small()
                    .ghost()
                    .w_full()
                    .justify_start()
                    .when(index == 0, |b| b.bg(p.soft_bg))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(div().size(px(5.)).rounded_full().bg(if index == 0 {
                                p.accent
                            } else {
                                p.muted.opacity(0.35)
                            }))
                            .child(
                                div()
                                    .truncate()
                                    .text_color(if index == 0 { p.ink } else { p.muted })
                                    .child(step.name.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.undo_to(index, cx);
                        window.focus(&this.canvas_focus, cx);
                    })),
            );
        }
        body.into_any_element()
    }

    /// The handoff's filled, round brush track; pointer and keyboard events use
    /// the editor's existing gesture handling. The hit area remains 24 px tall.
    #[allow(clippy::too_many_arguments)] // Same gesture parameters as the shared editor slider.
    fn photo_slider(
        &mut self,
        key: SliderKey,
        name: &str,
        display: String,
        norm: f32,
        spec: (f32, f32, f32),
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let track = self.tracks.entry(key).or_default().clone();
        let measure = track.clone();
        let norm = norm.clamp(0., 1.);
        let brush = matches!(
            key,
            SliderKey::PhotoBrushSize
                | SliderKey::PhotoBrushHardness
                | SliderKey::PhotoBrushOpacity
                | SliderKey::PhotoBrushFlow
        );
        let knob = if brush { 20. } else { 10. };
        let height = if brush { 14. } else { 3. };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(11.))
                    .child(div().text_color(p.muted).child(name.to_string()))
                    .child(mono(display, 10.5, p.ink)),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{key:?}")))
                    .relative()
                    .w_full()
                    .h(px(24.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .tab_index(0)
                    .key_context("Slider")
                    .role(Role::Slider)
                    .aria_label(name.to_string())
                    .test_support()
                    .aria_value(format!("{}", spec.0 + norm * (spec.1 - spec.0)))
                    .aria_min_numeric_value(spec.0 as f64)
                    .aria_max_numeric_value(spec.1 as f64)
                    .aria_description(
                        "Arrow keys adjust; Shift adjusts faster; Home and End go to limits",
                    )
                    .focus_visible(|s| s.bg(p.accent.opacity(0.2)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            this.slider_down(key, spec, e, cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, e, _, cx| this.slider_key(key, norm, spec, e, cx)),
                    )
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
                            .top(px((24. - height) / 2.))
                            .w_full()
                            .h(px(height))
                            .rounded_full()
                            .bg(p.soft_bg)
                            .border_1()
                            .border_color(p.line),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px((24. - height) / 2.))
                            .w(relative(norm))
                            .h(px(height))
                            .rounded_full()
                            .bg(if brush { p.accent.opacity(0.24) } else { p.ink }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px((24. - knob) / 2.))
                            .left(relative(norm))
                            .ml(px(-knob / 2.))
                            .size(px(knob))
                            .rounded_full()
                            .bg(if brush { p.accent } else { p.ink })
                            .border_2()
                            .border_color(p.ink),
                    ),
            )
            .into_any_element()
    }
}
