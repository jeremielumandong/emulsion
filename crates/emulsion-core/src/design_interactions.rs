//! Persisted click actions and isolated, source-preserving presentation state.
use crate::{Document, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayOperation {
    Show,
    Hide,
    Toggle,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Next,
    Previous,
    Back,
    Slide {
        page: u64,
    },
    Overlay {
        target: NodeId,
        operation: OverlayOperation,
    },
    CloseOverlay,
    Variant {
        target: NodeId,
        variant: String,
    },
}
impl Action {
    pub fn remap(&self, map: &HashMap<NodeId, NodeId>) -> Self {
        let mut action = self.clone();
        match &mut action {
            Self::Overlay { target, .. } | Self::Variant { target, .. } => {
                *target = map.get(target).copied().unwrap_or(*target)
            }
            _ => (),
        };
        action
    }
    pub fn retain_targets(&self, ids: &HashSet<NodeId>) -> bool {
        match self {
            Self::Overlay { target, .. } | Self::Variant { target, .. } => ids.contains(target),
            _ => true,
        }
    }
}
pub fn validate(
    actions: &BTreeMap<NodeId, Vec<Action>>,
    overlays: &BTreeSet<NodeId>,
    doc: &Document,
) -> Result<(), String> {
    if actions.len() > 1024 || overlays.len() > 64 {
        return Err("A page supports 1024 interactive objects and 64 overlays.".into());
    }
    for id in overlays {
        if !doc
            .node(*id)
            .is_some_and(|n| n.is_group() && n.parent.is_none())
        {
            return Err("Presentation overlays must be top-level groups. Move the group out of its parent first.".into());
        }
        if overlays
            .iter()
            .any(|other| other != id && doc.is_ancestor(*other, *id))
        {
            return Err("Overlay groups cannot be nested inside another overlay.".into());
        }
    }
    for (id, list) in actions {
        if doc.node(*id).is_none() || list.is_empty() || list.len() > 8 {
            return Err("Choose an existing object and 1–8 click actions.".into());
        }
        if list.iter().enumerate().any(|(index, action)| {
            matches!(
                action,
                Action::Next | Action::Previous | Action::Back | Action::Slide { .. }
            ) && index + 1 != list.len()
        }) {
            return Err("Slide navigation must be the final click action.".into());
        }
        for action in list {
            match action {
                Action::Slide { page } if *page == 0 => {
                    return Err("Slide IDs must be positive.".into());
                }
                Action::Overlay { target, .. } if !overlays.contains(target) => {
                    return Err("Choose a group marked as a presentation overlay.".into());
                }
                Action::Variant { target, variant } => {
                    if !doc.node(*target).is_some_and(|n| n.is_group())
                        || variant.trim().is_empty()
                        || variant.chars().count() > 80
                    {
                        return Err("Choose a component instance and a valid variant name.".into());
                    }
                    if !doc
                        .design
                        .component_links
                        .get(target)
                        .and_then(|link| doc.design.components.get(&link.component))
                        .is_some_and(|definition| definition.variants.contains_key(variant))
                    {
                        return Err("The component variant is missing.".into());
                    }
                }
                _ => (),
            }
        }
    }
    Ok(())
}

/// Author one object's actions and optional overlay status in one native Undo.
/// Removing an overlay also removes incoming overlay actions on this page.
pub fn author(
    editor: &mut crate::Editor,
    node: NodeId,
    actions: Vec<Action>,
    overlay: Option<bool>,
) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit before changing interactions.".into());
    }
    let locks = editor.doc.layer_locks(node);
    if editor.doc.node(node).is_none()
        || editor.doc.locked_ancestor(node).is_some()
        || locks.position
        || locks.pixels
        || locks.transparency
    {
        return Err("Unlock the interactive object before editing its actions.".into());
    }
    let mut design = editor.doc.design.clone();
    if actions.is_empty() {
        design.interactions.remove(&node);
    } else {
        design.interactions.insert(node, actions);
    }
    if let Some(enabled) = overlay {
        if enabled {
            design.overlays.insert(node);
        } else {
            design.overlays.remove(&node);
            design.interactions.retain(|_, actions| {
                actions
                    .retain(|action| !matches!(action,Action::Overlay{target,..} if *target==node));
                !actions.is_empty()
            });
        }
    }
    editor
        .execute(crate::Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Runtime {
    /// Bottom-to-top overlay order; Escape closes the last overlay first.
    pub open_overlays: Vec<NodeId>,
    pub variants: BTreeMap<NodeId, String>,
    pub generation: u64,
}
impl Runtime {
    pub fn overlay(&mut self, target: NodeId, operation: OverlayOperation) {
        let open = self.open_overlays.contains(&target);
        self.open_overlays.retain(|id| *id != target);
        if operation == OverlayOperation::Show || (operation == OverlayOperation::Toggle && !open) {
            self.open_overlays.push(target);
        }
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn close_overlay(&mut self) -> bool {
        if self.open_overlays.pop().is_some() {
            self.generation = self.generation.wrapping_add(1);
            true
        } else {
            false
        }
    }
    pub fn variant(&mut self, target: NodeId, variant: String) {
        self.variants.insert(target, variant);
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn source(&self, doc: &Document) -> Result<Document, String> {
        if self.variants.is_empty() {
            return Ok(doc.clone());
        }
        let mut editor = crate::Editor::new(doc.clone(), None);
        // Authoring locks do not prevent changing an isolated presentation view.
        for node in &mut editor.doc.nodes {
            node.locked = false;
            node.locks = Default::default();
        }
        for (id, variant) in &self.variants {
            crate::design_components::reset(&mut editor, *id, Some(variant))?;
        }
        Ok(editor.doc)
    }
    pub fn apply(&self, mut doc: Document) -> Document {
        if doc.design.overlays.is_empty() {
            return doc;
        }
        for id in doc.design.overlays.clone() {
            if let Some(node) = doc.node_mut(id) {
                node.visible = self.open_overlays.contains(&id);
            }
        }
        // Raise each open overlay within its parent without modifying authored order.
        for id in &self.open_overlays {
            let Some(parent) = doc.node(*id).map(|n| n.parent) else {
                continue;
            };
            let subtree: HashSet<_> = doc.subtree(*id).into_iter().collect();
            let copies: Vec<_> = doc
                .nodes
                .iter()
                .filter(|n| subtree.contains(&n.id))
                .cloned()
                .collect();
            doc.nodes.retain(|n| !subtree.contains(&n.id));
            let at = parent
                .and_then(|p| doc.nodes.iter().position(|n| n.id == p))
                .unwrap_or(doc.nodes.len());
            doc.nodes.splice(at..at, copies);
        }
        if !self.open_overlays.is_empty() {
            doc.normalize();
        }
        doc
    }
}

fn inside(doc: &Document, id: NodeId, point: (f64, f64)) -> bool {
    let Some(node) = doc.node(id) else {
        return false;
    };
    if !node.visible || node.opacity <= 0. || !crate::design_clipping::point_visible(doc, id, point)
    {
        return false;
    }
    if !crate::geometry::node_bounds(doc, id).is_some_and(|b| {
        point.0 >= f64::from(b.x)
            && point.1 >= f64::from(b.y)
            && point.0 < f64::from(b.right())
            && point.1 < f64::from(b.bottom())
    }) {
        return false;
    }
    if let Some(base) = node.clip_to
        && !inside(doc, base, point)
    {
        return false;
    }
    match &node.kind {
        NodeKind::Group { .. } => doc
            .children(Some(id))
            .into_iter()
            .any(|child| inside(doc, child, point)),
        NodeKind::Adjust(_) => false,
        NodeKind::Raster { raster, placement } => {
            let p = placement
                .to_doc(raster.width(), raster.height())
                .inverse()
                .transform_point2(glam::dvec2(point.0, point.1));
            p.x >= 0.
                && p.y >= 0.
                && p.x < f64::from(raster.width())
                && p.y < f64::from(raster.height())
                && raster.get(p.x as u32, p.y as u32)[3] > 0
        }
        NodeKind::Path { path, style, .. } => {
            let (mut winding, mut stroke) = (0_i32, false);
            for (points, closed) in path.flatten(0.25) {
                for i in 0..points.len() {
                    let (a, b) = (points[i], points[(i + 1) % points.len()]);
                    let cross = (b.0 - a.0) * (point.1 - a.1) - (point.0 - a.0) * (b.1 - a.1);
                    if a.1 <= point.1 && b.1 > point.1 && cross > 0. {
                        winding += 1;
                    }
                    if a.1 > point.1 && b.1 <= point.1 && cross < 0. {
                        winding -= 1;
                    }
                    if closed || i + 1 < points.len() {
                        let length = (b.0 - a.0).powi(2) + (b.1 - a.1).powi(2);
                        let t = if length > 0. {
                            (((point.0 - a.0) * (b.0 - a.0) + (point.1 - a.1) * (b.1 - a.1))
                                / length)
                                .clamp(0., 1.)
                        } else {
                            0.
                        };
                        stroke |= (point.0 - a.0 - t * (b.0 - a.0))
                            .hypot(point.1 - a.1 - t * (b.1 - a.1))
                            <= f64::from(style.width) / 2. + 0.5;
                    }
                }
            }
            style.fill.is_some_and(|c| c[3] > 0) && winding != 0
                || style.stroke.is_some_and(|c| c[3] > 0) && stroke
        }
        _ => true,
    }
}
/// Pick visible native geometry in paint order, then bubble to the nearest action.
/// An open overlay is modal: clicks cannot activate obscured/background actions.
pub fn hit_action(doc: &Document, runtime: &Runtime, point: (f64, f64)) -> Option<NodeId> {
    fn pick(doc: &Document, parent: Option<NodeId>, point: (f64, f64)) -> Option<NodeId> {
        let id = doc
            .children(parent)
            .into_iter()
            .rev()
            .find(|id| inside(doc, *id, point))?;
        if doc.node(id)?.is_group() {
            pick(doc, Some(id), point).or(Some(id))
        } else {
            Some(id)
        }
    }
    let overlay = runtime.open_overlays.last().copied();
    let mut id = if let Some(overlay) = overlay {
        if !inside(doc, overlay, point) {
            return None;
        }
        pick(doc, Some(overlay), point).or(Some(overlay))?
    } else {
        pick(doc, None, point)?
    };
    loop {
        if doc.design.interactions.contains_key(&id) {
            return Some(id);
        }
        if Some(id) == overlay {
            return None;
        }
        id = doc.node(id)?.parent?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Node, command::Slot};
    use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
    fn add(editor: &mut Editor, x: f64) -> NodeId {
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Button",
                    std::sync::Arc::new(rectangle(x, 20., 50., 40.)),
                    PathStyle {
                        fill: Some([255; 4]),
                        stroke: None,
                        ..Default::default()
                    },
                    400,
                    300,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap()
    }
    #[test]
    fn overlays_hit_in_paint_order_keep_authored_document_and_clear_links_with_undo() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let button = add(&mut e, 20.);
        let close = add(&mut e, 120.);
        let overlay = e
            .execute(Command::Group {
                ids: vec![close],
                name: "Modal".into(),
            })
            .unwrap()
            .unwrap();
        author(&mut e, overlay, vec![], Some(true)).unwrap();
        author(
            &mut e,
            button,
            vec![Action::Overlay {
                target: overlay,
                operation: OverlayOperation::Show,
            }],
            None,
        )
        .unwrap();
        author(&mut e, close, vec![Action::CloseOverlay], None).unwrap();
        let original = e.doc.clone();
        let mut runtime = Runtime::default();
        let preview = runtime.apply(original.clone());
        assert!(!preview.node(overlay).unwrap().visible);
        assert_eq!(hit_action(&preview, &runtime, (30., 30.)), Some(button));
        runtime.overlay(overlay, OverlayOperation::Show);
        let preview = runtime.apply(original.clone());
        assert!(preview.node(overlay).unwrap().visible);
        assert_eq!(hit_action(&preview, &runtime, (130., 30.)), Some(close));
        assert_eq!(hit_action(&preview, &runtime, (30., 30.)), None);
        assert!(runtime.close_overlay());
        assert!(!runtime.close_overlay());
        assert_eq!(e.doc, original);
        author(&mut e, overlay, vec![], Some(false)).unwrap();
        assert!(!e.doc.design.interactions.contains_key(&button));
        assert!(e.undo());
        assert_eq!(e.doc, original);
    }
    #[test]
    fn invalid_actions_locked_edits_and_navigation_sequences_are_atomic() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let button = add(&mut e, 20.);
        let before = e.doc.clone();
        for actions in [
            vec![Action::Slide { page: 0 }],
            vec![Action::Next, Action::CloseOverlay],
            vec![Action::Overlay {
                target: button,
                operation: OverlayOperation::Show,
            }],
            vec![Action::Variant {
                target: button,
                variant: "Missing".into(),
            }],
        ] {
            assert!(author(&mut e, button, actions, None).is_err());
            assert_eq!(e.doc, before);
        }
        e.execute(Command::SetLocked {
            id: button,
            locked: true,
        })
        .unwrap();
        let before = e.doc.clone();
        assert!(author(&mut e, button, vec![Action::Next], None).is_err());
        assert_eq!(e.doc, before);
    }
    #[test]
    fn action_remap_and_roundtrip_keep_external_slide_reference() {
        let action = Action::Overlay {
            target: 5,
            operation: OverlayOperation::Toggle,
        };
        let map = HashMap::from([(5, 15)]);
        assert_eq!(
            action.remap(&map),
            Action::Overlay {
                target: 15,
                operation: OverlayOperation::Toggle
            }
        );
        assert!(!action.retain_targets(&HashSet::new()));
        assert_eq!(
            Action::Slide { page: 5 }.remap(&map),
            Action::Slide { page: 5 }
        );
        let actions = vec![
            action,
            Action::CloseOverlay,
            Action::Variant {
                target: 7,
                variant: "Active".into(),
            },
            Action::Back,
        ];
        assert_eq!(
            serde_json::from_value::<Vec<Action>>(serde_json::to_value(&actions).unwrap()).unwrap(),
            actions
        );
    }
    #[test]
    fn component_variant_preview_does_not_mutate_source_or_history() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let member = add(&mut e, 20.);
        let root = crate::design_components::create(&mut e, &[member], "Control").unwrap();
        e.execute(Command::SetOpacity {
            id: member,
            opacity: 0.5,
        })
        .unwrap();
        crate::design_components::update(&mut e, root, Some("Dim")).unwrap();
        let original = e.doc.clone();
        let history = e.history.len();
        let mut runtime = Runtime::default();
        runtime.variant(root, "Default".into());
        let preview = runtime.source(&original).unwrap();
        let member = preview.children(Some(root))[0];
        assert_eq!(preview.node(member).unwrap().opacity, 1.);
        assert_eq!(e.doc, original);
        assert_eq!(e.history.len(), history);
    }
}
