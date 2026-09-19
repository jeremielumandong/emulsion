//! Smart layers in the panel: convert a pixel node, add and tune filters.

use super::*;
use emulsion_filters::Filter;

#[derive(Default)]
pub(crate) struct SmartUi {
    /// The add-filter menu is open for this node.
    pub menu_for: Option<NodeId>,
    /// Filter slider edits apply at most this often while dragging.
    last_apply: Option<Instant>,
    pending: Option<(NodeId, usize, &'static str, f32)>,
}

impl EditorView {
    /// Turn the selected pixel node into a smart layer (or back).
    pub fn convert_smart(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        match &n.kind {
            NodeKind::Raster { .. } => {
                self.execute(Command::ConvertToSmart { id }, cx);
                self.set_status(
                    "Smart layer: filters stay editable. Painting goes on a new layer.",
                    false,
                    cx,
                );
            }
            NodeKind::Smart { .. } => {
                self.execute(Command::Rasterize { id }, cx);
                self.set_status("Rasterized: the filters are baked in.", false, cx);
            }
            _ => self.set_status("Select a pixel layer to make it smart.", false, cx),
        }
    }

    pub fn add_filter(&mut self, id: NodeId, f: Filter, cx: &mut Context<Self>) {
        let Some(NodeKind::Smart { filters, .. }) = self.editor.doc.node(id).map(|n| &n.kind)
        else {
            return;
        };
        let mut filters = filters.clone();
        filters.push(f);
        self.execute(Command::SetFilters { id, filters }, cx);
        self.smart.menu_for = None;
    }

    pub fn remove_filter(&mut self, id: NodeId, idx: usize, cx: &mut Context<Self>) {
        let Some(NodeKind::Smart { filters, .. }) = self.editor.doc.node(id).map(|n| &n.kind)
        else {
            return;
        };
        let mut filters = filters.clone();
        if idx < filters.len() {
            filters.remove(idx);
            self.execute(Command::SetFilters { id, filters }, cx);
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
        let throttled = self
            .smart
            .last_apply
            .is_some_and(|t| t.elapsed().as_millis() < 120);
        if throttled && !final_step {
            self.smart.pending = Some((id, idx, key, v));
            return;
        }
        self.smart.pending = None;
        let Some(NodeKind::Smart { filters, .. }) = self.editor.doc.node(id).map(|n| &n.kind)
        else {
            return;
        };
        let mut filters = filters.clone();
        if let Some(f) = filters.get_mut(idx)
            && f.set_param(key, v)
        {
            self.smart.last_apply = Some(Instant::now());
            self.execute(Command::SetFilters { id, filters }, cx);
        }
    }

    /// Apply a throttled value that was still pending when the drag ended.
    pub(crate) fn flush_filter_param(&mut self, cx: &mut Context<Self>) {
        if let Some((id, idx, key, v)) = self.smart.pending.take() {
            self.set_filter_param(id, idx, key, v, true, cx);
        }
    }

    /// The filter stack controls for a smart node.
    pub(crate) fn smart_panel(
        &mut self,
        id: NodeId,
        filters: &[Filter],
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
                        .on_click(cx.listener(|this, _, _, cx| this.convert_smart(cx))),
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
}
