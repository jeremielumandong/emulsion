//! Layer keyframes on the Stage (L3, L5, L9): Set key and auto-key for the
//! selected layer at the playhead, the animated picture at the playhead, the
//! motion path with its key handles, the pivot, effect parameter keys and
//! the panel inspector's Layer animation section.
//!
//! Keys live on the board (`Panel.motion`, edited through `edit_board`, one
//! Undo step each). The layer's own placement is its rest pose; keys hold
//! the offsets, so the Stage draws `Storyboard::animate_panel` at the
//! playhead. While a panel animates, the Move tool's box and pointer follow
//! the animated layer: its drags map back through the layer's transform at
//! the playhead (frozen for the drag), and with auto-key on, a finished drag
//! becomes keys instead of a change to the layer.
use super::*;
use emulsion_core::motion::{Curve, Easing};
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{
    KeyframeSync, LayerMotion, LayerProperty, MotionKey, PropertyTrack,
};
use glam::{DAffine2, DVec2, dvec2};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::collections::{BTreeMap, HashSet};

#[cfg(test)]
#[path = "storyboard_keyframes_tests.rs"]
mod tests;

/// A panel's layer keyframes, by layer.
pub(crate) type Motion = BTreeMap<NodeId, LayerMotion>;

/// Document polylines and handle points for the Stage overlay.
type PathOverlay = (Vec<Vec<(f64, f64)>>, Vec<(f64, f64)>);

/// Grab distance for Stage handles, in screen pixels.
const HANDLE_PX: f64 = 7.;

/// Keys picked on the timeline or the Stage: one property's key, or every
/// key the layer has at that frame (`property` is `None`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeyRef {
    pub(crate) panel: PageId,
    pub(crate) layer: NodeId,
    pub(crate) property: Option<LayerProperty>,
    pub(crate) frame: u64,
}

/// A drag on keys in progress.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum KeyDrag {
    /// Diamonds on the timeline, retimed by the pointer's travel from
    /// `origin` (the frame under the pointer when the drag began).
    Timeline { key: KeyRef, origin: f64, to: u64 },
    /// A motion path key point on the Stage.
    Path {
        panel: PageId,
        layer: NodeId,
        frame: u64,
    },
    /// The pivot on the Stage.
    Pivot { panel: PageId, layer: NodeId },
}

/// The selected layer's animated transform for one Move or Transform drag.
#[derive(Clone, Copy, Debug)]
struct Gesture {
    panel: PageId,
    layer: NodeId,
    frame: u64,
    matrix: DAffine2,
    pivot: DVec2,
    start_bounds: Option<emulsion_raster::IRect>,
}

#[derive(Default)]
pub(crate) struct LayerKeysUi {
    /// Move and Transform drags record keys at the playhead.
    pub(crate) auto_key: bool,
    pub(crate) selected: Option<KeyRef>,
    /// Keys being dragged, shown until release.
    pub(crate) pending: Option<(PageId, Motion)>,
    pub(crate) drag: Option<KeyDrag>,
    /// Timeline panels whose layer rows are open, and layers whose
    /// property rows are open.
    pub(crate) open_panels: HashSet<PageId>,
    pub(crate) open_layers: HashSet<(PageId, NodeId)>,
    /// Fixed when the canvas is pressed, for the Move or Transform drag it
    /// may start: the layer's rest pose changes during the drag.
    gesture: Option<Gesture>,
    armed: bool,
    /// What the Stage last drew animated: panel, frame and keys.
    shown: Option<(PageId, u64, Motion)>,
    pub(crate) curves: super::storyboard_curve_editor::CurveUi,
}

// ── Pure key edits ─────────────────────────────────────────────────────

/// Put a key of `value` at `frame` on `property`, replacing one there.
pub(crate) fn put_key(layer: &mut LayerMotion, property: &LayerProperty, frame: u64, value: f64) {
    let index = match layer.tracks.iter().position(|t| &t.property == property) {
        Some(index) => index,
        None => {
            layer.tracks.push(PropertyTrack {
                property: property.clone(),
                keys: Vec::new(),
            });
            layer.tracks.len() - 1
        }
    };
    let keys = &mut layer.tracks[index].keys;
    match keys.binary_search_by_key(&frame, |k| k.frame) {
        Ok(i) => keys[i].value = value,
        Err(i) => keys.insert(
            i,
            MotionKey {
                frame,
                value,
                easing: Easing::Linear,
                curve: None,
            },
        ),
    }
}

/// Tracks `property` picks: that one, or all of them.
fn picks(track: &PropertyTrack, property: Option<&LayerProperty>) -> bool {
    property.is_none_or(|p| &track.property == p)
}

/// Remove the keys at `frame` (of `property`, or of every property), and
/// tracks left without keys.
pub(crate) fn delete_keys(layer: &mut LayerMotion, property: Option<&LayerProperty>, frame: u64) {
    for track in &mut layer.tracks {
        if picks(track, property) {
            track.keys.retain(|k| k.frame != frame);
        }
    }
    layer.tracks.retain(|t| !t.keys.is_empty());
}

