//! Direct-call atomicity and retained mapping regressions. Authored source only.
use super::*;
use crate::command::{AlignTarget, Alignment};
use crate::{Node, SmartFilterMask};

fn perspective() -> Projective2 {
    Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.125, 0.0, 1.0]).unwrap()
}

fn smart(id: NodeId, placement: SmartPlacement) -> Node {
    let mut node = Node::smart(
        id,
        "Retained Smart",
        Arc::new(Raster::solid(4, 4, [1.0; 4])),
        vec![],
        Placement::default(),
    );
    let NodeKind::Smart {
        placement: target,
        original_image,
        ..
    } = &mut node.kind
    else {
        unreachable!()
    };
    *target = placement;
    *original_image = Some(Arc::new(crate::node::OriginalImage::new(
        Arc::new(vec![137, 80, 78, 71]),
        [17; 32],
        [29; 32],
    )));
    node
}

fn document(node: Node) -> Document {
    let mut doc = Document::new(32, 24);
    doc.nodes = vec![
        Node::raster(
            1,
            "First",
            Arc::new(Raster::solid(2, 2, [1.0; 4])),
            Placement::at(3.25, 4.5),
        ),
        node,
    ];
    doc.guides.push(crate::document::Guide {
        vertical: true,
        pos: 7.0,
    });
    doc.selection = Some(Arc::new(Mask::from_fn(32, 24, 0, |x, y| {
        if x == 5 && y == 3 { 255 } else { 0 }
    })));
    doc
}

fn mapping_bits(mapping: Mapping2) -> Vec<u64> {
    match mapping {
        Mapping2::Affine(affine) => affine.to_cols_array().map(f64::to_bits).to_vec(),
        Mapping2::Projective(projective) => projective.to_row_major().map(f64::to_bits).to_vec(),
    }
}

fn same_resources(before: &Node, after: &Node) {
    let NodeKind::Smart {
        source,
        cache,
        offset,
        original_image,
        ..
    } = &before.kind
    else {
        panic!("Smart")
    };
    let NodeKind::Smart {
        source: new_source,
        cache: new_cache,
        offset: new_offset,
        original_image: new_original,
        ..
    } = &after.kind
    else {
        panic!("Smart")
    };
    assert!(Arc::ptr_eq(source, new_source));
    assert!(Arc::ptr_eq(cache, new_cache));
    assert_eq!(offset, new_offset);
    let original = original_image.as_ref().unwrap();
    let new_original = new_original.as_ref().unwrap();
    assert!(Arc::ptr_eq(original, new_original));
    assert!(Arc::ptr_eq(original.bytes(), new_original.bytes()));
    assert_eq!(original.encoded_sha256(), new_original.encoded_sha256());
    assert_eq!(original.source_sha256(), new_original.source_sha256());
    if let (Some(mask), Some(new_mask)) = (&before.mask, &after.mask) {
        assert!(Arc::ptr_eq(mask, new_mask));
    }
    if let (Some(mask), Some(new_mask)) = (
        crate::smart_filter_mask::descriptor(before),
        crate::smart_filter_mask::descriptor(after),
    ) {
        assert!(Arc::ptr_eq(&mask.pixels, &new_mask.pixels));
    }
}

fn assert_world_close(actual: Mapping2, expected: Mapping2) {
    for point in [dvec2(0.25, 0.5), dvec2(1.5, 2.5), dvec2(3.25, 1.75)] {
        let a = actual.map_point(point).unwrap();
        let b = expected.map_point(point).unwrap();
        assert!((a - b).abs().max_element() < 1e-7, "{a:?} != {b:?}");
    }
}

