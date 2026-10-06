//! Smart layers in the panel: convert a pixel node, add and tune filters.

use super::*;
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::{BlendMode, Mask};
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

#[derive(Clone)]
struct FilterMaskCapture {
    selection: Option<Arc<Mask>>,
    source_to_document: glam::DAffine2,
    baseline: Node,
    expected: Node,
    history_epoch: u64,
    operation_epoch: u64,
}

impl FilterMaskCapture {
    fn matches_node(node: Option<&Node>, expected: &Node) -> bool {
        let Some(node) = node else { return false };
        if node != expected
            || emulsion_core::smart_support::MappingKey::from(node.mask_transform)
                != emulsion_core::smart_support::MappingKey::from(expected.mask_transform)
        {
            return false;
        }
        // Node equality intentionally omits derived caches. Capture ownership
        // also rejects an independently replaced cache or its placement offset.
        match (&node.kind, &expected.kind) {
            (
                NodeKind::Smart {
                    cache: actual,
                    offset: a,
                    placement: actual_placement,
                    filter_mask: actual_mask,
                    ..
                },
                NodeKind::Smart {
                    cache: expected,
                    offset: b,
                    placement: expected_placement,
                    filter_mask: expected_mask,
                    ..
                },
            ) => {
                Arc::ptr_eq(actual, expected)
                    && a == b
                    && emulsion_core::smart_support::SmartPlacementKey::from(*actual_placement)
                        == emulsion_core::smart_support::SmartPlacementKey::from(
                            *expected_placement,
                        )
                    && actual_mask
                        .as_ref()
                        .map(|mask| emulsion_core::smart_support::MappingKey::from(mask.transform))
                        == expected_mask.as_ref().map(|mask| {
                            emulsion_core::smart_support::MappingKey::from(mask.transform)
                        })
            }
            _ => true,
        }
    }

    fn is_current(&self, node: Option<&Node>, history_epoch: u64, operation_epoch: u64) -> bool {
        Self::matches_node(node, &self.expected)
            && self.history_epoch == history_epoch
            && self.operation_epoch == operation_epoch
    }
}

struct FilterRequest {
    generation: u64,
    filters: Vec<Filter>,
    styles: Vec<FilterStyle>,
    enabled: bool,
    operation_epoch: u64,
    initial_mask: Option<FilterMaskCapture>,
}

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
    requests: HashMap<NodeId, FilterRequest>,
    /// Initial selection survives only this filter gesture's provisional cache.
    /// Its exact last-published node and tickets bound the rollback handoff.
    gesture_initial_mask: Option<FilterMaskCapture>,
    edited_filter: Option<(NodeId, usize)>,
    #[cfg(test)]
    render_barrier: Option<(async_channel::Sender<()>, async_channel::Receiver<()>)>,
    #[cfg(test)]
    pub(crate) apply_capture_barrier:
        Option<(async_channel::Sender<()>, async_channel::Receiver<()>)>,
}

impl SmartUi {
    /// Hold only this editor's next completed render before publication. Dropping
    /// the release sender resumes it, including when its test fixture unwinds.
    #[cfg(test)]
    pub(crate) fn pause_next_render(
        &mut self,
    ) -> (async_channel::Receiver<()>, async_channel::Sender<()>) {
        let (ready, ready_rx) = async_channel::bounded(1);
        let (release, release_rx) = async_channel::bounded(1);
        assert!(self.render_barrier.is_none());
        self.render_barrier = Some((ready, release_rx));
        (ready_rx, release)
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.requests.is_empty() || self.pending.is_some()
    }

    pub(super) fn cancel_pending(&mut self) {
        self.requests.clear();
        self.pending = None;
        self.gesture_initial_mask = None;
    }