/// Move the keys at `from` to `to`, replacing keys already there.
pub(crate) fn move_keys(
    layer: &mut LayerMotion,
    property: Option<&LayerProperty>,
    from: u64,
    to: u64,
) {
    for track in &mut layer.tracks {
        if !picks(track, property) {
            continue;
        }
        let Some(i) = track.keys.iter().position(|k| k.frame == from) else {
            continue;
        };
        let mut key = track.keys.remove(i);
        key.frame = to;
        track.keys.retain(|k| k.frame != to);
        let at = track.keys.partition_point(|k| k.frame < to);
        track.keys.insert(at, key);
    }
}

/// Set how the segments starting at `frame` ease.
pub(crate) fn set_ease(
    layer: &mut LayerMotion,
    property: Option<&LayerProperty>,
    frame: u64,
    easing: Easing,
    curve: Option<Curve>,
) {
    for track in &mut layer.tracks {
        if picks(track, property)
            && let Some(key) = track.keys.iter_mut().find(|k| k.frame == frame)
        {
            key.easing = easing;
            key.curve = curve;
        }
    }
}

/// Whether `property` has a key at `frame`.
pub(crate) fn has_key(layer: &LayerMotion, property: &LayerProperty, frame: u64) -> bool {
    layer
        .track(property)
        .is_some_and(|t| t.keys.iter().any(|k| k.frame == frame))
}

/// Frames holding keys, of `property` or of every property, in order.
pub(crate) fn key_frames(layer: &LayerMotion, property: Option<&LayerProperty>) -> Vec<u64> {
    let mut frames: Vec<u64> = layer
        .tracks
        .iter()
        .filter(|t| picks(t, property))
        .flat_map(|t| t.keys.iter().map(|k| k.frame))
        .collect();
    frames.sort_unstable();
    frames.dedup();
    frames
}

/// The motion values that make `matrix` about `pivot`: offset, rotation,
/// scale and horizontal skew, keeping the vertical skew `skew_y`. The
/// rotation is the turn nearest `near`, so keys never spin the long way.
pub(crate) fn decompose(
    matrix: DAffine2,
    pivot: DVec2,
    skew_y: f64,
    near: f64,
) -> [(LayerProperty, f64); 6] {
    let offset = matrix.transform_point2(pivot) - pivot;
    let (c0, c1) = (matrix.matrix2.x_axis, matrix.matrix2.y_axis);
    let ky = skew_y.to_radians();
    let theta = c0.y.atan2(c0.x) - ky;
    let mut rotation = theta.to_degrees();
    rotation += ((near - rotation) / 360.).round() * 360.;
    let scale_x = c0.length() * ky.cos();
    let u = DAffine2::from_angle(-theta).transform_vector2(c1);
    let scale_y = u.y;
    let skew_x = if scale_y.abs() > 1e-9 {
        (u.x / scale_y).atan().to_degrees().clamp(-80., 80.)
    } else {
        0.
    };
    [
        (LayerProperty::X, offset.x),
        (LayerProperty::Y, offset.y),
        (LayerProperty::Rotation, rotation),
        (LayerProperty::ScaleX, scale_x),
        (LayerProperty::ScaleY, scale_y),
        (LayerProperty::SkewX, skew_x),
    ]
}

/// Key the properties `values` changes at `frame`.
pub(crate) fn key_changes(layer: &mut LayerMotion, frame: u64, values: &[(LayerProperty, f64)]) {
    for (property, value) in values {
        let current = layer.value(property, frame as f64);
        if (current - value).abs() > 1e-6 {
            put_key(layer, property, frame, *value);
        }
    }
}

/// A short readout of a property's value.
pub(crate) fn value_text(property: &LayerProperty, value: f64) -> String {
    match property {
        LayerProperty::ScaleX | LayerProperty::ScaleY | LayerProperty::Opacity => {
            format!("{:.0}%", value * 100.)
        }
        LayerProperty::Rotation | LayerProperty::SkewX | LayerProperty::SkewY => {
            format!("{value:.1}°")
        }
        _ => format!("{value:.1}"),
    }
}

/// A property's name without its unit.
pub(crate) fn short_label(property: &LayerProperty) -> String {
    match property {
        LayerProperty::X => "X".into(),
        LayerProperty::Y => "Y".into(),
        LayerProperty::ScaleX => "Scale X".into(),
        LayerProperty::ScaleY => "Scale Y".into(),
        LayerProperty::Rotation => "Rotation".into(),
        LayerProperty::SkewX => "Skew X".into(),
        LayerProperty::SkewY => "Skew Y".into(),
        LayerProperty::Opacity => "Opacity".into(),
        LayerProperty::Effect(key) => key.replace('_', " "),
    }
}

fn centre(b: emulsion_raster::IRect) -> DVec2 {
    dvec2(
        f64::from(b.x) + f64::from(b.w) / 2.,
        f64::from(b.y) + f64::from(b.h) / 2.,
    )
}

impl EditorView {
    // ── Where keys go ──

