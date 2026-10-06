//! Editable layer fragments for clipboard, templates and local asset placement.
use crate::{
    Command, Document, Editor, Node, NodeId, NodeKind, command::Slot, vector_cache::VectorRaster,
};
use glam::{DAffine2, dvec2};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const SHAPE_ONLY_BACKGROUND_CLIPBOARD_ERROR: &str = "This editable page-background photo frame cannot be copied or pasted as ordinary artwork without changing its appearance. Copy the photo layer alone, or duplicate the Design page instead.";

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
        if let Some(background) = doc.design.page_background
            && let Some(image) = background.image
            && included.contains(&image.boundary)
            && included.contains(&image.image)
            && doc
                .node(image.boundary)
                .is_some_and(crate::design_background::shape_only_boundary)
        {
            // Object paste cannot retain this role, and partial frame copies
            // would erase it below. Reject before replacing clipboard contents
            // or deleting Cut sources, while source metadata proves the role.
            return Err(SHAPE_ONLY_BACKGROUND_CLIPBOARD_ERROR.into());
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

    /// One atomic, undoable object paste, without assuming a workspace kind.
    /// Source page-background roles are not adopted by the destination. IDs,
    /// group parents and clipping relationships are remapped; vector sources
    /// and shared pixel buffers are retained.
    pub fn paste(
        &self,
        editor: &mut Editor,
        slot: Slot,
        offset: (f64, f64),
    ) -> Result<Vec<NodeId>, String> {
        self.paste_with_destination(editor, slot, offset, false)
    }

    /// Destination-aware placement for UI clipboard and asset flows. Only a
    /// confirmed Design page protects its background role before root artwork
    /// is pasted; Photo and Diagram retain ordinary object-paste behavior.
    pub fn paste_into_project(
        &self,
        editor: &mut crate::project::ProjectEditor,
        slot: Slot,
        offset: (f64, f64),
    ) -> Result<Vec<NodeId>, String> {
        let design_destination = editor.kind() == Some(crate::project::ProjectKind::Design);
        self.paste_with_destination(editor, slot, offset, design_destination)
    }

    /// Placement changes the coordinate basis of a complete copied asset.
    /// Mask linkage governs later content edits, not this initial placement.
    /// Prepare and validate detached clones before touching destination history.
    fn placed_nodes(&self, size: (u32, u32), offset: (f64, f64)) -> Result<Vec<Node>, String> {
        let mut prepared = Document::new(size.0, size.1);
        prepared.nodes = self.nodes.clone();
        prepared.validate().map_err(|error| error.to_string())?;
        if self.roots.iter().any(|id| prepared.node(*id).is_none()) {
            return Err("Missing fragment root".into());
        }
        // Component libraries also travel with a fragment. Only explicit roots
        // and their descendants move; dependency definitions retain their basis.
        let roots = crate::layer_links::selected_roots(&prepared, &self.roots)
            .map_err(|error| error.to_string())?;
        let mut moved = HashSet::new();
        for root in roots {
            let subtree = prepared.subtree(root);
            if offset != (0., 0.)
                && !subtree.iter().any(|id| {
                    let node = prepared.node(*id).expect("validated fragment subtree");
                    match &node.kind {
                        NodeKind::Raster { .. }
                        | NodeKind::Smart { .. }
                        | NodeKind::Text { .. } => true,
                        NodeKind::Path { path, .. } => path.anchor_count() > 0,
                        NodeKind::Strokes { strokes, .. } => strokes.bounds().is_some(),
                        NodeKind::Fill { .. } | NodeKind::Adjust(_) => node.has_mask(),
                        NodeKind::Group { .. } => false,
                    }
                })
            {
                return Err(crate::CommandError::NothingToMove(root).to_string());
            }
            moved.extend(subtree);
        }
        for node in &mut prepared.nodes {
            let delta = if moved.contains(&node.id) {
                offset
            } else {
                (0., 0.)
            };
            place_node(node, size, delta)?;
            let mut mappings =
                vec![crate::transform::local_to_document(node).map_err(|e| e.to_string())?];
            // Relative C may cross a horizon. Fragment placement changes the
            // world basis only; finite-grid support, not C's intrinsic AABB,
            // decides whether the retained component can render.
            mappings.push(node.mask_transform);
            if let Some(mask) = crate::smart_filter_mask::descriptor(node) {
                mappings.push(mask.transform);
            }
            if !node.has_projective_metadata() {
                // Finite legacy H and C can still overflow in H*C. Retain the
                // existing source-based world products and finite-only gate;
                // projective components use finite-grid support below instead.
                if node.mask.is_some() {
                    mappings
                        .push(crate::transform::mask_to_document(node).map_err(|e| e.to_string())?);
                }
                if let Some(world) =
                    crate::transform::vector_mask_to_document(node).map_err(|e| e.to_string())?
                {
                    mappings.push(crate::Mapping2::Affine(world));
                }
                if let Some(world) =
                    crate::smart_filter_mask::to_document(node).map_err(|e| e.to_string())?
                {
                    mappings.push(world);
                }
            }
            for map in mappings {
                map.validate_representation().map_err(|e| e.to_string())?;
            }
            crate::smart_support::validate_node(node).map_err(|e| e.to_string())?;
        }
        prepared.validate().map_err(|error| error.to_string())?;
        Ok(prepared.nodes)
    }

    fn paste_with_destination(
        &self,
        editor: &mut Editor,
        slot: Slot,
        offset: (f64, f64),
        design_destination: bool,
    ) -> Result<Vec<NodeId>, String> {
        if editor.in_transaction() {
            return Err("Finish the current edit before placing objects.".into());
        }
        if !offset.0.is_finite() || !offset.1.is_finite() {
            return Err("Invalid placement.".into());
        }
        if self
            .design
            .page_background
            .and_then(|background| background.image)
            .is_some_and(|image| {
                self.nodes.iter().any(|node| {
                    node.id == image.boundary && crate::design_background::shape_only_boundary(node)
                })
            })
        {
            // All object-paste destinations intentionally drop page roles.
            // No native object representation preserves this shape-only role
            // and its existing crop, rotation and visibility controls. Reject
            // before opening a transaction or touching destination history.
            return Err(SHAPE_ONLY_BACKGROUND_CLIPBOARD_ERROR.into());
        }
        let nodes = self.placed_nodes((editor.doc.width, editor.doc.height), offset)?;
        editor.begin("Paste editable objects");
        let result = (|| {
            if design_destination && slot.parent.is_none() && self.design.page_background.is_some()
            {
                crate::design_background::ensure_destination(editor)?;
            }
            // Apply the root floor once, before adding the first copied root.
            // Pinning each insertion alone could reverse pasted root order.
            let slot = if design_destination && slot.parent.is_none() {
                Slot {
                    index: slot
                        .index
                        .max(crate::design_background::foreground_start(&editor.doc)),
                    ..slot
                }
            } else {
                slot
            };
            let mut map = HashMap::new();
            let mut waiting: Vec<_> = nodes.iter().collect();
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
            check_placed_masks(&nodes, &map, &editor.doc)?;
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

fn place_node(node: &mut Node, size: (u32, u32), offset: (f64, f64)) -> Result<(), String> {
    let (dx, dy) = offset;
    let translated = offset != (0., 0.);
    match &mut node.kind {
        NodeKind::Raster { placement, .. }
        | NodeKind::Smart {
            placement: crate::SmartPlacement::Legacy(placement),
            ..
        } => {
            if translated {
                placement.x += dx;
                placement.y += dy;
            }
            // These masks all use intrinsic source coordinates. Retaining C
            // exactly makes their world map T * H * C without resampling or
            // changing the authored linkage, including disabled components.
            return Ok(());
        }
        NodeKind::Smart {
            placement: crate::SmartPlacement::Projective(map),
            ..
        } => {
            if translated {
                let delta = emulsion_raster::projective::Projective2::from_affine(
                    DAffine2::from_translation(dvec2(dx, dy)),
                )
                .map_err(|e| e.to_string())?;
                *map = delta.compose(*map).map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        NodeKind::Path { path, style, cache } => {
            if translated {
                let mut updated = (**path).clone();
                updated.translate(dx, dy);
                *path = Arc::new(updated);
            }
            if path
                .subpaths
                .iter()
                .flat_map(|sub| &sub.anchors)
                .any(|anchor| {
                    [anchor.p, anchor.h_in, anchor.h_out]
                        .iter()
                        .any(|point| !point.0.is_finite() || !point.1.is_finite())
                })
            {
                return Err("Invalid fragment path placement.".into());
            }
            *cache = VectorRaster::path(path.clone(), *style, size.0, size.1);
        }
        NodeKind::Text { spec, cache } => {
            if translated {
                let mut updated = (**spec).clone();
                updated.x = (f64::from(updated.x) + dx) as f32;
                updated.y = (f64::from(updated.y) + dy) as f32;
                *spec = Arc::new(updated);
            }
            if !spec.x.is_finite() || !spec.y.is_finite() {
                return Err("Invalid fragment text placement.".into());
            }
            *cache = VectorRaster::text(spec.clone(), size.0, size.1);
        }
        NodeKind::Strokes { strokes, cache } => {
            if translated {
                let mut updated = (**strokes).clone();
                updated.translate(dx, dy);
                *strokes = Arc::new(updated);
            }
            strokes.validate().map_err(|error| error.to_string())?;
            *cache = VectorRaster::strokes(strokes.clone(), size.0, size.1);
        }
        NodeKind::Group { .. } | NodeKind::Fill { .. } | NodeKind::Adjust(_) => {}
    }
    if translated {
        let translation = DAffine2::from_translation(dvec2(dx, dy));
        // Non-pixel nodes have an identity source basis. Move the affine,
        // preserving the complete raw plane and all off-canvas path detail.
        if node.mask.is_some() {
            node.mask_transform = crate::transform::compose_maps(
                crate::Mapping2::Affine(translation),
                node.mask_transform,
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(mask) = &mut node.vector_mask {
            mask.transform =
                (translation * DAffine2::from_cols_array(&mask.transform)).to_cols_array();
        }
    }
    Ok(())
}

/// Responsive layouts can move or resize inserted content through ordinary
/// editing commands. Do not commit a paste if that separates an unlinked mask
/// from its copied artwork, including a mask attached to an ancestor group.
fn check_placed_masks(
    nodes: &[Node],
    map: &HashMap<NodeId, NodeId>,
    doc: &Document,
) -> Result<(), String> {
    let originals: HashMap<_, _> = nodes.iter().map(|node| (node.id, node)).collect();
    for original in nodes {
        let pasted = doc.node(map[&original.id]).ok_or("Missing placed object")?;
        if same_geometry(&original.kind, &pasted.kind)
            && original.mask_transform == pasted.mask_transform
            && original.vector_mask.as_ref().map(|mask| mask.transform)
                == pasted.vector_mask.as_ref().map(|mask| mask.transform)
            && crate::smart_filter_mask::descriptor(original).map(|mask| mask.transform)
                == crate::smart_filter_mask::descriptor(pasted).map(|mask| mask.transform)
        {
            continue;
        }
        let mut ancestor = Some(original);
        while let Some(node) = ancestor {
            if (node.mask.is_some() && !node.mask_linked)
                || node.vector_mask.as_ref().is_some_and(|mask| !mask.linked)
                || crate::smart_filter_mask::descriptor(node).is_some_and(|mask| !mask.linked)
            {
                return Err("Placement would separate an unlinked mask from its artwork. Paste outside the responsive layout, or link the mask before placing it.".into());
            }
            ancestor = node.parent.and_then(|id| originals.get(&id).copied());
        }
    }
    Ok(())
}

fn same_geometry(before: &NodeKind, after: &NodeKind) -> bool {
    match (before, after) {
        (NodeKind::Raster { placement: a, .. }, NodeKind::Raster { placement: b, .. }) => a == b,
        (NodeKind::Smart { placement: a, .. }, NodeKind::Smart { placement: b, .. }) => a == b,
        (NodeKind::Path { path: a, .. }, NodeKind::Path { path: b, .. }) => a == b,
        (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
            a.transform() == b.transform()
        }
        (NodeKind::Strokes { strokes: a, .. }, NodeKind::Strokes { strokes: b, .. }) => a == b,
        (NodeKind::Group { .. }, NodeKind::Group { .. })
        | (NodeKind::Fill { .. }, NodeKind::Fill { .. })
        | (NodeKind::Adjust(_), NodeKind::Adjust(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "fragment_mask_tests.rs"]
mod mask_tests;

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
