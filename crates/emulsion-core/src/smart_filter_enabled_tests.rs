//! Native enabled-state lifecycle. No interchange codec fixtures are involved.
use crate::{
    Command, Document, Editor, EmptyVectorCoverage, Node, NodeKind, SmartFilterMask, VectorMask,
};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::{BlendMode, Mask, Placement, Raster};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(40, 32);
    let source = Arc::new(Raster::from_fn(8, 6, [0; 4], |x, y| {
        [x as u16 * 2000, y as u16 * 1500, 3000, 32768]
    }));
    let mut node = Node::smart(
        1,
        "Photo",
        source,
        vec![Filter::GaussianBlur { radius: 2. }, Filter::Invert],
        Placement {
            x: 8.,
            y: 7.,
            scale_x: 1.3,
            scale_y: 0.8,
            rotation: 27.,
            ..Default::default()
        },
    );
    node.mask = Some(Arc::new(Mask::empty(8, 6, 180)));
    node.mask_enabled = false;
    node.vector_mask = Some(VectorMask::empty(EmptyVectorCoverage::RevealAll));
    node.vector_mask.as_mut().unwrap().enabled = false;
    if let NodeKind::Smart {
        original_image,
        filter_mask,
        filter_styles,
        ..
    } = &mut node.kind
    {
        // Toggle commands must preserve this opaque resource identity. Encoding
        // and digest checks are independently exercised by native IO tests.
        *original_image = Some(Arc::new(crate::node::OriginalImage::new(
            Arc::new(vec![1, 2, 3]),
            [1; 32],
            [2; 32],
        )));
        *filter_styles = vec![
            FilterStyle::default(),
            FilterStyle {
                enabled: false,
                opacity: 0.37,
                blend: BlendMode::Screen,
            },
        ];
        let mut mask = SmartFilterMask::new(Arc::new(Mask::from_fn(15, 11, 255, |x, _| {
            if x < 6 { 0 } else { 128 }
        })));
        mask.transform =
            crate::Mapping2::Affine(glam::DAffine2::from_cols_array(&[1., 0., 0., 1., -3., -2.]));
        *filter_mask = Some(mask);
    }
    // The second stage is deliberately dormant in the fixture.
    if let NodeKind::Smart {
        source,
        filters,
        filter_styles,
        filters_enabled,
        cache,
        offset,
        ..
    } = &mut node.kind
    {
        (*cache, *offset) =
            crate::smart::render_stack(source, filters, filter_styles, *filters_enabled);
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
fn raw(doc: &Document) -> (&Arc<Raster>, &Arc<Raster>, (i32, i32)) {
    let NodeKind::Smart {
        source,
        cache,
        offset,
        ..
    } = &doc.node(1).unwrap().kind
    else {
        panic!()
    };
    (source, cache, *offset)
}
fn retained(actual: &Document, expected: &Document) {
    let (a, b) = (actual.node(1).unwrap(), expected.node(1).unwrap());
    assert_eq!(
        a.mask.as_ref().map(Arc::as_ptr),
        b.mask.as_ref().map(Arc::as_ptr)
    );
    assert_eq!(
        (a.mask_enabled, a.mask_transform, a.mask_properties),
        (b.mask_enabled, b.mask_transform, b.mask_properties)
    );
    assert_eq!(a.vector_mask, b.vector_mask);
    let (
        NodeKind::Smart {
            source: sa,
            original_image: oa,
            filter_mask: ma,
            placement: crate::SmartPlacement::Legacy(pa),
            ..
        },
        NodeKind::Smart {
            source: sb,
            original_image: ob,
            filter_mask: mb,
            placement: crate::SmartPlacement::Legacy(pb),
            ..
        },
    ) = (&a.kind, &b.kind)
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(sa, sb));
    assert_eq!(oa.as_ref().map(Arc::as_ptr), ob.as_ref().map(Arc::as_ptr));
    assert_eq!((ma, pa), (mb, pb));
}
fn exact(actual: &Document, expected: &Document) {
    assert_eq!(actual, expected);
    retained(actual, expected);
    let (a, b) = (raw(actual), raw(expected));
    assert!(
        Arc::ptr_eq(a.1, b.1),
        "history must restore the exact cache snapshot"
    );
    assert_eq!(a.2, b.2);
}

