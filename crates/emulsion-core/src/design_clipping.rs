//! Derived responsive content clipping. Never modifies authored masks or clip links.
use crate::{Document, NodeId, design_layout};

/// Current breakpoint-resolved frame rectangle, in document coordinates.
pub fn frame_rect(doc: &Document, group: NodeId) -> Option<[f64; 4]> {
    let frame = design_layout::effective_frame(doc, group)?;
    if !frame.clip_content {
        return None;
    }
    let (x, y, w, h) = design_layout::bounds(doc, group)?;
    Some([x, y, w, h])
}

/// Clip applied to an immediate content child. Frame borders retain their full stroke.
pub fn content_rect(doc: &Document, id: NodeId) -> Option<[f64; 4]> {
    let parent = doc.node(id)?.parent?;
    let frame = doc.design.frames.get(&parent)?;
    if frame.boundary == id {
        return None;
    }
    frame_rect(doc, parent)
}

/// Canvas picking observes every clipped ancestor; layer-panel selection is unaffected.
pub fn point_visible(doc: &Document, id: NodeId, point: (f64, f64)) -> bool {
    let mut current = Some(id);
    for _ in 0..=doc.nodes.len() {
        let Some(id) = current else {
            return true;
        };
        if let Some([x, y, w, h]) = content_rect(doc, id)
            && !(point.0 >= x && point.1 >= y && point.0 < x + w && point.1 < y + h)
        {
            return false;
        }
        let Some(node) = doc.node(id) else {
            return false;
        };
        current = node.parent;
    }
    false
}

