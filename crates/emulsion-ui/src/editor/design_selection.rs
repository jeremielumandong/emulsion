//! Picking editable Design objects without rasterizing vector artwork.
use super::*;

fn contains(bounds: emulsion_raster::IRect, point: (f64, f64)) -> bool {
    point.0 >= bounds.x as f64
        && point.1 >= bounds.y as f64
        && point.0 < bounds.right() as f64
        && point.1 < bounds.bottom() as f64
}

fn hits(doc: &Document, id: NodeId, point: (f64, f64), tolerance: f64) -> bool {
    hits_for_drop(doc, id, point, tolerance, false)
}

fn hits_for_drop(
    doc: &Document,
    id: NodeId,
    point: (f64, f64),
    tolerance: f64,
    include_locked: bool,
) -> bool {
    hits_geometry(doc, id, point, tolerance, include_locked, false)
}

fn hits_geometry(
    doc: &Document,
    id: NodeId,
    point: (f64, f64),
    tolerance: f64,
    include_locked: bool,
    clip_source: bool,
) -> bool {
    let Some(node) = doc.node(id) else {
        return false;
    };
    // Clipping uses the source alpha before its paint opacity, just like the
    // compositor. A pasted background frame is ordinary selectable artwork,
    // even though its vector boundary paints invisibly.
    if !node.visible
        || (!clip_source
            && (emulsion_core::design_background::is_background_node(doc, id)
                || node.opacity <= 0.
                || (!include_locked && doc.locked_ancestor(id).is_some())))
        || !emulsion_core::design_clipping::point_visible(doc, id, point)
    {
        return false;
    }
    if let Some(mut base) = node.clip_to {
        while let Some(lower) = doc.node(base).and_then(|node| node.clip_to) {
            base = lower;
        }
        let Some(source) = doc.node(base) else {
            return false;
        };
        if (!source.blending.blend_clipped_layers_as_group
            && (source.opacity <= 0. || source.blending.fill_opacity <= 0.))
            || !hits_geometry(doc, base, point, tolerance, true, true)
        {
            return false;
        }
    }
    // Text frames include whitespace and overflow space beyond the ink bounds.
    // Strokes also need the same screen-space tolerance in the broad phase as
    // in the precise hit test below.
    if !matches!(node.kind, NodeKind::Text { .. } | NodeKind::Group { .. })
        && !emulsion_core::geometry::node_bounds(doc, id).is_some_and(|b| {
            contains(b, point)
                || (point.0 >= b.x as f64 - tolerance
                    && point.1 >= b.y as f64 - tolerance
                    && point.0 <= b.right() as f64 + tolerance
                    && point.1 <= b.bottom() as f64 + tolerance)
        })
    {
        return false;
    }
    match &node.kind {
        NodeKind::Group { .. } => doc
            .children(Some(id))
            .into_iter()
            .any(|child| hits_for_drop(doc, child, point, tolerance, include_locked)),
        NodeKind::Adjust(_) => false,
        // The page background has dedicated controls, so a blank canvas
        // click should deselect objects rather than try to drag an infinite fill.
        NodeKind::Fill { rgba } => node.mask_enabled && node.mask.is_some() && rgba[3] > 0,
        NodeKind::Text { spec, .. } => {
            let p = spec
                .transform()
                .inverse()
                .transform_point2(glam::dvec2(point.0, point.1));
            let b = emulsion_core::text::layout(spec).bounds();
            let tx = tolerance / f64::from(spec.scale_x.abs().max(0.01));
            let ty = tolerance / f64::from(spec.scale_y.abs().max(0.01));
            p.x >= f64::from(b.x.min(0.)) - tx
                && p.y >= f64::from(b.y.min(0.)) - ty
                && p.x <= f64::from(spec.width.unwrap_or(b.x + b.width)) + tx
                && p.y <= f64::from(spec.height.unwrap_or(b.y + b.height)) + ty
        }
        NodeKind::Strokes { strokes, .. } => strokes.hit(point, tolerance).is_some(),
        NodeKind::Path { path, style, .. } => {
            let mut winding = 0i32;
            let mut stroke_hit = false;
            for (points, closed) in path.flatten(tolerance.min(0.25)) {
                for i in 0..points.len() {
                    let a = points[i];
                    let b = points[(i + 1) % points.len()];
                    let cross = (b.0 - a.0) * (point.1 - a.1) - (point.0 - a.0) * (b.1 - a.1);
                    if a.1 <= point.1 && b.1 > point.1 && cross > 0. {
                        winding += 1;
                    }
                    if a.1 > point.1 && b.1 <= point.1 && cross < 0. {
                        winding -= 1;
                    }
                    if closed || i + 1 < points.len() {
                        let length2 = (b.0 - a.0).powi(2) + (b.1 - a.1).powi(2);
                        let t = if length2 > 0. {
                            (((point.0 - a.0) * (b.0 - a.0) + (point.1 - a.1) * (b.1 - a.1))
                                / length2)
                                .clamp(0., 1.)
                        } else {
                            0.
                        };
                        stroke_hit |= (point.0 - a.0 - t * (b.0 - a.0))
                            .hypot(point.1 - a.1 - t * (b.1 - a.1))
                            <= style.width as f64 / 2. + tolerance;
                    }
                }
            }
            (style.fill.is_some_and(|c| c[3] > 0) && winding != 0)
                || (style.stroke.is_some_and(|c| c[3] > 0) && stroke_hit)
        }
        NodeKind::Raster { raster, placement } => {
            let p = placement
                .to_doc(raster.width(), raster.height())
                .inverse()
                .transform_point2(glam::dvec2(point.0, point.1));
            p.x >= 0.
                && p.y >= 0.
                && p.x < raster.width() as f64
                && p.y < raster.height() as f64
                && raster.get(p.x as u32, p.y as u32)[3] > 0
        }
        NodeKind::Smart { .. } => true,
    }
}

