use crate::{Command, Document, Editor, MAX_MASK_FEATHER, MaskProperties, Node, NodeKind};
use emulsion_raster::{Mask, Placement, Raster};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(32, 24);
    let mut node = Node::raster(
        1,
        "Masked",
        Arc::new(Raster::solid(32, 24, [0.8, 0.4, 0.2, 1.])),
        Placement::default(),
    );
    node.mask = Some(Arc::new(Mask::from_fn(32, 24, 255, |x, _| {
        if (8..24).contains(&x) { 0 } else { 255 }
    })));
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
fn pixels(doc: &Document) -> Vec<u8> {
    emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8()
}
fn command(density: f32, feather: f32) -> Command {
    Command::SetMaskProperties {
        id: 1,
        properties: MaskProperties { density, feather },
    }
}
#[test]
fn mask_property_density_oracle_includes_outside_fill_and_preserves_raw() {
    for fill in [0, 64, 128, 255] {
        let mut doc = document();
        let raw = Arc::new(Mask::from_fn(4, 1, fill, |x, _| {
            [0, 64, 128, 255][x as usize]
        }));
        doc.nodes[0].kind = NodeKind::Raster {
            raster: Arc::new(Raster::solid(4, 1, [1.; 4])),
            placement: Placement::default(),
        };
        doc.nodes[0].mask = Some(raw.clone());
        for density in [0.0, 0.25, 0.5, 1.0] {
            command(density, 0.0).apply(&mut doc).unwrap();
            let mask = doc.composite_mask(&doc.nodes[0]).unwrap();
            let oracle = |v: u8| (255.0 - density * (255.0 - v as f32)).round() as u8;
            assert_eq!(mask.to_gray8(), [0, 64, 128, 255].map(oracle));
            assert_eq!(mask.fill(), oracle(fill));
            assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
            assert_eq!(raw.to_gray8(), [0, 64, 128, 255]);
            if density == 1.0 {
                assert!(Arc::ptr_eq(&mask, &raw));
            }
            if density == 0.0 {
                assert_eq!(mask.tile_count(), 0);
            }
        }
    }
}
#[test]
fn mask_property_feather_is_reversible_cached_and_nonaccumulating() {
    let mut doc = document();
    let raw = doc.nodes[0].mask.clone().unwrap();
    let kind = doc.nodes[0].kind.clone();
    let baseline = pixels(&doc);
    command(1., 6.).apply(&mut doc).unwrap();
    let first = doc.composite_mask(&doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &doc.composite_mask(&doc.nodes[0]).unwrap()
    ));
    let row = (0..32).map(|x| first.get(x, 12)).collect::<Vec<_>>();
    assert!(row[8] > 0 && row[8] < 255);
    for x in 0..16 {
        assert_eq!(row[x], row[31 - x]);
    }
    assert!(row[..16].windows(2).all(|v| v[0] >= v[1]));
    command(1., 20.).apply(&mut doc).unwrap();
    assert!(!Arc::ptr_eq(
        &first,
        &doc.composite_mask(&doc.nodes[0]).unwrap()
    ));
    command(1., 6.).apply(&mut doc).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &doc.composite_mask(&doc.nodes[0]).unwrap()
    ));
    command(1., 0.).apply(&mut doc).unwrap();
    assert_eq!(pixels(&doc), baseline);
    assert!(Arc::ptr_eq(
        &raw,
        &doc.composite_mask(&doc.nodes[0]).unwrap()
    ));
    assert_eq!(doc.nodes[0].kind, kind);
}
#[test]
fn mask_property_disabled_inspection_keeps_settings_without_affecting_canvas() {
    let mut doc = document();
    doc.nodes[0].mask_enabled = false;
    let baseline = pixels(&doc);
    command(0.5, 6.).apply(&mut doc).unwrap();
    assert!(doc.composite_mask(&doc.nodes[0]).is_none());
    assert_eq!(pixels(&doc), baseline);
    let inspection = doc.mask_for_inspection(&doc.nodes[0]).unwrap();
    assert!(inspection.get(15, 12) >= 128 && inspection.get(15, 12) < 255);
    doc.nodes[0].mask_enabled = true;
    assert!(Arc::ptr_eq(
        &inspection,
        &doc.composite_mask(&doc.nodes[0]).unwrap()
    ));
}
#[test]
fn mask_property_validation_is_bounded_and_atomic() {
    let mut doc = document();
    let before = doc.clone();
    for (density, feather) in [
        (f32::NAN, 0.),
        (f32::INFINITY, 0.),
        (-0.01, 0.),
        (1.01, 0.),
        (1., -1.),
        (1., f32::NAN),
        (1., f32::INFINITY),
        (1., MAX_MASK_FEATHER + 1.),
    ] {
        assert!(command(density, feather).apply(&mut doc).is_err());
        assert_eq!(doc, before);
    }
    command(0., MAX_MASK_FEATHER).apply(&mut doc).unwrap();
    doc.nodes[0].mask = None;
    assert!(command(0.5, 3.).apply(&mut doc).is_err());
}
#[test]
fn mask_property_gesture_undo_cancel_and_noop_are_atomic() {
    let mut editor = Editor::new(document(), None);
    let before = editor.doc.clone();
    editor.execute(command(1., 0.)).unwrap();
    assert_eq!(editor.history.len(), 0);
    editor.begin("Mask properties");
    for density in [0.9, 0.8, 0.7] {
        editor.preview(command(density, 6.)).unwrap();
    }
    editor.end();
    assert_eq!(editor.history.len(), 1);
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(editor.redo());
    let changed = editor.doc.clone();
    editor.begin("Mask properties");
    editor.preview(command(0.1, 100.)).unwrap();
    editor.cancel();
    assert_eq!(editor.doc, changed);
    assert_eq!(editor.history.len(), 1);
}
#[test]
fn mask_property_locks_allow_pixels_and_position_but_not_full_ancestor() {
    let mut doc = document();
    doc.nodes[0].locks.pixels = true;
    doc.nodes[0].locks.position = true;
    doc.nodes[0].locks.transparency = true;
    command(0.5, 6.).apply(&mut doc).unwrap();
    doc.nodes[0].locked = true;
    assert!(command(0.2, 3.).apply(&mut doc).is_err());
    doc.nodes[0].locked = false;
    let mut parent = Node::new(2, "Group", NodeKind::Group { collapsed: false });
    parent.locked = true;
    doc.nodes[0].parent = Some(2);
    doc.nodes.insert(0, parent);
    doc.next_id = 3;
    assert!(command(0.2, 3.).apply(&mut doc).is_err());
}
#[test]
fn mask_property_paint_invert_preserve_and_remove_new_reset() {
    let mut doc = document();
    doc.nodes[0].mask_linked = false;
    doc.nodes[0].locks.position = true;
    command(0.5, 6.).apply(&mut doc).unwrap();
    let inverted = Arc::new(emulsion_raster::select::invert(
        doc.nodes[0].mask.as_ref().unwrap(),
    ));
    Command::SetMask {
        id: 1,
        mask: Some(inverted),
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(
        doc.nodes[0].mask_properties,
        MaskProperties {
            density: 0.5,
            feather: 6.
        }
    );
    Command::SetMask { id: 1, mask: None }
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.nodes[0].mask_properties, MaskProperties::default());
    assert!(!doc.nodes[0].mask_linked);
    Command::SetMask {
        id: 1,
        mask: Some(Arc::new(Mask::white(32, 24))),
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(doc.nodes[0].mask_properties, MaskProperties::default());
    assert!(!doc.nodes[0].mask_linked);
}
#[test]
fn mask_property_apply_bakes_and_smart_rasterize_retains_intrinsic_mask() {
    for enabled in [true, false] {
        for smart in [true, false] {
            let mut doc = document();
            command(0.5, 3.).apply(&mut doc).unwrap();
            doc.nodes[0].mask_enabled = enabled;
            doc.nodes[0].mask_transform[4] = 2.;
            if smart {
                Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
                Command::SetFilters {
                    id: 1,
                    filters: vec![emulsion_filters::Filter::GaussianBlur { radius: 2. }],
                }
                .apply(&mut doc)
                .unwrap();
            }
            let before = pixels(&doc);
            let raw = doc.nodes[0].mask.clone().unwrap();
            let properties = doc.nodes[0].mask_properties;
            let world = crate::transform::mask_to_document(&doc.nodes[0]);
            if smart {
                Command::Rasterize { id: 1 }.apply(&mut doc).unwrap();
            } else {
                Command::ApplyLayerMask { id: 1 }.apply(&mut doc).unwrap();
            }
            if smart {
                assert_eq!(doc.nodes[0].mask_properties, properties);
                assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
                assert_eq!(crate::transform::mask_to_document(&doc.nodes[0]), world);
            } else {
                assert_eq!(doc.nodes[0].mask_properties, MaskProperties::default());
                assert!(doc.nodes[0].mask.is_none());
            }
            assert_eq!(pixels(&doc), before, "enabled={enabled}, smart={smart}");
        }
    }
}
#[test]
fn mask_property_styles_and_fingerprint_invalidate_on_settings() {
    let mut doc = document();
    doc.nodes[0].styles = vec![crate::styles::LayerStyle::Stroke {
        color: [255, 0, 0],
        opacity: 100.,
        size: 2.,
    }];
    let first = crate::styles::render(&doc, &doc.nodes[0]).unwrap();
    let original = pixels(&doc);
    let fingerprint = crate::storyboard_fingerprint::document_fingerprint(&doc);
    command(0., 0.).apply(&mut doc).unwrap();
    let next = crate::styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(&first, &next));
    assert_ne!(pixels(&doc), original);
    assert_ne!(
        crate::storyboard_fingerprint::document_fingerprint(&doc),
        fingerprint
    );
    command(1., 0.).apply(&mut doc).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &crate::styles::render(&doc, &doc.nodes[0]).unwrap()
    ));
    assert_eq!(pixels(&doc), original);
}
#[test]
fn mask_property_reprojected_content_rejects_double_application() {
    let mut doc = document();
    command(0.5, 3.).apply(&mut doc).unwrap();
    let before = doc.clone();
    let NodeKind::Raster { raster, placement } = &doc.nodes[0].kind else {
        panic!()
    };
    let command = Command::ReplaceContent {
        id: 1,
        raster: raster.clone(),
        placement: *placement,
        mask: doc.composite_mask(&doc.nodes[0]),
        label: "Warp".into(),
    };
    assert!(command.apply(&mut doc).is_err());
    assert_eq!(doc, before);
}

