//! Relative connector labels follow the routed path, including curved connectors.
use super::*;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeLabel {
    pub node: NodeId,
    /// -1 is the source, 0 the midpoint, +1 the target.
    pub position: f64,
    pub normal: f64,
    pub offset: (f64, f64),
}
pub(super) fn point(path: &Path, position: f64, normal: f64) -> (f64, f64) {
    let lines = path
        .flatten(0.2)
        .into_iter()
        .flat_map(|(p, _)| p.windows(2).map(|p| (p[0], p[1])).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let length = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
    let mut remain = lines.iter().map(|(a, b)| length(*a, *b)).sum::<f64>()
        * ((position + 1.) / 2.).clamp(0., 1.);
    for (i, (a, b)) in lines.iter().enumerate() {
        let len = length(*a, *b);
        if len < 1e-9 {
            continue;
        }
        if remain <= len || i + 1 == lines.len() {
            let t = (remain / len).clamp(0., 1.);
            let (dx, dy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
            return (
                a.0 + (b.0 - a.0) * t + dy * normal,
                a.1 + (b.1 - a.1) * t - dx * normal,
            );
        }
        remain -= len;
    }
    (0., 0.)
}
pub(super) fn synchronize(before: &Document, doc: &mut Document, diagram: &mut Diagram) {
    if !diagram.edges.values().any(|e| !e.labels.is_empty()) {
        return;
    }
    let indices = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect::<HashMap<_, _>>();
    let old = before
        .nodes
        .iter()
        .map(|n| (n.id, n))
        .collect::<HashMap<_, _>>();
    let (w, h) = (doc.width, doc.height);
    for edge in diagram.edges.values_mut() {
        let Some(&path_index) = indices.get(&edge.path) else {
            continue;
        };
        let NodeKind::Path { path, .. } = &doc.nodes[path_index].kind else {
            continue;
        };
        let path = path.clone();
        let path_unchanged = old
            .get(&edge.path)
            .is_some_and(|n| matches!(&n.kind,NodeKind::Path{path:p,..} if **p==*path));
        edge.labels.retain(|l| indices.contains_key(&l.node));
        for label in &mut edge.labels {
            let at = point(&path, label.position, label.normal);
            if let NodeKind::Text { spec, cache } = &mut doc.nodes[indices[&label.node]].kind {
                if path_unchanged
                    && let Some(old) = old.get(&label.node)
                    && let NodeKind::Text { spec: previous, .. } = &old.kind
                {
                    label.offset.0 += f64::from(spec.x - previous.x);
                    label.offset.1 += f64::from(spec.y - previous.y);
                }
                let mut next = (**spec).clone();
                next.x = (at.0 + label.offset.0 - f64::from(next.width.unwrap_or(0.)) / 2.) as f32;
                next.y = (at.1 + label.offset.1 - f64::from(next.size) * 0.6) as f32;
                if next != **spec {
                    *spec = Arc::new(next);
                    *cache = crate::vector_cache::VectorRaster::text(spec.clone(), w, h);
                }
            }
        }
    }
}