    /// The active panel and the frame into it at the playhead (its first
    /// frame when the playhead is on another panel).
    pub(crate) fn key_frame(&self) -> Option<(PageId, u64)> {
        let board = self.editor.storyboard()?;
        let panel = self.editor.active_page();
        let data = board.panels.get(&panel)?;
        let layout: Vec<_> = self.editor.page_list().iter().map(|m| m.id).collect();
        let local = board
            .animatic_frame(&layout, self.transport.frame)
            .filter(|at| at.panel == panel)
            .map_or(0, |at| at.local);
        Some((panel, local.min(u64::from(data.frames.saturating_sub(1)))))
    }

    /// `panel`'s keys as shown: the dragged ones while a drag runs.
    pub(crate) fn shown_motion(&self, panel: PageId) -> Option<&Motion> {
        if let Some((id, motion)) = &self.layer_keys.pending
            && *id == panel
        {
            return Some(motion);
        }
        self.editor
            .storyboard()?
            .panels
            .get(&panel)
            .map(|p| &p.motion)
    }

    /// The Stage draws the active panel animated at the playhead.
    pub(crate) fn layer_motion_shown(&self) -> bool {
        !self.board_open()
            && self
                .key_frame()
                .and_then(|(panel, _)| self.shown_motion(panel))
                .is_some_and(|m| !m.is_empty())
    }

    /// The active panel as it looks at the playhead, keys applied.
    pub(crate) fn layer_motion_doc(&self) -> Option<Document> {
        if !self.layer_motion_shown() {
            return None;
        }
        let (panel, frame) = self.key_frame()?;
        let board = self.editor.storyboard()?;
        let result = match &self.layer_keys.pending {
            Some((id, motion)) if *id == panel => {
                let mut board = board.clone();
                board.panels.get_mut(&panel)?.motion = motion.clone();
                board.animate_panel(panel, &self.editor.doc, frame as f64)
            }
            _ => board.animate_panel(panel, &self.editor.doc, frame as f64),
        };
        result.ok()
    }

    /// Rebuild the Stage's picture when the playhead, the panel or its keys
    /// change what it shows.
    pub(crate) fn sync_layer_motion_view(&mut self, cx: &mut Context<Self>) {
        let shown = if self.layer_motion_shown() {
            self.key_frame().and_then(|(panel, frame)| {
                self.shown_motion(panel)
                    .map(|motion| (panel, frame, motion.clone()))
            })
        } else {
            None
        };
        if shown != self.layer_keys.shown {
            self.layer_keys.shown = shown;
            self.seen_rev = u64::MAX;
            self.tree_dirty = emulsion_core::Dirty::All;
            self.notify_canvas(cx);
        }
    }

    /// Change `panel`'s keys as one Undo step. Layers left with no keys and
    /// no pivot drop out.
    pub(crate) fn edit_motion(
        &mut self,
        panel: PageId,
        edit: impl FnOnce(&mut Motion) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let done = self.edit_board(
            |board| {
                let p = board
                    .panels
                    .get_mut(&panel)
                    .ok_or("Panel does not exist.")?;
                edit(&mut p.motion)?;
                p.motion
                    .retain(|_, layer| !layer.tracks.is_empty() || layer.pivot.is_some());
                Ok(())
            },
            cx,
        );
        self.notify_canvas(cx);
        done
    }

    fn refuse_locked_keys(&mut self, panel: PageId, cx: &mut Context<Self>) -> bool {
        if self.editor.storyboard().is_some_and(|b| b.is_locked(panel)) {
            self.set_status("That panel is locked. Unlock it to change it.", true, cx);
            return true;
        }
        false
    }

    // ── Set key and auto-key ──

    /// Key the selected layers' current animated position, scale, rotation,
    /// skew and opacity at the playhead, as one Undo step.
    pub(crate) fn set_layer_key(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((panel, frame)) = self.key_frame() else {
            return false;
        };
        let ids = self.selected_layer_ids();
        if ids.is_empty() {
            self.set_status("Select a layer to key.", false, cx);
            return false;
        }
        if self.refuse_locked_keys(panel, cx) {
            return false;
        }
        let done = self.edit_motion(
            panel,
            |motion| {
                for id in ids {
                    let layer = motion.entry(id).or_default();
                    for property in &LayerProperty::TRANSFORMS {
                        let value = layer.value(property, frame as f64);
                        put_key(layer, property, frame, value);
                    }
                }
                Ok(())
            },
            cx,
        );
        if done {
            self.set_status(format!("Key set at frame {frame}."), false, cx);
        }
        done
    }

    pub(crate) fn toggle_auto_key(&mut self, cx: &mut Context<Self>) {
        self.layer_keys.auto_key = !self.layer_keys.auto_key;
        self.set_status(
            if self.layer_keys.auto_key {
                "Auto-key on: moving, scaling and turning a layer sets keys at the playhead."
            } else {
                "Auto-key off: moving a layer moves its rest pose."
            },
            false,
            cx,
        );
        self.notify_canvas(cx);
        cx.notify();
    }