#[test]
fn root_toggle_preserves_mixed_flags_masks_original_identity_and_exact_undo() {
    let original = document();
    let mut e = Editor::new(original.clone(), None);
    let saved = e.revision;
    let warm = crate::smart_filter_mask::effective_pixels(&e.doc.nodes[0])
        .unwrap()
        .unwrap();
    let mapping = crate::smart_filter_mask::to_document(&e.doc.nodes[0]).unwrap();
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: false,
    })
    .unwrap();
    let disabled = e.doc.clone();
    retained(&disabled, &original);
    let (source, cache, offset) = raw(&disabled);
    assert!(Arc::ptr_eq(source, cache));
    assert_eq!(offset, (0, 0));
    assert!(Arc::ptr_eq(
        source,
        &crate::smart_filter_mask::effective_pixels(&disabled.nodes[0])
            .unwrap()
            .unwrap()
    ));
    assert_eq!(
        crate::smart_filter_mask::to_document(&disabled.nodes[0]).unwrap(),
        mapping
    );
    assert!(e.is_modified());
    assert!(
        matches!(&disabled.nodes[0].kind, NodeKind::Smart { filters_enabled: false, filter_styles, .. } if filter_styles[0].enabled && !filter_styles[1].enabled)
    );
    assert!(e.undo());
    exact(&e.doc, &original);
    assert_eq!(e.revision, saved);
    assert!(!e.is_modified());
    assert!(e.history.can_redo());
    let revision = e.revision;
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: true,
    })
    .unwrap();
    exact(&e.doc, &original);
    assert_eq!(e.revision, revision);
    assert!(e.history.can_redo());
    assert!(e.redo());
    exact(&e.doc, &disabled);
    e.begin("Temporary enabled state");
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: true,
    })
    .unwrap();
    let restored = crate::smart_filter_mask::effective_pixels(&e.doc.nodes[0])
        .unwrap()
        .unwrap();
    assert_eq!(restored.to_pixels(), warm.to_pixels());
    e.cancel();
    exact(&e.doc, &disabled);
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: true,
    })
    .unwrap();
    retained(&e.doc, &original);
    assert_eq!(
        crate::smart_filter_mask::to_document(&e.doc.nodes[0]).unwrap(),
        mapping
    );
    assert_eq!(raw(&e.doc).2, raw(&original).2);
}

#[test]
fn disabled_empty_parent_survives_add_remove_and_parameter_style_edits() {
    let mut e = Editor::new(document(), None);
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: false,
    })
    .unwrap();
    e.execute(Command::SetFilters {
        id: 1,
        filters: vec![],
    })
    .unwrap();
    let empty = e.doc.clone();
    e.execute(Command::SetFilters {
        id: 1,
        filters: vec![Filter::GaussianBlur { radius: 4. }],
    })
    .unwrap();
    assert!(
        matches!(&e.doc.nodes[0].kind, NodeKind::Smart { filters_enabled: false, filter_styles, .. } if filter_styles[0].enabled)
    );
    e.execute(Command::SetFilterStyles {
        id: 1,
        styles: vec![FilterStyle {
            enabled: false,
            opacity: 0.,
            ..Default::default()
        }],
    })
    .unwrap();
    e.execute(Command::SetFilters {
        id: 1,
        filters: vec![Filter::GaussianBlur { radius: 6. }],
    })
    .unwrap();
    assert!(
        matches!(&e.doc.nodes[0].kind, NodeKind::Smart { filters_enabled: false, filter_styles, filters, .. } if !filter_styles[0].enabled && filter_styles[0].opacity == 0. && filters[0] == Filter::GaussianBlur { radius: 6. })
    );
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: true,
    })
    .unwrap();
    let (source, cache, offset) = raw(&e.doc);
    assert!(Arc::ptr_eq(source, cache));
    assert_eq!(offset, (0, 0));
    for _ in 0..4 {
        assert!(e.undo());
    }
    exact(&e.doc, &empty);
}

#[test]
fn replacement_and_rasterize_under_disabled_stack_keep_source_placement() {
    let mut e = Editor::new(document(), None);
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: false,
    })
    .unwrap();
    let before = e.doc.clone();
    let replacement = Arc::new(Raster::solid(8, 6, [0.7, 0.1, 0.2, 0.5]));
    crate::photo_source::replace(&mut e, 1, replacement.clone()).unwrap();
    assert!(Arc::ptr_eq(raw(&e.doc).0, &replacement));
    assert!(Arc::ptr_eq(raw(&e.doc).1, &replacement));
    assert!(
        matches!(&e.doc.nodes[0].kind, NodeKind::Smart { filters_enabled: false, filter_styles, original_image: None, .. } if !filter_styles[1].enabled)
    );
    e.execute(Command::SetFiltersEnabled {
        id: 1,
        enabled: true,
    })
    .unwrap();
    assert!(!Arc::ptr_eq(raw(&e.doc).1, &replacement));
    assert!(e.undo());
    assert!(e.undo());
    exact(&e.doc, &before);
    let source = raw(&before).0.clone();
    let placement = match &before.nodes[0].kind {
        NodeKind::Smart { placement, .. } => placement.require_legacy("legacy fixture").unwrap(),
        _ => panic!(),
    };
    e.execute(Command::Rasterize { id: 1 }).unwrap();
    assert!(
        matches!(&e.doc.nodes[0].kind, NodeKind::Raster { raster, placement: p } if Arc::ptr_eq(raster, &source) && *p == placement)
    );
    assert_eq!(e.doc.nodes[0].mask_enabled, before.nodes[0].mask_enabled);
    assert_eq!(e.doc.nodes[0].vector_mask, before.nodes[0].vector_mask);
    assert!(e.undo());
    exact(&e.doc, &before);
}

