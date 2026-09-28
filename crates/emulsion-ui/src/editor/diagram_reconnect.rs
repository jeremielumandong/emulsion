//! Endpoint drags keep the document untouched until release: one edit, one undo.
use super::*;

pub(super) struct EndpointDrag {
    edge: NodeId,
    source: bool,
    original: Endpoint,
    revision: u64,
    start: (f64, f64),
    segment: Option<(Vec<(f64, f64)>, usize)>,
    curve_point: Option<usize>,
    route: diagram::ConnectorPreview,
}

impl EditorView {
    pub(crate) fn single_selected_connector(&self) -> bool {
        self.selected_layer_roots().len() == 1
            && self.diagram_object().is_some_and(|id| {
                self.editor
                    .doc
                    .diagram
                    .as_ref()
                    .and_then(|d| d.edges.get(&id))
                    .is_some_and(|edge| self.selected != Some(edge.label))
            })
    }

    fn diagram_edge_ends(&self, id: NodeId) -> Option<[(f64, f64); 2]> {
        let cache = self.diagram_hit_cache();
        let (_, _, lines) = cache.edges.iter().find(|(edge, _, _)| *edge == id)?;
        Some([*lines.first()?.first()?, *lines.last()?.last()?])
    }

    pub(super) fn begin_diagram_endpoint_drag(
        &mut self,
        point: (f64, f64),
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(id) = self.diagram_object() else {
            return false;
        };
        if self.selected_layer_roots().len() != 1
            || self.assistant.running
            || self.drag.is_some()
            || self.warp.is_some()
            || self.editor.doc.locked_ancestor(id).is_some()
            || self.editor.doc.layer_locks(id).position
            || self.editor.in_transaction()
        {
            return false;
        }
        let Some(ends) = self.diagram_edge_ends(id) else {
            return false;
        };
        let distance = |p: (f64, f64)| (p.0 - point.0).hypot(p.1 - point.1);
        let source = distance(ends[0]) <= distance(ends[1]);
        let edge = &self.editor.doc.diagram.as_ref().unwrap().edges[&id];
        // A label explicitly selected in Layers keeps its independent move gesture.
        if self.selected == Some(edge.label) {
            return false;
        }
        let mut curve_point = None;
        let segment = if distance(ends[usize::from(!source)]) * self.view.zoom > 12. {
            let Some(Node {
                kind: NodeKind::Path { path, .. },
                ..
            }) = self.editor.doc.node(edge.path)
            else {
                return false;
            };
            let Some(line) = path.subpaths.first() else {
                return false;
            };
            if line.anchors.len() > 128 {
                return false;
            }
            if matches!(
                edge.routing,
                diagram::Routing::Curved | diagram::Routing::Cyclical
            ) {
                // Hit-test the displayed curve, not the chord between its anchors.
                if !path
                    .flatten((0.75 / self.view.zoom).clamp(0.02, 2.))
                    .iter()
                    .any(|(line, _)| {
                        line.windows(2)
                            .any(|p| segment_distance(point, p[0], p[1]) * self.view.zoom <= 7.)
                    })
                {
                    return false;
                }
                let mut points = if edge.waypoints.is_empty() {
                    line.anchors.iter().map(|a| a.p).collect::<Vec<_>>()
                } else {
                    std::iter::once(ends[0])
                        .chain(edge.waypoints.iter().copied())
                        .chain(std::iter::once(ends[1]))
                        .collect()
                };
                let nearby = (1..points.len() - 1)
                    .min_by(|&a, &b| distance(points[a]).total_cmp(&distance(points[b])))
                    .filter(|&i| distance(points[i]) * self.view.zoom <= 12.);
                let index = nearby.unwrap_or_else(|| {
                    let index = points
                        .windows(2)
                        .enumerate()
                        .min_by(|(_, a), (_, b)| {
                            segment_distance(point, a[0], a[1])
                                .total_cmp(&segment_distance(point, b[0], b[1]))
                        })
                        .map(|(i, _)| i + 1)
                        .unwrap_or(1);
                    points.insert(index, point);
                    index
                });
                curve_point = Some(index);
                Some((points, index))
            } else {
                let points: Vec<_> = line.anchors.iter().map(|a| a.p).collect();
                let nearest = points
                    .windows(2)
                    .enumerate()
                    .map(|(i, p)| {
                        let (dx, dy) = (p[1].0 - p[0].0, p[1].1 - p[0].1);
                        let t = (((point.0 - p[0].0) * dx + (point.1 - p[0].1) * dy)
                            / (dx * dx + dy * dy).max(1e-12))
                        .clamp(0., 1.);
                        (
                            i,
                            (point.0 - p[0].0 - t * dx).hypot(point.1 - p[0].1 - t * dy),
                        )
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let Some((index, d)) = nearest else {
                    return false;
                };
                if d * self.view.zoom > 7. {
                    return false;
                }
                Some((points, index))
            }
        } else {
            None
        };
        self.diagram_ui.endpoint_drag = Some(EndpointDrag {
            edge: id,
            source,
            original: if source {
                edge.source.clone()
            } else {
                edge.target.clone()
            },
            revision: self.editor.revision,
            start: point,
            segment,
            curve_point,
            route: diagram::ConnectorPreview::new(&self.editor.doc).unwrap(),
        });
        self.notify_canvas(cx);
        cx.notify();
        true
    }

    // Keep the attachment on its current object while dragging within that object,
    // even when another layer overlaps it. Leaving it allows another destination.
    fn diagram_endpoint_candidate(&self, point: (f64, f64)) -> Option<Endpoint> {
        let drag = self.diagram_ui.endpoint_drag.as_ref()?;
        if drag.segment.is_some() {
            return None;
        }
        let cache = self.diagram_hit_cache();
        let original = cache
            .shapes
            .iter()
            .find(|(id, _, _)| *id == drag.original.shape);
        let retained = original.and_then(|(id, _, [x, y, w, h])| {
            let margin = 10. / self.view.zoom;
            (point.0 >= x - margin
                && point.0 <= x + w + margin
                && point.1 >= y - margin
                && point.1 <= y + h + margin)
                .then_some(Endpoint {
                    shape: *id,
                    port: Port::Custom {
                        x: ((point.0 - x) / w).clamp(0., 1.),
                        y: ((point.1 - y) / h).clamp(0., 1.),
                    },
                })
        });
        drop(cache);
        let retained = retained.or_else(|| {
            diagram::connector_attachment(&self.editor.doc, drag.original.shape, point)
                .filter(|(_, distance)| *distance * self.view.zoom <= 10.)
                .map(|(endpoint, _)| endpoint)
        });
        let endpoint = retained.or_else(|| self.diagram_hit_excluding(point, Some(drag.edge)))?;
        // Shape targets cannot introduce connector dependency cycles.
        if self
            .editor
            .doc
            .diagram
            .as_ref()?
            .shapes
            .contains_key(&endpoint.shape)
        {
            return Some(endpoint);
        }
        // Do not create self attachments or cycles through another connector.
        let mut model = self.editor.doc.diagram.as_deref()?.clone();
        let edge = model.edges.get_mut(&drag.edge)?;
        if drag.source {
            edge.source = endpoint.clone();
        } else {
            edge.target = endpoint.clone();
        }
        model.edge_order().ok()?;
        Some(endpoint)
    }

    pub(super) fn finish_diagram_endpoint_drag(
        &mut self,
        point: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) {
        let endpoint = point.and_then(|p| self.diagram_endpoint_candidate(p));
        let bend = point.and_then(|p| {
            let drag = self.diagram_ui.endpoint_drag.as_ref()?;
            let moved = drag.bend_preview(p)?;
            (moved != drag.segment.as_ref()?.0).then_some(moved)
        });
        let drag = self.diagram_ui.endpoint_drag.take().unwrap();
        if point
            .is_some_and(|p| (p.0 - drag.start.0).hypot(p.1 - drag.start.1) * self.view.zoom > 3.)
            && self.editor.revision == drag.revision
            && (bend.is_some() || endpoint.as_ref().is_some_and(|e| *e != drag.original))
            && let Some(mut model) = self.editor.doc.diagram.as_deref().cloned()
            && let Some(edge) = model.edges.get_mut(&drag.edge)
        {
            if let Some(points) = bend {
                edge.waypoints = points[1..points.len() - 1].to_vec();
            } else if let Some(endpoint) = endpoint {
                if drag.source {
                    edge.source = endpoint;
                } else {
                    edge.target = endpoint;
                }
            }
            // Preserve deliberate bends and all connector formatting.
            self.execute(
                Command::SetDiagram {
                    diagram: Some(Arc::new(model)),
                },
                cx,
            );
        }
        self.notify_canvas(cx);
        cx.notify();
    }

    pub(super) fn diagram_endpoint_overlay(&self, overlay: &mut DiagramOverlay) {
        if self.tool != Tool::Move || self.space_held {
            return;
        }
        if let Some(id) = self.diagram_object()
            && self.editor.doc.diagram.as_ref().is_some_and(|d| {
                d.edges
                    .get(&id)
                    .is_some_and(|e| self.selected != Some(e.label))
            })
            && self.editor.doc.locked_ancestor(id).is_none()
            && !self.editor.doc.layer_locks(id).position
            && let Some(ends) = self.diagram_edge_ends(id)
        {
            overlay.connector_ends.extend(ends);
            let cache = self.diagram_hit_cache();
            if let Some((_, _, lines)) = cache.edges.iter().find(|(edge, _, _)| *edge == id) {
                overlay.connector_lines.extend(lines.iter().cloned());
                let curved = self
                    .editor
                    .doc
                    .diagram
                    .as_ref()
                    .unwrap()
                    .edges
                    .get(&id)
                    .is_some_and(|e| {
                        matches!(
                            e.routing,
                            diagram::Routing::Curved | diagram::Routing::Cyclical
                        )
                    });
                for line in lines {
                    let segments = line.windows(2).collect::<Vec<_>>();
                    if curved {
                        let total: f64 = segments
                            .iter()
                            .map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1))
                            .sum();
                        let mut remaining = total / 2.;
                        for p in segments {
                            let length = (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1);
                            if remaining <= length && length > 0. && total * self.view.zoom > 40. {
                                let t = remaining / length;
                                overlay.connector_bends.push((
                                    (
                                        p[0].0 + (p[1].0 - p[0].0) * t,
                                        p[0].1 + (p[1].1 - p[0].1) * t,
                                    ),
                                    (p[1].1 - p[0].1).abs() > (p[1].0 - p[0].0).abs(),
                                ));
                                break;
                            }
                            remaining -= length;
                        }
                    } else {
                        for p in segments {
                            if (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1) * self.view.zoom > 40. {
                                overlay.connector_bends.push((
                                    ((p[0].0 + p[1].0) / 2., (p[0].1 + p[1].1) / 2.),
                                    (p[1].1 - p[0].1).abs() > (p[1].0 - p[0].0).abs(),
                                ));
                            }
                        }
                    }
                }
            }
        }
        let Some(drag) = &self.diagram_ui.endpoint_drag else {
            return;
        };
        let Some(pointer) = self.diagram_ui.pointer else {
            return;
        };
        let Some(mut edge) = self
            .editor
            .doc
            .diagram
            .as_ref()
            .and_then(|d| d.edges.get(&drag.edge))
            .cloned()
        else {
            return;
        };
        if let Some(points) = drag.bend_preview(pointer) {
            edge.waypoints = points[1..points.len() - 1].to_vec();
        } else if let Some(candidate) = self.diagram_endpoint_candidate(pointer) {
            if drag.source {
                edge.source = candidate;
            } else {
                edge.target = candidate;
            }
        } else {
            // An unattached release cancels; show a tether while outside valid targets.
            if let Some(ends) = self.diagram_edge_ends(drag.edge) {
                overlay.preview = vec![ends[usize::from(drag.source)], pointer];
            }
            return;
        }
        if let Some(path) = drag.route.path(&edge) {
            overlay.preview = path
                .flatten((0.5 / self.view.zoom).clamp(0.02, 2.))
                .into_iter()
                .flat_map(|(points, _)| points)
                .collect();
            if drag.segment.is_none() {
                overlay.target = if drag.source {
                    overlay.preview.first().copied()
                } else {
                    overlay.preview.last().copied()
                };
            }
        }
    }

