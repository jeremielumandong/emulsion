//! Connector attachments form a DAG, routed in dependency order.
use super::*;
impl Diagram {
    pub fn contains_endpoint(&self, id: NodeId) -> bool {
        self.shapes.contains_key(&id) || self.edges.contains_key(&id)
    }
    pub fn edge_order(&self) -> Result<Vec<NodeId>, String> {
        if self.edges.values().all(|e|self.shapes.contains_key(&e.source.shape)&&self.shapes.contains_key(&e.target.shape)) {
            return Ok(self.edges.keys().copied().collect());
        }
        let mut incoming = HashMap::new();
        let mut dependents: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut ready = std::collections::BTreeSet::new();
        for (id, e) in &self.edges {
            let mut degree = 0;
            for endpoint in [&e.source, &e.target] {
                if self.edges.contains_key(&endpoint.shape) {
                    degree += 1;
                    dependents.entry(endpoint.shape).or_default().push(*id);
                }
            }
            incoming.insert(*id, degree);
            if degree == 0 {
                ready.insert(*id);
            }
        }
        let mut order = Vec::with_capacity(self.edges.len());
        while let Some(id) = ready.pop_first() {
            order.push(id);
            for child in dependents.get(&id).into_iter().flatten() {
                let degree = incoming.get_mut(child).unwrap();
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(*child);
                }
            }
        }
        if order.len() != self.edges.len() {
            return Err("Connector attachments form a cycle.".into());
        }
        Ok(order)
    }
}
pub(super) fn on_path(path: &Path, port: Port) -> ((f64, f64), (f64, f64)) {
    let fraction = match port {
        Port::Custom { x, .. } => x.clamp(0., 1.),
        Port::West => 0.,
        Port::East => 1.,
        _ => 0.5,
    };
    let point = labels::point(path, fraction * 2. - 1., 0.);
    let prior = labels::point(path, (fraction - 0.001).max(0.) * 2. - 1., 0.);
    let next = labels::point(path, (fraction + 0.001).min(1.) * 2. - 1., 0.);
    let delta = (next.0 - prior.0, next.1 - prior.1);
    let len = delta.0.hypot(delta.1).max(1e-9);
    (point, (-delta.1 / len, delta.0 / len))
}
/// Resolve a shape port or a position along a connector. Custom x is the arc-length fraction on an edge.
pub fn endpoint_position(
    doc: &Document,
    endpoint: &Endpoint,
    toward: (f64, f64),
) -> Option<(f64, f64)> {
    let model = doc.diagram.as_ref()?;
    if let Some(shape) = model.shapes.get(&endpoint.shape) {
        return Some(endpoint.port.anchor(shape_bounds(doc, shape)?, toward).0);
    }
    let edge = model.edges.get(&endpoint.shape)?;
    let NodeKind::Path { path, .. } = &doc.node(edge.path)?.kind else {
        return None;
    };
    Some(on_path(path, endpoint.port).0)
}
/// Nearest point on a connector, for pointer-driven attachments and imported endpoints.
pub fn connector_attachment(
    doc: &Document,
    id: NodeId,
    point: (f64, f64),
) -> Option<(Endpoint, f64)> {
    let edge = doc.diagram.as_ref()?.edges.get(&id)?;
    let NodeKind::Path { path, .. } = &doc.node(edge.path)?.kind else {
        return None;
    };
    let segments = path
        .flatten(0.15)
        .into_iter()
        .flat_map(|(p, _)| p.windows(2).map(|p| (p[0], p[1])).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let total = segments
        .iter()
        .map(|(a, b)| (b.0 - a.0).hypot(b.1 - a.1))
        .sum::<f64>();
    let mut best = (f64::INFINITY, 0.);
    let mut traversed = 0.;
    for (a, b) in segments {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy);
        if length < 1e-9 {
            continue;
        }
        let t = (((point.0 - a.0) * dx + (point.1 - a.1) * dy) / (length * length)).clamp(0., 1.);
        let distance = (point.0 - a.0 - t * dx).hypot(point.1 - a.1 - t * dy);
        if distance < best.0 {
            best = (distance, (traversed + t * length) / total.max(1e-9));
        }
        traversed += length;
    }
    best.0.is_finite().then_some((
        Endpoint {
            shape: id,
            port: Port::Custom { x: best.1, y: 0. },
        },
        best.0,
    ))
}