    /// Key or unkey `property` of `layer` at the playhead: a key there is
    /// removed, otherwise the current value is keyed.
    pub(crate) fn toggle_property_key(
        &mut self,
        layer: NodeId,
        property: LayerProperty,
        value: Option<f64>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((panel, frame)) = self.key_frame() else {
            return false;
        };
        if self.refuse_locked_keys(panel, cx) {
            return false;
        }
        self.edit_motion(
            panel,
            |motion| {
                let entry = motion.entry(layer).or_default();
                if has_key(entry, &property, frame) {
                    delete_keys(entry, Some(&property), frame);
                } else {
                    let value = value.unwrap_or_else(|| entry.value(&property, frame as f64));
                    put_key(entry, &property, frame, value);
                }
                Ok(())
            },
            cx,
        )
    }

    /// The key toggle beside an adjustment layer's parameter slider on a
    /// storyboard panel: keys the parameter's value at the playhead (L9).
    pub(crate) fn effect_key_toggle(
        &self,
        id: NodeId,
        key: &'static str,
        value: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (panel, frame) = self.key_frame()?;
        let property = LayerProperty::Effect(key.to_string());
        let layer = self.shown_motion(panel).and_then(|m| m.get(&id));
        let keyed = layer.is_some_and(|l| has_key(l, &property, frame));
        let animated = layer.is_some_and(|l| l.track(&property).is_some());
        let locked = self.editor.storyboard().is_some_and(|b| b.is_locked(panel));
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    Button::new(SharedString::from(format!("effect-key-{key}")))
                        .label(if keyed { "◆ Key" } else { "◇ Key" })
                        .tooltip(if keyed {
                            "Remove this parameter's key at the playhead"
                        } else {
                            "Key this value at the playhead"
                        })
                        .xsmall()
                        .ghost()
                        .disabled(locked)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_property_key(
                                id,
                                LayerProperty::Effect(key.to_string()),
                                Some(f64::from(value)),
                                cx,
                            );
                        })),
                )
                .when(animated, |d| {
                    d.child(mono(
                        format!(
                            "animated · {:.2} at frame {frame}",
                            layer.map_or(0., |l| l.value(&property, frame as f64))
                        ),
                        10.,
                        p.muted,
                    ))
                })
                .into_any_element(),
        )
    }

    // ── The Move tool on animated layers ──

    fn compute_gesture(&self) -> Option<Gesture> {
        if self.board_open() || self.tool != Tool::Move || self.selected_layer_ids().len() != 1 {
            return None;
        }
        let id = self.selected?;
        let (panel, frame) = self.key_frame()?;
        let layer = self.shown_motion(panel).and_then(|m| m.get(&id));
        if layer.is_none() && !self.layer_keys.auto_key {
            return None;
        }
        let layer = layer.cloned().unwrap_or_default();
        let start_bounds = emulsion_core::geometry::node_bounds(&self.editor.doc, id);
        let pivot = match layer.pivot {
            Some([x, y]) => dvec2(x, y),
            None => centre(start_bounds?),
        };
        Some(Gesture {
            panel,
            layer: id,
            frame,
            matrix: layer.transform(frame as f64, pivot),
            pivot,
            start_bounds,
        })
    }

    /// Fix the selected layer's animated transform for a press on the
    /// canvas, which may start a Move or Transform drag.
    pub(crate) fn arm_layer_gesture(&mut self) {
        self.layer_keys.gesture = self.compute_gesture();
        self.layer_keys.armed = true;
    }

    fn motion_gesture(&self) -> Option<Gesture> {
        if self.layer_keys.armed {
            return self.layer_keys.gesture;
        }
        self.compute_gesture()
    }

    /// The selected layer's animated transform at the playhead, which the
    /// Move tool's box and drags follow. Fixed from press to release.
    pub(crate) fn layer_motion_matrix(&self) -> DAffine2 {
        self.motion_gesture()
            .map_or(DAffine2::IDENTITY, |g| g.matrix)
    }

    /// A document point mapped back from the animated layer to its rest
    /// pose, for Move tool drags.
    pub(crate) fn layer_motion_point(&self, d: (f64, f64)) -> (f64, f64) {
        let m = self.layer_motion_matrix();
        if m == DAffine2::IDENTITY {
            return d;
        }
        let p = m.inverse().transform_point2(dvec2(d.0, d.1));
        (p.x, p.y)
    }

    /// With auto-key on, a finished Move or Transform drag of the selected
    /// layer becomes keys at the playhead: the layer goes back to its rest
    /// pose and the keys hold the change, as one Undo step. Returns whether
    /// it did (the drag is then over).
    pub(crate) fn key_layer_gesture(&mut self, cx: &mut Context<Self>) -> bool {
        let armed = std::mem::take(&mut self.layer_keys.armed);
        let gesture = self.layer_keys.gesture.take().filter(|_| armed);
        let grab = match &self.drag {
            Some(Drag::Move(_)) => None,
            Some(Drag::Transform(g)) => Some(*g),
            _ => return false,
        };
        let Some(g) = gesture else {
            return false;
        };
        if !self.layer_keys.auto_key || !self.editor.in_transaction() {
            return false;
        }
        // Text frames reflow from their edges: that edits the text itself.
        if let Some(grab) = grab
            && matches!(grab.handle, super::transform::Handle::Edge(_))
            && matches!(
                self.editor.doc.node(g.layer).map(|n| &n.kind),
                Some(NodeKind::Text { .. })
            )
        {
            return false;
        }
        // The change the drag made to the layer's rest pose.
        let delta = match grab {
            None => {
                let end = emulsion_core::geometry::node_bounds(&self.editor.doc, g.layer);
                match (g.start_bounds, end) {
                    (Some(a), Some(b)) => DAffine2::from_translation(dvec2(
                        f64::from(b.x - a.x),
                        f64::from(b.y - a.y),
                    )),
                    _ => return false,
                }
            }
            Some(grab) => {
                let Some((_, w, h, end)) = self.transformable() else {
                    return false;
                };
                end.to_doc(w, h) * grab.start.to_doc(grab.size.0, grab.size.1).inverse()
            }
        };
        self.drag = None;
        self.editor.cancel();
        self.after_change(cx);
        if delta.abs_diff_eq(DAffine2::IDENTITY, 1e-9) {
            return true;
        }
        let target = g.matrix * delta;
        let frame = g.frame;
        self.edit_motion(
            g.panel,
            |motion| {
                let layer = motion.entry(g.layer).or_default();
                let skew_y = layer.value(&LayerProperty::SkewY, frame as f64);
                let near = layer.value(&LayerProperty::Rotation, frame as f64);
                let values = decompose(target, g.pivot, skew_y, near);
                key_changes(layer, frame, &values);
                Ok(())
            },
            cx,
        );
        true
    }

    // ── Motion path and pivot on the Stage ──

    /// The selected layer's keys on the active panel, when the Stage shows
    /// its path and pivot.
    fn path_layer(&self) -> Option<(PageId, NodeId, u64, LayerMotion, DVec2)> {
        if self.board_open() || self.camera_view() || self.tool != Tool::Move {
            return None;
        }
        if self.selected_layer_ids().len() != 1 {
            return None;
        }
        let id = self.selected?;
        let (panel, frame) = self.key_frame()?;
        let layer = self.shown_motion(panel).and_then(|m| m.get(&id)).cloned();
        if layer.is_none() && !self.layer_keys.auto_key {
            return None;
        }
        let layer = layer.unwrap_or_default();
        let pivot = match layer.pivot {
            Some([x, y]) => dvec2(x, y),
            None => centre(emulsion_core::geometry::node_bounds(&self.editor.doc, id)?),
        };
        Some((panel, id, frame, layer, pivot))
    }

    /// Where the pivot sits at `frame`: the pivot moved by the offset keys.
    fn path_point(layer: &LayerMotion, pivot: DVec2, frame: f64) -> DVec2 {
        pivot
            + dvec2(
                layer.value(&LayerProperty::X, frame),
                layer.value(&LayerProperty::Y, frame),
            )
    }

    /// Motion path key points: (frame, document point).
    fn path_handles(layer: &LayerMotion, pivot: DVec2) -> Vec<(u64, DVec2)> {
        let mut frames = key_frames(layer, Some(&LayerProperty::X));
        frames.extend(key_frames(layer, Some(&LayerProperty::Y)));
        frames.sort_unstable();
        frames.dedup();
        frames
            .into_iter()
            .map(|f| (f, Self::path_point(layer, pivot, f as f64)))
            .collect()
    }

    /// The motion path (one point per frame of the panel) and the pivot's
    /// cross, as document polylines, with the key points as handles.
    pub(crate) fn motion_path_overlay(&self) -> PathOverlay {
        let Some((panel, _, frame, layer, pivot)) = self.path_layer() else {
            return (Vec::new(), Vec::new());
        };
        let frames = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&panel))
            .map_or(1, |p| p.frames.max(1));
        let mut lines = Vec::new();
        let handles: Vec<(f64, f64)> = Self::path_handles(&layer, pivot)
            .into_iter()
            .map(|(_, p)| (p.x, p.y))
            .collect();
        if !handles.is_empty() {
            lines.push(
                (0..frames)
                    .map(|f| {
                        let p = Self::path_point(&layer, pivot, f64::from(f));
                        (p.x, p.y)
                    })
                    .collect(),
            );
        }
        // The pivot as a cross where it is now.
        let at = Self::path_point(&layer, pivot, frame as f64);
        let r = 9. / self.view.zoom.max(1e-6);
        lines.push(vec![(at.x - r, at.y), (at.x + r, at.y)]);
        lines.push(vec![(at.x, at.y - r), (at.x, at.y + r)]);
        (lines, handles)
    }

    /// Start dragging a motion path key point, or the pivot (Alt drags the
    /// pivot even over a key point). Returns whether the press was taken.
    pub(crate) fn motion_handle_down(
        &mut self,
        e: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((panel, layer, frame, motion, pivot)) = self.path_layer() else {
            return false;
        };
        let Some(b) = self.canvas_bounds() else {
            return false;
        };
        let (sx, sy) = (
            f64::from(f32::from(e.position.x)),
            f64::from(f32::from(e.position.y)),
        );
        let near = |p: DVec2| {
            let s = self.view.doc_to_screen((p.x, p.y), &b);
            (s.0 - sx).hypot(s.1 - sy) <= HANDLE_PX
        };
        let key = Self::path_handles(&motion, pivot)
            .into_iter()
            .find(|(_, p)| near(*p))
            .map(|(f, _)| f);
        let on_pivot = near(Self::path_point(&motion, pivot, frame as f64));
        let drag = match key {
            Some(f) if !(e.modifiers.alt && on_pivot) => KeyDrag::Path {
                panel,
                layer,
                frame: f,
            },
            _ if on_pivot => KeyDrag::Pivot { panel, layer },
            _ => return false,
        };
        if self.refuse_locked_keys(panel, cx) {
            return true;
        }
        if let KeyDrag::Path { frame, .. } = &drag {
            self.layer_keys.selected = Some(KeyRef {
                panel,
                layer,
                property: None,
                frame: *frame,
            });
        }
        let current = self.shown_motion(panel).cloned().unwrap_or_default();
        self.layer_keys.pending = Some((panel, current));
        self.layer_keys.drag = Some(drag);
        cx.notify();
        true
    }

    /// Follow the pointer with the dragged path key or pivot.
    pub(crate) fn motion_drag_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let drag = match &self.layer_keys.drag {
            Some(d @ (KeyDrag::Path { .. } | KeyDrag::Pivot { .. })) => d.clone(),
            _ => return false,
        };
        let Some(d) = self.doc_point(pos) else {
            return true;
        };
        let Some(base_pivot) = self.path_layer().map(|(.., pivot)| pivot) else {
            return true;
        };
        let current = self.key_frame().map_or(0, |(_, f)| f);
        let Some((_, motion)) = &mut self.layer_keys.pending else {
            return true;
        };
        match drag {
            KeyDrag::Path { layer, frame, .. } => {
                let entry = motion.entry(layer).or_default();
                let pivot = entry.pivot.map_or(base_pivot, |[x, y]| dvec2(x, y));
                put_key(entry, &LayerProperty::X, frame, (d.0 - pivot.x).round());
                put_key(entry, &LayerProperty::Y, frame, (d.1 - pivot.y).round());
            }
            KeyDrag::Pivot { layer, .. } => {
                let entry = motion.entry(layer).or_default();
                let offset = dvec2(
                    entry.value(&LayerProperty::X, current as f64),
                    entry.value(&LayerProperty::Y, current as f64),
                );
                entry.pivot = Some([(d.0 - offset.x).round(), (d.1 - offset.y).round()]);
            }
            KeyDrag::Timeline { .. } => {}
        }
        self.notify_canvas(cx);
        cx.notify();
        true
    }

    /// Land a Stage key drag as one Undo step.
    pub(crate) fn motion_drag_end(&mut self, cx: &mut Context<Self>) -> bool {
        if !matches!(
            self.layer_keys.drag,
            Some(KeyDrag::Path { .. } | KeyDrag::Pivot { .. })
        ) {
            return false;
        }
        self.layer_keys.drag = None;
        if let Some((panel, motion)) = self.layer_keys.pending.take() {
            self.edit_motion(
                panel,
                |m| {
                    *m = motion;
                    Ok(())
                },
                cx,
            );
        }
        cx.notify();
        true
    }

    // ── Selected keys ──

    /// Remove the selected keys, one Undo step.
    pub(crate) fn delete_selected_key(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(key) = self.layer_keys.selected.clone() else {
            return false;
        };
        if self.refuse_locked_keys(key.panel, cx) {
            return true;
        }
        let done = self.edit_motion(
            key.panel,
            |motion| {
                let layer = motion
                    .get_mut(&key.layer)
                    .ok_or("That key no longer exists.")?;
                delete_keys(layer, key.property.as_ref(), key.frame);
                Ok(())
            },
            cx,
        );
        if done {
            self.layer_keys.selected = None;
        }
        true
    }

    /// The selected keys' easing, from the first of them.
    fn selected_ease(&self) -> Option<(KeyRef, Easing, Option<Curve>)> {
        let key = self.layer_keys.selected.clone()?;
        let layer = self.shown_motion(key.panel)?.get(&key.layer)?;
        let found = layer
            .tracks
            .iter()
            .filter(|t| picks(t, key.property.as_ref()))
            .find_map(|t| t.keys.iter().find(|k| k.frame == key.frame))?;
        Some((key, found.easing, found.curve))
    }

    /// Ease the segments after the selected keys, one Undo step.
    pub(crate) fn ease_selected_key(
        &mut self,
        easing: Easing,
        curve: Option<Curve>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(key) = self.layer_keys.selected.clone() else {
            return false;
        };
        if self.refuse_locked_keys(key.panel, cx) {
            return false;
        }
        self.edit_motion(
            key.panel,
            |motion| {
                let layer = motion
                    .get_mut(&key.layer)
                    .ok_or("That key no longer exists.")?;
                set_ease(layer, key.property.as_ref(), key.frame, easing, curve);
                Ok(())
            },
            cx,
        )
    }

    /// Choose what keys do when panel durations change, one Undo step.
    pub(crate) fn set_keyframe_sync(&mut self, sync: KeyframeSync, cx: &mut Context<Self>) -> bool {
        self.edit_board(
            |b| {
                b.keyframe_sync = sync;
                Ok(())
            },
            cx,
        )
    }

    /// Put the pivot back at the layer's centre, one Undo step.
    fn reset_pivot(&mut self, layer: NodeId, cx: &mut Context<Self>) {
        let Some((panel, _)) = self.key_frame() else {
            return;
        };
        if self.refuse_locked_keys(panel, cx) {
            return;
        }
        self.edit_motion(
            panel,
            |motion| {
                if let Some(l) = motion.get_mut(&layer) {
                    l.pivot = None;
                }
                Ok(())
            },
            cx,
        );
    }

    /// Type exact values for the selected layer at the playhead; changed
    /// values become keys (one Undo step).
    pub(crate) fn key_values_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((panel, frame)) = self.key_frame() else {
            return;
        };
        let Some(id) = self.selected else {
            self.set_status("Select a layer to key.", false, cx);
            return;
        };
        let layer = self
            .shown_motion(panel)
            .and_then(|m| m.get(&id))
            .cloned()
            .unwrap_or_default();
        let fields: Rc<Vec<(LayerProperty, Entity<InputState>)>> = Rc::new(
            LayerProperty::TRANSFORMS
                .iter()
                .map(|property| {
                    let value = layer.value(property, frame as f64);
                    let input =
                        cx.new(|cx| InputState::new(window, cx).default_value(format!("{value}")));
                    (property.clone(), input)
                })
                .collect(),
        );
        let error = Rc::new(RefCell::new(String::new()));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let mut body = div().flex().flex_col().gap_1();
            for (index, (property, input)) in fields.iter().enumerate() {
                body = body.child(property.label()).child(
                    Input::new(input)
                        .small()
                        .id(SharedString::from(format!("key-value-{index}"))),
                );
            }
            body = body
                .child(format!(
                    "Values at frame {frame} of this panel. Changed values become keys; scale and opacity are fractions (1 is 100%)."
                ))
                .child(error.borrow().clone());
            let (fields, owner, error) = (fields.clone(), owner.clone(), error.clone());
            dialog
                .title("Key values")
                .width(px(360.))
                .child(body)
                .footer(crate::widgets::form_dialog_footer("Set keys"))
                .on_ok(move |_, _, cx| {
                    let mut values = Vec::new();
                    for (property, input) in fields.iter() {
                        match input.read(cx).value().trim().parse::<f64>() {
                            Ok(v) if v.is_finite() => values.push((property.clone(), v)),
                            _ => {
                                *error.borrow_mut() =
                                    format!("Enter a number for {}.", property.label());
                                cx.refresh_windows();
                                return false;
                            }
                        }
                    }
                    let result = owner
                        .update(cx, |this, cx| {
                            if this.refuse_locked_keys(panel, cx) {
                                return Err("That panel is locked.".to_string());
                            }
                            match this.editor.edit_storyboard(|b| {
                                let p = b.panels.get_mut(&panel).ok_or("Panel does not exist.")?;
                                key_changes(p.motion.entry(id).or_default(), frame, &values);
                                p.motion.retain(|_, l| !l.tracks.is_empty() || l.pivot.is_some());
                                Ok(())
                            }) {
                                Ok(()) => {
                                    this.after_change(cx);
                                    this.notify_canvas(cx);
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            }
                        })
                        .unwrap_or_else(|_| Err("The editor closed.".into()));
                    match result {
                        Ok(()) => true,
                        Err(e) => {
                            *error.borrow_mut() = e;
                            cx.refresh_windows();
                            false
                        }
                    }
                })
        });
    }

    // ── Controls ──

    /// Key and Auto-key on the Stage toolbar.
    pub(crate) fn stage_key_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.editor.storyboard().is_none() || self.board_open() {
            return Vec::new();
        }
        let auto = self.layer_keys.auto_key;
        vec![
            div()
                .flex()
                .gap_1()
                .child(
            Button::new("stage-set-key")
                .label("◆ Key")
                .tooltip("Key the selected layer's position, scale, rotation, skew and opacity at the playhead")
                .xsmall()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.set_layer_key(cx);
                })),
                )
                .child(
            Button::new("stage-auto-key")
                .label("Auto-key")
                .tooltip("Moving, scaling and turning the selected layer sets keys at the playhead")
                .xsmall()
                .when(auto, |b| b.primary())
                .when(!auto, |b| b.ghost())
                .on_click(cx.listener(|this, _, _, cx| this.toggle_auto_key(cx))),
                )
                .into_any_element(),
        ]
    }

    /// The Panel inspector's Layer animation section: Set key, Auto-key,
    /// the selected layer's values with a key toggle each, the pivot, the
    /// selected key's easing and the board's keyframe sync.
    pub(crate) fn layer_animation_section(
        &mut self,
        panel: PageId,
        locked: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = self.key_frame().map_or(0, |(_, f)| f);
        let sync = self
            .editor
            .storyboard()
            .map_or(KeyframeSync::Scale, |b| b.keyframe_sync);
        let auto = self.layer_keys.auto_key;
        let mut root = div()
            .id("storyboard-layer-animation")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("Layer animation", p))
                    .child(div().flex_1())
                    .child(mono(format!("frame {frame}"), 10., p.muted)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .child(
                        Button::new("storyboard-set-key")
                            .label("◆ Set key")
                            .tooltip("Key the selected layer at the playhead")
                            .small()
                            .outline()
                            .disabled(locked)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_layer_key(cx);
                            })),
                    )
                    .child(
                        Button::new("storyboard-auto-key")
                            .label("Auto-key")
                            .tooltip("Moving, scaling and turning the selected layer sets keys")
                            .small()
                            .when(auto, |b| b.primary())
                            .when(!auto, |b| b.outline())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_auto_key(cx))),
                    )
                    .child(
                        Button::new("storyboard-key-values")
                            .label("Values…")
                            .tooltip("Type exact values to key at the playhead")
                            .small()
                            .ghost()
                            .disabled(locked)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.key_values_dialog(window, cx)
                            })),
                    ),
            );
        if let Some(id) = self
            .selected
            .filter(|id| self.editor.doc.node(*id).is_some())
        {
            let layer = self
                .shown_motion(panel)
                .and_then(|m| m.get(&id))
                .cloned()
                .unwrap_or_default();
            let name = self
                .editor
                .doc
                .node(id)
                .map_or(String::new(), |n| n.name.clone());
            root = root.child(mono(name, 11., p.ink));
            let mut grid = div().flex().flex_col().gap(px(1.));
            for (index, property) in LayerProperty::TRANSFORMS.iter().enumerate() {
                let keyed = has_key(&layer, property, frame);
                let animated = layer.track(property).is_some();
                let value = layer.value(property, frame as f64);
                let property = property.clone();
                grid = grid.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            Button::new(("storyboard-key-toggle", index))
                                .label(if keyed { "◆" } else { "◇" })
                                .tooltip(if keyed {
                                    "Remove the key at the playhead"
                                } else {
                                    "Key this value at the playhead"
                                })
                                .xsmall()
                                .ghost()
                                .disabled(locked)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_property_key(id, property.clone(), None, cx);
                                })),
                        )
                        .child(
                            div()
                                .w(px(70.))
                                .text_color(if animated { p.ink } else { p.muted })
                                .child(short_label(&LayerProperty::TRANSFORMS[index])),
                        )
                        .child(mono(
                            value_text(&LayerProperty::TRANSFORMS[index], value),
                            10.,
                            p.muted,
                        )),
                );
            }
            root = root.child(grid).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(mono(
                        match layer.pivot {
                            Some([x, y]) => format!("Pivot {x:.0}, {y:.0}"),
                            None => "Pivot at the layer's centre".into(),
                        },
                        10.,
                        p.muted,
                    ))
                    .when(layer.pivot.is_some(), |d| {
                        d.child(
                            Button::new("storyboard-reset-pivot")
                                .label("Centre")
                                .tooltip("Put the pivot back at the layer's centre")
                                .xsmall()
                                .ghost()
                                .disabled(locked)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.reset_pivot(id, cx)),
                                ),
                        )
                    }),
            );
        } else {
            root = root.child(mono(
                "Select a layer to key it at the playhead.",
                10.,
                p.muted,
            ));
        }
        if let Some((key, easing, curve)) = self.selected_ease() {
            let what = match &key.property {
                Some(property) => short_label(property),
                None => "every property".into(),
            };
            root = root
                .child(mono(
                    format!("Key at frame {} · {what} · ease to the next key", key.frame),
                    10.,
                    p.muted,
                ))
                .child(self.ease_curve_editor(
                    "layer-key-ease",
                    easing,
                    curve,
                    locked,
                    Rc::new(
                        |this: &mut EditorView, easing, curve, cx: &mut Context<EditorView>| {
                            this.ease_selected_key(easing, curve, cx);
                        },
                    ),
                    p,
                    cx,
                ));
        }
        let owner = cx.weak_entity();
        root.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(mono("When durations change", 10., p.muted))
                .child(
                    Button::new("storyboard-keyframe-sync")
                        .label(match sync {
                            KeyframeSync::Scale => "Keys stretch ▾",
                            KeyframeSync::Keep => "Keys keep frames ▾",
                        })
                        .xsmall()
                        .outline()
                        .disabled(locked)
                        .dropdown_menu(move |mut menu, _, _| {
                            for (choice, text) in [
                                (KeyframeSync::Scale, "Keys stretch with the panel"),
                                (KeyframeSync::Keep, "Keys keep their frames"),
                            ] {
                                let owner = owner.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(text).checked(sync == choice).on_click(
                                        move |_, _, cx| {
                                            owner
                                                .update(cx, |e, cx| {
                                                    e.set_keyframe_sync(choice, cx);
                                                })
                                                .ok();
                                        },
                                    ),
                                );
                            }
                            menu
                        }),
                ),
        )
        .into_any_element()
    }
}