    pub(super) fn carry_current_requests(&mut self, before: u64, after: u64) {
        // Carry only current filter-owned work across its own publications or
        // selection-only edits. Other async jobs still retire normally.
        for request in self.requests.values_mut() {
            if request.operation_epoch == before {
                request.operation_epoch = after;
                if let Some(capture) = &mut request.initial_mask
                    && capture.operation_epoch == before
                {
                    capture.operation_epoch = after;
                }
            }
        }
        if let Some(capture) = &mut self.gesture_initial_mask
            && capture.operation_epoch == before
        {
            capture.operation_epoch = after;
        }
    }

    fn clear_gesture_initial_mask(&mut self, id: NodeId) {
        if self
            .gesture_initial_mask
            .as_ref()
            .is_some_and(|capture| capture.baseline.id == id)
        {
            self.gesture_initial_mask = None;
        }
    }
}

impl EditorView {
    fn current_filter_initial_mask(&self, id: NodeId) -> Option<FilterMaskCapture> {
        let request = self.smart.requests.get(&id);
        if request.is_some_and(|request| {
            request.operation_epoch != self.operation_epoch || request.filters.is_empty()
        }) {
            return None;
        }
        request
            .and_then(|request| request.initial_mask.as_ref())
            .or(self
                .smart
                .gesture_initial_mask
                .as_ref()
                .filter(|_| self.editor.in_transaction()))
            .filter(|capture| {
                capture.is_current(
                    self.editor.doc.node(id),
                    self.history_epoch,
                    self.operation_epoch,
                )
            })
            .cloned()
    }

    pub(super) fn requested_stack(
        &self,
        id: NodeId,
    ) -> Option<(Vec<Filter>, Vec<FilterStyle>, bool)> {
        let (mut filters, styles, enabled) = if let Some(request) = self.smart.requests.get(&id) {
            (
                request.filters.clone(),
                request.styles.clone(),
                request.enabled,
            )
        } else {
            match &self.editor.doc.node(id)?.kind {
                NodeKind::Smart {
                    filters,
                    filter_styles,
                    filters_enabled,
                    ..
                } => {
                    let mut styles = filter_styles.clone();
                    styles.resize(filters.len(), FilterStyle::default());
                    (filters.clone(), styles, *filters_enabled)
                }
                NodeKind::Raster { .. } => (Vec::new(), Vec::new(), true),
                _ => return None,
            }
        };
        // Capture the newest throttled value before replacing or reindexing a
        // request. Pending intent belongs to its original item, not its next row.
        if let Some((node, index, key, value)) = self.smart.pending
            && node == id
            && let Some(filter) = filters.get_mut(index)
        {
            filter.set_param(key, value);
        }
        Some((filters, styles, enabled))
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
        if self.refuse_projective_tool("Rasterize", cx) {
            return;
        }
        let Some(id) = self.selected else { return };
        self.finish_tool_interaction(cx);
        self.close_text_field(cx);
        self.execute(Command::Rasterize { id }, cx);
    }

    pub fn convert_smart_to_layers(&mut self, cx: &mut Context<Self>) {
        if self.refuse_projective_tool("Convert Smart to layers", cx) {
            return;
        }
        let Some(id) = self.selected else { return };
        self.finish_tool_interaction(cx);
        self.close_text_field(cx);
        self.execute(Command::ConvertToLayers { id }, cx);
    }

    pub fn add_filter(&mut self, id: NodeId, f: Filter, cx: &mut Context<Self>) {
        let Some((mut filters, mut styles, enabled)) = self.requested_stack(id) else {
            return;
        };
        if filters.len() >= 32 {
            self.set_status(t!("editor.smart.max_filters"), false, cx);
            return;
        }
        filters.push(f.clone());
        styles.push(FilterStyle::default());
        self.set_filters_async(
            id,
            filters,
            styles,
            enabled,
            Some(f),
            "Add filter",
            false,
            None,
            cx,
        );
        self.smart.menu_for = None;
    }

