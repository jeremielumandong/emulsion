//! Select affected responsive roots without weakening validation or atomic mutation checks.
use super::*;
use std::collections::{BTreeSet, HashMap, HashSet};
struct Index<'a> {
    nodes: HashMap<NodeId, &'a crate::Node>,
    children: HashMap<Option<NodeId>, Vec<NodeId>>,
}
impl<'a> Index<'a> {
    fn new(doc: &'a Document) -> Self {
        let mut children: HashMap<_, Vec<_>> = HashMap::new();
        for node in &doc.nodes {
            children.entry(node.parent).or_default().push(node.id);
        }
        Self {
            nodes: doc.nodes.iter().map(|n| (n.id, n)).collect(),
            children,
        }
    }
    fn root(&self, frames: &BTreeMap<NodeId, Frame>, id: NodeId) -> Option<NodeId> {
        let mut at = Some(id);
        let mut root = None;
        let mut remaining = self.nodes.len() + 1;
        while let Some(id) = at {
            if remaining == 0 {
                return None;
            }
            remaining -= 1;
            if frames.contains_key(&id) {
                root = Some(id);
            }
            at = self.nodes.get(&id).and_then(|n| n.parent);
        }
        root
    }
}
fn affected_roots(before: &Document, next: &Document) -> BTreeSet<NodeId> {
    let old = Index::new(before);
    let new = Index::new(next);
    let mut dirty = HashSet::new();
    for (id, node) in &new.nodes {
        if old.nodes.get(id).copied() != Some(*node) {
            dirty.insert(*id);
        }
    }
    for id in old.nodes.keys() {
        if !new.nodes.contains_key(id) {
            dirty.insert(*id);
        }
    }
    for id in before.design.frames.keys().chain(next.design.frames.keys()) {
        if before.design.frames.get(id) != next.design.frames.get(id) {
            dirty.insert(*id);
        }
    }
    // Reordering siblings changes flow even when every Node value remains identical.
    for parent in old.children.keys().chain(new.children.keys()).flatten() {
        if old.children.get(&Some(*parent)) != new.children.get(&Some(*parent)) {
            dirty.insert(*parent);
        }
    }
    let mut result = BTreeSet::new();
    for id in &dirty {
        for (index, frames) in [(&old, &before.design.frames), (&new, &next.design.frames)] {
            if let Some(root) = index.root(frames, *id)
                && next.design.frames.contains_key(&root)
            {
                result.insert(root);
            }
        }
    }
    // A moved/reparented non-layout ancestor can carry independently rooted frames.
    for id in next.design.frames.keys() {
        let root = new.root(&next.design.frames, *id).unwrap_or(*id);
        if result.contains(&root) {
            continue;
        }
        let mut at = Some(root);
        let mut remaining = new.nodes.len() + 1;
        while let Some(parent) = at {
            if remaining == 0 {
                break;
            }
            remaining -= 1;
            if dirty.contains(&parent) {
                result.insert(root);
                break;
            }
            at = new.nodes.get(&parent).and_then(|n| n.parent);
        }
    }
    // Removed/disabled outer frames expose new independent roots; refresh those descendants.
    for id in before
        .design
        .frames
        .keys()
        .filter(|id| !next.design.frames.contains_key(id))
    {
        for root in next.design.frames.keys() {
            if old.root(&before.design.frames, *root) == Some(*id) {
                result.insert(new.root(&next.design.frames, *root).unwrap_or(*root));
            }
        }
    }
    result
        .into_iter()
        .filter_map(|id| new.root(&next.design.frames, id))
        .collect()
}
pub(crate) fn reflow_after(before: &Document, next: &mut Document) -> Result<(), String> {
    // Diagram synchronization, variable propagation, and movement links may have
    // dependencies outside the layout tree. Keep their conservative full pass until dependencies are explicit.
    if before.width != next.width
        || before.height != next.height
        || before.resolution != next.resolution
        || before.design.fonts != next.design.fonts
        || before.diagram.is_some()
        || next.diagram.is_some()
        || !before.design.variable_bindings.is_empty()
        || !next.design.variable_bindings.is_empty()
        || before
            .nodes
            .iter()
            .chain(&next.nodes)
            .any(|n| n.link_group.is_some())
    {
        return super::reflow(next);
    }
    validate(&next.design.frames, next)?;
    let roots = affected_roots(before, next);
    for id in roots {
        super::reflow_subtree(next, id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Node, command::Slot};
    use emulsion_raster::vector_geometry;
    use std::sync::Arc;
    fn add(e: &mut Editor, node: Node, parent: Option<NodeId>) -> NodeId {
        e.execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot::top_of(parent),
        })
        .unwrap()
        .unwrap()
    }
    fn frame(e: &mut Editor, parent: Option<NodeId>) -> (NodeId, Vec<NodeId>) {
        let group = add(e, Node::group(0, "Frame"), parent);
        let children = (0..3)
            .map(|i| {
                add(
                    e,
                    Node::path(
                        0,
                        "Item",
                        Arc::new(vector_geometry::rectangle(
                            10. + i as f64 * 20.,
                            10.,
                            15.,
                            12.,
                        )),
                        Default::default(),
                        800,
                        600,
                    ),
                    Some(group),
                )
            })
            .collect();
        super::super::enable(
            e,
            group,
            Frame {
                flow: Flow::Row,
                wrap: false,
                ..Default::default()
            },
            (300., 120.),
        )
        .unwrap();
        (group, children)
    }
    fn full_matches(e: &Editor) {
        let mut full = e.doc.clone();
        super::super::reflow(&mut full).unwrap();
        assert_eq!(full, e.doc, "Incremental result differs from full reflow");
    }
    #[test]
    fn incremental_reflow_limits_work_and_matches_full_for_edits_reorder_insert_remove_resize() {
        let mut e = Editor::new(Document::new(800, 600), None);
        let (a, children) = frame(&mut e, None);
        let (b, _) = frame(&mut e, None);
        let before = e.doc.clone();
        let mut next = before.clone();
        next.design.frames.get_mut(&a).unwrap().gap = 23.;
        assert_eq!(affected_roots(&before, &next), BTreeSet::from([a]));
        assert!(!affected_roots(&before, &next).contains(&b));
        e.execute(Command::SetDesign {
            design: Box::new(next.design),
        })
        .unwrap();
        full_matches(&e);
        e.execute(Command::MoveNode {
            id: children[0],
            slot: Slot::top_of(Some(a)),
        })
        .unwrap();
        full_matches(&e);
        let child = add(
            &mut e,
            Node::path(
                0,
                "New",
                Arc::new(vector_geometry::rectangle(0., 0., 25., 30.)),
                Default::default(),
                800,
                600,
            ),
            Some(a),
        );
        full_matches(&e);
        e.execute(Command::RemoveNode { id: child }).unwrap();
        full_matches(&e);
        e.execute(Command::TranslateNode {
            id: children[1],
            dx: 50.,
            dy: 20.,
        })
        .unwrap();
        full_matches(&e);
        let before = e.doc.clone();
        let mut next = before.clone();
        crate::geometry::resize(&mut next, 1000, 700);
        reflow_after(&before, &mut next).unwrap();
        let mut full = next.clone();
        super::super::reflow(&mut full).unwrap();
        assert_eq!(next, full);
    }
    #[test]
    fn nested_metadata_and_ancestor_changes_reflow_from_outer_root_and_locks_fail_atomically() {
        let mut e = Editor::new(Document::new(800, 600), None);
        let (outer, _) = frame(&mut e, None);
        let (inner, children) = frame(&mut e, Some(outer));
        let before = e.doc.clone();
        let mut next = before.clone();
        next.design.frames.get_mut(&inner).unwrap().gap = 8.;
        assert_eq!(affected_roots(&before, &next), BTreeSet::from([outer]));
        e.execute(Command::SetDesign {
            design: Box::new(next.design),
        })
        .unwrap();
        full_matches(&e);
        e.execute(Command::SetLocked {
            id: children[1],
            locked: true,
        })
        .unwrap();
        let before = e.doc.clone();
        let mut design = before.design.clone();
        design.frames.get_mut(&inner).unwrap().gap = 37.;
        assert!(
            e.execute(Command::SetDesign {
                design: Box::new(design)
            })
            .is_err()
        );
        assert_eq!(e.doc, before);
    }
    #[test]
    fn cross_root_movement_links_preserve_full_pass_semantics() {
        let mut e = Editor::new(Document::new(800, 600), None);
        let (a, aa) = frame(&mut e, None);
        let (_, bb) = frame(&mut e, None);
        // Legacy row layout moves linked objects together, including another root.
        e.doc.node_mut(aa[1]).unwrap().link_group = Some(1);
        e.doc.node_mut(bb[1]).unwrap().link_group = Some(1);
        let before = e.doc.clone();
        let mut next = before.clone();
        next.design.frames.get_mut(&a).unwrap().gap = 40.;
        let mut full = next.clone();
        super::super::reflow(&mut full).unwrap();
        reflow_after(&before, &mut next).unwrap();
        assert_eq!(next, full);
    }
    #[test]
    fn container_breakpoints_and_variable_driven_frames_match_full_reflow() {
        let mut e = Editor::new(Document::new(800, 600), None);
        let (outer, _) = frame(&mut e, None);
        let (inner, _) = frame(&mut e, Some(outer));
        let mut design = e.doc.design.clone();
        let nested = design.frames.get_mut(&inner).unwrap();
        nested.breakpoint_reference = BreakpointReference::Container;
        nested.breakpoints = vec![Breakpoint {
            min_width: 400.,
            overrides: FrameOverrides {
                gap: Some(30.),
                ..Default::default()
            },
        }];
        e.execute(Command::SetDesign {
            design: Box::new(design),
        })
        .unwrap();
        full_matches(&e);
        assert_eq!(effective_frame(&e.doc, inner).unwrap().gap, 16.);
        let boundary = e.doc.design.frames[&outer].boundary;
        let (x, y, _, h) = bounds(&e.doc, outer).unwrap();
        let NodeKind::Path { style, .. } = &e.doc.node(boundary).unwrap().kind else {
            panic!()
        };
        let style = *style;
        e.execute(Command::SetPath {
            id: boundary,
            path: Arc::new(vector_geometry::rectangle(x, y, 600., h)),
            style,
        })
        .unwrap();
        assert_eq!(effective_frame(&e.doc, inner).unwrap().gap, 30.);
        full_matches(&e);
        crate::design_variables::set(
            &mut e,
            "Spacing",
            crate::design_variables::Value::Number(12.),
        )
        .unwrap();
        crate::design_variables::bind(
            &mut e,
            &[outer],
            crate::design_variables::Property::FrameGap,
            Some("Spacing"),
        )
        .unwrap();
        full_matches(&e);
        crate::design_variables::set(
            &mut e,
            "Spacing",
            crate::design_variables::Value::Number(25.),
        )
        .unwrap();
        assert_eq!(e.doc.design.frames[&outer].gap, 25.);
        full_matches(&e);
    }
}
