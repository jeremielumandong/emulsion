//! Source-only regressions for Smart effective-grid and group style admission.
use super::*;
use crate::{NodeKind, SmartPlacement};
use emulsion_raster::projective::Projective2;

fn scene() -> Document {
    let mut doc = Document::new(32, 24);
    let mut node = Node::smart(
        1,
        "Projected blur",
        Arc::new(Raster::solid(4, 4, [1.; 4])),
        vec![emulsion_filters::Filter::GaussianBlur { radius: 2. }],
        Placement::default(),
    );
    let NodeKind::Smart { placement, .. } = &mut node.kind else {
        panic!()
    };
    *placement = SmartPlacement::Projective(Projective2::IDENTITY);
    node.styles.push(LayerStyle::DropShadow {
        color: [0; 3],
        opacity: 100.,
        angle: 45.,
        distance: 2.,
        size: 1.,
    });
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
#[test]
fn retained_raw_cache_cannot_alias_enabled_bypass_reenable_or_new_source_effects() {
    let mut doc = scene();
    let first_key = key_for(&doc, &doc.nodes[0]).unwrap();
    let first = try_render(&doc, &doc.nodes[0]).unwrap().unwrap();
    let raw = match &doc.nodes[0].kind {
        NodeKind::Smart { cache, .. } => cache.clone(),
        _ => panic!(),
    };
    let NodeKind::Smart {
        filters_enabled, ..
    } = &mut doc.nodes[0].kind
    else {
        panic!()
    };
    *filters_enabled = false;
    doc.validate().unwrap();
    let bypass_key = key_for(&doc, &doc.nodes[0]).unwrap();
    assert_ne!(first_key, bypass_key);
    let bypass = try_render(&doc, &doc.nodes[0]).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&first, &bypass));
    let NodeKind::Smart {
        filters_enabled, ..
    } = &mut doc.nodes[0].kind
    else {
        panic!()
    };
    *filters_enabled = true;
    assert!(Arc::ptr_eq(
        &first,
        &try_render(&doc, &doc.nodes[0]).unwrap().unwrap()
    ));
    let NodeKind::Smart {
        filters_enabled,
        source,
        cache,
        ..
    } = &mut doc.nodes[0].kind
    else {
        panic!()
    };
    *filters_enabled = false;
    *source = Arc::new(Raster::from_fn(4, 4, [0; 4], |x, y| {
        if x == 1 && y == 1 { [65535; 4] } else { [0; 4] }
    }));
    assert!(Arc::ptr_eq(cache, &raw));
    assert_ne!(bypass_key, key_for(&doc, &doc.nodes[0]).unwrap());
    let replacement = try_render(&doc, &doc.nodes[0]).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&bypass, &replacement));
    assert_ne!(
        bypass.below[0].raster.to_srgba8(),
        replacement.below[0].raster.to_srgba8()
    );
}
#[test]
fn per_stage_bypass_changes_style_identity_with_same_raw_cache() {
    let mut doc = scene();
    let before = key_for(&doc, &doc.nodes[0]).unwrap();
    let NodeKind::Smart { filter_styles, .. } = &mut doc.nodes[0].kind else {
        panic!()
    };
    *filter_styles = vec![emulsion_filters::FilterStyle {
        enabled: false,
        ..Default::default()
    }];
    doc.validate().unwrap();
    assert_ne!(before, key_for(&doc, &doc.nodes[0]).unwrap());
    try_render(&doc, &doc.nodes[0]).unwrap().unwrap();
}
#[test]
fn nonempty_projected_styled_group_preserves_model_order_in_solo_render() {
    let mut doc = scene();
    let mut group = Node::group(2, "Effects group");
    group.styles = std::mem::take(&mut doc.nodes[0].styles);
    let mut inner = Node::group(3, "Nested");
    inner.parent = Some(2);
    doc.nodes[0].parent = Some(3);
    doc.nodes.push(inner);
    doc.nodes.push(group);
    doc.next_id = 4;
    doc.validate().unwrap();
    assert!(try_render(&doc, &doc.nodes[2]).unwrap().is_some());
    let tree = doc.try_composite_tree().unwrap();
    assert_eq!(tree.nodes.len(), 1);
    assert!(matches!(
        tree.nodes[0].content,
        NodeContent::StyledGroup { .. }
    ));
}

