//! Ruler guides and snapping.
//!
//! Drag from a ruler to make a guide; drag a guide back onto a ruler (or
//! off the canvas) to remove it. Guides live in the document, so they are
//! saved and undoable. Moving a node snaps its edges and centre to the
//! canvas edges and centre, to guides, and to other nodes, within a few
//! screen pixels. Hold Ctrl while dragging to move freely.

use super::*;
use emulsion_core::document::Guide;
use emulsion_raster::IRect;

/// A full-length line: (vertical, position in document pixels).
pub(crate) type Line = (bool, f64);

/// Snap distance in screen pixels.
const SNAP_PX: f64 = 6.0;
/// Grab distance for an existing guide, in screen pixels.
const GRAB_PX: f64 = 4.0;
/// Ruler thickness; matches the viewport's.
const RULER_PX: f32 = 16.0;

impl EditorView {
    /// Top-left of the canvas area, for tests that aim at the rulers.
    #[cfg(test)]
    pub(crate) fn canvas_origin(&self) -> Option<Point<Pixels>> {
        self.canvas_bounds().map(|b| b.origin)
    }

    fn rulers_shown(&self) -> bool {
        self.rulers && self.view.rotation.rem_euclid(360.0) == 0.0
    }

    /// Which ruler `pos` is over: Some(true) for the left (vertical guides),
    /// Some(false) for the top (horizontal guides).
    pub(crate) fn ruler_hit(&self, pos: Point<Pixels>) -> Option<bool> {
        let b = self.canvas_bounds()?;
        if !self.rulers_shown() || !b.contains(&pos) {
            return None;
        }
        let (x, y) = (pos.x - b.origin.x, pos.y - b.origin.y);
        if y < px(RULER_PX) && x >= px(RULER_PX) {
            Some(false)
        } else if x < px(RULER_PX) && y >= px(RULER_PX) {
            Some(true)
        } else {
            None
        }
    }

    /// The guide under `pos`, if any.
    pub(crate) fn guide_hit(&self, pos: Point<Pixels>) -> Option<usize> {
        let b = self.canvas_bounds()?;
        if self.view.rotation.rem_euclid(360.0) != 0.0 {
            return None;
        }
        let (sx, sy) = (f32::from(pos.x) as f64, f32::from(pos.y) as f64);
        self.editor.doc.guides.iter().position(|g| {
            let s = if g.vertical {
                self.view.doc_to_screen((g.pos, 0.0), &b).0 - sx
            } else {
                self.view.doc_to_screen((0.0, g.pos), &b).1 - sy
            };
            s.abs() <= GRAB_PX
        })
    }

    /// Where a guide being dragged would land, or None to remove it.
    pub(crate) fn guide_position(&self, vertical: bool, pos: Point<Pixels>) -> Option<f64> {
        let b = self.canvas_bounds()?;
        if !b.contains(&pos) || self.ruler_hit(pos).is_some() {
            return None;
        }
        let d = self.doc_point(pos)?;
        let v = if vertical { d.0 } else { d.1 };
        // Whole pixels unless zoomed in far enough to place finer.
        Some(if self.view.zoom >= 8.0 {
            (v * 4.0).round() / 4.0
        } else {
            v.round()
        })
    }

    /// Finish a guide drag.
    pub(crate) fn drop_guide(
        &mut self,
        vertical: bool,
        pos: Option<f64>,
        existing: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let mut guides = self.editor.doc.guides.clone();
        match (existing, pos) {
            (Some(i), Some(p)) if i < guides.len() => guides[i].pos = p,
            (Some(i), None) if i < guides.len() => {
                guides.remove(i);
            }
            (None, Some(p)) => guides.push(Guide { vertical, pos: p }),
            _ => return,
        }
        if guides != self.editor.doc.guides {
            self.execute(Command::SetGuides { guides }, cx);
        }
    }