impl EditorView {
    pub(super) fn design_hit(&self, point: (f64, f64), deep: bool) -> Option<NodeId> {
        self.design_hit_for_drop(point, deep, false)
    }

    pub(super) fn design_hit_for_drop(
        &self,
        point: (f64, f64),
        deep: bool,
        include_locked: bool,
    ) -> Option<NodeId> {
        let doc = &self.editor.doc;
        let tolerance = 3. / self.view.zoom.max(0.01);
        let mut parent = None;
        loop {
            let hit = doc
                .children(parent)
                .into_iter()
                .rev()
                .find(|id| hits_for_drop(doc, *id, point, tolerance, include_locked));
            if !deep || !hit.is_some_and(|id| doc.node(id).is_some_and(|n| n.is_group())) {
                return hit.or(parent);
            }
            parent = hit;
        }
    }

    pub(super) fn select_design_at(
        &mut self,
        point: (f64, f64),
        toggle: bool,
        deep: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        // A drag inside the current selection owns that selection, including an
        // object picked in Layers underneath overlapping artwork. Modifiers
        // still explicitly request toggling or picking inside a group.
        if !toggle && !deep {
            let doc = &self.editor.doc;
            let tolerance = 3. / self.view.zoom.max(0.01);
            let selected_hit = self.selected_layer_roots().into_iter().any(|id| {
                let mut current = Some(id);
                while let Some(id) = current {
                    let Some(node) = doc.node(id) else {
                        return false;
                    };
                    if !node.visible || node.opacity <= 0. {
                        return false;
                    }
                    current = node.parent;
                }
                hits(doc, id, point, tolerance)
            });
            if selected_hit {
                return true;
            }
        }
        let Some(id) = self.design_hit(point, deep) else {
            if !toggle {
                self.set_layer_selection(Vec::new(), None);
            }
            cx.notify();
            return false;
        };
        if toggle {
            let mut ids = self.selected_layer_ids();
            if self.layer_is_selected(id) {
                ids.retain(|other| *other != id);
                let active = ids.last().copied();
                self.set_layer_selection(ids, active);
                cx.notify();
                return false;
            }
            ids.push(id);
            self.set_layer_selection(ids, Some(id));
        } else if !self.layer_is_selected(id) {
            self.set_layer_selection(vec![id], Some(id));
        }
        cx.notify();
        true
    }
}