    /// Append several filters as one edit (Enhance looks).
    pub(crate) fn add_filters(&mut self, id: NodeId, add: Vec<Filter>, cx: &mut Context<Self>) {
        let Some((mut filters, mut styles, enabled)) = self.requested_stack(id) else {
            return;
        };
        if filters.len() + add.len() > 32 {
            self.set_status(t!("editor.smart.max_filters"), false, cx);
            return;
        }
        let last = add.last().cloned();
        filters.extend(add);
        styles.resize(filters.len(), FilterStyle::default());
        self.set_filters_async(
            id,
            filters,
            styles,
            enabled,
            last,
            "Add filters",
            false,
            None,
            cx,
        );
        self.smart.menu_for = None;
    }

    pub fn remove_filter(&mut self, id: NodeId, idx: usize, cx: &mut Context<Self>) {
        let Some((mut filters, mut styles, enabled)) = self.requested_stack(id) else {
            return;
        };
        if idx < filters.len() {
            filters.remove(idx);
            styles.remove(idx);
            self.set_filters_async(
                id,
                filters,
                styles,
                enabled,
                None,
                "Remove filter",
                false,
                None,
                cx,
            );
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
        let Some((mut filters, styles, enabled)) = self.requested_stack(id) else {
            return;
        };
        if let Some(f) = filters.get_mut(idx)
            && f.set_param(key, v)
        {
            self.smart.last_apply = Some(Instant::now());
            let repeat = f.clone();
            self.set_filters_async(
                id,
                filters,
                styles,
                enabled,
                Some(repeat),
                "Filter parameters",
                true,
                None,
                cx,
            );
        }
    }

    /// Render the stack off the UI thread, then set filters and cache in
    /// one undoable step. A newer request supersedes an older one.
    #[allow(clippy::too_many_arguments)] // Complete immutable render/publication request.
    fn set_filters_async(
        &mut self,
        id: NodeId,
        filters: Vec<Filter>,
        styles: Vec<FilterStyle>,
        filters_enabled: bool,
        repeat: Option<Filter>,
        label: &'static str,
        allow_filter_preview: bool,
        initial_mask: Option<FilterMaskCapture>,
        cx: &mut Context<Self>,
    ) {
        if self.editor.is_read_only() || !self.photo_transform_ready(cx) || self.editor.in_preview()
        {
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
        if let Some(node) = self
            .editor
            .doc
            .node(id)
            .filter(|node| node.has_projective_metadata())
        {
            let result = (|| {
                if let NodeKind::Smart {
                    filters: old,
                    filter_mask,
                    ..
                } = &node.kind
                    && old.is_empty()
                    && filter_mask.is_none()
                    && !filters.is_empty()
                {
                    node.require_affine_capability("Initialize filter mask from selection")?;
                }
                let mut metadata = emulsion_core::smart_support::metadata_for_node(node)?;
                metadata.filters = &filters;
                metadata.styles = &styles;
                metadata.filters_enabled = filters_enabled;
                emulsion_core::smart_support::preflight_stack_support(metadata)?;
                Ok::<_, emulsion_core::GeometryError>(())
            })();
            if let Err(error) = result {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        }
        let (source, convert) = match self.editor.doc.node(id).map(|n| &n.kind) {
            Some(NodeKind::Smart { source, .. }) => (source.clone(), false),
            Some(NodeKind::Raster { raster, .. }) => (raster.clone(), true),
            _ => return,
        };
        if filters.is_empty() {
            self.smart.clear_gesture_initial_mask(id);
        }
        if convert && filters.is_empty() {
            self.smart.requests.remove(&id);
            return;
        }
        // A same-state setter neither retires existing work nor destroys redo.
        let unchanged = if let Some(request) = self.smart.requests.get(&id) {
            request.filters == filters
                && request.styles == styles
                && request.enabled == filters_enabled
        } else {
            matches!(&self.editor.doc.node(id).unwrap().kind,
                NodeKind::Smart { filters: current, filter_styles, filters_enabled: enabled, .. }
                if current == &filters && *enabled == filters_enabled
                    && filter_styles.iter().copied().chain(std::iter::repeat(FilterStyle::default())).take(filters.len()).eq(styles.iter().copied()))
        };
        if unchanged {
            // This complete request can supersede an older throttled value
            // even when the worker already has the desired final descriptors.
            if self.smart.pending.is_some_and(|(node, _, _, _)| node == id) {
                self.smart.pending = None;
            }
            return;
        }
        if self.smart.pending.is_some_and(|(node, _, _, _)| node == id) {
            self.smart.pending = None;
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
        // The first initialization owns its selection and source placement.
        // Replacing an unpublished generation must not silently recapture them.
        let mut initial_mask = if initialize_mask {
            let captured = initial_mask
                .filter(|capture| {
                    capture.is_current(original.as_ref(), self.history_epoch, self.operation_epoch)
                })
                .or_else(|| self.current_filter_initial_mask(id));
            match captured {
                Some(capture) => Some(capture),
                None => {
                    let Some(baseline) = original.clone() else {
                        return;
                    };
                    let source_to_document = match super::transform::affine_tool_mapping(&baseline)
                    {
                        Ok(mapping) => mapping,
                        Err(error) => {
                            self.set_status(error.to_string(), true, cx);
                            return;
                        }
                    };
                    Some(FilterMaskCapture {
                        selection: self.editor.doc.selection.clone(),
                        source_to_document,
                        expected: baseline.clone(),
                        baseline,
                        history_epoch: self.history_epoch,
                        operation_epoch: self.operation_epoch,
                    })
                }
            }
        } else {
            None
        };
        // Retire older source/RAW/MCP jobs without clearing the filter request
        // or cancelling its own slider transaction.
        self.cancel_raw_develop();
        let previous_epoch = self.operation_epoch;
        self.operation_epoch = self.operation_epoch.wrapping_add(1);
        self.smart
            .carry_current_requests(previous_epoch, self.operation_epoch);
        if let Some(capture) = &mut initial_mask {
            capture.operation_epoch = self.operation_epoch;
        }
        let history_epoch = self.history_epoch;
        self.smart.render_gen = self.smart.render_gen.wrapping_add(1);
        let generation = self.smart.render_gen;
        self.smart.requests.insert(
            id,
            FilterRequest {
                generation,
                filters: filters.clone(),
                styles: styles.clone(),
                enabled: filters_enabled,
                operation_epoch: self.operation_epoch,
                initial_mask: initial_mask.clone(),
            },
        );
        cx.notify();
        #[cfg(test)]
        let render_barrier = self.smart.render_barrier.take();
        cx.spawn(async move |this, cx| {
            let f2 = filters.clone();
            let render_styles = styles.clone();
            let prepared = cx.background_spawn(async move {
                let (cache, offset) = emulsion_core::smart::render_stack(&source, &f2, &render_styles, filters_enabled);
                let mask = if let Some(capture) = initial_mask {
                    Some(smart_filter_mask_ui::initial_mask(cache.width(), cache.height(), offset,
                        capture.source_to_document, capture.selection.as_deref(), false)?)
                } else { None };
                Ok::<_, &'static str>((cache, offset, mask))
            }).await;
            #[cfg(test)]
            if let Some((ready, release)) = render_barrier {
                let _ = ready.try_send(());
                let _ = release.recv().await;
            }
            let mut ready = Some((filters, styles, prepared));
            loop {
                let done = this.update(cx, |this, cx| {
                    if this.smart.requests.get(&id).map(|request| request.generation) != Some(generation) {
                        return true;
                    }
                    let locks = this.editor.doc.layer_locks(id);
                    if this.smart.requests.get(&id).is_none_or(|request| request.operation_epoch != this.operation_epoch)
                        || this.history_epoch != history_epoch
                        || !original.as_ref().is_some_and(|expected| FilterMaskCapture::matches_node(this.editor.doc.node(id), expected))
                        || this.editor.doc.locked_ancestor(id).is_some()
                        || locks.pixels || locks.transparency
                    {
                        this.smart.requests.remove(&id);
                        this.smart.clear_gesture_initial_mask(id);
                        cx.notify();
                        return true;
                    }
                    // A filter may preview inside its own slider gesture, but
                    // never join another layer's or another tool's undo step.
                    let owns_gesture = allow_filter_preview && matches!(&this.drag,
                        Some(Drag::Slider { key: SliderKey::Filter(node, _, _), .. }) if *node == id);
                    if this.editor.in_transaction() && !owns_gesture {
                        return false;
                    }
                    let gesture_capture = owns_gesture.then(|| this.current_filter_initial_mask(id)).flatten();
                    this.smart.requests.remove(&id);
                    let (filters, styles, prepared) = ready.take().expect("one filter result");
                    let (cache, offset, mask) = match prepared {
                        Ok(result) => result,
                        Err(error) => { this.set_status(error, true, cx); return true; }
                    };
                    let mut commands = Vec::new();
                    if convert { commands.push(Command::ConvertToSmart { id }); }
                    commands.push(Command::SetSmartCache { id, filters, styles, filters_enabled, cache, offset });
                    if let Some(mask) = mask {
                        commands.push(Command::SetSmartFilterMask { id, mask: Some(mask) });
                    }
                    let before = this.editor.revision;
                    // Whole sequence is trial-applied before publication. A late
                    // size/lock failure cannot leave conversion or an orphan mask.
                    let previous_epoch = this.operation_epoch;
                    this.execute_layer_commands(label, commands, cx);
                    // after_change advances the epoch for this synchronous
                    // publication. Carry sibling filter requests forward; do
                    // not grant this exemption to a later external edit job.
                    this.smart.carry_current_requests(previous_epoch, this.operation_epoch);
                    if owns_gesture && this.editor.in_transaction() && this.editor.revision != before
                        && let Some(mut capture) = gesture_capture {
                        capture.expected = this.editor.doc.node(id).unwrap().clone();
                        capture.operation_epoch = this.operation_epoch;
                        this.smart.gesture_initial_mask = Some(capture);
                    } else if !this.editor.in_transaction() {
                        this.smart.clear_gesture_initial_mask(id);
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
        let stack = self.requested_stack(id);
        let (mut filters, styles, enabled) = match stack {
            Some(stack) => stack,
            None => {
                self.smart.clear_gesture_initial_mask(id);
                return;
            }
        };
        if let Some((node, idx, key, value)) = self.smart.pending.take()
            && node == id
            && let Some(filter) = filters.get_mut(idx)
        {
            filter.set_param(key, value);
        }
        // A completed preview no longer has a request. Its gesture-owned record
        // must be checked before rollback, then against the restored baseline.
        let initial_mask = self.current_filter_initial_mask(id);
        self.smart.clear_gesture_initial_mask(id);
        self.smart.requests.remove(&id);
        self.editor.cancel();
        let previous_epoch = self.operation_epoch;
        self.after_change(cx);
        // Gesture rollback advances the epoch just like publication. Preserve
        // still-current sibling filter requests that waited for this gesture,
        // without reviving requests already retired by a newer external job.
        self.smart
            .carry_current_requests(previous_epoch, self.operation_epoch);
        let initial_mask = initial_mask.and_then(|mut capture| {
            if !FilterMaskCapture::matches_node(self.editor.doc.node(id), &capture.baseline)
                || self.history_epoch != capture.history_epoch
            {
                return None;
            }
            capture.expected = capture.baseline.clone();
            capture.operation_epoch = self.operation_epoch;
            Some(capture)
        });
        let repeat = self
            .smart
            .edited_filter
            .filter(|(node, _)| *node == id)
            .and_then(|(_, idx)| filters.get(idx).cloned());
        self.set_filters_async(
            id,
            filters,
            styles,
            enabled,
            repeat,
            "Filter parameters",
            false,
            initial_mask,
            cx,
        );
    }

    /// The filter stack controls for a smart node.
    pub(crate) fn smart_panel(
        &mut self,
        id: NodeId,
        filters: &[Filter],
        styles: &[FilterStyle],
        filters_enabled: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let (filters, styles, filters_enabled) = self
            .requested_stack(id)
            .unwrap_or_else(|| (filters.to_vec(), styles.to_vec(), filters_enabled));
        let mut v: Vec<AnyElement> = Vec::new();
        v.push(self.smart_source_controls(id, p, cx));
        v.push(self.smart_filter_mask_controls(id, p, cx));
        v.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(label(t!("editor.smart.filters"), p))
                .child(
                    chip(
                        ("smart-filters-enabled", id),
                        if filters_enabled {
                            t!("editor.smart.enabled")
                        } else {
                            t!("editor.smart.disabled")
                        },
                        filters_enabled,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_filters_enabled(id, !filters_enabled, cx)
                    }))
                    .test_support(),
                )
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
                    }))
                    .test_support(),
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
                let text = super::filters::filter_label(&f);
                menu = menu.child(
                    chip(("filter-add", i), text, false, p)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.add_filter(id, f.clone(), cx);
                            if this.smart.menu_for.is_none() {
                                // The focused catalogue chip is about to unmount.
                                // Keep shortcuts in the live panel scope now;
                                // render completion must not reclaim later input.
                                window.focus(&this.panel_focus, cx);
                            }
                        }))
                        .test_support(),
                );
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
                    .child(mono(
                        format!("{}. {}", idx + 1, super::filters::filter_label(f)),
                        10.5,
                        p.ink,
                    ))
                    .child(div().flex_1())
                    .child(
                        chip(
                            ("filter-enabled", idx),
                            if style.enabled {
                                t!("editor.smart.enabled")
                            } else {
                                t!("editor.smart.disabled")
                            },
                            style.enabled,
                            p,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_filter_enabled(id, idx, !style.enabled, cx)
                        }))
                        .test_support(),
                    )
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

    pub(super) fn set_filter_style(
        &mut self,
        id: NodeId,
        index: usize,
        blend: Option<BlendMode>,
        opacity: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        let Some((filters, mut styles, enabled)) = self.requested_stack(id) else {
            return;
        };
        let Some(style) = styles.get_mut(index) else {
            return;
        };
        if let Some(blend) = blend {
            style.blend = blend;
        }
        if let Some(opacity) = opacity {
            style.opacity = opacity;
        }
        self.set_filters_async(
            id,
            filters,
            styles,
            enabled,
            None,
            "Filter blending options",
            false,
            None,
            cx,
        );
    }
    pub(crate) fn set_filters_enabled(
        &mut self,
        id: NodeId,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((filters, styles, current)) = self.requested_stack(id) else {
            return;
        };
        if current == enabled {
            return;
        }
        self.set_filters_async(
            id,
            filters,
            styles,
            enabled,
            None,
            if enabled {
                "Enable Smart Filters"
            } else {
                "Disable Smart Filters"
            },
            false,
            None,
            cx,
        );
    }

    pub(crate) fn set_filter_enabled(
        &mut self,
        id: NodeId,
        index: usize,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((filters, mut styles, root_enabled)) = self.requested_stack(id) else {
            return;
        };
        let Some(style) = styles.get_mut(index) else {
            return;
        };
        if style.enabled == enabled {
            return;
        }
        style.enabled = enabled;
        self.set_filters_async(
            id,
            filters,
            styles,
            root_enabled,
            None,
            if enabled {
                "Enable filter"
            } else {
                "Disable filter"
            },
            false,
            None,
            cx,
        );
    }

    pub(super) fn cancel_filter_edits(&mut self, cx: &mut Context<Self>) -> bool {
        let owns_gesture = matches!(
            self.drag,
            Some(Drag::Slider {
                key: SliderKey::Filter(..),
                ..
            })
        );
        if !owns_gesture && !self.smart.has_pending() {
            return false;
        }
        self.smart.cancel_pending();
        self.smart.edited_filter = None;
        self.operation_epoch = self.operation_epoch.wrapping_add(1);
        if owns_gesture {
            self.drag = None;
            self.editor.cancel();
            self.after_change(cx);
        } else {
            cx.notify();
        }
        true
    }
}

#[cfg(test)]
#[path = "smart_initial_mask_tests.rs"]
mod initial_mask_tests;