#[test]
fn padded_projective_effect_canvas_has_independent_resource_admission() {
    let mut doc = Document::new(30_000, 1);
    let mut node = Node::smart(
        1,
        "Wide",
        Arc::new(Raster::solid(30_000, 1, [1.; 4])),
        vec![],
        Placement::default(),
    );
    let NodeKind::Smart { placement, .. } = &mut node.kind else {
        panic!()
    };
    *placement = SmartPlacement::Projective(Projective2::IDENTITY);
    node.styles.push(LayerStyle::DropShadow {
        color: [0; 3],
        opacity: 100.,
        angle: 0.,
        distance: 0.,
        size: 1.,
    });
    doc.nodes.push(node);
    doc.next_id = 2;
    assert!(doc.validate().is_err());
    assert!(try_render(&doc, &doc.nodes[0]).is_err());
    assert!(doc.try_composite_tree().is_err());
    let NodeKind::Smart { placement, .. } = &mut doc.nodes[0].kind else {
        panic!()
    };
    *placement = SmartPlacement::Legacy(Placement::default());
    doc.validate().unwrap();
    let before = doc.clone();
    let delta =
        Projective2::from_affine(glam::DAffine2::from_translation(glam::dvec2(0., 0.25))).unwrap();
    assert!(
        crate::Command::TransformSmartProjective { id: 1, delta }
            .apply(&mut doc)
            .is_err()
    );
    assert_eq!(doc, before);
}

#[test]
fn disabled_group_effects_do_not_traverse_or_require_document_membership() {
    let mut doc = Document::new(4, 4);
    let detached = Node::group(100, "Detached");
    assert!(try_render(&doc, &detached).unwrap().is_none());
    // A proof-sensitive descendant makes an accidental support traversal
    // observable without timing assertions or work proportional to pixel count.
    let mut invalid = Node::raster(
        1,
        "Unadmitted",
        Arc::new(Raster::solid(1, 1, [1.; 4])),
        Placement::default(),
    );
    invalid.parent = Some(100);
    invalid.mask_transform = crate::Mapping2::Projective(Projective2::IDENTITY);
    doc.nodes.push(invalid);
    assert!(try_render(&doc, &detached).unwrap().is_none());
    let mut styled = detached.clone();
    styled.styles.push(LayerStyle::ColorOverlay {
        color: [255, 0, 0],
        opacity: 100.,
    });
    assert!(try_render(&doc, &styled).is_err());
}

#[test]
fn effect_preflight_covers_tile_sample_halos_and_styled_ancestors() {
    for ancestor in [false, true] {
        let mut doc = Document::new(30_000, 1);
        let mut node = Node::smart(
            1,
            "Scaled",
            Arc::new(Raster::solid(14_997, 1, [1.; 4])),
            vec![],
            Placement {
                scale_x: 2.,
                ..Placement::default()
            },
        );
        let shadow = LayerStyle::DropShadow {
            color: [0; 3],
            opacity: 100.,
            angle: 0.,
            distance: 0.,
            size: 1.,
        };
        if ancestor {
            node.parent = Some(2);
            doc.nodes.push(node);
            let mut group = Node::group(2, "Shadow ancestor");
            group.styles.push(shadow);
            doc.nodes.push(group);
        } else {
            node.styles.push(shadow);
            doc.nodes.push(node);
        }
        doc.next_id = 3;
        doc.validate().unwrap();
        let before = doc.clone();
        // Raw scaled ink width29994 plus6 padding would pass. The renderer's
        // tile window can observe a bilinear fringe; the full canvas reserve
        // rejects before either direct or ancestor effects can fail at render.
        let delta =
            Projective2::from_affine(glam::DAffine2::from_translation(glam::dvec2(0., 0.25)))
                .unwrap();
        assert!(
            crate::Command::TransformSmartProjective { id: 1, delta }
                .apply(&mut doc)
                .is_err()
        );
        assert_eq!(doc, before);
    }
}
