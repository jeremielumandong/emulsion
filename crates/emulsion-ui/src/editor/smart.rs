//! Smart layers in the panel: convert a pixel node, add and tune filters.

use super::*;
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::BlendMode;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

#[derive(Default)]
pub(crate) struct SmartUi {
    pub(crate) source_session: Option<super::smart_source_ui::SourceSession>,
    pub(crate) source_watch_started: bool,
    pub(crate) source_watch_error: Option<String>,
    pub(crate) source_watch_facts: HashMap<NodeId, super::smart_source_ui::WatchFact>,
    pub(crate) source_watch_cursor: usize,
    /// The add-filter menu is open for this node.
    pub menu_for: Option<NodeId>,
    /// Filter slider edits apply at most this often while dragging.
    last_apply: Option<Instant>,
    pending: Option<(NodeId, usize, &'static str, f32)>,
    /// Bumped per request so stale renders are dropped.
    render_gen: u64,
    requests: HashMap<NodeId, (u64, Vec<Filter>, Vec<FilterStyle>)>,
    edited_filter: Option<(NodeId, usize)>,
}

impl SmartUi {
    pub(super) fn has_pending(&self) -> bool {
        !self.requests.is_empty() || self.pending.is_some()
    }

    pub(super) fn cancel_pending(&mut self) {
        self.requests.clear();
        self.pending = None;
    }
}

impl EditorView {
    fn requested_stack(&self, id: NodeId) -> Option<(Vec<Filter>, Vec<FilterStyle>)> {
        if let Some((_, filters, styles)) = self.smart.requests.get(&id) {
            return Some((filters.clone(), styles.clone()));
        }
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Smart {
                filters,
                filter_styles,
                ..
            } => {
                let mut styles = filter_styles.clone();
                styles.resize(filters.len(), FilterStyle::default());
                styles.truncate(filters.len());
                Some((filters.clone(), styles))
            }
            NodeKind::Raster { .. } => Some((Vec::new(), Vec::new())),
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
                    self.set_status(t!("editor.smart.converted"), false, cx);
                }
            }
            NodeKind::Smart { .. } => {
                self.set_status(t!("editor.smart.already"), false, cx);
            }
            _ => self.set_status(t!("editor.smart.select_layer"), false, cx),
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
        let Some((mut filters, mut styles)) = self.requested_stack(id) else {
            return;
        };
        if filters.len() >= 32 {
            self.set_status(t!("editor.smart.max_filters"), false, cx);
            return;
        }
        filters.push(f.clone());
        styles.push(FilterStyle::default());
        self.set_filters_async(id, filters, styles, Some(f), cx);
        self.smart.menu_for = None;
    }

    /// Append several filters as one edit (Enhance looks).
    pub(crate) fn add_filters(&mut self, id: NodeId, add: Vec<Filter>, cx: &mut Context<Self>) {
        let Some((mut filters, mut styles)) = self.requested_stack(id) else {
            return;
        };
        if filters.len() + add.len() > 32 {
            self.set_status(t!("editor.smart.max_filters"), false, cx);
            return;
        }
        let last = add.last().cloned();
        filters.extend(add);
        styles.resize(filters.len(), FilterStyle::default());
        self.set_filters_async(id, filters, styles, last, cx);
        self.smart.menu_for = None;
    }

    pub fn remove_filter(&mut self, id: NodeId, idx: usize, cx: &mut Context<Self>) {
        let Some((mut filters, mut styles)) = self.requested_stack(id) else {
            return;
        };
        if idx < filters.len() {
            filters.remove(idx);
            styles.remove(idx);
            self.set_filters_async(id, filters, styles, None, cx);
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
        let Some((mut filters, styles)) = self.requested_stack(id) else {
            return;
        };
        if let Some(f) = filters.get_mut(idx)
            && f.set_param(key, v)
        {
            self.smart.last_apply = Some(Instant::now());
            let repeat = f.clone();
            self.set_filters_async(id, filters, styles, Some(repeat), cx);
        }
    }

    /// Render the stack off the UI thread, then set filters and cache in
    /// one undoable step. A newer request supersedes an older one.
    fn set_filters_async(
        &mut self,
        id: NodeId,
        filters: Vec<Filter>,
        styles: Vec<FilterStyle>,
        repeat: Option<Filter>,
        cx: &mut Context<Self>,
    ) {
        if !self.photo_transform_ready(cx) || self.editor.in_preview() {
            return;
        }
        if self.editor.doc.locked_ancestor(id).is_some() {
            self.set_status(t!("editor.smart.locked"), true, cx);
            return;
        }
        let locks = self.editor.doc.layer_locks(id);
        if locks.pixels || locks.transparency {
            self.set_status(t!("editor.smart.unlock_pixels"), true, cx);
            return;
        }
        let (source, convert) = match self.editor.doc.node(id).map(|n| &n.kind) {
            Some(NodeKind::Smart { source, .. }) => (source.clone(), false),
            Some(NodeKind::Raster { raster, .. }) => (raster.clone(), true),
            _ => return,
        };
        if convert && filters.is_empty() {
            self.smart.requests.remove(&id);
            return;
        }
        let original = self.editor.doc.node(id).cloned();
        let initialize_mask = !filters.is_empty()
            && original.as_ref().is_some_and(|node| match &node.kind {
                NodeKind::Raster { .. } => true,
                NodeKind::Smart {
                    filters,
                    filter_mask,
                    ..
                } => filters.is_empty() && filter_mask.is_none(),
                _ => false,
            });
        let selection = self.editor.doc.selection.clone();
        let source_to_document = original
            .as_ref()
            .map(emulsion_core::transform::local_to_document)
            .unwrap_or(glam::DAffine2::IDENTITY);
        let history_epoch = self.history_epoch;
        self.smart.render_gen += 1;
        let generation = self.smart.render_gen;
        self.smart
            .requests
            .insert(id, (generation, filters.clone(), styles.clone()));
        cx.spawn(async move |this, cx| {
            let f2 = filters.clone();
            let render_styles = styles.clone();
            let prepared = cx.background_spawn(async move {
                let (cache, offset) = emulsion_core::smart::render_styled(&source, &f2, &render_styles);
                let mask = if initialize_mask {
                    Some(smart_filter_mask_ui::initial_mask(cache.width(), cache.height(), offset,
                        source_to_document, selection.as_deref(), false)?)
                } else { None };
                Ok::<_, &'static str>((cache, offset, mask))
            }).await;
            let mut ready = Some((filters, styles, prepared));
            loop {
                let done = this.update(cx, |this, cx| {
                    if this.smart.requests.get(&id).map(|r| r.0) != Some(generation) {
                        return true;
                    }
                    let locks = this.editor.doc.layer_locks(id);
                    if this.history_epoch != history_epoch
                        || this.editor.doc.node(id) != original.as_ref()
                        || this.editor.doc.locked_ancestor(id).is_some()
                        || locks.pixels || locks.transparency
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
                    let (filters, styles, prepared) = ready.take().expect("one filter result");
                    let (cache, offset, mask) = match prepared {
                        Ok(result) => result,
                        Err(error) => { this.set_status(error, true, cx); return true; }
                    };
                    let mut commands = Vec::new();
                    if convert { commands.push(Command::ConvertToSmart { id }); }
                    commands.push(Command::SetSmartCache { id, filters, styles, cache, offset });
                    if let Some(mask) = mask {
                        commands.push(Command::SetSmartFilterMask { id, mask: Some(mask) });
                    }
                    let before = this.editor.revision;
                    // Whole sequence is trial-applied before publication. A late
                    // size/lock failure cannot leave conversion or an orphan mask.
                    this.execute_layer_commands("Apply filter", commands, cx);
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
        let stack = self.requested_stack(id);
        let (mut filters, styles) = match stack {
            Some(stack) => stack,
            None => return,
        };
        if let Some((node, idx, key, value)) = self.smart.pending.take()
            && node == id
            && let Some(filter) = filters.get_mut(idx)
        {
            filter.set_param(key, value);
        }
        self.smart.requests.remove(&id);
        self.editor.cancel();
        self.after_change(cx);
        let repeat = self
            .smart
            .edited_filter
            .filter(|(node, _)| *node == id)
            .and_then(|(_, idx)| filters.get(idx).cloned());
        self.set_filters_async(id, filters, styles, repeat, cx);
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
        v.push(self.smart_source_controls(id, p, cx));
        v.push(self.smart_filter_mask_controls(id, p, cx));
        v.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(label(t!("editor.smart.filters"), p))
                .child(div().flex_1())
                .child(
                    chip(
                        "smart-add",
                        t!("editor.smart.add_filter"),
                        self.smart.menu_for == Some(id),
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.smart.menu_for = if this.smart.menu_for == Some(id) {
                            None
                        } else {
                            Some(id)
                        };
                        cx.notify();
                    })),
                )
                .child(
                    chip("smart-raster", t!("editor.smart.rasterize"), false, p)
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
            v.push(mono(t!("editor.smart.no_filters"), 10., p.muted).into_any_element());
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
                    .child(
                        chip(("filter-del", idx), "×", false, p)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.remove_filter(id, idx, cx)),
                            )
                            .test_support(),
                    )
                    .into_any_element(),
            );
            let editor = cx.weak_entity();
            v.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(mono(t!("editor.smart.blending"), 10., p.muted))
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
        if !self.layer_menu_ready() || !self.photo_transform_ready(cx) {
            return;
        }
        self.smart.requests.remove(&id);
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