/// One shared content clip: blend the complete authored stack with a boundary-only
/// baseline. This preserves fractional-edge coverage and sibling clipping indices.
pub(crate) fn composite_children(
    doc: &Document,
    group: NodeId,
    children: Vec<emulsion_raster::CompositeNode>,
) -> Vec<emulsion_raster::CompositeNode> {
    use emulsion_raster::{BlendMode, CompositeNode, composite::NodeContent};
    let Some(rect) = frame_rect(doc, group) else {
        return children;
    };
    let boundary = doc.design.frames[&group].boundary;
    let Some(index) = children.iter().position(|n| n.id == boundary) else {
        return children;
    };
    let mut needed = std::collections::HashSet::from([index]);
    let mut current = children[index].clip_to;
    while let Some(i) = current {
        if i >= children.len() || !needed.insert(i) {
            break;
        }
        current = children[i].clip_to;
    }
    let baseline = children
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if needed.contains(&i) {
                let mut n = n.clone();
                if i != index {
                    n.opacity = 0.;
                }
                n
            } else {
                CompositeNode {
                    id: n.id,
                    visible: false,
                    opacity: 0.,
                    blend: BlendMode::Normal,
                    blending: Default::default(),
                    mask: None,
                    clip_to: None,
                    clip_rect: None,
                    content: NodeContent::Fill([0.; 4]),
                }
            }
        })
        .collect();
    vec![CompositeNode {
        id: group ^ (1 << 61),
        visible: true,
        opacity: 1.,
        blend: BlendMode::PassThrough,
        blending: Default::default(),
        mask: None,
        clip_to: None,
        clip_rect: Some(rect),
        content: NodeContent::ClippedGroup { children, baseline },
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Command, Node,
        command::Slot,
        design_layout::{Child, Frame},
    };
    use emulsion_raster::{Mask, vector::PathStyle, vector_geometry};
    use std::sync::Arc;

    fn add(doc: &mut Document, node: Node, parent: Option<NodeId>) -> NodeId {
        Command::AddNode {
            node: Box::new(node),
            slot: Slot::top_of(parent),
        }
        .apply(doc)
        .unwrap()
        .unwrap()
    }
    #[test]
    fn clipping_is_derived_preserves_masks_and_intersects_ancestor_picking() {
        let mut doc = Document::new(64, 64);
        let outer = add(&mut doc, Node::group(0, "Outer"), None);
        let inner = add(&mut doc, Node::group(0, "Inner"), Some(outer));
        let boundary = |rect: [f64; 4]| {
            Node::path(
                0,
                "Border",
                Arc::new(vector_geometry::rectangle(
                    rect[0], rect[1], rect[2], rect[3],
                )),
                PathStyle {
                    fill: None,
                    stroke: None,
                    ..Default::default()
                },
                64,
                64,
            )
        };
        let ob = add(&mut doc, boundary([10., 10., 30., 30.]), Some(outer));
        let ib = add(&mut doc, boundary([20., 5., 30., 30.]), Some(inner));
        let mut fill = Node::new(
            0,
            "Content",
            crate::NodeKind::Fill {
                rgba: [255, 0, 0, 255],
            },
        );
        let mask = Arc::new(Mask::from_fn(
            64,
            64,
            0,
            |x, _| if x < 30 { 255 } else { 0 },
        ));
        fill.mask = Some(mask.clone());
        let content = add(&mut doc, fill, Some(inner));
        for (id, b) in [(outer, ob), (inner, ib)] {
            doc.design.frames.insert(
                id,
                Frame {
                    boundary: b,
                    clip_content: true,
                    children: doc
                        .children(Some(id))
                        .into_iter()
                        .filter(|c| *c != b)
                        .map(|c| {
                            (
                                c,
                                Child {
                                    absolute: true,
                                    ..Default::default()
                                },
                            )
                        })
                        .collect(),
                    ..Default::default()
                },
            );
        }
        let before = doc.clone();
        assert_eq!(content_rect(&doc, ob), None);
        assert_eq!(content_rect(&doc, ib), None);
        assert!(point_visible(&doc, content, (25., 20.)));
        assert!(
            !point_visible(&doc, content, (25., 7.)),
            "outer clip applies through inner frame"
        );
        assert!(
            !point_visible(&doc, content, (15., 20.)),
            "inner clip also applies"
        );
        assert!(
            point_visible(&doc, ob, (9., 20.)),
            "own border outside stroke stays pickable"
        );
        let tree = doc.composite_tree();
        let pixels = emulsion_raster::composite::flatten(&tree, 0);
        assert!(pixels.get(25, 20)[3] > 60000);
        assert_eq!(
            pixels.get(35, 20)[3],
            0,
            "authored mask still intersects content clip"
        );
        assert_eq!(pixels.get(25, 7)[3], 0);
        assert_eq!(pixels.get(15, 20)[3], 0);
        assert_eq!(
            doc, before,
            "deriving clipping cannot rewrite authored content"
        );
        assert!(Arc::ptr_eq(
            doc.node(content).unwrap().mask.as_ref().unwrap(),
            &mask
        ));
    }
    #[test]
    fn shared_fractional_clip_does_not_accumulate_overlaps_or_square_clip_chains() {
        let mut doc = Document::new(64, 64);
        let group = add(&mut doc, Node::group(0, "Frame"), None);
        let border = add(
            &mut doc,
            Node::path(
                0,
                "Border",
                Arc::new(vector_geometry::rectangle(10.25, 10., 20., 20.)),
                PathStyle {
                    fill: None,
                    stroke: None,
                    ..Default::default()
                },
                64,
                64,
            ),
            Some(group),
        );
        let base = add(
            &mut doc,
            Node::new(
                0,
                "Red",
                crate::NodeKind::Fill {
                    rgba: [255, 0, 0, 255],
                },
            ),
            Some(group),
        );
        let mut top = Node::new(
            0,
            "Blue",
            crate::NodeKind::Fill {
                rgba: [0, 0, 255, 255],
            },
        );
        top.clip_to = Some(base);
        add(&mut doc, top, Some(group));
        doc.design.frames.insert(
            group,
            Frame {
                boundary: border,
                clip_content: true,
                ..Default::default()
            },
        );
        let pixels = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
        for (x, expected) in [(9, 0.), (10, 0.75), (11, 1.), (30, 0.25), (31, 0.)] {
            let pixel = emulsion_raster::color::px_to_f(pixels.get(x, 20));
            assert!(
                (pixel[3] - expected).abs() < 0.0001,
                "x{x}: alpha{} expected{expected}",
                pixel[3]
            );
            assert!(
                pixel[0] < 0.0001,
                "opaque blue must fully cover red before shared clipping"
            );
        }
    }

    #[test]
    fn reordered_boundary_keeps_authored_clipping_dependency_outside_content() {
        let mut doc = Document::new(64, 64);
        let group = add(&mut doc, Node::group(0, "Frame"), None);
        let base = add(
            &mut doc,
            Node::new(
                0,
                "Source",
                crate::NodeKind::Fill {
                    rgba: [255, 0, 0, 255],
                },
            ),
            Some(group),
        );
        let mut border = Node::path(
            0,
            "Border",
            Arc::new(vector_geometry::rectangle(10., 10., 20., 20.)),
            PathStyle {
                fill: None,
                stroke: Some([0, 0, 0, 255]),
                width: 4.,
                ..Default::default()
            },
            64,
            64,
        );
        border.clip_to = Some(base);
        let boundary = add(&mut doc, border, Some(group));
        doc.design.frames.insert(
            group,
            Frame {
                boundary,
                clip_content: true,
                ..Default::default()
            },
        );
        let before = doc.clone();
        let pixels = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
        assert!(
            pixels.get(9, 20)[3] > 60000,
            "boundary stroke outside survives its original clipping dependency"
        );
        assert_eq!(
            pixels.get(5, 20)[3],
            0,
            "transparent dependency source must not leak outside clip"
        );
        assert_eq!(doc, before);
    }
}
