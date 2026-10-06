//! Geometry and conversion contracts for independently editable vector masks.
use crate::{
    Command, Document, Editor, EmptyVectorCoverage, MaskProperties, Node, NodeKind, VectorMask,
};
use emulsion_raster::{
    IRect, Mask, Placement, Raster,
    vector::{Path, PathStyle},
};
use glam::{DAffine2, dvec2};
use std::sync::Arc;

fn vector() -> VectorMask {
    VectorMask {
        path: Arc::new(Path::from_svg("M -5 -4 L 14 -4 L 14 11 L -5 11 Z").unwrap()),
        properties: MaskProperties {
            density: 0.8,
            feather: 1.5,
        },
        ..Default::default()
    }
}
fn document() -> Document {
    let mut doc = Document::new(80, 60);
    let mut node = Node::raster(
        1,
        "Both masks",
        Arc::new(Raster::solid(16, 12, [0.8, 0.3, 0.1, 1.])),
        Placement::at(20., 15.),
    );
    node.mask = Some(Arc::new(Mask::from_fn(16, 12, 255, |x, _| {
        if x < 8 { 80 } else { 255 }
    })));
    node.vector_mask = Some(vector());
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
fn world(doc: &Document) -> DAffine2 {
    crate::transform::vector_mask_to_document(doc.node(1).unwrap())
        .unwrap()
        .unwrap()
}
fn assert_world(actual: DAffine2, expected: DAffine2) {
    assert!(
        actual.abs_diff_eq(expected, 1e-8),
        "actual {actual:?}; expected {expected:?}"
    );
}
fn pixels(doc: &Document) -> Vec<u8> {
    emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8()
}
fn assert_pixels_close(actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len());
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
}

#[test]
fn vector_geometry_affine_content_transform_preserves_independent_world_bases() {
    for linked in [false, true] {
        for enabled in [false, true] {
            let mut doc = document();
            let node = doc.node_mut(1).unwrap();
            let NodeKind::Raster { placement, .. } = &mut node.kind else {
                panic!()
            };
            *placement = Placement {
                x: 20.,
                y: 15.,
                scale_x: 1.7,
                scale_y: 0.8,
                rotation: 23.,
                flip_x: true,
                flip_y: false,
            };
            node.mask_linked = !linked;
            let vector = node.vector_mask.as_mut().unwrap();
            vector.linked = linked;
            vector.enabled = enabled;
            vector.transform = (DAffine2::from_translation(dvec2(-3., 2.))
                * DAffine2::from_angle(0.2))
            .to_cols_array();
            let path = vector.path.clone();
            let old = world(&doc);
            let old_raster = crate::transform::mask_to_document(doc.node(1).unwrap())
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap();
            let transform = DAffine2::from_translation(dvec2(4., -2.))
                * DAffine2::from_angle(0.4)
                * DAffine2::from_scale(dvec2(1.2, 1.2));
            Command::TransformNodes {
                ids: vec![1],
                transform: transform.to_cols_array(),
            }
            .apply(&mut doc)
            .unwrap();
            assert_world(world(&doc), if linked { transform * old } else { old });
            assert_world(
                crate::transform::mask_to_document(doc.node(1).unwrap())
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
                if linked {
                    old_raster
                } else {
                    transform * old_raster
                },
            );
            let after = doc.node(1).unwrap().vector_mask.as_ref().unwrap();
            assert_eq!(after.enabled, enabled);
            assert!(Arc::ptr_eq(&after.path, &path));
        }
    }
}