#[test]
fn mask_property_graph_diff_merge_and_conflict_preserve_settings() {
    let base = document();
    let mut ours = base.clone();
    command(0.5, 6.).apply(&mut ours).unwrap();
    assert!(
        crate::graph::compare(&base, &ours)
            .iter()
            .any(|row| row.label.contains("mask"))
    );
    let mut theirs = base.clone();
    theirs.nodes[0].name = "Renamed".into();
    let crate::graph::MergeOutcome::Merged(merged) =
        crate::graph::merge(&base, &ours, &theirs, &Default::default()).unwrap()
    else {
        panic!("independent fields should merge")
    };
    assert_eq!(
        merged.nodes[0].mask_properties,
        ours.nodes[0].mask_properties
    );
    assert_eq!(merged.nodes[0].name, "Renamed");
    command(0.75, 3.).apply(&mut theirs).unwrap();
    assert!(matches!(
        crate::graph::merge(&base, &ours, &theirs, &Default::default()).unwrap(),
        crate::graph::MergeOutcome::Conflicts(_)
    ));
}

#[test]
fn mask_property_linked_and_unlinked_transforms_retain_raw_settings() {
    for linked in [true, false] {
        for enabled in [true, false] {
            let mut doc = document();
            command(0.5, 3.).apply(&mut doc).unwrap();
            doc.nodes[0].mask_linked = linked;
            doc.nodes[0].mask_enabled = enabled;
            let raw = doc.nodes[0].mask.clone().unwrap();
            let old_world = crate::transform::mask_to_document(&doc.nodes[0]);
            let delta = glam::DAffine2::from_translation(glam::dvec2(4., 2.));
            Command::TransformNodes {
                ids: vec![1],
                transform: delta.to_cols_array(),
            }
            .apply(&mut doc)
            .unwrap();
            assert_eq!(
                doc.nodes[0].mask_properties,
                MaskProperties {
                    density: 0.5,
                    feather: 3.
                }
            );
            assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
            assert_eq!(
                crate::transform::mask_to_document(&doc.nodes[0]),
                if linked { delta * old_world } else { old_world }
            );
            Command::SetMaskTransform {
                id: 1,
                transform: [1., 0., 0., 1., 7., 0.],
            }
            .apply(&mut doc)
            .unwrap();
            assert_eq!(doc.nodes[0].mask_properties.feather, 3.);
        }
    }
}

