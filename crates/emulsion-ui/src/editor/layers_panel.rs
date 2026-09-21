//! Layer list search, filtering, locks, and independent mask targeting.
use super::*;
use emulsion_core::document::PanelRow;
use emulsion_core::node::LayerColor;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LayerKindFilter {
    #[default]
    All,
    Pixels,
    Text,
    Adjustments,
    Shapes,
    Smart,
    Groups,
    Masks,
}

impl LayerKindFilter {
    const ALL: [Self; 8] = [
        Self::All,
        Self::Pixels,
        Self::Text,
        Self::Adjustments,
        Self::Shapes,
        Self::Smart,
        Self::Groups,
        Self::Masks,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All kinds",
            Self::Pixels => "Pixels",
            Self::Text => "Text",
            Self::Adjustments => "Adjustments",
            Self::Shapes => "Shapes",
            Self::Smart => "Smart Objects",
            Self::Groups => "Groups",
            Self::Masks => "With masks",
        }
    }

    fn matches(self, node: &Node) -> bool {
        match self {
            Self::All => true,
            Self::Pixels => matches!(node.kind, NodeKind::Raster { .. }),
            Self::Text => matches!(node.kind, NodeKind::Text { .. }),
            Self::Adjustments => matches!(node.kind, NodeKind::Adjust(_)),
            Self::Shapes => matches!(node.kind, NodeKind::Path { .. } | NodeKind::Fill { .. }),
            Self::Smart => matches!(node.kind, NodeKind::Smart { .. }),
            Self::Groups => node.is_group(),
            Self::Masks => node.mask.is_some(),
        }
    }
}

#[derive(Default)]
pub(crate) struct LayerPanelState {
    pub search: Option<(Entity<InputState>, Subscription)>,
    pub query: String,
    pub kind: LayerKindFilter,
    pub compact: bool,
    pub controls_open: bool,
    pub compact_height: Option<f32>,
    pub dock_bounds: TrackBounds,
    masks: HashMap<NodeId, (Arc<emulsion_raster::Mask>, Arc<RenderImage>)>,
}

// These colors represent user-assigned label data, not interface semantics.
pub(super) fn label_color(label: LayerColor, p: &Palette) -> Hsla {
    match label {
        LayerColor::None => p.line,
        LayerColor::Red => rgb(0xD65852).into(),
        LayerColor::Orange => rgb(0xDB923E).into(),
        LayerColor::Yellow => rgb(0xD5BB48).into(),
        LayerColor::Green => rgb(0x69A96B).into(),
        LayerColor::Blue => rgb(0x629AC8).into(),
        LayerColor::Violet => rgb(0xA17BCC).into(),
        LayerColor::Gray => p.muted,
    }
}

