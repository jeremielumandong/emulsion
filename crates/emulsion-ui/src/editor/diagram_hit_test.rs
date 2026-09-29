//! Retain hit geometry between pointer events; rebuild once per document/zoom change.
use super::*;
type Key = (u64, u64, u64, u64);
type EdgeHitGeometry = (NodeId, [f64; 4], Vec<Vec<(f64, f64)>>);
#[derive(Default)]
pub(super) struct HitCache {
    key: Option<Key>,
    pub(super) order: std::collections::HashMap<NodeId, usize>,
    pub(super) shapes: Vec<(NodeId, ShapeKind, [f64; 4])>,
    pub(super) edges: Vec<EdgeHitGeometry>,
}
impl EditorView {
    pub(super) fn diagram_hit_cache(&self) -> std::cell::Ref<'_, HitCache> {
        let key = (
            self.editor.active_page(),
            self.editor.revision,
            self.render_epoch,
            self.view.zoom.to_bits(),
        );
        let mut cache = self.diagram_ui.hit_cache.borrow_mut();
        if cache.key != Some(key) {
            *cache = HitCache {
                key: Some(key),
                ..Default::default()
            };
            let doc = &self.editor.doc;
            if let Some(model) = &doc.diagram {
                let nodes = doc
                    .nodes
                    .iter()
                    .map(|n| (n.id, n))
                    .collect::<std::collections::HashMap<_, _>>();
                let visible = |id| {
                    let mut current = Some(id);
                    while let Some(id) = current {
                        let Some(node) = nodes.get(&id) else {
                            return false;
                        };
                        if !node.visible || node.locked {
                            return false;
                        }
                        current = node.parent;
                    }
                    true
                };
                // Paint order is hierarchical, not the flat allocation order of nodes.
                let mut children = std::collections::HashMap::<Option<NodeId>, Vec<NodeId>>::new();
                for node in &doc.nodes {
                    children.entry(node.parent).or_default().push(node.id);
                }
                let mut stack = children.get(&None).cloned().unwrap_or_default();
                stack.reverse();
                let mut ordered = Vec::new();
                while let Some(id) = stack.pop() {
                    if let Some(kids) = children.get(&Some(id)) {
                        stack.extend(kids.iter().rev());
                    }
                    ordered.push(id);
                }
                ordered.reverse();
                for (rank, id) in ordered.iter().rev().enumerate() {
                    cache.order.insert(*id, rank);
                }
                for node in ordered
                    .iter()
                    .filter_map(|id| nodes.get(id))
                    .filter(|n| visible(n.id))
                {
                    if let Some(shape) = model.shapes.get(&node.id)
                        && let Some(body) = nodes.get(&shape.body)
                        && let NodeKind::Path { path, .. } = &body.kind
                        && let Some((x, y, w, h)) = emulsion_raster::vector_geometry::bounds(path)
                    {
                        cache
                            .shapes
                            .push((node.id, shape.kind, [x, y, w.max(1.), h.max(1.)]));
                    }
                    if let Some(edge) = model.edges.get(&node.id)
                        && let Some(body) = nodes.get(&edge.path)
                        && let NodeKind::Path { path, .. } = &body.kind
                        && let Some((x, y, w, h)) = emulsion_raster::vector_geometry::bounds(path)
                    {
                        cache.edges.push((
                            node.id,
                            [x, y, w, h],
                            path.flatten((0.75 / self.view.zoom).clamp(0.02, 2.))
                                .into_iter()
                                .map(|(points, _)| points)
                                .collect(),
                        ));
                    }
                }
            }
        }
        drop(cache);
        self.diagram_ui.hit_cache.borrow()
    }
}
