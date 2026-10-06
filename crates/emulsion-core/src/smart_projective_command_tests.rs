//! Native command contract regressions, authored without runtime execution.
use crate::{Command, Document, Editor, Mapping2, Node, NodeKind, SmartPlacement};
use emulsion_raster::projective::Projective2;
use emulsion_raster::{Mask, Placement, Raster};
use std::sync::Arc;

fn perspective(g: f64) -> Projective2 {
    Projective2::from_row_major([1., 0., 0., 0., 1., 0., g, 0., 1.]).unwrap()
}
fn document() -> Document {
    let mut doc = Document::new(32, 24);
    let mut node = Node::smart(
        1,
        "Retained",
        Arc::new(Raster::solid(4, 4, [0.2, 0.4, 0.6, 1.])),
        vec![],
        Placement::default(),
    );
    let NodeKind::Smart { original_image, .. } = &mut node.kind else {
        unreachable!()
    };
    *original_image = Some(Arc::new(crate::node::OriginalImage::new(
        Arc::new(vec![137, 80, 78, 71, 13, 10]),
        [7; 32],
        [9; 32],
    )));
    doc.nodes.push(node);
    doc.next_id = 2;
    doc.selection = Some(Arc::new(Mask::empty(32, 24, 128)));
    doc
}
fn retained(a: &Node, b: &Node) {
    let NodeKind::Smart {
        source,
        cache,
        offset,
        original_image,
        filters,
        filter_styles,
        filters_enabled,
        editable,
        ..
    } = &a.kind
    else {
        panic!()
    };
    let NodeKind::Smart {
        source: bs,
        cache: bc,
        offset: bo,
        original_image: bi,
        filters: bf,
        filter_styles: bst,
        filters_enabled: be,
        editable: bed,
        ..
    } = &b.kind
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(source, bs));
    assert!(Arc::ptr_eq(cache, bc));
    assert_eq!(offset, bo);
    assert_eq!(filters, bf);
    assert_eq!(filter_styles, bst);
    assert_eq!(filters_enabled, be);
    assert_eq!(editable, bed);
    let (a, b) = (original_image.as_ref().unwrap(), bi.as_ref().unwrap());
    assert!(Arc::ptr_eq(a, b));
    assert!(Arc::ptr_eq(a.bytes(), b.bytes()));
    assert_eq!(a.encoded_sha256(), b.encoded_sha256());
    assert_eq!(a.source_sha256(), b.source_sha256());
}
fn command(delta: Projective2) -> Command {
    Command::TransformSmartProjective { id: 1, delta }
}

#[test]
fn projection_retains_rotated_reflected_source_then_supports_affine_motion_and_single_undo() {
    let mut doc = document();
    let NodeKind::Smart { placement, .. } = &mut doc.nodes[0].kind else {
        panic!()
    };
    *placement = SmartPlacement::Legacy(Placement {
        rotation: 25.,
        flip_x: true,
        ..Placement::at(3., 2.)
    });
    let before = doc.clone();
    let mut editor = Editor::new(doc, None);
    editor.execute(command(perspective(1. / 128.))).unwrap();
    assert_eq!(editor.history.len(), 1);
    retained(&before.nodes[0], &editor.doc.nodes[0]);
    assert!(matches!(
        editor.doc.try_composite_tree().unwrap().nodes[0].content,
        emulsion_raster::composite::NodeContent::ProjectivePixels(_)
    ));
    let projected = editor.doc.clone();
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(editor.redo());
    assert_eq!(editor.doc, projected);
    editor.execute(command(perspective(1. / 256.))).unwrap();
    editor
        .execute(Command::TransformNodes {
            ids: vec![1],
            transform: [1., 0., 0., 1., 2., -1.],
        })
        .unwrap();
    retained(&before.nodes[0], &editor.doc.nodes[0]);
    assert!(matches!(
        editor.doc.nodes[0].kind,
        NodeKind::Smart {
            placement: SmartPlacement::Projective(_),
            ..
        }
    ));
}

