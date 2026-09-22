//! Lossless document-space transforms of editable nodes and mask placement.
use crate::{CommandError, Document, DocumentError, Node, NodeId, NodeKind};
use emulsion_raster::{IRect, Mask, Placement};
use glam::{DAffine2, dvec2};
use std::sync::Arc;

pub fn local_to_document(node: &Node) -> DAffine2 {
    match &node.kind {
        NodeKind::Raster { raster, placement }
        | NodeKind::Smart {
            source: raster,
            placement,
            ..
        } => placement.to_doc(raster.width(), raster.height()),
        _ => DAffine2::IDENTITY,
    }
}
pub fn mask_to_document(node: &Node) -> DAffine2 {
    local_to_document(node) * DAffine2::from_cols_array(&node.mask_transform)
}
pub fn mask_bounds(node: &Node) -> Option<IRect> {
    let mask = node.mask.as_ref()?;
    let coverage = emulsion_raster::select::bounds(mask);
    let b = if coverage.is_empty() {
        mask.bounds()
    } else {
        coverage
    };
    let m = mask_to_document(node);
    let points = [
        dvec2(b.x as f64, b.y as f64),
        dvec2(b.right() as f64, b.y as f64),
        dvec2(b.right() as f64, b.bottom() as f64),
        dvec2(b.x as f64, b.bottom() as f64),
    ]
    .map(|p| m.transform_point2(p));
    let lo = points.into_iter().reduce(|a, b| a.min(b))?;
    let hi = points.into_iter().reduce(|a, b| a.max(b))?;
    Some(IRect::new(
        lo.x.floor() as i32,
        lo.y.floor() as i32,
        (hi.x.ceil() - lo.x.floor()) as i32,
        (hi.y.ceil() - lo.y.floor()) as i32,
    ))
}
fn matrix(values: [f64; 6], id: NodeId) -> Result<DAffine2, CommandError> {
    let m = DAffine2::from_cols_array(&values);
    if !m.is_finite() || m.matrix2.determinant().abs() < 1e-10 {
        return Err(DocumentError::BadValue(id, "singular transform").into());
    }
    Ok(m)
}
fn unlocked(doc: &Document, id: NodeId) -> Result<(), CommandError> {
    if let Some(locked) = doc.locked_ancestor(id) {
        return Err(CommandError::Locked(locked));
    }
    if doc.layer_locks(id).position {
        return Err(CommandError::Locked(id));
    }
    Ok(())
}
pub fn set_mask_transform(
    doc: &mut Document,
    id: NodeId,
    values: [f64; 6],
) -> Result<Option<NodeId>, CommandError> {
    matrix(values, id)?;
    unlocked(doc, id)?;
    let node = doc.node_mut(id).ok_or(CommandError::NoSuchNode(id))?;
    if node.mask.is_none() {
        return Err(CommandError::NoSuchParam(id, "mask".into()));
    }
    node.mask_transform = values;
    Ok(None)
}
pub fn set_placement(
    doc: &mut Document,
    id: NodeId,
    placement: Placement,
) -> Result<Option<NodeId>, CommandError> {
    unlocked(doc, id)?;
    let node = doc.node_mut(id).ok_or(CommandError::NoSuchNode(id))?;
    let old_mask = mask_to_document(node);
    match &mut node.kind {
        NodeKind::Raster { placement: p, .. } | NodeKind::Smart { placement: p, .. } => {
            *p = placement
        }
        _ => return Err(CommandError::NoSuchParam(id, "placement".into())),
    }
    if !node.mask_linked && node.mask.is_some() {
        node.mask_transform = (local_to_document(node).inverse() * old_mask).to_cols_array();
    }
    Ok(None)
}
fn decompose(m: DAffine2, id: NodeId) -> Result<(f64, f64, f64), CommandError> {
    let a = m.matrix2.x_axis;
    let b = m.matrix2.y_axis;
    let sx = a.length();
    let sy = b.length();
    if sx < 1e-8 || sy < 1e-8 || a.dot(b).abs() > 1e-7 * sx * sy {
        return Err(DocumentError::BadValue(
            id,
            "this transform would skew editable content; use proportional scaling",
        )
        .into());
    }
    Ok((
        sx,
        sy * m.matrix2.determinant().signum(),
        a.y.atan2(a.x).to_degrees(),
    ))
}
fn placed(m: DAffine2, w: u32, h: u32, id: NodeId) -> Result<Placement, CommandError> {
    let (sx, sy, rotation) = decompose(m, id)?;
    let mut p = Placement {
        scale_x: sx,
        scale_y: sy.abs(),
        rotation,
        flip_y: sy < 0.,
        ..Default::default()
    };
    let origin = p.to_doc(w, h).translation;
    p.x = m.translation.x - origin.x;
    p.y = m.translation.y - origin.y;
    Ok(p)
}
pub fn transform_nodes(
    doc: &mut Document,
    ids: &[NodeId],
    values: [f64; 6],
) -> Result<Option<NodeId>, CommandError> {
    let m = matrix(values, ids.first().copied().unwrap_or(0))?;
    if m == DAffine2::IDENTITY {
        return Ok(None);
    }
    let roots = crate::layer_links::movement_roots(doc, ids)?;
    let all: std::collections::HashSet<_> = roots.iter().flat_map(|id| doc.subtree(*id)).collect();
    for id in &all {
        unlocked(doc, *id)?;
    }
    let mut result = doc.clone();
    let (w, h) = (doc.width, doc.height);
    for node in &mut result.nodes {
        if !all.contains(&node.id) {
            continue;
        }
        // A full-canvas Fill has no finite geometry until transformed. Give
        // it an opaque canvas-sized mask so scale/rotation move real bounds.
        if matches!(node.kind, NodeKind::Fill { .. }) && node.mask.is_none() {
            node.mask = Some(Arc::new(Mask::from_fn(w, h, 0, |_, _| 255)));
            node.mask_transform = crate::node::default_mask_transform();
        }
        let old_mask = mask_to_document(node);
        match &mut node.kind {
            NodeKind::Raster { raster, placement }
            | NodeKind::Smart {
                source: raster,
                placement,
                ..
            } => {
                *placement = placed(
                    m * placement.to_doc(raster.width(), raster.height()),
                    raster.width(),
                    raster.height(),
                    node.id,
                )?;
            }
            NodeKind::Path { path, style, cache } => {
                let mut updated = (**path).clone();
                updated.transform(m);
                let scale = m.matrix2.determinant().abs().sqrt();
                style.width = (style.width as f64 * scale) as f32;
                for length in &mut style.dash {
                    *length = (*length as f64 * scale) as f32;
                }
                style.dash_offset = (style.dash_offset as f64 * scale) as f32;
                *cache = Arc::new(updated.rasterize(style, w, h));
                *path = Arc::new(updated);
            }
            NodeKind::Text { spec, cache } => {
                let combined = m * spec.transform();
                let (sx, sy, rotation) = decompose(combined, node.id)?;
                let mut updated = (**spec).clone();
                updated.x = combined.translation.x as f32;
                updated.y = combined.translation.y as f32;
                updated.scale_x = sx as f32;
                updated.scale_y = sy as f32;
                updated.rotation = rotation as f32;
                *cache = Arc::new(crate::text::rasterize(&updated, w, h));
                *spec = Arc::new(updated);
            }
            _ => {}
        }
        if node.mask.is_some() {
            let target = if node.mask_linked {
                m * old_mask
            } else {
                old_mask
            };
            node.mask_transform = (local_to_document(node).inverse() * target).to_cols_array();
        }
    }
    *doc = result;
    Ok(None)
}
/// Sample stored mask data through an inverse source transform. Outside pixels
/// retain the mask fill, so white masks and black reveal masks stay distinct.
pub fn sample_mask(mask: &Mask, source: glam::DVec2) -> u8 {
    let p = source - dvec2(0.5, 0.5);
    let x = p.x.floor();
    let y = p.y.floor();
    let get = |x: f64, y: f64| -> f64 {
        if x < 0. || y < 0. || x >= mask.width() as f64 || y >= mask.height() as f64 {
            mask.fill() as f64
        } else {
            mask.get(x as u32, y as u32) as f64
        }
    };
    let ax = p.x - x;
    let ay = p.y - y;
    let top = get(x, y) * (1. - ax) + get(x + 1., y) * ax;
    let bot = get(x, y + 1.) * (1. - ax) + get(x + 1., y + 1.) * ax;
    (top * (1. - ay) + bot * ay).round().clamp(0., 255.) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor};
    #[test]
    fn vector_stroke_dash_measurements_scale_with_shape_and_image() {
        use emulsion_raster::vector::{Path, PathStyle, StrokeAlignment};
        let mut original = Document::new(100, 80);
        original.nodes.push(Node::path(
            1,
            "Dashed shape",
            Arc::new(Path::from_svg("M 20 20 L 60 20 L 60 50 Z").unwrap()),
            PathStyle {
                width: 4.0,
                alignment: StrokeAlignment::Outside,
                dash: [8.0, 4.0, 0.0, 0.0, 0.0, 0.0],
                dash_count: 2,
                dash_offset: 3.0,
                ..Default::default()
            },
            100,
            80,
        ));
        for image in [false, true] {
            let mut doc = original.clone();
            if image {
                Command::ImageSize {
                    width: 200,
                    height: 160,
                }
                .apply(&mut doc)
                .unwrap();
            } else {
                transform_nodes(
                    &mut doc,
                    &[1],
                    DAffine2::from_scale(dvec2(2., 2.)).to_cols_array(),
                )
                .unwrap();
            }
            let NodeKind::Path { style, .. } = &doc.nodes[0].kind else {
                panic!()
            };
            assert_eq!(style.width, 8.);
            assert_eq!(&style.dash[..2], &[16., 8.]);
            assert_eq!(style.dash_offset, 6.);
        }
        let bounds = crate::geometry::node_bounds(&original, 1).unwrap();
        assert!(
            bounds.x <= 16 && bounds.right() >= 64,
            "outside stroke fits geometry bounds: {bounds:?}"
        );
    }
    #[test]
    fn crop_and_image_size_apply_document_mask_affine_once_and_keep_pixel_masks_local() {
        let mut original = Document::new(80, 60);
        let mut fill = Node::new(1, "Fill", NodeKind::Fill { rgba: [255; 4] });
        fill.mask = Some(Arc::new(emulsion_raster::select::rect(
            80, 60, 10., 10., 20., 10.,
        )));
        fill.mask_transform = DAffine2::from_translation(dvec2(5., 3.)).to_cols_array();
        original.nodes.push(fill);
        let mut resized = original.clone();
        Command::ImageSize {
            width: 160,
            height: 120,
        }
        .apply(&mut resized)
        .unwrap();
        let mask = Document::composite_mask(&resized.nodes[0]).unwrap();
        assert_eq!(mask.get(40, 30), 255);
        assert_eq!(mask.get(20, 20), 0);
        assert_eq!(
            resized.nodes[0].mask_transform,
            crate::node::default_mask_transform()
        );
        let mut cropped = original.clone();
        Command::Crop {
            rect: IRect::new(5, 3, 60, 40),
            rotation: 0.,
        }
        .apply(&mut cropped)
        .unwrap();
        let mask = Document::composite_mask(&cropped.nodes[0]).unwrap();
        assert_eq!(mask.get(20, 15), 255);
        assert_eq!(mask.get(5, 5), 0);
        let mut pixels = scene();
        pixels.nodes[0].mask_transform = DAffine2::from_translation(dvec2(3., 2.)).to_cols_array();
        let before = pixels.nodes[0].clone();
        Command::ImageSize {
            width: 400,
            height: 320,
        }
        .apply(&mut pixels)
        .unwrap();
        assert_eq!(pixels.nodes[0].mask_transform, before.mask_transform);
        assert!(Arc::ptr_eq(
            pixels.nodes[0].mask.as_ref().unwrap(),
            before.mask.as_ref().unwrap()
        ));
        let point = dvec2(10., 10.);
        assert!(
            (mask_to_document(&pixels.nodes[0]).transform_point2(point)
                - 2. * mask_to_document(&before).transform_point2(point))
            .length()
                < 1e-8
        );
    }
    #[test]
    fn full_canvas_fill_gets_finite_geometry_when_scaled() {
        let mut doc = Document::new(40, 30);
        doc.nodes.push(Node::new(
            1,
            "Fill",
            NodeKind::Fill {
                rgba: [255, 0, 0, 255],
            },
        ));
        transform_nodes(
            &mut doc,
            &[1],
            DAffine2::from_scale(dvec2(0.5, 0.5)).to_cols_array(),
        )
        .unwrap();
        let mask = Document::composite_mask(&doc.nodes[0]).unwrap();
        assert_eq!(mask.get(5, 5), 255);
        assert_eq!(mask.get(30, 20), 0);
        assert_eq!(doc.nodes[0].mask.as_ref().unwrap().fill(), 0);
    }
    fn scene() -> Document {
        let mut doc = Document::new(200, 160);
        let mut a = Node::raster(
            1,
            "A",
            Arc::new(emulsion_raster::Raster::solid(40, 30, [1.; 4])),
            Placement::at(20., 30.),
        );
        a.mask = Some(Arc::new(emulsion_raster::select::rect(
            40, 30, 5., 5., 20., 15.,
        )));
        let b = Node::raster(
            2,
            "B",
            Arc::new(emulsion_raster::Raster::solid(20, 20, [1.; 4])),
            Placement {
                rotation: 25.,
                ..Placement::at(100., 60.)
            },
        );
        doc.nodes = vec![a, b];
        doc
    }
    #[test]
    fn union_affine_preserves_source_masks_and_undo_cancel_are_atomic() {
        let original = scene();
        let mut editor = Editor::new(original.clone(), None);
        let m = DAffine2::from_translation(dvec2(10., 7.))
            * DAffine2::from_angle(0.4)
            * DAffine2::from_scale(dvec2(2., 2.));
        editor.begin("Transform layers");
        editor
            .preview(Command::TransformNodes {
                ids: vec![1, 2],
                transform: m.to_cols_array(),
            })
            .unwrap();
        for old in &original.nodes {
            let updated = editor.doc.node(old.id).unwrap();
            let NodeKind::Raster { raster: before, .. } = &old.kind else {
                panic!()
            };
            let NodeKind::Raster { raster: after, .. } = &updated.kind else {
                panic!()
            };
            assert!(Arc::ptr_eq(before, after));
            for p in [dvec2(0., 0.), dvec2(15., 10.)] {
                assert!(
                    (local_to_document(updated).transform_point2(p)
                        - (m * local_to_document(old)).transform_point2(p))
                    .length()
                        < 1e-8
                );
            }
            if let Some(mask) = &old.mask {
                assert!(Arc::ptr_eq(mask, updated.mask.as_ref().unwrap()));
            }
        }
        editor.end();
        assert_eq!(editor.history.len(), 1);
        assert!(editor.undo());
        assert_eq!(editor.doc, original);
        editor.begin("Transform layers");
        editor
            .preview(Command::TransformNodes {
                ids: vec![1, 2],
                transform: m.to_cols_array(),
            })
            .unwrap();
        editor.cancel();
        assert_eq!(editor.doc, original);
    }
    #[test]
    fn rotated_nonuniform_skew_and_locked_member_reject_entire_transform() {
        let mut doc = scene();
        let before = doc.clone();
        assert!(
            transform_nodes(
                &mut doc,
                &[1, 2],
                DAffine2::from_scale(dvec2(2., 1.)).to_cols_array()
            )
            .is_err()
        );
        assert_eq!(doc, before);
        doc.nodes[1].locks.position = true;
        let before = doc.clone();
        assert!(
            transform_nodes(
                &mut doc,
                &[1, 2],
                DAffine2::from_scale(dvec2(2., 2.)).to_cols_array()
            )
            .is_err()
        );
        assert_eq!(doc, before);
    }
    #[test]
    fn unlinked_mask_stays_fixed_as_content_moves_and_independent_mask_samples_correctly() {
        let mut doc = scene();
        doc.nodes[0].mask_linked = false;
        let before = mask_to_document(&doc.nodes[0]);
        let stored = doc.nodes[0].mask.clone().unwrap();
        set_placement(&mut doc, 1, Placement::at(30., 35.)).unwrap();
        let actual = mask_to_document(&doc.nodes[0]);
        assert!((actual.translation - before.translation).length() < 1e-8);
        assert!(Arc::ptr_eq(&stored, doc.nodes[0].mask.as_ref().unwrap()));
        let node = &doc.nodes[0];
        let delta = DAffine2::from_translation(dvec2(12., 8.));
        let values =
            (local_to_document(node).inverse() * delta * mask_to_document(node)).to_cols_array();
        set_mask_transform(&mut doc, 1, values).unwrap();
        let mask = Document::composite_mask(&doc.nodes[0]).unwrap();
        assert_eq!(mask.get(10, 12), 255);
        assert_eq!(mask.get(0, 0), 0);
        assert!(Arc::ptr_eq(&stored, doc.nodes[0].mask.as_ref().unwrap()));
    }
    #[test]
    fn group_transform_moves_each_descendant_once_and_keeps_editable_text() {
        let mut doc = scene();
        let mut group = Node::group(3, "Group");
        group.mask = Some(Arc::new(Mask::white(200, 160)));
        doc.nodes[0].parent = Some(3);
        doc.nodes.push(group);
        let mut text = Node::text(
            4,
            "Text",
            crate::text::TextSpec {
                text: "Hello".into(),
                x: 30.,
                y: 40.,
                ..Default::default()
            },
            200,
            160,
        );
        text.parent = Some(3);
        doc.nodes.push(text);
        transform_nodes(
            &mut doc,
            &[3, 1],
            DAffine2::from_translation(dvec2(5., 7.)).to_cols_array(),
        )
        .unwrap();
        let NodeKind::Raster { placement, .. } = &doc.node(1).unwrap().kind else {
            panic!()
        };
        assert_eq!((placement.x, placement.y), (25., 37.));
        let NodeKind::Text { spec, .. } = &doc.node(4).unwrap().kind else {
            panic!()
        };
        assert_eq!((spec.x, spec.y), (35., 47.));
        assert_eq!(spec.text, "Hello");
    }
}