#[test]
fn mask_property_trim_retains_raw_source_and_visible_feathered_edge() {
    for enabled in [true, false] {
        let mut doc = document();
        doc.width = 16;
        doc.height = 12;
        command(0.5, 6.).apply(&mut doc).unwrap();
        doc.nodes[0].mask_enabled = enabled;
        doc.nodes[0].mask_transform[4] = -3.;
        let before = pixels(&doc);
        let raw = doc.nodes[0].mask.clone().unwrap();
        let properties = doc.nodes[0].mask_properties;
        Command::TrimToCanvas.apply(&mut doc).unwrap();
        assert_eq!(pixels(&doc), before);
        assert_eq!(doc.nodes[0].mask_properties, properties);
        assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
        assert_eq!(doc.nodes[0].mask_enabled, enabled);
    }
}

#[test]
fn mask_property_source_resize_preserves_intrinsic_plane_and_world_affine() {
    let mut editor = Editor::new(document(), None);
    command(0.5, 3.).apply(&mut editor.doc).unwrap();
    let raw = editor.doc.nodes[0].mask.clone().unwrap();
    let world = crate::transform::mask_to_document(&editor.doc.nodes[0]);
    for (w, h) in [(64, 48), (64, 96)] {
        crate::photo_source::replace(&mut editor, 1, Arc::new(Raster::solid(w, h, [1.; 4])))
            .unwrap();
        assert_eq!(
            editor.doc.nodes[0].mask_properties,
            MaskProperties {
                density: 0.5,
                feather: 3.
            }
        );
        assert!(Arc::ptr_eq(
            editor.doc.nodes[0].mask.as_ref().unwrap(),
            &raw
        ));
        assert_eq!(
            crate::transform::mask_to_document(&editor.doc.nodes[0]),
            world
        );
    }
}