#[test]
fn linked_and_unlinked_disabled_masks_keep_metadata_and_world_geometry() {
    for linked in [false, true] {
        let mut doc = document();
        let n = &mut doc.nodes[0];
        n.mask = Some(Arc::new(Mask::from_fn(16, 4, 255, |x, y| {
            if x == 12 && y == 1 { 0 } else { 255 }
        })));
        n.mask_linked = linked;
        n.mask_enabled = false;
        let mut fm = crate::SmartFilterMask::new(Arc::new(Mask::white(16, 4)));
        fm.linked = linked;
        fm.enabled = false;
        let NodeKind::Smart { filter_mask, .. } = &mut n.kind else {
            panic!()
        };
        *filter_mask = Some(fm);
        let before = doc.clone();
        command(perspective(0.125)).apply(&mut doc).unwrap();
        let n = &doc.nodes[0];
        retained(&before.nodes[0], n);
        assert!(!n.mask_enabled);
        assert!(Arc::ptr_eq(
            n.mask.as_ref().unwrap(),
            before.nodes[0].mask.as_ref().unwrap()
        ));
        if linked {
            assert_eq!(n.mask_transform, Mapping2::IDENTITY);
        } else {
            assert!(matches!(n.mask_transform, Mapping2::Projective(_)));
            let world = crate::transform::mask_to_document(n).unwrap();
            for p in [glam::dvec2(1., 1.), glam::dvec2(12., 1.)] {
                assert!((world.map_point(p).unwrap() - p).length() < 1e-6);
            }
            assert!(
                n.mask_transform
                    .map_rect(crate::mapping::source_rect((16, 4)).unwrap())
                    .is_err()
            );
        }
        let old_filter = crate::smart_filter_mask::descriptor(&before.nodes[0]).unwrap();
        let filter = crate::smart_filter_mask::descriptor(n).unwrap();
        assert!(Arc::ptr_eq(&old_filter.pixels, &filter.pixels));
        assert_eq!(filter.enabled, old_filter.enabled);
        assert_eq!(filter.linked, linked);
        assert_eq!(filter.properties, old_filter.properties);
        let actual = crate::smart_filter_mask::to_document(n).unwrap().unwrap();
        let expected = if linked {
            Mapping2::Projective(perspective(0.125))
        } else {
            Mapping2::IDENTITY
        };
        for p in [glam::dvec2(1., 1.), glam::dvec2(12., 1.)] {
            assert!(
                (actual.map_point(p).unwrap() - expected.map_point(p).unwrap()).length() < 1e-6
            );
        }
        if linked {
            assert_eq!(filter.transform, old_filter.transform);
        } else {
            assert!(matches!(filter.transform, Mapping2::Projective(_)));
        }
        doc.try_composite_tree().unwrap();
    }
}

