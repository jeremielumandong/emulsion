//! Native connector artwork; persisted nodes also render in SVG/PDF exports.
use super::*;

fn parallel(path: &Path, distance: f64) -> Path {
    let mut result = path.clone();
    for sub in &mut result.subpaths {
        let original = sub.anchors.clone();
        for (i, anchor) in sub.anchors.iter_mut().enumerate() {
            let p = original[i];
            let prev = if p.h_in != p.p {
                p.h_in
            } else {
                original[i.saturating_sub(1)].p
            };
            let next = if p.h_out != p.p {
                p.h_out
            } else {
                original[(i + 1).min(original.len() - 1)].p
            };
            let (dx, dy) = (next.0 - prev.0, next.1 - prev.1);
            let length = dx.hypot(dy);
            if length < 1e-9 {
                continue;
            }
            let offset = (-dy / length * distance, dx / length * distance);
            for point in [&mut anchor.p, &mut anchor.h_in, &mut anchor.h_out] {
                point.0 += offset.0;
                point.1 += offset.1;
            }
        }
    }
    result
}
fn update(
    doc: &mut Document,
    parent: NodeId,
    before: NodeId,
    slot: &mut Option<NodeId>,
    value: Option<(Path, PathStyle)>,
    name: &str,
) {
    let Some((path, style)) = value else {
        if let Some(id) = slot.take() {
            doc.nodes.retain(|n| n.id != id);
        }
        return;
    };
    let (w, h) = (doc.width, doc.height);
    if let Some(id) = *slot
        && let Some(node) = doc.node_mut(id)
    {
        if let NodeKind::Path {
            path: old,
            style: paint,
            cache,
        } = &mut node.kind
            && (**old != path || *paint != style)
        {
            *old = Arc::new(path);
            *paint = style;
            *cache = crate::vector_cache::VectorRaster::path(old.clone(), style, w, h);
        }
        return;
    }
    let id = doc.alloc_id();
    let mut node = Node::path(id, name, Arc::new(path), style, w, h);
    node.parent = Some(parent);
    let index = doc
        .nodes
        .iter()
        .position(|n| n.id == before)
        .unwrap_or(doc.nodes.len());
    doc.nodes.insert(index, node);
    *slot = Some(id);
}
pub(super) fn synchronize(before: &Document, doc: &mut Document, model: &mut Diagram) {
    if !model.edges.values().any(|e| {
        e.double_line
            || e.double_path.is_some()
            || e.label_background.is_some()
            || e.label_background_path.is_some()
    }) {
        return;
    }
    for (id, e) in &mut model.edges {
        if e.double_path.is_some_and(|id| doc.node(id).is_none()) {
            e.double_path = None;
        }
        if e.label_background_path
            .is_some_and(|id| doc.node(id).is_none())
        {
            e.label_background_path = None;
        }
        let previous = before.diagram.as_ref().and_then(|d| d.edges.get(id));
        if e.double_line || e.double_path.is_some() {
            let line = doc.node(e.path);
            let unchanged = previous.is_some_and(|old| {
                old.double_line == e.double_line && old.double_path == e.double_path
            }) && line == before.node(e.path);
            if !unchanged {
                let value = if e.double_line {
                    line.and_then(|n| match &n.kind {
                        NodeKind::Path { path, style, .. } => {
                            Some((parallel(path, f64::from(style.width) * 2. + 1.), *style))
                        }
                        _ => None,
                    })
                } else {
                    None
                };
                update(
                    doc,
                    *id,
                    e.arrow,
                    &mut e.double_path,
                    value,
                    "Double connector line",
                );
            }
        }
        if e.label_background.is_some() || e.label_background_path.is_some() {
            let label = doc.node(e.label);
            let unchanged = previous.is_some_and(|old| {
                old.label_background == e.label_background
                    && old.label_background_path == e.label_background_path
            }) && label == before.node(e.label);
            if !unchanged {
                let value = e.label_background.and_then(|color| {
                    let NodeKind::Text { spec, .. } = &label?.kind else {
                        return None;
                    };
                    if spec.text.trim().is_empty() {
                        return None;
                    }
                    let b = crate::text::bounds(spec);
                    if b.is_empty() {
                        return None;
                    }
                    Some((
                        ShapeKind::Terminator.path([
                            f64::from(b.x) - 6.,
                            f64::from(b.y) - 4.,
                            f64::from(b.w) + 12.,
                            f64::from(b.h) + 8.,
                        ]),
                        PathStyle {
                            fill: Some(color),
                            stroke: None,
                            ..Default::default()
                        },
                    ))
                });
                update(
                    doc,
                    *id,
                    e.label,
                    &mut e.label_background_path,
                    value,
                    "Connector label background",
                );
            }
        }
    }
}