    pub fn clear_guides(&mut self, cx: &mut Context<Self>) {
        if !self.editor.doc.guides.is_empty() {
            self.execute(Command::SetGuides { guides: Vec::new() }, cx);
        }
    }

    /// Snap original document-space bounds, regardless of the node's kind.
    pub(crate) fn snap_node_move(&mut self, id: NodeId, b: IRect, dx: f64, dy: f64) -> (f64, f64) {
        self.snap_lines.clear();
        if !self.snap || self.snap_bypass || b.is_empty() {
            return (dx, dy);
        }
        let doc = &self.editor.doc;
        let (mut tx, mut ty) = (
            vec![0.0, doc.width as f64 / 2.0, doc.width as f64],
            vec![0.0, doc.height as f64 / 2.0, doc.height as f64],
        );
        for guide in &doc.guides {
            if guide.vertical {
                tx.push(guide.pos);
            } else {
                ty.push(guide.pos);
            }
        }
        for bounds in visible_snap_bounds(doc, id) {
            tx.extend([
                bounds.x as f64,
                bounds.x as f64 + bounds.w as f64 / 2.0,
                bounds.right() as f64,
            ]);
            ty.extend([
                bounds.y as f64,
                bounds.y as f64 + bounds.h as f64 / 2.0,
                bounds.bottom() as f64,
            ]);
        }
        let limit = SNAP_PX / self.view.zoom;
        let best = |edges: [f64; 3], targets: &[f64], delta: f64| -> Option<(f64, f64)> {
            let mut best: Option<(f64, f64)> = None;
            for edge in edges {
                for target in targets {
                    let diff = target - (edge + delta);
                    if diff.abs() <= limit
                        && best.is_none_or(|(current, _)| diff.abs() < current.abs())
                    {
                        best = Some((diff, *target));
                    }
                }
            }
            best
        };
        let xs = [b.x as f64, b.x as f64 + b.w as f64 / 2.0, b.right() as f64];
        let ys = [b.y as f64, b.y as f64 + b.h as f64 / 2.0, b.bottom() as f64];
        let (mut dx, mut dy) = (dx, dy);
        if let Some((diff, target)) = best(xs, &tx, dx) {
            dx += diff;
            self.snap_lines.push((true, target));
        }
        if let Some((diff, target)) = best(ys, &ty, dy) {
            dy += diff;
            self.snap_lines.push((false, target));
        }
        (dx, dy)
    }

    /// Guides and snap lines for the overlay, including one being dragged.
    pub(crate) fn guide_lines(&self) -> (Vec<Line>, Vec<Line>) {
        let mut guides: Vec<Line> = self
            .editor
            .doc
            .guides
            .iter()
            .enumerate()
            .filter(|(i, _)| !matches!(&self.drag, Some(Drag::Guide { existing: Some(e), .. }) if e == i))
            .map(|(_, g)| (g.vertical, g.pos))
            .collect();
        if let Some(Drag::Guide {
            vertical,
            pos: Some(p),
            ..
        }) = &self.drag
        {
            guides.push((*vertical, *p));
        }
        (guides, self.snap_lines.clone())
    }
}