#[test]
fn vector_geometry_legacy_translate_rotate_and_placement_keep_unlinked_masks_fixed() {
    for smart in [false, true] {
        for linked in [false, true] {
            let mut doc = document();
            if smart {
                Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
            }
            doc.node_mut(1)
                .unwrap()
                .vector_mask
                .as_mut()
                .unwrap()
                .linked = linked;
            let old = world(&doc);
            let move_by = DAffine2::from_translation(dvec2(7., -3.));
            Command::TranslateNode {
                id: 1,
                dx: 7.,
                dy: -3.,
            }
            .apply(&mut doc)
            .unwrap();
            assert_world(world(&doc), if linked { move_by * old } else { old });
            let old = world(&doc);
            let bounds = crate::geometry::node_bounds(&doc, 1).unwrap().unwrap();
            let pivot = dvec2(
                f64::from(bounds.x) + f64::from(bounds.w) / 2.,
                f64::from(bounds.y) + f64::from(bounds.h) / 2.,
            );
            let rotation = DAffine2::from_translation(pivot)
                * DAffine2::from_angle(0.5)
                * DAffine2::from_translation(-pivot);
            Command::RotateNode {
                id: 1,
                degrees: 0.5_f64.to_degrees(),
            }
            .apply(&mut doc)
            .unwrap();
            assert_world(world(&doc), if linked { rotation * old } else { old });
            let old = world(&doc);
            let old_local = crate::transform::local_to_document(doc.node(1).unwrap())
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap();
            Command::SetPlacement {
                id: 1,
                placement: Placement {
                    x: 8.,
                    y: 11.,
                    scale_x: 2.,
                    scale_y: 1.5,
                    rotation: 20.,
                    ..Default::default()
                },
            }
            .apply(&mut doc)
            .unwrap();
            let new_local = crate::transform::local_to_document(doc.node(1).unwrap())
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap();
            assert_world(
                world(&doc),
                if linked {
                    new_local * old_local.inverse() * old
                } else {
                    old
                },
            );
        }
    }
}

