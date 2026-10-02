//! Editable layer fragments for clipboard, templates and local asset placement.
use crate::{
    Command, Document, Editor, Node, NodeId, NodeKind, command::Slot, vector_cache::VectorRaster,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct Fragment {
    pub design: crate::design_metadata::Design,
    pub diagram: Option<crate::diagram::Diagram>,
    pub nodes: Vec<Node>,
    pub roots: Vec<NodeId>,
    pub raw_originals: Vec<std::path::PathBuf>,
}

impl Fragment {
    pub fn capture(doc: &Document, ids: &[NodeId]) -> Result<Self, String> {
        let mut selected: HashSet<_> = ids.iter().copied().collect();
        if selected.is_empty() || selected.iter().any(|id| doc.node(*id).is_none()) {
            return Err("Select existing layers to copy.".into());
        }
        if let Some(diagram) = &doc.diagram {
            let mut included: HashSet<_> =
                selected.iter().flat_map(|id| doc.subtree(*id)).collect();
            for id in diagram.edge_order()? {
                let edge = &diagram.edges[&id];
                if included.contains(&edge.source.shape) && included.contains(&edge.target.shape) {
                    selected.insert(id);
                    included.insert(id);
                }
            }
        }
        let roots: Vec<_> = doc
            .nodes
            .iter()
            .filter(|node| {
                selected.contains(&node.id)
                    && !selected
                        .iter()
                        .any(|other| *other != node.id && doc.is_ancestor(*other, node.id))
            })
            .map(|node| node.id)
            .collect();
        let mut included: HashSet<_> = roots.iter().flat_map(|id| doc.subtree(*id)).collect();
        // Nested instances carry the transitive dependency library as well.
        let mut components = HashSet::new();
        for _ in 0..=128 {
            let pending: Vec<_> = doc
                .design
                .component_links
                .iter()
                .filter(|(id, link)| included.contains(id) && !components.contains(&link.component))
                .map(|(_, link)| link.component.clone())
                .collect();
            if pending.is_empty() {
                break;
            }
            for name in pending {
                if !components.insert(name.clone()) {
                    continue;
                }
                if let Some(definition) = doc.design.components.get(&name) {
                    for root in definition.variants.values() {
                        included.extend(doc.subtree(*root));
                    }
                }
            }
        }
        let sources = crate::design_components::source_roots(&doc.design);
        let nodes = doc
            .nodes
            .iter()
            .filter(|n| included.contains(&n.id))
            .map(|n| {
                let mut n = n.clone();
                if roots.contains(&n.id) {
                    n.parent = None;
                    n.visible = !sources.contains(&n.id);
                }
                if n.clip_to.is_some_and(|id| !included.contains(&id)) {
                    n.clip_to = None;
                }
                n.link_group = None;
                n
            })
            .collect();
        let mut protected = Document::new(1, 1);
        protected.retain_raw_originals(doc);
        let mut design = doc.design.fragment(&included);
        // Only explicit Design roles travel with clipboard data. Inferring an
        // ordinary Photo document's bottom Fill here would change paste behavior.
        design.page_background = doc.design.page_background;
        design.retain_nodes(&included);
        Ok(Self {
            design,
            diagram: doc.diagram.as_ref().map(|d| d.fragment(&included)),
            nodes,
            roots,
            raw_originals: protected.raw_originals,
        })
    }

    /// One atomic, undoable paste. IDs, group parents and clipping relationships
    /// are remapped; vector sources and shared pixel buffers are retained.
    pub fn paste(
        &self,
        editor: &mut Editor,
        slot: Slot,
        offset: (f64, f64),
    ) -> Result<Vec<NodeId>, String> {
        if editor.in_transaction() {
            return Err("Finish the current edit before placing objects.".into());
        }
        if !offset.0.is_finite() || !offset.1.is_finite() {
            return Err("Invalid placement.".into());
        }
        editor.begin("Paste editable objects");
        let result = (|| {
            if self.design.page_background.is_some() {
                crate::design_background::ensure_destination(editor)?;
            }
            let mut map = HashMap::new();
            let mut waiting: Vec<_> = self.nodes.iter().collect();
            while !waiting.is_empty() {
                let before = waiting.len();
                let mut next = Vec::new();
                for original in waiting {
                    if original.parent.is_some_and(|id| !map.contains_key(&id)) {
                        next.push(original);
                        continue;
                    }
                    let mut node = original.clone();
                    node.locked = false;
                    node.locks = Default::default();
                    let (w, h) = (editor.doc.width, editor.doc.height);
                    match &mut node.kind {
                        NodeKind::Text { spec, cache } => {
                            *cache = VectorRaster::text(spec.clone(), w, h)
                        }
                        NodeKind::Path { path, style, cache } => {
                            *cache = VectorRaster::path(path.clone(), *style, w, h)
                        }
                        NodeKind::Strokes { strokes, cache } => {
                            *cache = VectorRaster::strokes(strokes.clone(), w, h)
                        }
                        _ => {}
                    }
                    let target = match original.parent {
                        Some(parent) => Slot::top_of(Some(map[&parent])),
                        None if !self.roots.contains(&original.id) => Slot::TOP,
                        None => Slot {
                            parent: slot.parent,
                            index: slot.index.saturating_add(
                                self.roots
                                    .iter()
                                    .position(|id| *id == original.id)
                                    .unwrap_or(0),
                            ),
                        },
                    };
                    let id = editor
                        .execute(Command::AddNode {
                            node: Box::new(node),
                            slot: target,
                        })
                        .map_err(|e| e.to_string())?
                        .ok_or("Object was not created")?;
                    map.insert(original.id, id);
                }
                if next.len() == before {
                    return Err("Invalid fragment group hierarchy.".into());
                }
                waiting = next;
            }
            for node in &self.nodes {
                if let Some(base) = node.clip_to.and_then(|id| map.get(&id)) {
                    editor
                        .execute(Command::SetClip {
                            id: map[&node.id],
                            clip_to: Some(*base),
                        })
                        .map_err(|e| e.to_string())?;
                }
            }
            let roots = self
                .roots
                .iter()
                .map(|id| {
                    map.get(id)
                        .copied()
                        .ok_or("Missing fragment root".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            if offset != (0., 0.) {
                editor
                    .execute(Command::TranslateNodes {
                        ids: roots.clone(),
                        dx: offset.0,
                        dy: offset.1,
                    })
                    .map_err(|e| e.to_string())?;
            }
            if !self.design.is_default() {
                let additions = self.design.remap(&map);
                let mut design = editor.doc.design.clone();
                if !additions.motion.is_empty() || !additions.keyframes.is_empty() {
                    design.duration_ms = design.duration_ms.max(additions.duration_ms);
                }
                crate::design_variables::merge_into(&mut design, &additions);
                design.interactions.extend(additions.interactions.clone());
                design
                    .interaction_triggers
                    .extend(additions.interaction_triggers.clone());
                design.overlays.extend(additions.overlays.clone());
                design.local_media.extend(additions.local_media.clone());
                design.data_bindings.extend(additions.data_bindings.clone());
                design.fonts.extend(additions.fonts.clone());
                design.keyframes.extend(additions.keyframes.clone());
                crate::design_styles::merge_into(&mut design, &additions);
                crate::design_components::merge_into(&mut design, &additions);
                design.constraints.extend(additions.constraints);
                design.frames.extend(additions.frames);
                design.charts.extend(additions.charts);
                design.media.extend(additions.media);
                design.motion.extend(additions.motion);
                editor
                    .execute(Command::SetDesign {
                        design: Box::new(design),
                    })
                    .map_err(|e| e.to_string())?;
            }
            if let Some(diagram) = &self.diagram {
                let mut additions = diagram.remap(&map);
                for edge in additions.edges.values_mut() {
                    for point in &mut edge.waypoints {
                        point.0 += offset.0;
                        point.1 += offset.1;
                    }
                }
                let mut model = editor.doc.diagram.as_deref().cloned().unwrap_or_default();
                model.shapes.extend(additions.shapes);
                model.edges.extend(additions.edges);
                model.settings.append_threads(additions.settings)?;
                editor
                    .execute(Command::SetDiagram {
                        diagram: Some(std::sync::Arc::new(model)),
                    })
                    .map_err(|e| e.to_string())?;
            }
            editor
                .execute(Command::SetSelection { selection: None })
                .map_err(|e| e.to_string())?;
            Ok(roots)
        })();
        match result {
            Ok(ids) => {
                for path in &self.raw_originals {
                    if !editor.doc.raw_originals.contains(path) {
                        editor.doc.raw_originals.push(path.clone());
                    }
                }
                editor.end();
                Ok(ids)
            }
            Err(error) => {
                editor.cancel();
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Element, TextPreset};
    #[test]
    fn groups_clips_and_multiple_text_layers_paste_without_rasterization() {
        let mut source = Editor::new(Document::new(600, 400), None);
        let base = source
            .execute(Command::AddNode {
                node: Box::new(Element::Circle.node((600, 400), [30, 60, 90, 255])),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let text = source
            .execute(Command::AddNode {
                node: Box::new(TextPreset::Heading.node((600, 400), "Geist", [255; 4])),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        source
            .execute(Command::SetClip {
                id: text,
                clip_to: Some(base),
            })
            .unwrap();
        let group = source
            .execute(Command::Group {
                ids: vec![base, text],
                name: "Artwork".into(),
            })
            .unwrap()
            .unwrap();
        let fragment = Fragment::capture(&source.doc, &[group, text]).unwrap();
        assert_eq!(fragment.roots, vec![group]);
        let mut target = Editor::new(Document::new(800, 600), None);
        let ids = fragment.paste(&mut target, Slot::TOP, (10., 20.)).unwrap();
        assert_eq!(target.history.len(), 1);
        let children = target.doc.children(Some(ids[0]));
        assert_eq!(children.len(), 2);
        let pasted = target.doc.node(children[1]).unwrap();
        assert_eq!(pasted.clip_to, Some(children[0]));
        let NodeKind::Text { spec, cache } = &pasted.kind else {
            panic!("text must stay editable")
        };
        assert_eq!(spec.text, "Your heading");
        assert_eq!(cache.size(), (800, 600));
        assert!(matches!(
            target.doc.node(children[0]).unwrap().kind,
            NodeKind::Path { .. }
        ));
        target.undo();
        assert!(target.doc.nodes.is_empty());
        target.redo();
        target.doc.validate().unwrap();
    }
    #[test]
    fn failed_paste_is_atomic_and_keeps_existing_history() {
        let source = Document::new(10, 10);
        assert!(Fragment::capture(&source, &[99]).is_err());
        let mut target = Editor::new(source.clone(), None);
        let fragment = Fragment {
            design: Default::default(),
            diagram: None,
            nodes: vec![TextPreset::Body.node((10, 10), "Geist", [0, 0, 0, 255])],
            roots: vec![0],
            raw_originals: Vec::new(),
        };
        assert!(
            fragment
                .paste(&mut target, Slot::top_of(Some(999)), (0., 0.))
                .is_err()
        );
        assert_eq!(target.doc, source);
        assert!(!target.in_transaction());
        assert!(target.history.is_empty());
    }
}
