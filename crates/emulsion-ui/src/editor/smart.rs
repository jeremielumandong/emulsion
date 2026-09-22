//! Smart layers in the panel: convert a pixel node, add and tune filters.

use super::*;
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::BlendMode;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

#[derive(Default)]
pub(crate) struct SmartUi {
    /// The add-filter menu is open for this node.
    pub menu_for: Option<NodeId>,
    /// Filter slider edits apply at most this often while dragging.
    last_apply: Option<Instant>,
    pending: Option<(NodeId, usize, &'static str, f32)>,
    /// Bumped per request so stale renders are dropped.
    render_gen: u64,
    requests: HashMap<NodeId, (u64, Vec<Filter>)>,
    edited_filter: Option<(NodeId, usize)>,
}

impl SmartUi {
    pub(super) fn cancel_pending(&mut self) {
        self.requests.clear();
        self.pending = None;
    }
}

impl EditorView {
    fn requested_filters(&self, id: NodeId) -> Option<Vec<Filter>> {
        if let Some((_, filters)) = self.smart.requests.get(&id) {
            return Some(filters.clone());
        }
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Smart { filters, .. } => Some(filters.clone()),
            NodeKind::Raster { .. } => Some(Vec::new()),
            _ => None,
        }
    }
    /// Turn the selected pixel node into a smart layer (or back).
    pub fn convert_smart(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        match &n.kind {
            NodeKind::Raster { .. } | NodeKind::Text { .. } | NodeKind::Path { .. } => {
                self.finish_tool_interaction(cx);
                self.close_text_field(cx);
                self.execute(Command::ConvertToSmart { id }, cx);
                if self
                    .editor
                    .doc
                    .node(id)
                    .is_some_and(|node| matches!(node.kind, NodeKind::Smart { .. }))
                {
                    self.set_status(
                        "Smart Object: editable source retained. Filters stay editable.",
                        false,
                        cx,
                    );
                }
            }
            NodeKind::Smart { .. } => {
                self.set_status("This layer is already a Smart Object.", false, cx);
            }
            _ => self.set_status(
                "Select a pixel, text, or path layer to make it smart.",
                false,
                cx,
            ),
        }
    }

    pub fn rasterize_layer(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        self.finish_tool_interaction(cx);
        self.close_text_field(cx);
        self.execute(Command::Rasterize { id }, cx);
    }

    pub fn convert_smart_to_layers(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        self.finish_tool_interaction(cx);
        self.close_text_field(cx);
        self.execute(Command::ConvertToLayers { id }, cx);
    }

    pub fn add_filter(&mut self, id: NodeId, f: Filter, cx: &mut Context<Self>) {
        let Some(mut filters) = self.requested_filters(id) else {
            return;
        };
        if filters.len() >= 32 {
            self.set_status("A smart layer supports up to 32 filters.", false, cx);
            return;
        }
        filters.push(f.clone());
        self.set_filters_async(id, filters, Some(f), cx);
        self.smart.menu_for = None;
    }

    pub fn remove_filter(&mut self, id: NodeId, idx: usize, cx: &mut Context<Self>) {
        let Some(mut filters) = self.requested_filters(id) else {
            return;
        };
        if idx < filters.len() {
            filters.remove(idx);
            self.set_filters_async(id, filters, None, cx);
        }
    }

    /// A filter slider moved. Re-rendering a stack can take a while on a
    /// big layer, so during a drag it applies at most every 120 ms.
    pub(crate) fn set_filter_param(
        &mut self,
        id: NodeId,
        idx: usize,
        key: &'static str,
        v: f32,
        final_step: bool,
        cx: &mut Context<Self>,
    ) {
        self.smart.edited_filter = Some((id, idx));
        let throttled = self
            .smart
            .last_apply
            .is_some_and(|t| t.elapsed().as_millis() < 120);
        if throttled && !final_step {
            self.smart.pending = Some((id, idx, key, v));
            return;
        }
        self.smart.pending = None;
        let Some(mut filters) = self.requested_filters(id) else {
            return;
        };
        if let Some(f) = filters.get_mut(idx)
            && f.set_param(key, v)
        {
            self.smart.last_apply = Some(Instant::now());
            let repeat = f.clone();
            self.set_filters_async(id, filters, Some(repeat), cx);
        }
    }