#[test]
fn mask_property_intrinsic_halo_contributes_from_all_off_grid_edges_and_smart_origin() {
    for fill in [0, 255] {
        for (dx, dy) in [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)] {
            for smart in [false, true] {
                let mut doc = Document::new(1, 1);
                let source = Arc::new(Raster::solid(1, 1, [1.; 4]));
                let mut node = Node::raster(1, "Halo", source.clone(), Placement::default());
                let raw = Arc::new(Mask::from_fn(1, 1, fill, |_, _| 255 - fill));
                node.mask = Some(raw.clone());
                node.mask_transform[4] = dx;
                node.mask_transform[5] = dy;
                node.mask_properties.feather = 1.;
                if smart {
                    node.kind = NodeKind::Smart {
                        source,
                        editable: None,
                        filter_mask: None,
                        filters: vec![],
                        filter_styles: vec![],
                        placement: Placement::default(),
                        cache: Arc::new(Raster::solid(3, 3, [1.; 4])),
                        offset: (-1, -1),
                    };
                }
                doc.nodes.push(node);
                let coverage = doc.composite_mask(&doc.nodes[0]).unwrap();
                let sample = if smart {
                    coverage.get(1, 1)
                } else {
                    coverage.get(0, 0)
                };
                assert_eq!(
                    sample,
                    if fill == 255 { 240 } else { 15 },
                    "fill={fill}, dx={dx}, dy={dy}, smart={smart}"
                );
                assert_eq!(coverage.fill(), fill);
                assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
            }
        }
    }
}