/// Resolve visibility and hierarchy once; group bounds are aggregated once
/// rather than recursively remeasuring every descendant for every ancestor.
fn visible_snap_bounds(doc: &Document, moving: NodeId) -> Vec<IRect> {
    let indices: HashMap<_, _> = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id, i))
        .collect();
    let mut children = vec![Vec::new(); doc.nodes.len()];
    let mut roots = Vec::new();
    for (i, node) in doc.nodes.iter().enumerate() {
        match node.parent {
            Some(parent) => {
                if let Some(&parent) = indices.get(&parent) {
                    children[parent].push(i);
                }
            }
            None => roots.push(i),
        }
    }
    let mut excluded = vec![false; doc.nodes.len()];
    if let Some(&index) = indices.get(&moving) {
        let mut stack = vec![index];
        while let Some(i) = stack.pop() {
            if excluded[i] {
                continue;
            }
            excluded[i] = true;
            stack.extend(children[i].iter().copied());
        }
        let mut parent = doc.nodes[index].parent;
        while let Some(i) = parent.and_then(|id| indices.get(&id).copied()) {
            if excluded[i] {
                break;
            }
            excluded[i] = true;
            parent = doc.nodes[i].parent;
        }
    }
    let mut visible = Vec::new();
    let mut stack = roots;
    while let Some(i) = stack.pop() {
        let node = &doc.nodes[i];
        if !node.visible || node.opacity <= 0.0 {
            continue;
        }
        visible.push(i);
        stack.extend(children[i].iter().copied());
    }
    let mut bounds: Vec<Option<IRect>> = vec![None; doc.nodes.len()];
    for i in visible.into_iter().rev() {
        if excluded[i] {
            continue;
        }
        let node = &doc.nodes[i];
        bounds[i] = if node.kind.is_group() {
            let mut union = children[i]
                .iter()
                .filter_map(|child| bounds[*child])
                .fold(IRect::default(), |a, b| a.union(&b));
            if node.mask_enabled
                && let Some(mask) = &node.mask
            {
                union = union.intersect(&emulsion_raster::select::bounds(mask));
            }
            (!union.is_empty()).then_some(union)
        } else {
            emulsion_core::geometry::node_bounds(doc, node.id)
        };
    }
    bounds.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::visible_snap_bounds;
    use emulsion_core::{Document, Node, NodeId};
    use emulsion_raster::{IRect, Placement, Raster, select};
    use std::sync::Arc;

    fn pixels(id: NodeId, parent: Option<NodeId>, x: f64) -> Node {
        let mut node = Node::raster(
            id,
            "Pixels",
            Arc::new(Raster::solid(10, 10, [1.0; 4])),
            Placement::at(x, 20.0),
        );
        node.parent = parent;
        node
    }

    #[test]
    fn moving_group_excludes_its_ancestors_and_descendants_but_keeps_siblings() {
        let mut doc = Document::new(200, 100);
        let mut moving = Node::group(2, "Moving");
        moving.parent = Some(1);
        doc.nodes = vec![
            Node::group(1, "Parent"),
            moving,
            pixels(3, Some(2), 10.0),
            pixels(4, Some(1), 60.0),
            pixels(5, None, 110.0),
        ];
        assert_eq!(
            visible_snap_bounds(&doc, 2),
            vec![IRect::new(60, 20, 10, 10), IRect::new(110, 20, 10, 10)]
        );
        // Selecting the nested leaf also excludes both ancestor groups.
        assert_eq!(visible_snap_bounds(&doc, 3), visible_snap_bounds(&doc, 2));
    }

    #[test]
    fn target_groups_ignore_hidden_content_and_respect_enabled_masks() {
        let mut doc = Document::new(200, 100);
        let mut group = Node::group(2, "Target");
        group.mask = Some(Arc::new(select::rect(200, 100, 42.0, 22.0, 5.0, 5.0)));
        let mut hidden_child = pixels(4, Some(2), 100.0);
        hidden_child.visible = false;
        let mut hidden_group = Node::group(5, "Hidden ancestor");
        hidden_group.visible = false;
        let mut transparent = pixels(7, None, 170.0);
        transparent.opacity = 0.0;
        doc.nodes = vec![
            pixels(1, None, 0.0),
            group,
            pixels(3, Some(2), 40.0),
            hidden_child,
            hidden_group,
            pixels(6, Some(5), 140.0),
            transparent,
        ];
        assert_eq!(
            visible_snap_bounds(&doc, 1),
            vec![IRect::new(42, 22, 5, 5), IRect::new(40, 20, 10, 10)]
        );
        doc.nodes[1].mask_enabled = false;
        assert_eq!(
            visible_snap_bounds(&doc, 1),
            vec![IRect::new(40, 20, 10, 10); 2]
        );
    }
}