    /// Render the stack off the UI thread, then set filters and cache in
    /// one undoable step. A newer request supersedes an older one.
    fn set_filters_async(
        &mut self,
        id: NodeId,
        filters: Vec<Filter>,
        repeat: Option<Filter>,
        cx: &mut Context<Self>,
    ) {
        if self.editor.doc.locked_ancestor(id).is_some() {
            self.set_status("That layer or its group is locked.", true, cx);
            return;
        }
        let locks = self.editor.doc.layer_locks(id);
        if locks.pixels || locks.transparency {
            self.set_status(
                "Unlock image pixels and transparency before applying filters.",
                true,
                cx,
            );
            return;
        }
        let (source, styles, convert) = match self.editor.doc.node(id).map(|n| &n.kind) {
            Some(NodeKind::Smart {
                source,
                filters: current,
                filter_styles,
                ..
            }) => (
                source.clone(),
                aligned_filter_styles(current, filter_styles, &filters),
                false,
            ),
            Some(NodeKind::Raster { raster, .. }) => (raster.clone(), Vec::new(), true),
            _ => return,
        };
        let original = self.editor.doc.node(id).cloned();
        let history_epoch = self.history_epoch;
        self.smart.render_gen += 1;
        let generation = self.smart.render_gen;
        self.smart
            .requests
            .insert(id, (generation, filters.clone()));
        cx.spawn(async move |this, cx| {
            let f2 = filters.clone();
            let (cache, offset) = cx
                .background_spawn(async move {
                    emulsion_core::smart::render_styled(&source, &f2, &styles)
                })
                .await;
            let mut ready = Some((filters, cache, offset));
            loop {
                let done = this.update(cx, |this, cx| {
                    if this.smart.requests.get(&id).map(|r| r.0) != Some(generation) {
                        return true;
                    }
                    if this.history_epoch != history_epoch
                        || this.editor.doc.node(id) != original.as_ref()
                        || this.editor.doc.locked_ancestor(id).is_some()
                    {
                        this.smart.requests.remove(&id);
                        return true;
                    }
                    // A filter may preview inside its own slider gesture, but
                    // never join another layer's or another tool's undo step.
                    let owns_gesture = matches!(&this.drag,
                        Some(Drag::Slider { key: SliderKey::Filter(node, _, _), .. }) if *node == id);
                    if this.editor.in_transaction() && !owns_gesture {
                        return false;
                    }
                    this.smart.requests.remove(&id);
                    let (filters, cache, offset) = ready.take().expect("one filter result");
                    if convert {
                        this.editor.begin("Apply filter");
                        this.execute(Command::ConvertToSmart { id }, cx);
                    }
                    let before = this.editor.revision;
                    this.execute(Command::SetSmartCache { id, filters, cache, offset }, cx);
                    if convert {
                        this.editor.end();
                        this.after_change(cx);
                    }
                    if this.editor.revision != before && !this.editor.in_transaction()
                        && let Some(filter) = &repeat {
                        cx.set_global(super::filters::LastFilter(filter.clone()));
                    }
                    true
                }).unwrap_or(true);
                if done { break; }
                cx.background_executor().timer(std::time::Duration::from_millis(32)).await;
            }
        })
        .detach();
    }

    /// Apply a throttled value that was still pending when the drag ended.
    pub(crate) fn flush_filter_param(&mut self, cx: &mut Context<Self>) {
        if let Some((id, idx, key, v)) = self.smart.pending.take() {
            self.set_filter_param(id, idx, key, v, true, cx);
        }
    }

    /// Preview mutations belong to the drag transaction. Restore that base,
    /// then commit the final render once, even when it finishes after release.
    pub(crate) fn finish_filter_gesture(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let mut filters = self.requested_filters(id);
        if let Some((node, idx, key, value)) = self.smart.pending.take()
            && node == id
            && let Some(filter) = filters.as_mut().and_then(|f| f.get_mut(idx))
        {
            filter.set_param(key, value);
        }
        self.smart.requests.remove(&id);
        self.editor.cancel();
        self.after_change(cx);
        if let Some(filters) = filters {
            let repeat = self
                .smart
                .edited_filter
                .filter(|(node, _)| *node == id)
                .and_then(|(_, idx)| filters.get(idx).cloned());
            self.set_filters_async(id, filters, repeat, cx);
        }
    }