#[test]
fn mask_property_source_crop_rejects_density_or_feather_without_mutating_history() {
    for (density, feather) in [(0., 0.), (0.5, 0.), (1., 3.)] {
        let mut editor = Editor::new(document(), None);
        editor.execute(command(density, feather)).unwrap();
        let before = editor.doc.clone();
        let steps = editor.history.len();
        let revision = editor.revision;
        let error = crate::photo_source::crop(&mut editor, 1, [2., 2., 8., 8.]).unwrap_err();
        assert!(error.contains("density and feather"));
        assert_eq!(editor.doc, before);
        assert_eq!(editor.history.len(), steps);
        assert_eq!(editor.revision, revision);
    }
}

fn mask_node(kind: usize, width: u32, height: u32) -> Node {
    use emulsion_raster::vector::{Path, PathStyle};
    let mut node = match kind {
        0 => Node::new(1, "Fill", NodeKind::Fill { rgba: [255; 4] }),
        1 => Node::group(1, "Group"),
        2 => Node::path(
            1,
            "Path",
            Arc::new(Path::from_svg("M 0 0 L 64 0 L 64 64 L 0 64 Z").unwrap()),
            PathStyle::default(),
            width,
            height,
        ),
        3 => Node::text(
            1,
            "Text",
            crate::text::TextSpec {
                text: "A".into(),
                ..Default::default()
            },
            width,
            height,
        ),
        4 => Node::raster(
            1,
            "Raster",
            Arc::new(Raster::solid(width, height, [1.; 4])),
            Placement::default(),
        ),
        _ => Node::smart(
            1,
            "Smart",
            Arc::new(Raster::solid(width, height, [1.; 4])),
            vec![],
            Placement::default(),
        ),
    };
    node.mask = Some(Arc::new(Mask::from_fn(width, height, 255, |x, y| {
        if (7..19).contains(&x) && (4..16).contains(&y) {
            0
        } else {
            255
        }
    })));
    node.mask_properties = MaskProperties {
        density: 0.8,
        feather: 3.4,
    };
    node
}
fn world_mask(doc: &Document, node: &Node, x: f64, y: f64) -> u8 {
    let mask = doc.mask_for_inspection(node).unwrap();
    let local = match &node.kind {
        NodeKind::Smart {
            source,
            placement,
            cache,
            offset,
            ..
        } => crate::smart::cache_placement(
            placement,
            (source.width(), source.height()),
            (cache.width(), cache.height()),
            *offset,
        )
        .to_doc(cache.width(), cache.height()),
        _ => crate::transform::local_to_document(node),
    };
    crate::transform::sample_mask(&mask, local.inverse().transform_point2(glam::dvec2(x, y)))
}

#[test]
fn mask_property_crop_and_uniform_resize_keep_intrinsic_raw_and_coverage_across_kinds() {
    for kind in 0..6 {
        for enabled in [true, false] {
            let mut doc = Document::new(32, 24);
            doc.nodes.push(mask_node(kind, 32, 24));
            doc.nodes[0].mask_enabled = enabled;
            doc.next_id = 2;
            let raw = doc.nodes[0].mask.clone().unwrap();
            let before = doc.clone();
            Command::Crop {
                rect: emulsion_raster::IRect::new(8, 5, 16, 12),
                rotation: 0.,
            }
            .apply(&mut doc)
            .unwrap();
            for y in 0..12 {
                for x in 0..16 {
                    assert_eq!(
                        world_mask(&doc, &doc.nodes[0], x as f64 + 0.5, y as f64 + 0.5),
                        world_mask(&before, &before.nodes[0], x as f64 + 8.5, y as f64 + 5.5),
                        "crop kind={kind}"
                    );
                }
            }
            Command::ImageSize {
                width: 32,
                height: 24,
            }
            .apply(&mut doc)
            .unwrap();
            for y in 0..24 {
                for x in 0..32 {
                    assert_eq!(
                        world_mask(&doc, &doc.nodes[0], x as f64 + 0.5, y as f64 + 0.5),
                        world_mask(
                            &before,
                            &before.nodes[0],
                            8. + (x as f64 + 0.5) / 2.,
                            5. + (y as f64 + 0.5) / 2.
                        ),
                        "resize kind={kind}"
                    );
                }
            }
            if kind == 0 {
                // The original neighbors beyond the crop edge still contribute.
                // Re-sampling the clipped 16×12 mask would incorrectly mix its
                // outside fill here and return 195 instead of intrinsic 159.
                assert_eq!(world_mask(&doc, &doc.nodes[0], 0.5, 0.5), 159);
            }
            assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
            assert_eq!(
                doc.nodes[0].mask_properties,
                before.nodes[0].mask_properties
            );
            assert_eq!(doc.nodes[0].mask_enabled, enabled);
        }
    }
}

