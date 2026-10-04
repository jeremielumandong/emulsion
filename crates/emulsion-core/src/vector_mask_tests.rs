use crate::{
    Command, Document, Editor, EmptyVectorCoverage, MaskProperties, Node, NodeKind, VectorMask,
};
use emulsion_raster::{
    Mask, Placement, Raster,
    vector::{Anchor, Path, SubPath},
};
use std::sync::Arc;

fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Arc<Path> {
    Arc::new(Path {
        subpaths: vec![SubPath {
            anchors: [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                .map(Anchor::corner)
                .to_vec(),
            closed: true,
        }],
    })
}
fn document() -> Document {
    let mut doc = Document::new(32, 24);
    let mut node = Node::raster(
        1,
        "Image",
        Arc::new(Raster::solid(32, 24, [1.; 4])),
        Placement::default(),
    );
    node.vector_mask = Some(VectorMask {
        path: rectangle(8., 4., 12., 12.),
        ..VectorMask::default()
    });
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
fn vector(doc: &Document) -> Arc<Mask> {
    doc.vector_mask_for_inspection(&doc.nodes[0]).unwrap()
}

#[test]
fn vector_mask_empty_reveal_hide_inversion_and_last_anchor_keep_image() {
    for empty_coverage in [EmptyVectorCoverage::RevealAll, EmptyVectorCoverage::HideAll] {
        for inverted in [false, true] {
            let mut doc = document();
            let original_kind = doc.nodes[0].kind.clone();
            let mask = VectorMask {
                empty_coverage,
                inverted,
                properties: MaskProperties {
                    density: 0.5,
                    feather: 1000.,
                },
                ..VectorMask::default()
            };
            let expected = if mask.empty_value() == 0 { 128 } else { 255 };
            Command::SetVectorMask {
                id: 1,
                mask: Some(mask),
            }
            .apply(&mut doc)
            .unwrap();
            assert_eq!(vector(&doc).fill(), expected);
            assert_eq!(vector(&doc).tile_count(), 0);
            Command::SetVectorMaskPath {
                id: 1,
                path: rectangle(2., 2., 5., 5.),
            }
            .apply(&mut doc)
            .unwrap();
            Command::SetVectorMaskPath {
                id: 1,
                path: Arc::new(Path::default()),
            }
            .apply(&mut doc)
            .unwrap();
            assert_eq!(doc.nodes.len(), 1);
            assert_eq!(doc.nodes[0].kind, original_kind);
            assert_eq!(vector(&doc).fill(), expected);
        }
    }
}

#[test]
fn vector_mask_combines_all_enable_states_and_independent_density() {
    for raster_enabled in [false, true] {
        for vector_enabled in [false, true] {
            let mut doc = document();
            let node = &mut doc.nodes[0];
            let raw = Arc::new(Mask::empty(32, 24, 64));
            node.mask = Some(raw.clone());
            node.mask_properties.density = 0.5;
            node.mask_enabled = raster_enabled;
            let path = node.vector_mask.as_ref().unwrap().path.clone();
            let vm = node.vector_mask.as_mut().unwrap();
            vm.enabled = vector_enabled;
            vm.properties.density = 0.25;
            let raster = doc.raster_mask_for_inspection(&doc.nodes[0]).unwrap();
            let vector = vector(&doc);
            let composite = doc.composite_mask(&doc.nodes[0]);
            for (x, y) in [(1, 1), (10, 8)] {
                let a = if raster_enabled {
                    raster.get(x, y)
                } else {
                    255
                };
                let b = if vector_enabled {
                    vector.get(x, y)
                } else {
                    255
                };
                assert_eq!(
                    composite.as_ref().map_or(255, |m| m.get(x, y)),
                    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
                );
            }
            if !raster_enabled && !vector_enabled {
                assert!(composite.is_none());
            }
            assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
            assert!(Arc::ptr_eq(
                &doc.nodes[0].vector_mask.as_ref().unwrap().path,
                &path
            ));
            assert_eq!(
                doc.mask_for_inspection(&doc.nodes[0]).unwrap().to_gray8(),
                raster.to_gray8()
            );
        }
    }
}

#[test]
fn vector_mask_compound_nonzero_fill_open_subpaths_and_huge_off_canvas_geometry() {
    let mut doc = document();
    let mut path = (*rectangle(-1e9, -1e9, 2e9, 2e9)).clone();
    let mut hole = rectangle(8., 4., 12., 12.).subpaths[0].clone();
    hole.anchors.reverse();
    hole.closed = false; // Fill is implicitly closed while geometry stays editable.
    path.subpaths.push(hole);
    Command::SetVectorMaskPath {
        id: 1,
        path: Arc::new(path),
    }
    .apply(&mut doc)
    .unwrap();
    let mask = vector(&doc);
    assert_eq!(mask.get(1, 1), 255);
    assert_eq!(mask.get(10, 8), 0);
    assert!(!doc.nodes[0].vector_mask.as_ref().unwrap().path.subpaths[1].closed);
}

#[test]
fn vector_mask_zero_feather_affine_is_fresh_output_geometry() {
    let mut doc = document();
    let mut path = (*rectangle(1.25, 1.75, 3.5, 3.5)).clone();
    let affine = glam::DAffine2::from_scale_angle_translation(
        glam::dvec2(3.0, 2.0),
        0.17,
        glam::dvec2(3., 2.),
    );
    let vm = doc.nodes[0].vector_mask.as_mut().unwrap();
    vm.path = Arc::new(path.clone());
    vm.transform = affine.to_cols_array();
    path.transform(affine);
    let polys = path
        .flatten(0.25)
        .into_iter()
        .map(|(p, _)| p)
        .collect::<Vec<_>>();
    let expected = emulsion_raster::vector::fill_coverage(&polys, 32, 24);
    assert_eq!(vector(&doc).to_gray8(), expected.to_gray8());
}

#[test]
fn vector_mask_feather_density_intrinsic_before_affine_matches_raster_oracle() {
    for inverted in [false, true] {
        let mut doc = document();
        let vm = doc.nodes[0].vector_mask.as_mut().unwrap();
        vm.path = rectangle(-3., -2., 16., 16.);
        vm.inverted = inverted;
        vm.properties = MaskProperties {
            density: 0.63,
            feather: 3.5,
        };
        vm.transform = glam::DAffine2::from_scale_angle_translation(
            glam::dvec2(1.4, 0.8),
            0.3,
            glam::dvec2(6., 4.),
        )
        .to_cols_array();
        let vector_result = vector(&doc);
        let mut oracle = doc.nodes[0].clone();
        let vm = oracle.vector_mask.take().unwrap();
        let raw =
            crate::vector_mask::rasterize_path_window(&vm.path, (-16., -16.), (64, 64)).unwrap();
        oracle.mask = Some(Arc::new(if inverted {
            Mask::from_fn(64, 64, 255, |x, y| 255 - raw.get(x, y))
        } else {
            raw
        }));
        oracle.mask_properties = vm.properties;
        oracle.mask_transform = (glam::DAffine2::from_cols_array(&vm.transform)
            * glam::DAffine2::from_translation(glam::dvec2(-16., -16.)))
        .to_cols_array();
        let expected = doc.raster_mask_for_inspection(&oracle).unwrap();
        assert_eq!(vector_result.to_gray8(), expected.to_gray8());
    }
}

#[test]
fn vector_mask_cached_identity_invalidation_and_disabled_inspection() {
    let mut doc = document();
    let first = vector(&doc);
    assert!(Arc::ptr_eq(&first, &vector(&doc)));
    Command::SetVectorMaskEnabled {
        id: 1,
        enabled: false,
    }
    .apply(&mut doc)
    .unwrap();
    assert!(doc.composite_mask(&doc.nodes[0]).is_none());
    assert!(Arc::ptr_eq(&first, &vector(&doc)));
    Command::SetVectorMaskInverted {
        id: 1,
        inverted: true,
    }
    .apply(&mut doc)
    .unwrap();
    let inverted = vector(&doc);
    assert!(!Arc::ptr_eq(&first, &inverted));
    Command::SetVectorMaskProperties {
        id: 1,
        properties: MaskProperties {
            density: 0.5,
            feather: 0.,
        },
    }
    .apply(&mut doc)
    .unwrap();
    let density = vector(&doc);
    assert!(!Arc::ptr_eq(&inverted, &density));
    Command::SetVectorMaskTransform {
        id: 1,
        transform: [1., 0., 0., 1., 3., 0.],
    }
    .apply(&mut doc)
    .unwrap();
    let moved = vector(&doc);
    assert!(!Arc::ptr_eq(&density, &moved));
    Command::SetVectorMaskPath {
        id: 1,
        path: rectangle(2., 2., 6., 6.),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(!Arc::ptr_eq(&moved, &vector(&doc)));
}

#[test]
fn vector_mask_smart_expanded_offset_and_document_nodes_use_correct_output_grid() {
    let mut doc = document();
    Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
    if let NodeKind::Smart { cache, offset, .. } = &mut doc.nodes[0].kind {
        *cache = Arc::new(Raster::empty(40, 32, [0; 4]));
        *offset = (-4, -4);
    }
    let mask = vector(&doc);
    assert_eq!((mask.width(), mask.height()), (40, 32));
    assert_eq!(mask.get(12, 8), 255);
    assert_eq!(mask.get(10, 6), 0);
    for kind in [
        NodeKind::Fill { rgba: [255; 4] },
        NodeKind::Group { collapsed: false },
    ] {
        doc.nodes[0].kind = kind;
        let mask = vector(&doc);
        assert_eq!((mask.width(), mask.height()), (32, 24));
    }
}

#[test]
fn vector_mask_large_affine_translation_is_preserved_and_invalid_maps_reject_atomically() {
    for translation in [-1e10, 1e10] {
        let mut doc = document();
        let path = rectangle((2. - translation) / 16., 2., 0.75, 8.);
        Command::SetVectorMaskPath {
            id: 1,
            path: path.clone(),
        }
        .apply(&mut doc)
        .unwrap();
        Command::SetVectorMaskTransform {
            id: 1,
            transform: [1., 0., 0., 1., translation, 0.],
        }
        .apply(&mut doc)
        .unwrap();
        assert!(vector(&doc).to_gray8().iter().all(|value| *value == 0));
        let mut editor = Editor::new(doc.clone(), None);
        let transform = [16., 0., 0., 1., translation, 0.];
        editor
            .execute(Command::SetVectorMaskTransform { id: 1, transform })
            .unwrap();
        let mask = editor.doc.nodes[0].vector_mask.as_ref().unwrap();
        assert_eq!(mask.transform, transform);
        assert!(Arc::ptr_eq(&mask.path, &path));
        let expected = Mask::from_fn(32, 24, 0, |x, y| {
            if (2..14).contains(&x) && (2..10).contains(&y) {
                255
            } else {
                0
            }
        });
        assert_eq!(vector(&editor.doc).to_gray8(), expected.to_gray8());
        let accepted = editor.doc.clone();
        // Every matrix entry and the determinant are finite, but mapping the
        // supported intrinsic envelope overflows; this is genuinely invalid.
        for invalid in [
            Command::SetVectorMaskTransform {
                id: 1,
                transform: [1e300, 0., 0., 1., 0., 0.],
            },
            Command::SetVectorMask {
                id: 1,
                mask: Some(VectorMask {
                    transform: [1e300, 0., 0., 1., 0., 0.],
                    ..mask.clone()
                }),
            },
        ] {
            assert!(editor.execute(invalid).is_err());
            assert_eq!(editor.doc, accepted);
            assert_eq!(editor.history.len(), 1);
        }
        assert!(editor.undo());
        assert_eq!(editor.doc, doc);
        assert!(editor.redo());
        assert_eq!(editor.doc, accepted);
    }
}

#[test]
fn vector_mask_commands_reject_invalid_atomically_respect_locks_and_allow_pixel_lock() {
    let mut doc = document();
    let before = doc.clone();
    let mut invalid = (*rectangle(1., 1., 2., 2.)).clone();
    invalid.subpaths[0].anchors[0].p.0 = f64::NAN;
    assert!(
        Command::SetVectorMaskPath {
            id: 1,
            path: Arc::new(invalid)
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    assert!(
        Command::SetVectorMaskTransform {
            id: 1,
            transform: [0.; 6]
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    assert!(
        Command::SetVectorMaskProperties {
            id: 1,
            properties: MaskProperties {
                density: 1.1,
                feather: 0.
            }
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    doc.nodes[0].locks.pixels = true;
    Command::SetVectorMaskPath {
        id: 1,
        path: rectangle(2., 2., 5., 5.),
    }
    .apply(&mut doc)
    .unwrap();
    doc.nodes[0].locks.position = true;
    assert!(
        Command::SetVectorMaskPath {
            id: 1,
            path: rectangle(3., 3., 5., 5.)
        }
        .apply(&mut doc)
        .is_err()
    );
    Command::SetVectorMaskInverted {
        id: 1,
        inverted: true,
    }
    .apply(&mut doc)
    .unwrap();
    doc.nodes[0].locked = true;
    assert!(
        Command::SetVectorMaskInverted {
            id: 1,
            inverted: false
        }
        .apply(&mut doc)
        .is_err()
    );
}

#[test]
fn vector_mask_noop_cancel_and_one_transaction_undo_retain_raw_components() {
    let doc = document();
    let original_path = doc.nodes[0].vector_mask.as_ref().unwrap().path.clone();
    let mut editor = Editor::new(doc.clone(), None);
    editor.begin("Vector gesture");
    editor
        .execute(Command::SetVectorMaskPath {
            id: 1,
            path: rectangle(3., 3., 5., 5.),
        })
        .unwrap();
    editor
        .execute(Command::SetVectorMaskProperties {
            id: 1,
            properties: MaskProperties {
                density: 0.5,
                feather: 2.,
            },
        })
        .unwrap();
    editor.cancel();
    assert_eq!(editor.doc, doc);
    assert!(!editor.undo());
    editor
        .execute(Command::SetVectorMaskPath {
            id: 1,
            path: Arc::new((*original_path).clone()),
        })
        .unwrap();
    assert!(!editor.undo());
    editor.begin("Vector gesture");
    editor
        .execute(Command::SetVectorMaskPath {
            id: 1,
            path: rectangle(3., 3., 5., 5.),
        })
        .unwrap();
    editor
        .execute(Command::SetVectorMaskProperties {
            id: 1,
            properties: MaskProperties {
                density: 0.5,
                feather: 2.,
            },
        })
        .unwrap();
    editor.end();
    assert!(editor.undo());
    assert_eq!(editor.doc, doc);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_ne!(editor.doc, doc);
}

#[test]
fn vector_mask_style_cache_and_fingerprint_observe_geometry_and_properties() {
    let mut doc = document();
    doc.nodes[0]
        .styles
        .push(crate::styles::LayerStyle::ColorOverlay {
            color: [230, 20, 90],
            opacity: 100.,
        });
    let first = crate::styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &crate::styles::render(&doc, &doc.nodes[0]).unwrap()
    ));
    let fingerprint = crate::storyboard_fingerprint::document_fingerprint(&doc);
    Command::SetVectorMaskProperties {
        id: 1,
        properties: MaskProperties {
            density: 0.5,
            feather: 2.,
        },
    }
    .apply(&mut doc)
    .unwrap();
    let changed = crate::styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    assert_ne!(
        fingerprint,
        crate::storyboard_fingerprint::document_fingerprint(&doc)
    );
    Command::SetVectorMaskEnabled {
        id: 1,
        enabled: false,
    }
    .apply(&mut doc)
    .unwrap();
    assert!(!Arc::ptr_eq(
        &changed,
        &crate::styles::render(&doc, &doc.nodes[0]).unwrap()
    ));
}

#[test]
fn vector_mask_branch_merge_keeps_independent_path_properties_and_raster_edits() {
    let base = document();
    let mut ours = base.clone();
    let mut theirs = base.clone();
    let path = rectangle(2., 2., 6., 6.);
    Command::SetVectorMaskPath {
        id: 1,
        path: path.clone(),
    }
    .apply(&mut ours)
    .unwrap();
    Command::SetVectorMaskProperties {
        id: 1,
        properties: MaskProperties {
            density: 0.5,
            feather: 2.,
        },
    }
    .apply(&mut theirs)
    .unwrap();
    Command::SetMask {
        id: 1,
        mask: Some(Arc::new(Mask::empty(32, 24, 128))),
    }
    .apply(&mut theirs)
    .unwrap();
    let crate::graph::MergeOutcome::Merged(merged) =
        crate::graph::merge(&base, &ours, &theirs, &Default::default()).unwrap()
    else {
        panic!("independent fields must merge");
    };
    assert_eq!(merged.nodes[0].vector_mask.as_ref().unwrap().path, path);
    assert_eq!(
        merged.nodes[0]
            .vector_mask
            .as_ref()
            .unwrap()
            .properties
            .density,
        0.5
    );
    assert!(merged.nodes[0].mask.is_some());
}

#[test]
fn vector_mask_path_buffers_are_counted_once_when_shared_with_content_and_duplicates() {
    let mut doc = document();
    let path = doc.nodes[0].vector_mask.as_ref().unwrap().path.clone();
    let buffer = path.subpaths[0].anchors.as_ptr() as usize;
    doc.nodes.push(Node::path(
        2,
        "Shared path",
        path.clone(),
        Default::default(),
        32,
        24,
    ));
    let clone = doc.nodes[0].clone();
    doc.nodes.push(clone);
    let buffers = doc.buffers();
    assert_eq!(buffers.iter().filter(|(ptr, _)| *ptr == buffer).count(), 1);
    assert!(
        buffers
            .iter()
            .any(|(ptr, size)| *ptr == buffer && *size >= 4 * std::mem::size_of::<Anchor>())
    );
    let NodeKind::Path { cache, .. } = &doc.nodes[1].kind else {
        panic!()
    };
    assert!(!cache.is_rendered());
    let mut shared = std::collections::HashSet::new();
    assert_eq!(doc.buffers_once(&mut shared), buffers);
    assert!(doc.clone().buffers_once(&mut shared).is_empty());
}

#[test]
fn vector_mask_removal_and_raster_edits_preserve_other_component() {
    let mut doc = document();
    let path = doc.nodes[0].vector_mask.as_ref().unwrap().path.clone();
    let raw = Arc::new(Mask::empty(32, 24, 120));
    Command::SetMask {
        id: 1,
        mask: Some(raw.clone()),
    }
    .apply(&mut doc)
    .unwrap();
    let before = vector(&doc).to_gray8();
    Command::SetMask {
        id: 1,
        mask: Some(Arc::new(Mask::empty(32, 24, 80))),
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(vector(&doc).to_gray8(), before);
    assert!(Arc::ptr_eq(
        &path,
        &doc.nodes[0].vector_mask.as_ref().unwrap().path
    ));
    Command::SetMask { id: 1, mask: None }
        .apply(&mut doc)
        .unwrap();
    assert_eq!(
        doc.composite_mask(&doc.nodes[0]).unwrap().to_gray8(),
        before
    );
    Command::SetMask {
        id: 1,
        mask: Some(raw.clone()),
    }
    .apply(&mut doc)
    .unwrap();
    Command::SetVectorMask { id: 1, mask: None }
        .apply(&mut doc)
        .unwrap();
    assert!(Arc::ptr_eq(&raw, doc.nodes[0].mask.as_ref().unwrap()));
    assert!(Arc::ptr_eq(
        &raw,
        &doc.composite_mask(&doc.nodes[0]).unwrap()
    ));
}

#[test]
fn vector_mask_combined_coverage_reaches_group_adjustment_clipping_and_effects() {
    use emulsion_raster::{Adjustment, BlendMode};
    for hides_effects in [false, true] {
        let mut doc = document();
        doc.nodes[0].mask = Some(Arc::new(Mask::empty(32, 24, 180)));
        doc.nodes[0].styles.push(crate::styles::LayerStyle::Stroke {
            color: [180, 20, 40],
            opacity: 65.,
            size: 2.,
        });
        doc.nodes[0].blending.layer_mask_hides_effects = hides_effects;
        let mut fill = Node::new(
            2,
            "Clipped fill",
            NodeKind::Fill {
                rgba: [30, 130, 50, 160],
            },
        );
        fill.clip_to = Some(1);
        fill.vector_mask = Some(VectorMask {
            path: rectangle(3., 6., 22., 11.),
            ..Default::default()
        });
        let mut adjustment = Node::adjust(
            3,
            Adjustment::Exposure {
                exposure: 0.5,
                offset: 0.,
                gamma: 1.,
            },
        );
        adjustment.vector_mask = Some(VectorMask {
            path: rectangle(1., 1., 16., 20.),
            ..Default::default()
        });
        let mut group = Node::group(4, "Pass through group");
        group.blend = BlendMode::PassThrough;
        group.vector_mask = Some(VectorMask {
            path: rectangle(4., 2., 24., 20.),
            properties: MaskProperties {
                density: 0.75,
                feather: 1.,
            },
            ..Default::default()
        });
        for node in [&mut doc.nodes[0], &mut fill, &mut adjustment] {
            node.parent = Some(4);
        }
        doc.nodes.extend([fill, adjustment, group]);
        doc.next_id = 5;
        doc.normalize();
        doc.validate().unwrap();
        let expected = emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
        let derived = doc
            .nodes
            .iter()
            .map(|node| doc.composite_mask(node))
            .collect::<Vec<_>>();
        let mut raster_reference = doc.clone();
        for (node, mask) in raster_reference.nodes.iter_mut().zip(derived) {
            node.mask = mask;
            node.mask_enabled = true;
            node.mask_properties = Default::default();
            node.mask_transform = crate::node::default_mask_transform();
            node.vector_mask = None;
        }
        assert_eq!(
            expected,
            emulsion_raster::composite::flatten(&raster_reference.composite_tree(), 0).to_srgba8()
        );
        assert!(expected.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
    }
}

#[test]
fn vector_mask_huge_cubic_is_visible_through_the_public_component_renderer() {
    let mut a = Anchor::corner((-1e8, 1.5e8));
    a.h_out = (-1e8, -0.5e8);
    let mut b = Anchor::corner((1e8, 1.5e8));
    b.h_in = (1e8, -0.5e8);
    let path = Arc::new(Path {
        subpaths: vec![SubPath {
            anchors: vec![a, b],
            closed: true,
        }],
    });
    let mut doc = document();
    doc.nodes[0].vector_mask = Some(VectorMask {
        path: path.clone(),
        transform: [1., 0., 0., 1., 5000., 0.],
        ..Default::default()
    });
    doc.validate().unwrap();
    let coverage = vector(&doc);
    // Analytic edge y=6R*d² with x=R*(3d-4d³) lies near0.1666px.
    // Exactly three of the four y subsamples are inside row0; all of row1 is.
    for x in 0..32 {
        assert_eq!(coverage.get(x, 0), 191);
        assert_eq!(coverage.get(x, 1), 255);
    }
    assert!(Arc::ptr_eq(
        &path,
        &doc.nodes[0].vector_mask.as_ref().unwrap().path
    ));
}

#[test]
fn smart_vector_canonical_grid_rasterize_preserves_exact_component_and_composite_bytes() {
    for properties in [
        MaskProperties::default(),
        MaskProperties {
            density: 0.63,
            feather: 2.,
        },
    ] {
        for vector_enabled in [false, true] {
            for raster_enabled in [false, true] {
                let mut doc = Document::new(40, 30);
                let mut node = Node::smart(
                    1,
                    "Canonical vector cache",
                    Arc::new(Raster::solid(12, 8, [0.7, 0.2, 0.1, 0.8])),
                    vec![emulsion_filters::Filter::GaussianBlur { radius: 1.5 }],
                    Placement {
                        x: 6.,
                        y: 5.,
                        scale_x: 1.3,
                        scale_y: 0.9,
                        rotation: 17.,
                        ..Default::default()
                    },
                );
                node.mask = Some(Arc::new(Mask::from_fn(12, 8, 0, |x, y| {
                    ((x * 17 + y * 23) % 256) as u8
                })));
                node.mask_enabled = raster_enabled;
                node.mask_properties = MaskProperties {
                    density: 0.45,
                    feather: 2.,
                };
                node.mask_transform = [1., 0., 0.1, 1., 1.25, -0.5];
                node.vector_mask = Some(VectorMask {
                    path: Arc::new(
                        Path::from_svg("M -3.25 1.125 C 4.5 -2 10.25 1.5 11.75 6.75 L 2 11 Z")
                            .unwrap(),
                    ),
                    enabled: vector_enabled,
                    linked: false,
                    inverted: true,
                    transform: glam::DAffine2::from_scale_angle_translation(
                        glam::dvec2(1.2, 0.8),
                        0.23,
                        glam::dvec2(2.25, -1.75),
                    )
                    .to_cols_array(),
                    properties,
                    ..Default::default()
                });
                let NodeKind::Smart { offset, .. } = &node.kind else {
                    panic!()
                };
                let offset = *offset;
                assert_ne!(offset, (0, 0));
                let path = node.vector_mask.as_ref().unwrap().path.clone();
                let expected_transform = crate::composite_mask_cache::mask_to_output(
                    node.vector_mask.as_ref().unwrap().transform,
                    offset,
                )
                .to_cols_array();
                let raw = node.mask.clone().unwrap();
                doc.nodes.push(node);
                doc.next_id = 2;
                doc.validate().unwrap();
                let before_vector = vector(&doc);
                let before_raster = doc.raster_mask_for_inspection(&doc.nodes[0]).unwrap();
                let before =
                    emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
                Command::Rasterize { id: 1 }.apply(&mut doc).unwrap();
                let after = doc.nodes[0].vector_mask.as_ref().unwrap();
                assert!(Arc::ptr_eq(&after.path, &path));
                assert!(Arc::ptr_eq(doc.nodes[0].mask.as_ref().unwrap(), &raw));
                assert_eq!(after.transform, expected_transform);
                assert_eq!(after.properties, properties);
                assert_eq!(after.enabled, vector_enabled);
                assert!(!after.linked);
                assert_eq!(before_vector.to_gray8(), vector(&doc).to_gray8());
                assert_eq!(before_vector.fill(), vector(&doc).fill());
                assert_eq!(
                    before_raster.to_gray8(),
                    doc.raster_mask_for_inspection(&doc.nodes[0])
                        .unwrap()
                        .to_gray8()
                );
                assert_eq!(
                    before,
                    emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8()
                );
            }
        }
    }
}

#[test]
fn apply_raster_on_smart_rebases_retained_vector_without_roundtrip_or_double_properties() {
    for feather in [0., 2.] {
        for enabled in [false, true] {
            let mut doc = document();
            let placement = Placement {
                x: 6.,
                y: 5.,
                scale_x: 1.3,
                scale_y: 0.9,
                rotation: 17.,
                ..Default::default()
            };
            if let NodeKind::Raster { placement: p, .. } = &mut doc.nodes[0].kind {
                *p = placement;
            }
            let vm = doc.nodes[0].vector_mask.as_mut().unwrap();
            vm.properties = MaskProperties {
                density: 0.63,
                feather,
            };
            vm.transform = [1., 0.125, -0.25, 1., 1.25, -0.5];
            vm.enabled = enabled;
            let original_transform = vm.transform;
            Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
            assert_eq!(
                doc.nodes[0].vector_mask.as_ref().unwrap().transform,
                original_transform
            );
            Command::SetFilters {
                id: 1,
                filters: vec![emulsion_filters::Filter::GaussianBlur { radius: 1.5 }],
            }
            .apply(&mut doc)
            .unwrap();
            doc.nodes[0].mask = Some(Arc::new(Mask::empty(32, 24, 255)));
            let NodeKind::Smart { offset, .. } = &doc.nodes[0].kind else {
                panic!()
            };
            let expected_transform =
                crate::composite_mask_cache::mask_to_output(original_transform, *offset)
                    .to_cols_array();
            let before = vector(&doc).to_gray8();
            let pixels = emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
            let path = doc.nodes[0].vector_mask.as_ref().unwrap().path.clone();
            Command::ApplyLayerMask { id: 1 }.apply(&mut doc).unwrap();
            assert_eq!(vector(&doc).to_gray8(), before);
            assert!(doc.nodes[0].mask.is_none());
            let mask = doc.nodes[0].vector_mask.as_ref().unwrap();
            assert_eq!(mask.transform, expected_transform);
            assert_eq!(
                mask.properties,
                MaskProperties {
                    density: 0.63,
                    feather
                }
            );
            assert!(Arc::ptr_eq(&mask.path, &path));
            assert_eq!(
                emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8(),
                pixels
            );
        }
    }
}
