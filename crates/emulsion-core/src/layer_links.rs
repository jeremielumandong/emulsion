//! Persistent movement links and explicit multi-layer layout operations.
use crate::{CommandError, Document, NodeId, command::Alignment};
use emulsion_raster::IRect;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrangeTarget {
    Canvas,
    PixelSelection,
    SelectedLayers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distribution {
    Left,
    HorizontalCenter,
    Right,
    Top,
    VerticalCenter,
    Bottom,
    HorizontalGap,
    VerticalGap,
}

impl Distribution {
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Distribute left edges",
            Self::HorizontalCenter => "Distribute horizontal centers",
            Self::Right => "Distribute right edges",
            Self::Top => "Distribute top edges",
            Self::VerticalCenter => "Distribute vertical centers",
            Self::Bottom => "Distribute bottom edges",
            Self::HorizontalGap => "Distribute horizontal spacing",
            Self::VerticalGap => "Distribute vertical spacing",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrange {
    Align(Alignment),
    Distribute(Distribution),
}

impl Arrange {
    pub fn label(self) -> &'static str {
        match self {
            Self::Align(_) => "Align layers",
            Self::Distribute(distribution) => distribution.label(),
        }
    }
}

/// Selected parents already contain their descendants. Preserve document order.
pub fn selected_roots(doc: &Document, ids: &[NodeId]) -> Result<Vec<NodeId>, CommandError> {
    for id in ids {
        if doc.node(*id).is_none() {
            return Err(CommandError::NoSuchNode(*id));
        }
    }
    let selected: HashSet<_> = ids.iter().copied().collect();
    Ok(doc
        .nodes
        .iter()
        .filter(|n| selected.contains(&n.id) && !has_selected_parent(doc, n.parent, &selected))
        .map(|n| n.id)
        .collect())
}

fn has_selected_parent(doc: &Document, mut parent: Option<NodeId>, ids: &HashSet<NodeId>) -> bool {
    while let Some(id) = parent {
        if ids.contains(&id) {
            return true;
        }
        parent = doc.node(id).and_then(|node| node.parent);
    }
    false
}

/// Expand links, including links belonging to descendants of a moved group.
/// This is shared by pointer movement, keyboard movement, and Free Transform.
pub fn movement_roots(doc: &Document, ids: &[NodeId]) -> Result<Vec<NodeId>, CommandError> {
    selected_roots(doc, ids)?;
    let mut included: HashSet<_> = ids.iter().copied().collect();
    loop {
        let mut changed = false;
        let groups: HashSet<_> = doc
            .nodes
            .iter()
            .filter(|n| included.contains(&n.id))
            .filter_map(|n| n.link_group)
            .collect();
        for node in &doc.nodes {
            if node.parent.is_some_and(|id| included.contains(&id))
                || node.link_group.is_some_and(|group| groups.contains(&group))
            {
                changed |= included.insert(node.id);
            }
        }
        if !changed {
            break;
        }
    }
    selected_roots(doc, &included.into_iter().collect::<Vec<_>>())
}

pub fn check_movable(doc: &Document, ids: &[NodeId]) -> Result<(), CommandError> {
    for id in ids {
        for member in doc.subtree(*id) {
            if let Some(locked) = doc.locked_ancestor(member) {
                return Err(CommandError::Locked(locked));
            }
            if doc.layer_locks(member).position {
                return Err(CommandError::Locked(member));
            }
        }
    }
    Ok(())
}

pub(crate) fn translate(
    doc: &mut Document,
    ids: &[NodeId],
    dx: f64,
    dy: f64,
) -> Result<Option<NodeId>, CommandError> {
    let roots = movement_roots(doc, ids)?;
    check_movable(doc, &roots)?;
    for id in roots {
        crate::geometry::translate_node(doc, id, dx, dy)?;
    }
    Ok(None)
}

pub(crate) fn set_links(
    doc: &mut Document,
    ids: &[NodeId],
    linked: bool,
) -> Result<Option<NodeId>, CommandError> {
    let roots = selected_roots(doc, ids)?;
    if roots.is_empty() || (linked && roots.len() < 2) {
        return Err(CommandError::Empty);
    }
    let mut members: HashSet<_> = roots.iter().copied().collect();
    let mut changes = HashMap::new();
    if linked {
        let groups: HashSet<_> = roots
            .iter()
            .filter_map(|id| doc.node(*id)?.link_group)
            .collect();
        members.extend(
            doc.nodes
                .iter()
                .filter(|n| n.link_group.is_some_and(|g| groups.contains(&g)))
                .map(|n| n.id),
        );
        // A former member can reuse its node ID while its old link set survives.
        // Allocate outside tokens belonging to any unmerged set.
        let used: HashSet<_> = doc
            .nodes
            .iter()
            .filter(|n| !members.contains(&n.id))
            .filter_map(|n| n.link_group)
            .collect();
        let token = (0..)
            .find(|token| !used.contains(token))
            .expect("available link token");
        for id in members {
            changes.insert(id, Some(token));
        }
    } else {
        let affected: HashSet<_> = members
            .iter()
            .filter_map(|id| doc.node(*id)?.link_group)
            .collect();
        for id in members {
            changes.insert(id, None);
        }
        let mut remaining: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for node in &doc.nodes {
            if !changes.contains_key(&node.id)
                && let Some(group) = node.link_group
                && affected.contains(&group)
            {
                remaining.entry(group).or_default().push(node.id);
            }
        }
        for ids in remaining.values().filter(|ids| ids.len() == 1) {
            changes.insert(ids[0], None);
        }
    }
    for (id, value) in &changes {
        if doc.node(*id).is_some_and(|node| node.link_group != *value)
            && let Some(locked) = doc.locked_ancestor(*id)
        {
            return Err(CommandError::Locked(locked));
        }
    }
    for node in &mut doc.nodes {
        if let Some(value) = changes.get(&node.id) {
            node.link_group = *value;
        }
    }
    Ok(None)
}

pub(crate) fn arrange(
    doc: &mut Document,
    ids: &[NodeId],
    operation: Arrange,
    target: ArrangeTarget,
) -> Result<Option<NodeId>, CommandError> {
    let roots = selected_roots(doc, ids)?;
    if roots.is_empty() {
        return Err(CommandError::Empty);
    }
    check_movable(doc, &roots)?;
    let mut items = roots
        .iter()
        .map(|id| {
            crate::geometry::node_bounds(doc, *id)
                .map(|b| (*id, b))
                .ok_or(CommandError::NothingToMove(*id))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bounds = match target {
        ArrangeTarget::Canvas => IRect::new(0, 0, doc.width as i32, doc.height as i32),
        ArrangeTarget::PixelSelection => doc
            .selection
            .as_ref()
            .map(|m| emulsion_raster::select::bounds(m))
            .filter(|b| !b.is_empty())
            .ok_or(CommandError::EmptyAlignmentSelection)?,
        ArrangeTarget::SelectedLayers => items
            .iter()
            .map(|(_, b)| *b)
            .reduce(|a, b| a.union(&b))
            .ok_or(CommandError::Empty)?,
    };
    let mut offsets = Vec::new();
    match operation {
        Arrange::Align(alignment) => {
            for (id, b) in items {
                let (dx, dy) = match alignment {
                    Alignment::Left => ((bounds.x - b.x) as f64, 0.0),
                    Alignment::HorizontalCenter => {
                        ((bounds.x - b.x) as f64 + (bounds.w - b.w) as f64 / 2.0, 0.0)
                    }
                    Alignment::Right => ((bounds.right() - b.right()) as f64, 0.0),
                    Alignment::Top => (0.0, (bounds.y - b.y) as f64),
                    Alignment::VerticalCenter => {
                        (0.0, (bounds.y - b.y) as f64 + (bounds.h - b.h) as f64 / 2.0)
                    }
                    Alignment::Bottom => (0.0, (bounds.bottom() - b.bottom()) as f64),
                };
                offsets.push((id, dx.round(), dy.round()));
            }
        }
        Arrange::Distribute(distribution) => {
            if items.len() < 3 {
                return Err(CommandError::Empty);
            }
            let horizontal = matches!(
                distribution,
                Distribution::Left
                    | Distribution::HorizontalCenter
                    | Distribution::Right
                    | Distribution::HorizontalGap
            );
            let gap = matches!(
                distribution,
                Distribution::HorizontalGap | Distribution::VerticalGap
            );
            let fraction = match distribution {
                Distribution::HorizontalCenter | Distribution::VerticalCenter => 0.5,
                Distribution::Right | Distribution::Bottom => 1.0,
                _ => 0.0,
            };
            let start = |b: IRect| if horizontal { b.x as f64 } else { b.y as f64 };
            let size = |b: IRect| if horizontal { b.w as f64 } else { b.h as f64 };
            let position = |b: IRect| start(b) + fraction * size(b);
            items.sort_by(|a, b| {
                position(a.1)
                    .total_cmp(&position(b.1))
                    .then_with(|| a.0.cmp(&b.0))
            });
            let n = items.len();
            let (first, last) = (items[0].1, items[n - 1].1);
            let (lo, hi) = if target == ArrangeTarget::SelectedLayers && !gap {
                (position(first), position(last))
            } else {
                (
                    start(bounds) + fraction * size(first),
                    start(bounds) + size(bounds) - (1.0 - fraction) * size(last),
                )
            };
            let spacing = if gap {
                (size(bounds) - items.iter().map(|(_, b)| size(*b)).sum::<f64>()) / (n - 1) as f64
            } else {
                (hi - lo) / (n - 1) as f64
            };
            let mut cursor = start(bounds);
            for (i, (id, b)) in items.into_iter().enumerate() {
                let delta = if gap {
                    let d = cursor - start(b);
                    cursor += size(b) + spacing;
                    d
                } else {
                    lo + i as f64 * spacing - position(b)
                };
                offsets.push((
                    id,
                    if horizontal { delta.round() } else { 0.0 },
                    if horizontal { 0.0 } else { delta.round() },
                ));
            }
        }
    }
    // Explicit layout changes relative offsets within a link set; it does not
    // drag every linked member again for each selected item's offset.
    for (id, dx, dy) in offsets {
        if dx != 0.0 || dy != 0.0 {
            crate::geometry::translate_node(doc, id, dx, dy)?;
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Node, command::Slot};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn scene() -> (Document, Vec<NodeId>) {
        let mut doc = Document::new(100, 80);
        let ids = [(5., 7., 10), (30., 20., 20), (75., 40., 10), (90., 60., 5)]
            .into_iter()
            .map(|(x, y, w)| {
                Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        "Layer",
                        Arc::new(Raster::solid(w, 10, [1.; 4])),
                        Placement::at(x, y),
                    )),
                    slot: Slot::TOP,
                }
                .apply(&mut doc)
                .unwrap()
                .unwrap()
            })
            .collect();
        (doc, ids)
    }
    fn x(doc: &Document, id: NodeId) -> i32 {
        crate::geometry::node_bounds(doc, id).unwrap().x
    }
    #[test]
    fn links_survive_selection_changes_move_once_and_undo() {
        let (doc, ids) = scene();
        let mut e = Editor::new(doc, None);
        e.execute(Command::SetLayerLinks {
            ids: ids[..2].to_vec(),
            linked: true,
        })
        .unwrap();
        let linked = e.doc.clone();
        e.execute(Command::TranslateNodes {
            ids: ids[..2].to_vec(),
            dx: 4.,
            dy: 0.,
        })
        .unwrap();
        assert_eq!((x(&e.doc, ids[0]), x(&e.doc, ids[1])), (9, 34));
        assert!(e.undo());
        assert_eq!(e.doc, linked);
        e.execute(Command::TranslateNode {
            id: ids[0],
            dx: 3.,
            dy: 0.,
        })
        .unwrap();
        assert_eq!(x(&e.doc, ids[1]), 33);
        e.execute(Command::SetLayerLinks {
            ids: vec![ids[0]],
            linked: false,
        })
        .unwrap();
        e.execute(Command::TranslateNode {
            id: ids[0],
            dx: 2.,
            dy: 0.,
        })
        .unwrap();
        assert_eq!(x(&e.doc, ids[1]), 33);
        assert!(e.doc.node(ids[1]).unwrap().link_group.is_none());
    }
    #[test]
    fn relinking_former_token_owner_does_not_merge_old_set() {
        let (mut doc, ids) = scene();
        Command::SetLayerLinks {
            ids: ids[..3].to_vec(),
            linked: true,
        }
        .apply(&mut doc)
        .unwrap();
        Command::SetLayerLinks {
            ids: vec![ids[0]],
            linked: false,
        }
        .apply(&mut doc)
        .unwrap();
        Command::SetLayerLinks {
            ids: vec![ids[0], ids[3]],
            linked: true,
        }
        .apply(&mut doc)
        .unwrap();
        assert_ne!(
            doc.node(ids[0]).unwrap().link_group,
            doc.node(ids[1]).unwrap().link_group
        );
        Command::TranslateNode {
            id: ids[0],
            dx: 2.,
            dy: 0.,
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!((x(&doc, ids[1]), x(&doc, ids[3])), (30, 92));
    }
    #[test]
    fn linked_position_lock_rejects_whole_move() {
        let (mut doc, ids) = scene();
        Command::SetLayerLinks {
            ids: ids[..2].to_vec(),
            linked: true,
        }
        .apply(&mut doc)
        .unwrap();
        doc.node_mut(ids[1]).unwrap().locks.position = true;
        let original = doc.clone();
        assert!(
            Command::TranslateNode {
                id: ids[0],
                dx: 9.,
                dy: 0.
            }
            .apply(&mut doc)
            .is_err()
        );
        assert_eq!(doc, original);
        assert!(
            Command::ArrangeLayers {
                ids: ids[..3].to_vec(),
                operation: Arrange::Align(Alignment::Left),
                target: ArrangeTarget::Canvas
            }
            .apply(&mut doc)
            .is_err()
        );
        assert_eq!(doc, original);
    }
    #[test]
    fn distribution_handles_unequal_widths_and_explicit_references() {
        let (mut doc, ids) = scene();
        Command::ArrangeLayers {
            ids: ids[..3].to_vec(),
            operation: Arrange::Distribute(Distribution::HorizontalGap),
            target: ArrangeTarget::SelectedLayers,
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(
            (x(&doc, ids[0]), x(&doc, ids[1]), x(&doc, ids[2])),
            (5, 35, 75)
        );
        Command::ArrangeLayers {
            ids: ids[..3].to_vec(),
            operation: Arrange::Distribute(Distribution::HorizontalCenter),
            target: ArrangeTarget::Canvas,
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(
            (x(&doc, ids[0]), x(&doc, ids[1]), x(&doc, ids[2])),
            (0, 40, 90)
        );
        Command::ArrangeLayers {
            ids: ids[..3].to_vec(),
            operation: Arrange::Align(Alignment::Bottom),
            target: ArrangeTarget::SelectedLayers,
        }
        .apply(&mut doc)
        .unwrap();
        for id in &ids[..3] {
            assert_eq!(
                crate::geometry::node_bounds(&doc, *id).unwrap().bottom(),
                50
            );
        }
        assert_eq!(x(&doc, ids[3]), 90);
    }
}