#[test]
fn mask_property_output_size_invalidates_mask_and_style_cache() {
    let mut doc = Document::new(32, 24);
    doc.nodes.push(mask_node(0, 32, 24));
    doc.nodes[0].styles = vec![crate::styles::LayerStyle::Stroke {
        color: [255, 0, 0],
        opacity: 100.,
        size: 2.,
    }];
    let mask = doc.mask_for_inspection(&doc.nodes[0]).unwrap();
    let style = crate::styles::render(&doc, &doc.nodes[0]).unwrap();
    doc.width = 20;
    doc.height = 16;
    let changed = doc.mask_for_inspection(&doc.nodes[0]).unwrap();
    assert_eq!((changed.width(), changed.height()), (20, 16));
    assert!(!Arc::ptr_eq(&mask, &changed));
    assert!(!Arc::ptr_eq(
        &style,
        &crate::styles::render(&doc, &doc.nodes[0]).unwrap()
    ));
}

#[test]
fn mask_property_unlinked_scale_rotation_preserves_world_blur_with_resampling_tolerance() {
    for smart in [false, true] {
        let mut doc = Document::new(32, 32);
        let source = Arc::new(Raster::solid(128, 128, [1.; 4]));
        let mut node = if smart {
            Node::smart(1, "Smart", source, vec![], Placement::at(-48., -48.))
        } else {
            Node::raster(1, "Raster", source, Placement::at(-48., -48.))
        };
        node.mask = Some(Arc::new(Mask::from_fn(128, 128, 255, |x, y| {
            if (56..72).contains(&x) && (56..72).contains(&y) {
                0
            } else {
                255
            }
        })));
        node.mask_properties.feather = 6.;
        node.mask_linked = false;
        doc.nodes.push(node);
        doc.next_id = 2;
        let before = doc.clone();
        let expected = pixels(&doc);
        for delta in [
            glam::DAffine2::from_scale(glam::dvec2(2., 2.)),
            glam::DAffine2::from_angle(std::f64::consts::FRAC_PI_2),
            glam::DAffine2::from_angle(0.35),
        ] {
            let delta = glam::DAffine2::from_translation(glam::dvec2(16., 16.))
                * delta
                * glam::DAffine2::from_translation(glam::dvec2(-16., -16.));
            doc = before.clone();
            Command::TransformNodes {
                ids: vec![1],
                transform: delta.to_cols_array(),
            }
            .apply(&mut doc)
            .unwrap();
            let actual = pixels(&doc);
            // The existing compositor resamples mask/source grids. This allows
            // at most four byte levels of interpolation, not a scaled feather.
            let max_alpha_error = actual
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.as_chunks::<4>().0.iter())
                .map(|(a, b)| a[3].abs_diff(b[3]))
                .max()
                .unwrap();
            assert!(
                max_alpha_error <= 4,
                "smart={smart}, error={max_alpha_error}"
            );
            assert!(Arc::ptr_eq(
                doc.nodes[0].mask.as_ref().unwrap(),
                before.nodes[0].mask.as_ref().unwrap()
            ));
            assert_eq!(
                doc.nodes[0].mask_properties,
                before.nodes[0].mask_properties
            );
        }
    }
}
