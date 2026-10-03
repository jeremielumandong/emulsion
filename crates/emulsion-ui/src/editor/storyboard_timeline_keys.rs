//! Layer key rows on the Timeline (L3, L4; SBP 24): under the panel track,
//! each panel with layer keys gets a row that opens into one row per
//! animated layer, and each layer into one row per property (opacity and
//! effect parameters included). Diamonds mark keys: click selects (the
//! Panel inspector then edits the key's easing), drag retimes within the
//! panel, Delete removes. Each change is one Undo step.
use super::super::storyboard_keyframes::{
    KeyDrag, KeyRef, Motion, key_frames, move_keys, short_label,
};
use super::view::HEADER_W;
use super::*;
use emulsion_core::storyboard::LayerProperty;

const KEY_ROW_H: f32 = 20.;

/// One key row: which keys it shows and how it is labelled.
struct KeyRow {
    panel: PageId,
    start: u64,
    /// `None` on a panel's summary row.
    layer: Option<NodeId>,
    /// `Some` on a property row.
    property: Option<LayerProperty>,
    label: String,
    depth: usize,
    open: Option<bool>,
    frames: Vec<u64>,
}

impl EditorView {
    /// The rows, in panel order.
    fn key_rows(&self) -> Vec<KeyRow> {
        let Some(board) = self.timeline_board() else {
            return Vec::new();
        };
        let layout = self.timeline_layout();
        let names: HashMap<PageId, String> = self
            .editor
            .page_list()
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();
        let mut rows = Vec::new();
        for (panel, start) in board.panel_starts(&layout) {
            let Some(motion) = self
                .shown_motion(panel)
                .or(board.panels.get(&panel).map(|p| &p.motion))
            else {
                continue;
            };
            if motion.is_empty() {
                continue;
            }
            let open = self.layer_keys.open_panels.contains(&panel);
            let mut all: Vec<u64> = motion.values().flat_map(|l| key_frames(l, None)).collect();
            all.sort_unstable();
            all.dedup();
            rows.push(KeyRow {
                panel,
                start,
                layer: None,
                property: None,
                label: format!(
                    "{} · {} layer{}",
                    names.get(&panel).cloned().unwrap_or_default(),
                    motion.len(),
                    if motion.len() == 1 { "" } else { "s" }
                ),
                depth: 0,
                open: Some(open),
                frames: all,
            });
            if !open {
                continue;
            }
            let doc = self.editor.page(panel).map(|e| &e.doc);
            for (id, layer) in motion {
                let name = doc
                    .and_then(|d| d.node(*id))
                    .map_or_else(|| format!("Layer {id}"), |n| n.name.clone());
                let layer_open = self.layer_keys.open_layers.contains(&(panel, *id));
                rows.push(KeyRow {
                    panel,
                    start,
                    layer: Some(*id),
                    property: None,
                    label: name,
                    depth: 1,
                    open: Some(layer_open),
                    frames: key_frames(layer, None),
                });
                if layer_open {
                    for track in &layer.tracks {
                        rows.push(KeyRow {
                            panel,
                            start,
                            layer: Some(*id),
                            property: Some(track.property.clone()),
                            label: short_label(&track.property),
                            depth: 2,
                            open: None,
                            frames: track.keys.iter().map(|k| k.frame).collect(),
                        });
                    }
                }
            }
        }
        rows
    }

    /// Open or close a panel's (or a layer's) rows.
    pub(crate) fn toggle_key_row(
        &mut self,
        panel: PageId,
        layer: Option<NodeId>,
        cx: &mut Context<Self>,
    ) {
        let ui = &mut self.layer_keys;
        match layer {
            None => {
                if !ui.open_panels.remove(&panel) {
                    ui.open_panels.insert(panel);
                }
            }
            Some(layer) => {
                if !ui.open_layers.remove(&(panel, layer)) {
                    ui.open_layers.insert((panel, layer));
                }
            }
        }
        cx.notify();
    }

    /// Select keys and start retiming them from window x `x`.
    pub(crate) fn timeline_key_down(&mut self, key: KeyRef, x: Pixels, cx: &mut Context<Self>) {
        self.timeline_ui.clip = None;
        self.timeline_ui.marker = None;
        self.layer_keys.selected = Some(key.clone());
        let locked = self
            .editor
            .storyboard()
            .is_some_and(|b| b.is_locked(key.panel));
        if !locked {
            let origin = self.timeline_frame_at(x);
            let to = key.frame;
            self.layer_keys.drag = Some(KeyDrag::Timeline { key, origin, to });
        }
        cx.notify();
    }

    /// Retime the dragged keys by the pointer's travel, within the panel.
    pub(crate) fn timeline_key_move(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(KeyDrag::Timeline { key, origin, .. }) = self.layer_keys.drag.clone() else {
            return;
        };
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let Some(panel) = board.panels.get(&key.panel) else {
            return;
        };
        let last = f64::from(panel.frames.saturating_sub(1));
        let travel = self.timeline_frame_at(x) - origin;
        let to = (key.frame as f64 + travel).round().clamp(0., last) as u64;
        let mut motion: Motion = panel.motion.clone();
        if let Some(layer) = motion.get_mut(&key.layer) {
            move_keys(layer, key.property.as_ref(), key.frame, to);
        }
        self.layer_keys.pending = (to != key.frame).then_some((key.panel, motion));
        self.layer_keys.drag = Some(KeyDrag::Timeline { key, origin, to });
        self.notify_canvas(cx);
        cx.notify();
    }