    /// The filter stack controls for a smart node.
    pub(crate) fn smart_panel(
        &mut self,
        id: NodeId,
        filters: &[Filter],
        styles: &[FilterStyle],
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        v.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(label("Filters", p))
                .child(div().flex_1())
                .child(
                    chip("smart-add", "+ filter", self.smart.menu_for == Some(id), p).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.smart.menu_for = if this.smart.menu_for == Some(id) {
                                None
                            } else {
                                Some(id)
                            };
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    chip("smart-raster", "rasterize", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.rasterize_layer(cx))),
                )
                .into_any_element(),
        );
        if self.smart.menu_for == Some(id) {
            let mut menu = div().flex().flex_wrap().gap(px(4.));
            for (i, f) in Filter::catalogue().into_iter().enumerate() {
                let text = f.label();
                menu = menu.child(chip(("filter-add", i), text, false, p).on_click(
                    cx.listener(move |this, _, _, cx| this.add_filter(id, f.clone(), cx)),
                ));
            }
            v.push(menu.into_any_element());
        }
        if filters.is_empty() {
            v.push(mono("no filters yet · + filter", 10., p.muted).into_any_element());
        }
        for (idx, f) in filters.iter().enumerate() {
            let style = styles.get(idx).copied().unwrap_or_default().sanitized();
            v.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pt(px(4.))
                    .child(mono(format!("{}. {}", idx + 1, f.label()), 10.5, p.ink))
                    .child(div().flex_1())
                    .child(chip(("filter-del", idx), "×", false, p).on_click(
                        cx.listener(move |this, _, _, cx| this.remove_filter(id, idx, cx)),
                    ))
                    .into_any_element(),
            );
            let editor = cx.weak_entity();
            v.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(mono("blending", 10., p.muted))
                    .child(
                        Button::new(format!("filter-blend-{id}-{idx}"))
                            .label(format!("{} ▾", style.blend.label()))
                            .small()
                            .bg(p.soft_bg)
                            .text_color(p.ink)
                            .dropdown_menu(move |mut menu, _, _| {
                                for mode in BlendMode::MENU.iter().flatten().copied() {
                                    let editor = editor.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(mode.label())
                                            .checked(mode == style.blend)
                                            .on_click(move |_, _, cx| {
                                                editor
                                                    .update(cx, |this, cx| {
                                                        this.set_filter_style(
                                                            id,
                                                            idx,
                                                            Some(mode),
                                                            None,
                                                            cx,
                                                        );
                                                    })
                                                    .ok();
                                            }),
                                    );
                                }
                                menu
                            }),
                    )
                    .child({
                        let editor = cx.weak_entity();
                        Button::new(format!("filter-opacity-{id}-{idx}"))
                            .label(format!("{:.0}% ▾", style.opacity * 100.0))
                            .small()
                            .bg(p.soft_bg)
                            .text_color(p.ink)
                            .dropdown_menu(move |mut menu, _, _| {
                                for percent in [0_u8, 25, 50, 75, 100] {
                                    let editor = editor.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(format!("{percent}%"))
                                            .checked(
                                                (style.opacity * 100.0 - percent as f32).abs()
                                                    < 0.5,
                                            )
                                            .on_click(move |_, _, cx| {
                                                editor
                                                    .update(cx, |this, cx| {
                                                        this.set_filter_style(
                                                            id,
                                                            idx,
                                                            None,
                                                            Some(percent as f32 / 100.0),
                                                            cx,
                                                        );
                                                    })
                                                    .ok();
                                            }),
                                    );
                                }
                                menu
                            })
                    })
                    .into_any_element(),
            );
            for spec in f.params() {
                let norm = (spec.value - spec.min) / (spec.max - spec.min).max(1e-6);
                let key = SliderKey::Filter(id, idx, spec.key);
                v.push(
                    self.param_slider(
                        key,
                        spec.label,
                        spec.display(),
                        norm,
                        (spec.min, spec.max, spec.step),
                        p,
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        v
    }

    fn set_filter_style(
        &mut self,
        id: NodeId,
        index: usize,
        blend: Option<BlendMode>,
        opacity: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        let Some(NodeKind::Smart {
            filters,
            filter_styles,
            ..
        }) = self.editor.doc.node(id).map(|node| &node.kind)
        else {
            return;
        };
        let mut styles = filter_styles.clone();
        styles.resize(filters.len(), FilterStyle::default());
        let Some(style) = styles.get_mut(index) else {
            return;
        };
        if let Some(blend) = blend {
            style.blend = blend;
        }
        if let Some(opacity) = opacity {
            style.opacity = opacity;
        }
        self.execute(Command::SetFilterStyles { id, styles }, cx);
    }
}

fn aligned_filter_styles(
    current: &[Filter],
    styles: &[FilterStyle],
    requested: &[Filter],
) -> Vec<FilterStyle> {
    let mut used = vec![false; current.len()];
    requested
        .iter()
        .enumerate()
        .map(|(index, filter)| {
            let matched = current
                .iter()
                .enumerate()
                .find(|(old, candidate)| !used[*old] && *candidate == filter)
                .map(|(old, _)| old);
            if let Some(old) = matched {
                used[old] = true;
                styles.get(old).copied().unwrap_or_default()
            } else {
                styles.get(index).copied().unwrap_or_default()
            }
        })
        .collect()
}