#[test]
fn vector_geometry_document_masks_retain_off_canvas_anchors_on_move_and_return() {
    for linked in [false, true] {
        let mut doc = Document::new(30, 20);
        let mut node = Node::new(
            1,
            "Masked fill",
            NodeKind::Fill {
                rgba: [220, 80, 30, 255],
            },
        );
        node.vector_mask = Some(VectorMask { linked, ..vector() });
        let path = node.vector_mask.as_ref().unwrap().path.clone();
        doc.nodes.push(node);
        doc.next_id = 2;
        let before = pixels(&doc);
        let old = world(&doc);
        Command::TranslateNode {
            id: 1,
            dx: 500.,
            dy: -300.,
        }
        .apply(&mut doc)
        .unwrap();
        assert_world(
            world(&doc),
            if linked {
                DAffine2::from_translation(dvec2(500., -300.)) * old
            } else {
                old
            },
        );
        assert!(doc.node(1).unwrap().mask.is_none());
        Command::TranslateNode {
            id: 1,
            dx: -500.,
            dy: 300.,
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(pixels(&doc), before);
        assert!(Arc::ptr_eq(
            &doc.node(1).unwrap().vector_mask.as_ref().unwrap().path,
            &path
        ));
    }
}

#[test]
fn vector_geometry_image_operations_move_disabled_unlinked_masks_without_clipping() {
    for raster in [false, true] {
        let mut doc = document();
        let node = doc.node_mut(1).unwrap();
        if !raster {
            node.kind = NodeKind::Fill {
                rgba: [220, 80, 30, 255],
            };
            node.mask = None;
        }
        let mask = node.vector_mask.as_mut().unwrap();
        mask.enabled = false;
        mask.linked = false;
        let path = mask.path.clone();
        let old = world(&doc);
        crate::geometry::crop(&mut doc, IRect::new(10, 5, 40, 30), 0.).unwrap();
        let after_crop = DAffine2::from_translation(dvec2(-10., -5.)) * old;
        assert_world(world(&doc), after_crop);
        crate::geometry::resize(&mut doc, 80, 60).unwrap();
        let after_resize = DAffine2::from_scale(dvec2(2., 2.)) * after_crop;
        assert_world(world(&doc), after_resize);
        crate::geometry::rotate_image(&mut doc, 90.).unwrap();
        let image_rotation = DAffine2::from_translation(dvec2(30., 40.))
            * DAffine2::from_angle(std::f64::consts::FRAC_PI_2)
            * DAffine2::from_translation(dvec2(-40., -30.));
        assert_world(world(&doc), image_rotation * after_resize);
        assert!(Arc::ptr_eq(
            &doc.node(1).unwrap().vector_mask.as_ref().unwrap().path,
            &path
        ));
    }
}

#[test]
fn vector_geometry_source_replace_and_trim_preserve_world_and_properties() {
    let mut doc = document();
    let old = world(&doc);
    let original = doc.node(1).unwrap().vector_mask.as_ref().unwrap().clone();
    let mut editor = Editor::new(doc.clone(), None);
    crate::photo_source::replace(
        &mut editor,
        1,
        Arc::new(Raster::solid(32, 36, [1., 0., 0., 1.])),
    )
    .unwrap();
    assert_world(world(&editor.doc), old);
    assert_eq!(
        editor
            .doc
            .node(1)
            .unwrap()
            .vector_mask
            .as_ref()
            .unwrap()
            .properties,
        original.properties
    );
    assert!(Arc::ptr_eq(
        &editor
            .doc
            .node(1)
            .unwrap()
            .vector_mask
            .as_ref()
            .unwrap()
            .path,
        &original.path
    ));
    assert!(editor.undo());
    assert_eq!(editor.doc, doc);
    let NodeKind::Raster { placement, .. } = &mut doc.node_mut(1).unwrap().kind else {
        panic!()
    };
    *placement = Placement::at(-5., -4.);
    let old = world(&doc);
    let before = pixels(&doc);
    assert_eq!(crate::geometry::trim_to_canvas(&mut doc).unwrap(), 1);
    assert_world(world(&doc), old);
    assert_pixels_close(&pixels(&doc), &before);
    assert!(Arc::ptr_eq(
        &doc.node(1).unwrap().vector_mask.as_ref().unwrap().path,
        &original.path
    ));
}

#[test]
fn vector_geometry_smart_cache_rasterize_preserves_raw_components_and_world() {
    let mut doc = document();
    doc.node_mut(1).unwrap().mask_properties = MaskProperties {
        density: 0.65,
        feather: 2.,
    };
    Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
    Command::SetFilters {
        id: 1,
        filters: vec![emulsion_filters::Filter::GaussianBlur { radius: 3. }],
    }
    .apply(&mut doc)
    .unwrap();
    let original = doc.node(1).unwrap().clone();
    let old = world(&doc);
    let before = pixels(&doc);
    let composite = doc.composite_mask(&original).unwrap().unwrap().to_gray8();
    Command::Rasterize { id: 1 }.apply(&mut doc).unwrap();
    let node = doc.node(1).unwrap();
    assert_world(world(&doc), old);
    assert_eq!(node.mask_properties, original.mask_properties);
    assert!(Arc::ptr_eq(
        node.mask.as_ref().unwrap(),
        original.mask.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        &node.vector_mask.as_ref().unwrap().path,
        &original.vector_mask.as_ref().unwrap().path
    ));
    assert_eq!(
        doc.composite_mask(node).unwrap().unwrap().to_gray8(),
        composite
    );
    assert_eq!(pixels(&doc), before);
}

#[test]
fn vector_geometry_smart_source_restore_retains_editable_mask_basis() {
    let mut doc = Document::new(80, 60);
    let mut node = Node::path(
        1,
        "Path",
        Arc::new(Path::from_svg("M 10 10 L 40 10 L 40 40 L 10 40 Z").unwrap()),
        PathStyle::default(),
        80,
        60,
    );
    node.vector_mask = Some(vector());
    doc.nodes.push(node);
    doc.next_id = 2;
    let old = world(&doc);
    let path = doc
        .node(1)
        .unwrap()
        .vector_mask
        .as_ref()
        .unwrap()
        .path
        .clone();
    Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
    assert_world(world(&doc), old);
    Command::SetPlacement {
        id: 1,
        placement: Placement {
            x: 4.,
            y: 3.,
            scale_x: 1.2,
            scale_y: 1.2,
            rotation: 25.,
            ..Default::default()
        },
    }
    .apply(&mut doc)
    .unwrap();
    let old = world(&doc);
    Command::ConvertToLayers { id: 1 }.apply(&mut doc).unwrap();
    assert!(matches!(doc.node(1).unwrap().kind, NodeKind::Path { .. }));
    assert_world(world(&doc), old);
    assert!(Arc::ptr_eq(
        &doc.node(1).unwrap().vector_mask.as_ref().unwrap().path,
        &path
    ));
}

#[test]
fn vector_geometry_apply_raster_keeps_vector_independent_for_all_enabled_states() {
    for smart in [false, true] {
        for raster_enabled in [false, true] {
            for vector_enabled in [false, true] {
                let mut doc = document();
                doc.node_mut(1).unwrap().mask_enabled = raster_enabled;
                doc.node_mut(1)
                    .unwrap()
                    .vector_mask
                    .as_mut()
                    .unwrap()
                    .enabled = vector_enabled;
                if smart {
                    Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
                    Command::SetFilters {
                        id: 1,
                        filters: vec![emulsion_filters::Filter::GaussianBlur { radius: 2. }],
                    }
                    .apply(&mut doc)
                    .unwrap();
                }
                let old = world(&doc);
                let original = doc.node(1).unwrap().vector_mask.as_ref().unwrap().clone();
                let before = pixels(&doc);
                Command::ApplyLayerMask { id: 1 }.apply(&mut doc).unwrap();
                let node = doc.node(1).unwrap();
                assert!(node.mask.is_none());
                let mask = node.vector_mask.as_ref().unwrap();
                assert_eq!(mask.enabled, original.enabled);
                assert_eq!(mask.linked, original.linked);
                assert_eq!(mask.properties, original.properties);
                assert!(Arc::ptr_eq(&mask.path, &original.path));
                assert_world(world(&doc), old);
                assert_pixels_close(&pixels(&doc), &before);
            }
        }
    }
}

#[test]
fn vector_geometry_rasterize_vector_retains_intrinsic_coverage_flags_and_properties() {
    for enabled in [false, true] {
        for inverted in [false, true] {
            for feather in [0., 2.] {
                let mut doc = document();
                let node = doc.node_mut(1).unwrap();
                node.mask = None;
                let mask = node.vector_mask.as_mut().unwrap();
                mask.enabled = enabled;
                mask.linked = false;
                mask.inverted = inverted;
                mask.properties = MaskProperties {
                    density: 0.7,
                    feather,
                };
                let properties = mask.properties;
                let before = doc
                    .vector_mask_for_inspection(doc.node(1).unwrap())
                    .unwrap()
                    .unwrap()
                    .to_gray8();
                crate::vector_mask_conversion::rasterize(&mut doc, 1).unwrap();
                let node = doc.node(1).unwrap();
                assert!(node.vector_mask.is_none());
                assert_eq!(node.mask_properties, properties);
                assert_eq!(node.mask_enabled, enabled);
                assert!(!node.mask_linked);
                assert_eq!(
                    node.mask.as_ref().unwrap().fill(),
                    if inverted { 255 } else { 0 }
                );
                assert_eq!(
                    doc.raster_mask_for_inspection(node)
                        .unwrap()
                        .unwrap()
                        .to_gray8(),
                    before
                );
            }
        }
    }
}

#[test]
fn vector_geometry_rasterize_empty_and_transformed_vectors_preserves_state_without_clipping() {
    for empty in [EmptyVectorCoverage::RevealAll, EmptyVectorCoverage::HideAll] {
        for inverted in [false, true] {
            let mut doc = document();
            let node = doc.node_mut(1).unwrap();
            node.mask = None;
            let mut vector = VectorMask::empty(empty);
            vector.inverted = inverted;
            vector.properties = MaskProperties {
                density: 0.4,
                feather: 25.,
            };
            let value = vector.empty_value();
            node.vector_mask = Some(vector);
            let before = pixels(&doc);
            crate::vector_mask_conversion::rasterize(&mut doc, 1).unwrap();
            assert_eq!(doc.node(1).unwrap().mask.as_ref().unwrap().fill(), value);
            assert_eq!(pixels(&doc), before);
        }
    }
    let mut doc = document();
    let node = doc.node_mut(1).unwrap();
    node.mask = None;
    let transform = DAffine2::from_translation(dvec2(12.5, -2.5))
        * DAffine2::from_angle(0.4)
        * DAffine2::from_scale(dvec2(2., 0.8));
    node.vector_mask.as_mut().unwrap().transform = transform.to_cols_array();
    crate::vector_mask_conversion::rasterize(&mut doc, 1).unwrap();
    let node = doc.node(1).unwrap();
    // Full original hull -5,-4 .. 14,11 plus integer AA border.
    assert_eq!(
        (
            node.mask.as_ref().unwrap().width(),
            node.mask.as_ref().unwrap().height()
        ),
        (21, 17)
    );
    assert_world(
        node.mask_transform
            .require_affine("legacy fixture")
            .unwrap(),
        transform * DAffine2::from_translation(dvec2(-6., -5.)),
    );
}

#[test]
fn vector_geometry_rasterize_rejects_existing_raster_oversize_and_locks_atomically() {
    let mut doc = document();
    let before = doc.clone();
    assert!(crate::vector_mask_conversion::rasterize(&mut doc, 1).is_err());
    assert_eq!(doc, before);
    doc.node_mut(1).unwrap().mask = None;
    doc.node_mut(1).unwrap().vector_mask.as_mut().unwrap().path =
        Arc::new(Path::from_svg("M -100000 0 L 100000 0 L 100000 10 Z").unwrap());
    let before = doc.clone();
    assert!(crate::vector_mask_conversion::rasterize(&mut doc, 1).is_err());
    assert_eq!(doc, before);
    doc.node_mut(1).unwrap().vector_mask = Some(vector());
    doc.node_mut(1).unwrap().locks.position = true;
    let before = doc.clone();
    assert!(crate::vector_mask_conversion::rasterize(&mut doc, 1).is_err());
    assert_eq!(doc, before);
    doc.node_mut(1).unwrap().locks.position = false;
    doc.node_mut(1).unwrap().locks.pixels = true;
    crate::vector_mask_conversion::rasterize(&mut doc, 1).unwrap();
}

#[test]
fn vector_geometry_vector_only_transform_respects_locks_and_never_moves_raster_mask() {
    let mut doc = document();
    let before = doc.node(1).unwrap().clone();
    doc.node_mut(1).unwrap().locks.pixels = true;
    crate::transform::set_vector_mask_transform(&mut doc, 1, [1., 0., 0., 1., 4., -3.]).unwrap();
    assert_eq!(doc.node(1).unwrap().mask_transform, before.mask_transform);
    assert!(Arc::ptr_eq(
        doc.node(1).unwrap().mask.as_ref().unwrap(),
        before.mask.as_ref().unwrap()
    ));
    doc.node_mut(1).unwrap().locks.position = true;
    let before = doc.clone();
    assert!(
        crate::transform::set_vector_mask_transform(&mut doc, 1, [1., 0., 0., 1., 0., 0.]).is_err()
    );
    assert_eq!(doc, before);
    assert!(crate::transform::set_vector_mask_transform(&mut doc, 1, [0.; 6]).is_err());
    assert_eq!(doc, before);
}

#[test]
fn vector_geometry_groups_and_adjustments_move_components_without_changing_paths() {
    for linked in [false, true] {
        let mut doc = document();
        let mut group = Node::group(2, "Group");
        group.vector_mask = Some(VectorMask { linked, ..vector() });
        doc.node_mut(1).unwrap().parent = Some(2);
        let mut adjustment = Node::adjust(
            3,
            emulsion_raster::adjust::Adjustment::Exposure {
                exposure: 1.,
                offset: 0.,
                gamma: 1.,
            },
        );
        adjustment.parent = Some(2);
        adjustment.vector_mask = Some(VectorMask { linked, ..vector() });
        // A group's contiguous descendants precede the group in paint order.
        doc.nodes.extend([adjustment, group]);
        doc.next_id = 4;
        doc.validate().unwrap();
        let old = [2, 3].map(|id| {
            crate::transform::vector_mask_to_document(doc.node(id).unwrap())
                .unwrap()
                .unwrap()
        });
        let paths = [2, 3].map(|id| {
            doc.node(id)
                .unwrap()
                .vector_mask
                .as_ref()
                .unwrap()
                .path
                .clone()
        });
        let transform = DAffine2::from_translation(dvec2(6., 7.));
        Command::TranslateNode {
            id: 2,
            dx: 6.,
            dy: 7.,
        }
        .apply(&mut doc)
        .unwrap();
        for (i, id) in [2, 3].into_iter().enumerate() {
            let node = doc.node(id).unwrap();
            assert_world(
                crate::transform::vector_mask_to_document(node)
                    .unwrap()
                    .unwrap(),
                if linked { transform * old[i] } else { old[i] },
            );
            assert!(Arc::ptr_eq(
                &node.vector_mask.as_ref().unwrap().path,
                &paths[i]
            ));
            assert!(node.mask.is_none());
        }
        let old = crate::transform::vector_mask_to_document(doc.node(3).unwrap())
            .unwrap()
            .unwrap();
        Command::TranslateNode {
            id: 3,
            dx: 2.,
            dy: -4.,
        }
        .apply(&mut doc)
        .unwrap();
        assert_world(
            crate::transform::vector_mask_to_document(doc.node(3).unwrap())
                .unwrap()
                .unwrap(),
            if linked {
                DAffine2::from_translation(dvec2(2., -4.)) * old
            } else {
                old
            },
        );
    }
}

#[test]
fn vector_geometry_transform_gesture_noop_cancel_and_conversion_undo_are_atomic() {
    let mut doc = document();
    doc.node_mut(1).unwrap().mask = None;
    let mut editor = Editor::new(doc.clone(), None);
    let command = |dx| Command::SetVectorMaskTransform {
        id: 1,
        transform: [1., 0., 0., 1., dx, 0.],
    };
    editor.execute(command(0.)).unwrap();
    assert_eq!(editor.history.len(), 0);
    editor.begin("Move vector mask");
    for dx in [2., 4., 6.] {
        editor.preview(command(dx)).unwrap();
    }
    editor.end();
    assert_eq!(editor.history.len(), 1);
    assert!(editor.undo());
    assert_eq!(editor.doc, doc);
    assert!(editor.redo());
    let before = editor.doc.clone();
    editor.begin("Move vector mask");
    editor.preview(command(100.)).unwrap();
    editor.cancel();
    assert_eq!(editor.doc, before);
    assert_eq!(editor.history.len(), 1);
    editor
        .execute(Command::RasterizeVectorMask { id: 1 })
        .unwrap();
    assert_eq!(editor.history.len(), 2);
    assert!(editor.doc.node(1).unwrap().vector_mask.is_none());
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
}

#[test]
fn vector_geometry_distort_and_destructive_replacement_reject_atomically() {
    let mut doc = document();
    doc.node_mut(1).unwrap().mask = None;
    let before = doc.clone();
    let distortion = crate::distort::Distortion::new(
        IRect::new(20, 15, 16, 12),
        crate::distort::DistortKind::Perspective,
    );
    let error = crate::distort::distort_command(&doc, 1, None, &distortion).unwrap_err();
    assert!(error.contains("vector mask"));
    assert_eq!(doc, before);
    assert!(
        Command::ReplaceContent {
            id: 1,
            raster: Arc::new(Raster::transparent(4, 4)),
            mask: None,
            placement: Placement::default(),
            label: "Destructive replacement".into()
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
}