#[test]
fn disabled_cache_install_canonicalizes_and_same_state_setters_keep_cache() {
    let mut doc = document();
    let NodeKind::Smart {
        filters,
        filter_styles,
        ..
    } = &doc.nodes[0].kind
    else {
        panic!()
    };
    let (filters, styles) = (filters.clone(), filter_styles.clone());
    Command::SetSmartCache {
        id: 1,
        filters: filters.clone(),
        styles: styles.clone(),
        filters_enabled: false,
        cache: Arc::new(Raster::empty(80, 60, [0; 4])),
        offset: (-10, -10),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(raw(&doc).0, raw(&doc).1));
    assert_eq!(raw(&doc).2, (0, 0));
    let before = doc.clone();
    for cmd in [
        Command::SetFilters {
            id: 1,
            filters: filters.clone(),
        },
        Command::SetFilterStyles {
            id: 1,
            styles: styles.clone(),
        },
        Command::SetFilterStack {
            id: 1,
            filters,
            styles,
        },
    ] {
        cmd.apply(&mut doc).unwrap();
        exact(&doc, &before);
    }
}

#[test]
fn root_visibility_is_locked_read_only_and_fingerprinted() {
    let original = document();
    let mut hidden = original.clone();
    Command::SetFiltersEnabled {
        id: 1,
        enabled: false,
    }
    .apply(&mut hidden)
    .unwrap();
    assert_ne!(
        crate::storyboard_fingerprint::document_fingerprint(&original),
        crate::storyboard_fingerprint::document_fingerprint(&hidden)
    );
    for lock in 0..4 {
        let mut doc = original.clone();
        match lock {
            0 => doc.nodes[0].locks.pixels = true,
            1 => doc.nodes[0].locks.transparency = true,
            2 => {
                let mut group = Node::group(2, "Locked parent");
                group.locked = true;
                doc.nodes[0].parent = Some(2);
                doc.nodes.insert(0, group);
                doc.next_id = 3;
            }
            _ => {}
        }
        let mut e = Editor::new(doc.clone(), None);
        if lock == 3 {
            e.set_read_only(true);
        }
        let revision = e.revision;
        assert!(
            e.execute(Command::SetFiltersEnabled {
                id: 1,
                enabled: false
            })
            .is_err()
        );
        exact(&e.doc, &doc);
        assert_eq!(e.revision, revision);
        assert!(e.history.is_empty());
    }
}

#[test]
fn dormant_mask_editing_preserves_raw_extent_while_filter_bounds_shrink() {
    let mut doc = document();
    Command::SetFiltersEnabled {
        id: 1,
        enabled: false,
    }
    .apply(&mut doc)
    .unwrap();
    let descriptor = crate::smart_filter_mask::descriptor(&doc.nodes[0]).unwrap();
    let padded = crate::smart_filter_mask::pad_to_cache(&doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(&padded.pixels, &descriptor.pixels));
    assert_eq!(padded.transform, descriptor.transform);
    assert_eq!((padded.pixels.width(), padded.pixels.height()), (15, 11));
    let pixels = padded
        .pixels
        .write_rect(emulsion_raster::IRect::new(14, 10, 1, 1), &[0]);
    Command::SetSmartFilterMaskPixels {
        id: 1,
        pixels: Arc::new(pixels),
    }
    .apply(&mut doc)
    .unwrap();
    Command::SetFiltersEnabled {
        id: 1,
        enabled: true,
    }
    .apply(&mut doc)
    .unwrap();
    let descriptor = crate::smart_filter_mask::descriptor(&doc.nodes[0]).unwrap();
    assert_eq!(descriptor.pixels.get(14, 10), 0);
    assert_eq!(
        (descriptor.pixels.width(), descriptor.pixels.height()),
        (15, 11)
    );
}