    /// Land the retime as one Undo step.
    pub(crate) fn timeline_key_end(&mut self, cx: &mut Context<Self>) {
        let Some(KeyDrag::Timeline { key, to, .. }) = self.layer_keys.drag.clone() else {
            return;
        };
        self.layer_keys.drag = None;
        let Some((panel, motion)) = self.layer_keys.pending.take() else {
            cx.notify();
            return;
        };
        if self.edit_motion(
            panel,
            |m| {
                *m = motion;
                Ok(())
            },
            cx,
        ) {
            self.layer_keys.selected = Some(KeyRef { frame: to, ..key });
        }
        cx.notify();
    }

    pub(super) fn timeline_key_rows(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = self.key_rows();
        let (zoom, scroll) = (self.timeline_ui.zoom, self.timeline_ui.scroll);
        let width = self.timeline_lane_width();
        let selected = self.layer_keys.selected.clone();
        let mut out = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            let mut lane = Self::timeline_lane(("timeline-key-lane", index), KEY_ROW_H)
                .test_support()
                .border_b_1()
                .border_color(p.line)
                .bg(p.soft_bg.opacity(0.5))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, window, cx| {
                        this.layer_keys.selected = None;
                        this.timeline_begin(
                            TimelineDrag::Scrub,
                            e.position,
                            e.modifiers,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
                .on_scroll_wheel(
                    cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.timeline_wheel(e, cx)),
                );
            if index == 0 {
                // Follow key drags anywhere in the window.
                let owner = cx.weak_entity();
                lane = lane.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            let (moved, released) = (owner.clone(), owner.clone());
                            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    moved
                                        .update(cx, |this, cx| {
                                            if matches!(
                                                this.layer_keys.drag,
                                                Some(KeyDrag::Timeline { .. })
                                            ) {
                                                this.timeline_key_move(e.position.x, cx);
                                            }
                                        })
                                        .ok();
                                }
                            });
                            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Capture && e.button == MouseButton::Left
                                {
                                    released
                                        .update(cx, |this, cx| this.timeline_key_end(cx))
                                        .ok();
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                );
            }
            for frame in &row.frames {
                let x = (row.start + frame) as f32 * zoom - scroll;
                if x < -10. || x > width + 10. {
                    continue;
                }
                let lit = row.layer.is_some()
                    && selected.as_ref().is_some_and(|s| {
                        s.panel == row.panel
                            && Some(s.layer) == row.layer
                            && s.frame == *frame
                            && (s.property == row.property || s.property.is_none())
                    });
                let mut diamond = div()
                    .id(SharedString::from(format!(
                        "timeline-key-{}-{}-{}-{frame}",
                        row.panel,
                        row.layer.unwrap_or(0),
                        row.property
                            .as_ref()
                            .map_or_else(|| "all".to_string(), |p| format!("{p:?}"))
                    )))
                    .test_support()
                    .absolute()
                    .left(px(x - 6.))
                    .top(px(2.))
                    .w(px(12.))
                    .h(px(KEY_ROW_H - 4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(if row.layer.is_some() { 11. } else { 9. }))
                    .text_color(if lit {
                        p.accent
                    } else if row.layer.is_some() {
                        p.ink
                    } else {
                        p.muted
                    })
                    .child("◆");
                if let Some(layer) = row.layer {
                    let key = KeyRef {
                        panel: row.panel,
                        layer,
                        property: row.property.clone(),
                        frame: *frame,
                    };
                    diamond = diamond.cursor(CursorStyle::ResizeLeftRight).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            let focus = this.timeline_focus(cx);
                            window.focus(&focus, cx);
                            this.timeline_key_down(key.clone(), e.position.x, cx);
                            cx.stop_propagation();
                        }),
                    );
                }
                lane = lane.child(diamond);
            }
            lane = lane.children(self.timeline_playhead_line(p));
            let (panel, layer) = (row.panel, row.layer);
            let header =
                Self::timeline_header(p, KEY_ROW_H)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .pl(px(8. + row.depth as f32 * 12.))
                    .text_size(px(10.))
                    .text_color(if row.depth == 2 { p.muted } else { p.ink })
                    .children(row.open.map(|open| {
                        div()
                            .id(("timeline-key-toggle", index))
                            .test_support()
                            .cursor_pointer()
                            .child(if open { "▾" } else { "▸" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_key_row(panel, layer, cx)
                            }))
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(row.label),
                    );
            out.push(
                div()
                    .flex()
                    .flex_none()
                    .w_full()
                    .child(header.w(px(HEADER_W)))
                    .child(lane)
                    .into_any_element(),
            );
        }
        out
    }
}
