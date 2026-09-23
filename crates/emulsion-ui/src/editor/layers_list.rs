//! Variable-height Layers viewport. Retain geometry, build only visible items.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Layer,
    Effects,
    Effect { id: u64, index: usize },
    EmptySpace,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Row {
    node: NodeId,
    depth: usize,
    kind: RowKind,
}

impl Row {
    fn identity(self) -> (NodeId, u8, u64) {
        match self.kind {
            RowKind::Layer => (self.node, 0, 0),
            RowKind::Effects => (self.node, 1, 0),
            RowKind::Effect { id, .. } => (self.node, 2, id),
            RowKind::EmptySpace => (0, 3, 0),
        }
    }
}

pub(crate) struct LayerList {
    pub(crate) state: ListState,
    rows: Rc<Vec<Row>>,
    revision: u64,
    renaming: Option<(NodeId, FocusHandle)>,
    rem_size: Pixels,
    compact: bool,
    selected: Option<NodeId>,
}

impl EditorView {
    pub(super) fn layers_list(
        &mut self,
        layers: &[emulsion_core::document::PanelRow],
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // These are cheap identities, not elements or thumbnails. Resolve nodes
        // once so preparing a large layer stack does not do quadratic lookups.
        let nodes: HashMap<_, _> = self.editor.doc.nodes.iter().map(|n| (n.id, n)).collect();
        let mut rows = Vec::new();
        for layer in layers {
            let row = Row {
                node: layer.id,
                depth: layer.depth,
                kind: RowKind::Layer,
            };
            rows.push(row);
            if let Some(node) = nodes.get(&layer.id).filter(|n| !n.styles.is_empty()) {
                rows.push(Row {
                    kind: RowKind::Effects,
                    ..row
                });
                if !self.layer_panel.effects_collapsed.contains(&layer.id) {
                    for (index, option) in styles_ui::effect_options(node).iter().enumerate() {
                        rows.push(Row {
                            kind: RowKind::Effect {
                                id: option.id,
                                index,
                            },
                            ..row
                        });
                    }
                }
            }
        }
        // Keep a real trailing click target even when the list fills the dock.
        rows.push(Row {
            node: 0,
            depth: 0,
            kind: RowKind::EmptySpace,
        });
        let renaming = self
            .renaming
            .as_ref()
            .map(|(id, input, _)| (*id, input.read(cx).focus_handle(cx)));
        let compact = crate::app_state::settings(cx).compact_chrome;
        let reveal = self.layer_panel.reveal.take();
        let list = self.layer_panel.list.get_or_insert_with(|| LayerList {
            state: ListState::new(0, ListAlignment::Top, px(0.)),
            rows: Rc::new(Vec::new()),
            revision: self.editor.revision,
            renaming: None,
            rem_size: window.rem_size(),
            compact,
            selected: self.selected,
        });
        if *list.rows != rows || list.renaming != renaming {
            let offset = list.state.logical_scroll_top();
            let anchor = list.rows.get(offset.item_ix).map(|row| row.identity());
            list.state.splice_focusable(
                0..list.rows.len(),
                rows.iter().map(|row| {
                    renaming
                        .as_ref()
                        .filter(|(id, _)| *id == row.node && row.kind == RowKind::Layer)
                        .map(|(_, focus)| focus.clone())
                }),
            );
            // Estimates allow wheel scrolling across unmeasured items. Actual
            // visible heights replace them; do not eagerly measure the stack.
            list.state
                .clone()
                .with_uniform_item_height(window.rem_size() * 2.5);
            let item_ix = anchor
                .and_then(|key| rows.iter().position(|row| row.identity() == key))
                .unwrap_or(offset.item_ix.min(rows.len() - 1));
            list.state.scroll_to(ListOffset {
                item_ix,
                offset_in_item: offset.offset_in_item,
            });
            list.rows = Rc::new(rows);
            list.renaming = renaming;
        }
        if list.revision != self.editor.revision
            || list.compact != compact
            || list.rem_size != window.rem_size()
        {
            // Width changes are handled by List itself. Other content/density
            // changes must also discard offscreen height measurements.
            if list.compact != compact || list.rem_size != window.rem_size() {
                list.state.remeasure();
            } else {
                list.state.remeasure_items(0..list.rows.len());
            }
            list.revision = self.editor.revision;
            list.compact = compact;
            list.rem_size = window.rem_size();
        }
        if list.selected != self.selected || reveal.is_some() {
            list.selected = self.selected;
            if let Some(item_ix) = list.rows.iter().position(|row| {
                Some(row.node) == reveal.or(self.selected) && row.kind == RowKind::Layer
            }) {
                if list.state.bounds_for_item(item_ix).is_some() {
                    list.state.scroll_to_reveal_item(item_ix);
                } else {
                    // A distant unmeasured row has no pixel offset yet.
                    list.state.scroll_to(ListOffset {
                        item_ix,
                        offset_in_item: px(0.),
                    });
                }
            }
        }
        let rows = list.rows.clone();
        let state = list.state.clone();
        let editor = cx.entity().downgrade();
        let palette = *p;
        gpui_kit::list(state, move |index, _, cx| {
            let row = rows[index];
            editor
                .update(cx, |this, cx| {
                    let content = match row.kind {
                        RowKind::Layer => this
                            .node_row(row.node, row.depth, &palette, cx)
                            .into_any_element(),
                        RowKind::Effects => this
                            .layer_effect_header(row.node, row.depth, &palette, cx)
                            .unwrap_or_else(|| div().into_any_element()),
                        RowKind::Effect { index, .. } => this
                            .layer_effect_row(row.node, row.depth, index, &palette, cx)
                            .unwrap_or_else(|| div().into_any_element()),
                        RowKind::EmptySpace => return div().h(px(10.)).into_any_element(),
                    };
                    // Match the former gap between layers and effect groups;
                    // individual effects within a group remain contiguous.
                    let gap = matches!(row.kind, RowKind::Layer)
                        || rows.get(index + 1).is_none_or(|next| {
                            matches!(next.kind, RowKind::Layer | RowKind::EmptySpace)
                        });
                    div()
                        .w_full()
                        .when(gap, |d| d.pb(px(2.)))
                        .child(content)
                        .into_any_element()
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .w_full()
        .h_full()
    }
}