#[test]
fn latent_component_attach_remove_and_conversion_are_checked() {
    let mut doc = document();
    let n = &mut doc.nodes[0];
    n.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    n.mask_linked = false;
    n.mask_enabled = false;
    n.mask_properties.density = 0.5;
    let baseline = n.clone();
    doc.validate().unwrap();
    Command::SetMask {
        id: 1,
        mask: Some(Arc::new(Mask::white(16, 4))),
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(doc.nodes[0].mask_transform, baseline.mask_transform);
    assert_eq!(doc.nodes[0].mask_properties, baseline.mask_properties);
    assert!(!doc.nodes[0].mask_enabled);
    Command::SetMask { id: 1, mask: None }
        .apply(&mut doc)
        .unwrap();
    assert!(doc.nodes[0].mask.is_none());
    assert!(doc.nodes[0].has_projective_metadata());
    let before = doc.clone();
    for c in [
        Command::Rasterize { id: 1 },
        Command::ApplyLayerMask { id: 1 },
        Command::ConvertToLayers { id: 1 },
        Command::SetMaskTransform {
            id: 1,
            transform: [1., 0., 0., 1., 0., 0.],
        },
        Command::SetVectorMask {
            id: 1,
            mask: Some(crate::VectorMask::default()),
        },
    ] {
        assert!(c.apply(&mut doc).is_err());
        assert_eq!(doc, before);
    }
    assert!(matches!(
        doc.try_composite_tree().unwrap().nodes[0].content,
        emulsion_raster::composite::NodeContent::Pixels { .. }
    ));
}

#[test]
fn identity_failure_and_preview_preserve_revision_redo_selection_allocator_and_transaction() {
    let mut editor = Editor::new(document(), None);
    editor
        .execute(Command::Rename {
            id: 1,
            name: "Changed".into(),
        })
        .unwrap();
    editor.undo();
    let before = editor.doc.clone();
    let revision = editor.revision;
    let history = editor.history.len();
    editor.take_dirty();
    editor.execute(command(Projective2::IDENTITY)).unwrap();
    assert_eq!(editor.doc, before);
    assert_eq!(editor.revision, revision);
    assert_eq!(editor.history.len(), history);
    assert!(editor.history.can_redo());
    assert_eq!(editor.take_dirty(), crate::Dirty::Nothing);
    editor.begin("Perspective preview");
    let unsafe_h = perspective(-0.25);
    assert!(editor.preview(command(unsafe_h)).is_err());
    assert_eq!(editor.doc, before);
    assert_eq!(editor.revision, revision);
    assert!(editor.history.can_redo());
    assert!(editor.in_transaction());
    editor.preview(command(perspective(0.125))).unwrap();
    editor.cancel();
    assert_eq!(editor.doc, before);
    assert_eq!(editor.revision, revision);
    assert!(editor.history.can_redo());
    assert_eq!(editor.doc.next_id, before.next_id);
    assert!(Arc::ptr_eq(
        editor.doc.selection.as_ref().unwrap(),
        before.selection.as_ref().unwrap()
    ));
}

#[test]
fn wrong_locked_linked_and_vector_targets_refuse_even_identity() {
    for mode in 0..4 {
        let mut doc = document();
        match mode {
            0 => doc.nodes[0].locks.position = true,
            1 => doc.nodes[0].vector_mask = Some(crate::VectorMask::default()),
            2 => {
                doc.nodes[0].link_group = Some(1);
                let mut b = doc.nodes[0].clone();
                b.id = 2;
                doc.nodes.push(b);
                doc.next_id = 3;
            }
            _ => doc.nodes[0].kind = NodeKind::Fill { rgba: [255; 4] },
        }
        let before = doc.clone();
        assert!(command(Projective2::IDENTITY).apply(&mut doc).is_err());
        assert_eq!(doc, before);
    }
}

#[test]
fn source_basis_replacement_preserves_latent_and_dormant_world_maps() {
    for projected in [false, true] {
        let mut doc = document();
        doc.nodes[0].mask_transform = Mapping2::Projective(Projective2::IDENTITY);
        doc.nodes[0].mask_linked = true;
        if projected {
            let NodeKind::Smart { placement, .. } = &mut doc.nodes[0].kind else {
                panic!()
            };
            *placement = SmartPlacement::Projective(perspective(0.125));
        }
        let before = crate::transform::mask_to_document(&doc.nodes[0]).unwrap();
        let mut editor = Editor::try_new(doc, None).unwrap();
        crate::photo_source::replace(&mut editor, 1, Arc::new(Raster::solid(2, 2, [1.; 4])))
            .unwrap();
        let after = crate::transform::mask_to_document(&editor.doc.nodes[0]).unwrap();
        for p in [glam::dvec2(0.5, 0.5), glam::dvec2(1., 1.)] {
            assert!((before.map_point(p).unwrap() - after.map_point(p).unwrap()).length() < 1e-6);
        }
        assert!(editor.doc.nodes[0].mask.is_none());
        assert!(matches!(
            editor.doc.nodes[0].mask_transform,
            Mapping2::Projective(_)
        ));
        let NodeKind::Smart { original_image, .. } = &editor.doc.nodes[0].kind else {
            panic!()
        };
        assert!(original_image.is_none());
    }
}

#[test]
fn prospective_blur_and_actual_worker_grid_fail_without_publication() {
    let mut doc = document();
    command(perspective(0.125)).apply(&mut doc).unwrap();
    let before = doc.clone();
    let filters = vec![emulsion_filters::Filter::GaussianBlur { radius: 30. }];
    assert!(
        Command::SetFilters { id: 1, filters }
            .apply(&mut doc)
            .is_err()
    );
    assert_eq!(doc, before);
    let c = Command::SetSmartCache {
        id: 1,
        filters: vec![emulsion_filters::Filter::Invert],
        styles: vec![Default::default()],
        filters_enabled: true,
        cache: Arc::new(Raster::solid(5, 4, [1.; 4])),
        offset: (0, 0),
    };
    assert!(c.apply(&mut doc).is_err());
    assert_eq!(doc, before);
}

#[test]
fn pending_mask_metadata_never_substitutes_for_detail_support() {
    let mut doc = document();
    let node = &mut doc.nodes[0];
    node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    node.mask = Some(Arc::new(Mask::from_fn(30_000, 1, 0, |x, _| {
        if x == 1 { 255 } else { 0 }
    })));
    node.mask_properties.feather = 1.;
    let metadata = crate::smart_support::metadata_for_node(node).unwrap();
    crate::smart_support::preflight_pending_mask_resources(metadata).unwrap();
    assert!(crate::smart_support::preflight_stack_support(metadata).is_err());
    assert!(doc.validate().is_err());
    doc.nodes[0].mask = Some(Arc::new(Mask::empty(30_000, 1, 0)));
    doc.validate().unwrap();
}

#[test]
fn latent_identity_changes_fingerprint_and_tiny_authored_edits_remain_edits() {
    let a = document();
    let mut b = a.clone();
    b.nodes[0].mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    assert_ne!(a, b);
    assert_ne!(
        crate::storyboard_fingerprint::document_fingerprint(&a),
        crate::storyboard_fingerprint::document_fingerprint(&b)
    );
    let mut editor = Editor::new(a, None);
    let tiny = Projective2::from_row_major([1., 0., 1e-12, 0., 1., 0., 0., 0., 1.]).unwrap();
    editor.execute(command(tiny)).unwrap();
    assert_eq!(editor.history.len(), 1);
    assert!(matches!(
        editor.doc.nodes[0].kind,
        NodeKind::Smart {
            placement: SmartPlacement::Projective(_),
            ..
        }
    ));
}

#[test]
fn mixed_legacy_motion_never_demotes_identity_projective_components() {
    let mut doc = document();
    doc.nodes[0].mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    doc.nodes[0].mask_linked = false;
    let mut mask = crate::SmartFilterMask::new(Arc::new(Mask::white(4, 4)));
    mask.transform = Mapping2::Projective(Projective2::IDENTITY);
    mask.linked = false;
    mask.enabled = false;
    let NodeKind::Smart { filter_mask, .. } = &mut doc.nodes[0].kind else {
        panic!()
    };
    *filter_mask = Some(mask);
    Command::SetPlacement {
        id: 1,
        placement: Placement::at(2., 3.),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(matches!(
        doc.nodes[0].mask_transform,
        Mapping2::Projective(_)
    ));
    assert!(matches!(
        crate::smart_filter_mask::descriptor(&doc.nodes[0])
            .unwrap()
            .transform,
        Mapping2::Projective(_)
    ));
    assert!(matches!(
        doc.nodes[0].kind,
        NodeKind::Smart {
            placement: SmartPlacement::Legacy(_),
            ..
        }
    ));
}

#[test]
fn direct_mapping_queries_reject_unsupported_component_owners() {
    let mut raster = Node::raster(
        1,
        "Invalid latent",
        Arc::new(Raster::solid(1, 1, [1.; 4])),
        Placement::default(),
    );
    raster.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    assert!(crate::transform::local_to_document(&raster).is_err());
    assert!(crate::transform::mask_to_document(&raster).is_err());
    let mut smart = document().nodes.remove(0);
    smart.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    smart.vector_mask = Some(crate::VectorMask::default());
    assert!(crate::transform::local_to_document(&smart).is_err());
}

#[test]
fn parent_projection_retains_opaque_nested_archive_without_rewriting_it() {
    let mut doc = document();
    let archive = Arc::new(vec![9, 0, 16, 42, 255]);
    let NodeKind::Smart {
        editable,
        original_image,
        ..
    } = &mut doc.nodes[0].kind
    else {
        panic!()
    };
    *original_image = None;
    *editable = Some(crate::node::SmartEditable::Document {
        archive: archive.clone(),
        external: None,
    });
    let before = doc.clone();
    command(perspective(0.125)).apply(&mut doc).unwrap();
    let NodeKind::Smart {
        editable:
            Some(crate::node::SmartEditable::Document {
                archive: retained, ..
            }),
        source,
        ..
    } = &doc.nodes[0].kind
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(retained, &archive));
    assert_eq!(retained.as_slice(), [9, 0, 16, 42, 255]);
    let NodeKind::Smart { source: old, .. } = &before.nodes[0].kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(source, old));
}

