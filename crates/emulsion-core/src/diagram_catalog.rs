//! Reuse imported artwork directly from the active diagram without flattening it.
use super::*;
use crate::fragment::Fragment;

#[derive(Clone, Debug, Serialize)]
pub struct DocumentStencil {
    pub source: NodeId,
    pub name: String,
}
pub fn document_stencils(doc: &Document) -> Vec<DocumentStencil> {
    let Some(model) = &doc.diagram else {
        return Vec::new();
    };
    let nodes = doc
        .nodes
        .iter()
        .map(|n| (n.id, n))
        .collect::<HashMap<_, _>>();
    let mut child_counts: HashMap<NodeId, usize> = HashMap::new();
    for node in &doc.nodes {
        if let Some(parent) = node.parent {
            *child_counts.entry(parent).or_default() += 1;
        }
    }
    let mut seen = HashSet::new();
    model
        .shapes
        .iter()
        .filter_map(|(id, shape)| {
            if shape.data.contains_key("emulsion_drawio_endpoint") {
                return None;
            }
            let node = nodes.get(id)?;
            let mut current = Some(*id);
            while let Some(id) = current {
                let node = nodes.get(&id)?;
                if !node.visible {
                    return None;
                }
                current = node.parent;
            }
            let caption = nodes
                .get(&shape.label)
                .and_then(|n| match &n.kind {
                    NodeKind::Text { spec, .. } => Some(spec.text.trim()),
                    _ => None,
                })
                .unwrap_or("");
            let name = if caption.is_empty() {
                node.name.clone()
            } else {
                caption
                    .chars()
                    .take(120)
                    .collect::<String>()
                    .replace('\n', " ")
            };
            // Ordinary shapes share one toolbox entry even when their captions differ.
            // Complex artwork keeps individual entries so different vendor symbols never merge.
            if child_counts.get(id) == Some(&2)
                && node.styles.is_empty()
                && node.mask.is_none()
                && let Some(body) = nodes.get(&shape.body)
                && body.styles.is_empty()
                && body.mask.is_none()
                && let NodeKind::Path { path, style, .. } = &body.kind
                && let Some((x, y, _, _)) = emulsion_raster::vector_geometry::bounds(path)
                && let Some(Node {
                    kind: NodeKind::Text { spec, .. },
                    ..
                }) = nodes.get(&shape.label).copied()
            {
                let mut normalized = (**path).clone();
                normalized.transform(glam::DAffine2::from_translation(glam::dvec2(-x, -y)));
                for anchor in normalized.subpaths.iter_mut().flat_map(|s| &mut s.anchors) {
                    for point in [&mut anchor.p, &mut anchor.h_in, &mut anchor.h_out] {
                        point.0 = (point.0 * 1e6).round() / 1e6;
                        point.1 = (point.1 * 1e6).round() / 1e6;
                    }
                }
                let mut text_style = (**spec).clone();
                text_style.text.clear();
                text_style.x = (text_style.x - x as f32).round();
                text_style.y = (text_style.y - y as f32).round();
                if let Ok(key) = serde_json::to_string(&(
                    shape.kind,
                    normalized,
                    style,
                    node.opacity,
                    body.opacity,
                    text_style,
                )) && !seen.insert(key)
                {
                    return None;
                }
            }

            Some(DocumentStencil { source: *id, name })
        })
        .collect()
}
/// Capture the object's own artwork, excluding contents of a container and its connections.
pub fn document_stencil(doc: &Document, id: NodeId) -> Result<Fragment, String> {
    let model = doc.diagram.as_ref().ok_or("Document has no diagram")?;
    let shape = model
        .shapes
        .get(&id)
        .ok_or("Select a diagram shape to reuse")?;
    if shape.data.contains_key("emulsion_drawio_endpoint") {
        return Err("Loose connector endpoints are not stencils".into());
    }
    let mut children: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for node in &doc.nodes {
        if let Some(parent) = node.parent {
            children.entry(parent).or_default().push(node.id);
        }
    }
    let mut included = HashSet::new();
    let mut queue = vec![id];
    while let Some(next) = queue.pop() {
        if next != id && (model.shapes.contains_key(&next) || model.edges.contains_key(&next)) {
            continue;
        }
        included.insert(next);
        queue.extend(children.get(&next).into_iter().flatten());
    }
    // Start from a bounded document so capture cannot pull nested graph objects back in.
    let mut source = Document::new(doc.width, doc.height);
    source.nodes = doc
        .nodes
        .iter()
        .filter(|n| included.contains(&n.id))
        .cloned()
        .map(|mut n| {
            if n.id == id {
                n.parent = None;
            }
            n
        })
        .collect();
    source.diagram = Some(Arc::new(model.fragment(&included)));
    source.design = doc.design.fragment(&included);
    source.retain_raw_originals(doc);
    Fragment::capture(&source, &[id])
}
pub fn insert_document_stencil(
    editor: &mut Editor,
    source: NodeId,
    center: (f64, f64),
) -> Result<Vec<NodeId>, String> {
    if !center.0.is_finite()
        || !center.1.is_finite()
        || center.0.abs() > 1e6
        || center.1.abs() > 1e6
    {
        return Err("Invalid stencil position".into());
    }
    let fragment = document_stencil(&editor.doc, source)?;
    let model = editor.doc.diagram.as_ref().unwrap();
    let [x, y, w, h] =
        shape_bounds(&editor.doc, &model.shapes[&source]).ok_or("Object has no bounds")?;
    fragment.paste(
        editor,
        Slot::TOP,
        (center.0 - x - w / 2., center.1 - y - h / 2.),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reused_container_excludes_contents_preserves_artwork_and_undo() {
        let mut b = Builder::new(800, 600).unwrap();
        let container = b
            .add_shape(ShapeKind::Container, [40., 40., 300., 250.], "Network")
            .unwrap();
        let inside = b
            .add_shape(ShapeKind::Process, [100., 100., 120., 60.], "Server")
            .unwrap();
        let outside = b
            .add_shape(ShapeKind::Process, [500., 100., 120., 60.], "Client")
            .unwrap();
        b.connect(
            Endpoint {
                shape: inside,
                port: Port::East,
            },
            Endpoint {
                shape: outside,
                port: Port::West,
            },
            "Traffic",
            Routing::Orthogonal,
        )
        .unwrap();
        let mut e = Editor::new(b.finish().unwrap(), None);
        e.execute(Command::MoveNode {
            id: inside,
            slot: Slot::top_of(Some(container)),
        })
        .unwrap();
        let mut model = e.doc.diagram.as_deref().unwrap().clone();
        model.shapes.get_mut(&inside).unwrap().container = Some(container);
        e.execute(Command::SetDiagram {
            diagram: Some(Arc::new(model)),
        })
        .unwrap();
        let before = e.doc.clone();
        let history = e.history.len();
        let ids = insert_document_stencil(&mut e, container, (500., 400.)).unwrap();
        assert_eq!(ids.len(), 1);
        assert_ne!(ids[0], container);
        assert_eq!(e.history.len(), history + 1);
        let model = e.doc.diagram.as_ref().unwrap();
        assert_eq!(model.shapes.len(), 4);
        assert_eq!(model.edges.len(), 1);
        assert_eq!(
            shape_bounds(&e.doc, &model.shapes[&ids[0]]).unwrap(),
            [350., 275., 300., 250.]
        );
        e.undo();
        assert_eq!(e.doc, before);
        let entries = document_stencils(&e.doc);
        assert_eq!(
            entries.len(),
            2,
            "same shape style has one entry despite distinct labels"
        );
        let ids = insert_document_stencil(&mut e, inside, (420., 400.)).unwrap();
        let model = e.doc.diagram.as_ref().unwrap();
        assert_eq!(model.shapes[&ids[0]].container, None);
        assert_eq!(model.edges.len(), 1);
        e.doc.validate().unwrap();
        e.undo();
        assert_eq!(e.doc, before);
    }
}