#[test]
fn direct_canvas_rebases_retain_linked_unlinked_disabled_and_latent_components() {
    for linked in [false, true] {
        for plane in [false, true] {
            let h = perspective();
            let c = Mapping2::Projective(h.inverse().unwrap());
            let mut node = smart(2, SmartPlacement::Projective(h));
            node.mask_transform = c;
            node.mask_linked = linked;
            node.mask_enabled = false;
            if plane {
                // C crosses the intrinsic forward horizon at x=8. It remains
                // safe on the finite source grid and H*C is identity.
                node.mask = Some(Arc::new(Mask::from_fn(16, 4, 255, |x, y| {
                    if x == 12 && y == 1 { 0 } else { 255 }
                })));
            }
            let mut filter_mask = SmartFilterMask::new(Arc::new(Mask::white(4, 4)));
            filter_mask.transform = Mapping2::Projective(Projective2::IDENTITY);
            filter_mask.linked = linked;
            filter_mask.enabled = false;
            let NodeKind::Smart {
                filter_mask: target,
                cache,
                offset,
                filters_enabled,
                ..
            } = &mut node.kind
            else {
                unreachable!()
            };
            *target = Some(filter_mask);
            // An inactive retained cache is not normalized by a basis edit.
            *cache = Arc::new(Raster::solid(8, 8, [0.5; 4]));
            *offset = (-2, -2);
            *filters_enabled = false;
            let mut doc = document(node);
            doc.validate().unwrap();
            let baseline = doc.node(2).unwrap().clone();
            let raster_bits = mapping_bits(baseline.mask_transform);
            let filter_bits = mapping_bits(
                crate::smart_filter_mask::descriptor(&baseline)
                    .unwrap()
                    .transform,
            );
            crop(&mut doc, IRect::new(2, 1, 16, 12), 0.0).unwrap();
            resize(&mut doc, 32, 7).unwrap();
            rotate_image(&mut doc, 90.0).unwrap();
            let actual = doc.node(2).unwrap();
            assert_eq!(mapping_bits(actual.mask_transform), raster_bits);
            assert_eq!(
                mapping_bits(
                    crate::smart_filter_mask::descriptor(actual)
                        .unwrap()
                        .transform
                ),
                filter_bits
            );
            assert_eq!(actual.mask.is_some(), plane);
            assert_eq!(actual.mask_linked, linked);
            assert!(!actual.mask_enabled);
            same_resources(&baseline, actual);
            assert_eq!((doc.width, doc.height), (7, 32));
        }
    }
}

#[test]
fn image_size_keeps_uniform_width_scale_for_projective_and_legacy_geometry() {
    let h = perspective();
    let mut doc = document(smart(2, SmartPlacement::Projective(h)));
    let old = doc.node(2).unwrap().clone();
    resize(&mut doc, 64, 7).unwrap();
    let expected = Projective2::from_affine(DAffine2::from_scale(dvec2(2.0, 2.0)))
        .unwrap()
        .compose(h)
        .unwrap();
    let NodeKind::Smart { placement, .. } = &doc.node(2).unwrap().kind else {
        panic!()
    };
    assert_eq!(*placement, SmartPlacement::Projective(expected));
    let NodeKind::Raster { placement, .. } = &doc.node(1).unwrap().kind else {
        panic!()
    };
    assert_eq!(
        (
            placement.x,
            placement.y,
            placement.scale_x,
            placement.scale_y
        ),
        (6.5, 9.0, 2.0, 2.0)
    );
    assert_eq!((doc.width, doc.height), (64, 7));
    same_resources(&old, doc.node(2).unwrap());
}

#[test]
fn direct_crop_failure_keeps_all_nodes_guides_canvas_and_selection_arc() {
    let mut doc = document(smart(2, SmartPlacement::Projective(perspective())));
    doc.validate().unwrap();
    let before = doc.clone();
    // The finite input delta puts the projected bounds outside the checked
    // i32 world range after the first legacy node has already been prepared.
    assert!(matches!(
        crop(&mut doc, IRect::new(i32::MIN, 0, 16, 12), 0.0),
        Err(crate::CommandError::Geometry(_))
    ));
    assert_eq!(doc, before);
    assert!(Arc::ptr_eq(
        doc.selection.as_ref().unwrap(),
        before.selection.as_ref().unwrap()
    ));
    same_resources(before.node(2).unwrap(), doc.node(2).unwrap());
}