    /// A directly selected object stays editable through overlapping artwork.
    pub(super) fn diagram_active_hit(&self, point: (f64, f64)) -> Option<NodeId> {
        let id = self.diagram_object()?;
        self.diagram_hit_excluding(point, None)
            .filter(|e| e.shape == id)
            .map(|e| e.shape)
            .or_else(|| {
                let cache = self.diagram_hit_cache();
                if cache.edges.iter().any(|(edge, _, _)| *edge == id) {
                    drop(cache);
                    diagram::connector_attachment(&self.editor.doc, id, point)
                        .filter(|(_, d)| *d <= 7. / self.view.zoom)
                        .map(|_| id)
                } else {
                    let (_, kind, [x, y, w, h]) =
                        cache.shapes.iter().find(|(shape, _, _)| *shape == id)?;
                    (!kind.is_container()
                        && point.0 >= *x
                        && point.0 <= x + w
                        && point.1 >= *y
                        && point.1 <= y + h)
                        .then_some(id)
                }
            })
    }
}

impl EndpointDrag {
    fn bend_preview(&self, pointer: (f64, f64)) -> Option<Vec<(f64, f64)>> {
        let (points, index) = self.segment.as_ref()?;
        if let Some(index) = self.curve_point {
            let mut moved = points.clone();
            moved[index] = (
                points[index].0 + pointer.0 - self.start.0,
                points[index].1 + pointer.1 - self.start.1,
            );
            return Some(moved);
        }
        let (a, b) = (points[*index], points[*index + 1]);
        let mut delta = (pointer.0 - self.start.0, pointer.1 - self.start.1);
        if (a.1 - b.1).abs() < 0.001 {
            delta.0 = 0.;
        } else if (a.0 - b.0).abs() < 0.001 {
            delta.1 = 0.;
        }
        if delta == (0., 0.) {
            return Some(points.clone());
        }
        let mut moved = points.clone();
        moved[*index] = (a.0 + delta.0, a.1 + delta.1);
        moved[*index + 1] = (b.0 + delta.0, b.1 + delta.1);
        if *index == 0 {
            moved.insert(0, a);
        }
        if *index + 2 == points.len() {
            moved.push(b);
        }
        Some(moved)
    }
}

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0., 1.);
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}