#[test]
fn reenable_recertifies_dormant_filter_spread_before_publication() {
    let mut doc = document();
    command(perspective(0.125)).apply(&mut doc).unwrap();
    let NodeKind::Smart {
        filters,
        filters_enabled,
        ..
    } = &mut doc.nodes[0].kind
    else {
        panic!()
    };
    *filters = vec![emulsion_filters::Filter::GaussianBlur { radius: 30. }];
    *filters_enabled = false;
    doc.validate().unwrap();
    let before = doc.clone();
    assert!(
        Command::SetFiltersEnabled {
            id: 1,
            enabled: true
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    retained(&before.nodes[0], &doc.nodes[0]);
}

#[test]
fn source_replacement_preserves_disabled_filter_planes_properties_and_world_maps() {
    for projected in [false, true] {
        for linked in [false, true] {
            let mut doc = document();
            doc.nodes[0].mask_transform = Mapping2::Projective(Projective2::IDENTITY);
            let mut filter =
                crate::SmartFilterMask::new(Arc::new(Mask::from_fn(8, 4, 255, |x, y| {
                    if x == 6 && y == 2 { 0 } else { 255 }
                })));
            filter.enabled = false;
            filter.linked = linked;
            filter.properties = crate::MaskProperties {
                density: 0.7,
                feather: 0.5,
            };
            filter.transform = Mapping2::Projective(
                Projective2::from_affine(glam::DAffine2::from_translation(glam::dvec2(0.25, 0.5)))
                    .unwrap(),
            );
            let NodeKind::Smart {
                placement,
                filter_mask,
                ..
            } = &mut doc.nodes[0].kind
            else {
                panic!()
            };
            if projected {
                *placement = SmartPlacement::Projective(perspective(0.125));
            }
            *filter_mask = Some(filter.clone());
            let world = crate::smart_filter_mask::to_document(&doc.nodes[0])
                .unwrap()
                .unwrap();
            let mut editor = Editor::try_new(doc, None).unwrap();
            crate::photo_source::replace(&mut editor, 1, Arc::new(Raster::solid(2, 2, [1.; 4])))
                .unwrap();
            let updated = crate::smart_filter_mask::descriptor(&editor.doc.nodes[0]).unwrap();
            assert!(Arc::ptr_eq(&updated.pixels, &filter.pixels));
            assert_eq!(updated.pixels.get(6, 2), 0);
            assert_eq!(updated.enabled, filter.enabled);
            assert_eq!(updated.linked, filter.linked);
            assert_eq!(updated.properties, filter.properties);
            assert!(matches!(updated.transform, Mapping2::Projective(_)));
            let actual = crate::smart_filter_mask::to_document(&editor.doc.nodes[0])
                .unwrap()
                .unwrap();
            for p in [glam::dvec2(0.5, 0.5), glam::dvec2(6., 2.)] {
                assert!(
                    (actual.map_point(p).unwrap() - world.map_point(p).unwrap()).length() < 1e-6
                );
            }
            assert!(editor.doc.nodes[0].mask.is_none());
            assert!(matches!(
                editor.doc.nodes[0].mask_transform,
                Mapping2::Projective(_)
            ));
        }
    }
}

#[test]
fn unsafe_source_basis_replacement_preserves_editor_history_redo_and_resources() {
    let mut doc = document();
    command(perspective(0.125)).apply(&mut doc).unwrap();
    let mut filter = crate::SmartFilterMask::new(Arc::new(Mask::from_fn(16, 4, 255, |x, y| {
        if x == 12 && y == 1 { 0 } else { 255 }
    })));
    filter.enabled = false;
    filter.linked = false;
    filter.transform = Mapping2::Projective(Projective2::IDENTITY);
    let NodeKind::Smart { filter_mask, .. } = &mut doc.nodes[0].kind else {
        panic!()
    };
    *filter_mask = Some(filter);
    let mut editor = Editor::try_new(doc, None).unwrap();
    editor
        .execute(Command::Rename {
            id: 1,
            name: "Redo remains".into(),
        })
        .unwrap();
    assert!(editor.undo());
    let before = editor.doc.clone();
    let revision = editor.revision;
    let steps = editor.history.len();
    editor.begin("Pending edit");
    // The source basis scales x by4. On the new1x4 source, the largest mip's
    // x halo reaches-2, exactly the forward horizon of H*Scale(4,1).
    editor.take_dirty();
    let error =
        crate::photo_source::replace(&mut editor, 1, Arc::new(Raster::solid(1, 4, [1.; 4])))
            .unwrap_err();
    assert!(
        error.contains("unsafe Source projective pixel support"),
        "{error}"
    );
    assert_eq!(editor.take_dirty(), crate::Dirty::Nothing);
    assert_eq!(editor.doc, before);
    assert_eq!(editor.doc.next_id, before.next_id);
    assert_eq!(editor.revision, revision);
    assert_eq!(editor.history.len(), steps);
    assert!(editor.history.can_redo());
    assert!(editor.in_transaction());
    retained(&before.nodes[0], &editor.doc.nodes[0]);
    assert!(Arc::ptr_eq(
        editor.doc.selection.as_ref().unwrap(),
        before.selection.as_ref().unwrap()
    ));
    let old = crate::smart_filter_mask::descriptor(&before.nodes[0]).unwrap();
    let new = crate::smart_filter_mask::descriptor(&editor.doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(&old.pixels, &new.pixels));
    assert_eq!(old, new);
    editor.cancel();
    // The same baseline admits a meaningful2x2 replacement: its isotropic
    // mip halo scales proportionally and does not reach the forward horizon.
    let mut control = Editor::try_new(before, None).unwrap();
    crate::photo_source::replace(&mut control, 1, Arc::new(Raster::solid(2, 2, [1.; 4]))).unwrap();
    let NodeKind::Smart {
        source,
        original_image,
        ..
    } = &control.doc.nodes[0].kind
    else {
        panic!()
    };
    assert_eq!((source.width(), source.height()), (2, 2));
    assert!(original_image.is_none());
    assert_eq!(control.history.len(), 1);
}