#[test]
fn direct_resize_failure_is_atomic_after_valid_off_canvas_projective_support() {
    let h = Projective2::from_affine(DAffine2::from_cols_array(&[1e6, 0.0, 0.0, 1e6, 1e9, 1e9]))
        .unwrap();
    let mut doc = document(smart(2, SmartPlacement::Projective(h)));
    doc.validate().unwrap();
    let before = doc.clone();
    // The canvas stays tiny; only projected world bounds overflow. That is a
    // geometry refusal, never a request to allocate that huge off-canvas area.
    assert!(matches!(
        resize(&mut doc, 128, 96),
        Err(crate::CommandError::Geometry(_))
    ));
    assert_eq!(doc, before);
    assert!(Arc::ptr_eq(
        doc.selection.as_ref().unwrap(),
        before.selection.as_ref().unwrap()
    ));
    same_resources(before.node(2).unwrap(), doc.node(2).unwrap());
}

#[test]
fn content_motion_compensates_latent_unlinked_maps_in_projective_and_mixed_states() {
    for placement in [
        SmartPlacement::Legacy(Placement::at(8.0, 6.0)),
        SmartPlacement::Projective(perspective()),
    ] {
        for linked in [false, true] {
            let mut node = smart(2, placement);
            node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
            node.mask_linked = linked;
            node.mask_enabled = false;
            let mut doc = document(node);
            doc.validate().unwrap();
            let baseline = doc.node(2).unwrap().clone();
            let world = crate::transform::mask_to_document(&baseline).unwrap();
            translate_node(&mut doc, 2, 2.0, 3.0).unwrap();
            rotate_node(&mut doc, 2, 90.0).unwrap();
            align_node(&mut doc, 2, Alignment::Left, AlignTarget::Canvas).unwrap();
            let actual = doc.node(2).unwrap();
            assert!(actual.mask.is_none());
            assert!(matches!(actual.mask_transform, Mapping2::Projective(_)));
            if linked {
                assert_eq!(
                    mapping_bits(actual.mask_transform),
                    mapping_bits(baseline.mask_transform)
                );
            } else {
                assert_world_close(crate::transform::mask_to_document(actual).unwrap(), world);
            }
            same_resources(&baseline, actual);
            assert_eq!(
                matches!(
                    actual.kind,
                    NodeKind::Smart {
                        placement: SmartPlacement::Legacy(_),
                        ..
                    }
                ),
                matches!(placement, SmartPlacement::Legacy(_))
            );
        }
    }
}

#[test]
fn direct_subtree_move_refuses_atomically_when_a_later_projected_child_fails() {
    let mut node = smart(2, SmartPlacement::Projective(perspective()));
    node.parent = Some(3);
    let mut doc = document(node);
    doc.node_mut(1).unwrap().parent = Some(3);
    doc.nodes.push(Node::group(3, "Both"));
    doc.validate().unwrap();
    let before = doc.clone();
    assert!(matches!(
        translate_node(&mut doc, 3, f64::from(i32::MAX), 0.0),
        Err(crate::CommandError::Geometry(_))
    ));
    assert_eq!(doc, before);
    assert!(Arc::ptr_eq(
        doc.selection.as_ref().unwrap(),
        before.selection.as_ref().unwrap()
    ));
}

#[test]
fn bounds_errors_propagate_through_groups_and_alignment_without_empty_fallback() {
    let horizon =
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.5, 0.0, 1.0]).unwrap();
    let mut node = smart(2, SmartPlacement::Projective(horizon));
    node.parent = Some(3);
    let mut doc = document(node);
    doc.node_mut(1).unwrap().parent = Some(3);
    doc.nodes.push(Node::group(3, "Unsafe"));
    let before = doc.clone();
    assert!(node_bounds(&doc, 2).is_err());
    assert!(node_bounds(&doc, 3).is_err());
    assert!(align_node(&mut doc, 3, Alignment::Left, AlignTarget::Canvas).is_err());
    assert!(rotate_node(&mut doc, 3, 90.0).is_err());
    assert_eq!(doc, before);
}

#[test]
fn affine_capability_query_refuses_dormant_identity_projective_metadata() {
    let mut node = smart(2, SmartPlacement::Legacy(Placement::default()));
    node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    assert!(node.mask.is_none());
    let doc = document(node);
    assert!(node_bounds(&doc, 2).unwrap().is_some());
    assert!(affine_capability_bounds(&doc, 2).is_none());
    assert!(affine_capability_bounds(&doc, 1).is_some());
}