impl EditorView {
    pub(super) fn ensure_layer_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layer_panel.search.is_some() {
            return;
        }
        let state = cx.new(|cx| InputState::new(window, cx).placeholder("Search layers"));
        let subscription = cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.layer_panel.query = state.read(cx).value().to_lowercase();
                cx.notify();
            }
        });
        self.layer_panel.search = Some((state, subscription));
    }

    pub(crate) fn filtered_layer_rows(&self) -> Vec<PanelRow> {
        let query = self.layer_panel.query.trim();
        if query.is_empty() && self.layer_panel.kind == LayerKindFilter::All {
            return self.editor.doc.panel_rows();
        }
        // Search reaches layers inside collapsed groups without changing them.
        fn visit(
            doc: &Document,
            parent: Option<NodeId>,
            depth: usize,
            query: &str,
            kind: LayerKindFilter,
            rows: &mut Vec<PanelRow>,
        ) {
            for id in doc.children(parent).into_iter().rev() {
                let Some(node) = doc.node(id) else { continue };
                if node.name.to_lowercase().contains(query) && kind.matches(node) {
                    rows.push(PanelRow { id, depth });
                }
                if node.is_group() {
                    visit(doc, Some(id), depth + 1, query, kind, rows);
                }
            }
        }
        let mut rows = Vec::new();
        visit(
            &self.editor.doc,
            None,
            0,
            query,
            self.layer_panel.kind,
            &mut rows,
        );
        rows
    }

    pub(super) fn layer_filter_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let kind = self.layer_panel.kind;
        let editor = cx.weak_entity();
        let mut controls = div().flex().flex_none().items_center().gap_1().pb_1();
        if let Some((state, _)) = &self.layer_panel.search {
            controls = controls.child(
                div().flex_1().min_w_0().child(
                    Input::new(state)
                        .id("layer-search")
                        .aria_label("Search layers by name")
                        .small(),
                ),
            );
        }
        controls
            .child(
                Button::new("layer-kind-filter")
                    .label(format!("{} ▾", kind.label()))
                    .small()
                    .bg(p.soft_bg)
                    .text_color(p.ink)
                    .dropdown_menu(move |mut menu, _, _| {
                        for choice in LayerKindFilter::ALL {
                            let editor = editor.clone();
                            menu = menu.item(
                                PopupMenuItem::new(choice.label())
                                    .checked(choice == kind)
                                    .on_click(move |_, window, cx| {
                                        editor
                                            .update(cx, |this, cx| {
                                                this.layer_panel.kind = choice;
                                                window.focus(&this.panel_focus, cx);
                                                cx.notify();
                                            })
                                            .ok();
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }

    pub(super) fn layer_lock_controls(
        &self,
        p: &Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let ids = self.selected_layer_ids();
        if ids.is_empty() {
            return None;
        }
        let mut row = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_1()
            .pb_1()
            .child(div().text_xs().text_color(p.muted).child("Lock:"));
        for (index, title) in [
            (0usize, "Alpha"),
            (1, "Pixels"),
            (2, "Position"),
            (3, "All"),
        ] {
            let active = ids.iter().all(|id| {
                self.editor.doc.node(*id).is_some_and(|node| match index {
                    0 => node.locks.transparency,
                    1 => node.locks.pixels,
                    2 => node.locks.position,
                    _ => node.locked,
                })
            });
            row = row.child(
                chip(("layer-lock", index), title, active, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let commands = this
                            .selected_layer_ids()
                            .into_iter()
                            .filter_map(|id| {
                                let node = this.editor.doc.node(id)?;
                                if index == 3 {
                                    return Some(Command::SetLocked {
                                        id,
                                        locked: !active,
                                    });
                                }
                                let mut locks = node.locks;
                                match index {
                                    0 => locks.transparency = !active,
                                    1 => locks.pixels = !active,
                                    _ => locks.position = !active,
                                }
                                Some(Command::SetLayerLocks { id, locks })
                            })
                            .collect();
                        this.close_text_field(cx);
                        this.execute_layer_commands("Layer locks", commands, cx);
                        window.focus(&this.panel_focus, cx);
                    })),
            );
        }
        Some(row.into_any_element())
    }

    pub(super) fn layer_blend_controls(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let node = self.editor.doc.node(self.selected?)?.clone();
        let ids = self.selected_layer_ids();
        let mixed_blend = ids.iter().any(|id| {
            self.editor
                .doc
                .node(*id)
                .is_some_and(|n| n.blend != node.blend)
        });
        let group_only = ids
            .iter()
            .all(|id| self.editor.doc.node(*id).is_some_and(Node::is_group));
        let current = node.blend;
        let editor = cx.weak_entity();
        let blend = Button::new("layers-blend-mode")
            .label(if mixed_blend {
                "Mixed blend modes ▾".to_string()
            } else {
                format!("{} ▾", current.label())
            })
            .small()
            .bg(p.soft_bg)
            .text_color(p.ink)
            .dropdown_menu(move |mut menu, _, _| {
                let modes = group_only
                    .then_some(BlendMode::PassThrough)
                    .into_iter()
                    .chain(BlendMode::MENU.iter().flatten().copied());
                for mode in modes {
                    let editor = editor.clone();
                    menu = menu.item(
                        PopupMenuItem::new(mode.label())
                            .checked(!mixed_blend && mode == current)
                            .on_click(move |_, window, cx| {
                                editor
                                    .update(cx, |this, cx| {
                                        this.close_text_field(cx);
                                        let commands = this
                                            .selected_layer_ids()
                                            .into_iter()
                                            .map(|id| Command::SetBlend { id, blend: mode })
                                            .collect();
                                        this.execute_layer_commands(
                                            "Layer blend mode",
                                            commands,
                                            cx,
                                        );
                                        window.focus(&this.panel_focus, cx);
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            });
        let opacity_mixed = ids.iter().any(|id| {
            self.editor
                .doc
                .node(*id)
                .is_some_and(|n| n.opacity != node.opacity)
        });
        let fill_mixed = ids.iter().any(|id| {
            self.editor
                .doc
                .node(*id)
                .is_some_and(|n| n.blending.fill_opacity != node.blending.fill_opacity)
        });
        let opacity = self.param_slider(
            SliderKey::LayerOpacity(node.id),
            "Opacity",
            if opacity_mixed {
                "Mixed".into()
            } else {
                format!("{:.0}%", node.opacity * 100.)
            },
            node.opacity,
            (0., 100., 1.),
            p,
            cx,
        );
        let fill = self.param_slider(
            SliderKey::LayerFillOpacity(node.id),
            "Fill",
            if fill_mixed {
                "Mixed".into()
            } else {
                format!("{:.0}%", node.blending.fill_opacity * 100.)
            },
            node.blending.fill_opacity,
            (0., 100., 1.),
            p,
            cx,
        );
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .gap_2()
                .pb_2()
                .child(blend)
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(div().flex_1().min_w_0().child(opacity))
                        .child(div().flex_1().min_w_0().child(fill)),
                )
                .into_any_element(),
        )
    }

    pub(super) fn mask_thumbnail(
        &mut self,
        id: NodeId,
        mask: &Arc<emulsion_raster::Mask>,
    ) -> Arc<RenderImage> {
        if let Some((old, image)) = self.layer_panel.masks.get(&id)
            && Arc::ptr_eq(old, mask)
        {
            return image.clone();
        }
        self.layer_panel
            .masks
            .retain(|id, _| self.editor.doc.node(*id).is_some());
        let scale = 28. / mask.width().max(mask.height()).max(1) as f64;
        let width = (mask.width() as f64 * scale).round().max(1.) as u32;
        let height = (mask.height() as f64 * scale).round().max(1.) as u32;
        let mut bgra = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = mask.get(x * mask.width() / width, y * mask.height() / height);
                bgra.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let image = Arc::new(viewport::bgra_image(width, height, bgra));
        self.layer_panel
            .masks
            .insert(id, (mask.clone(), image.clone()));
        image
    }

    pub(super) fn select_layer_mask(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.select_layer_row(id, false, false, cx);
        self.set_tool(Tool::Mask, cx);
    }

    pub(super) fn select_layer_content(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.select_layer_row(id, false, false, cx);
        if self.tool == Tool::Mask {
            self.set_tool(Tool::Brush, cx);
        }
        self.set_mask_edit(false, cx);
    }
}
