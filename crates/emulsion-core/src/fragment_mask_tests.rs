//! A placed fragment carries all mask components with its copied artwork.
use super::*;
use crate::{EmptyVectorCoverage, MaskProperties, SmartFilterMask, VectorMask};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::{
    Mask, Placement, Raster,
    composite::flatten,
    strokes::{Stroke, StrokePoint, StrokeSet},
    vector::{Path, PathStyle},
};

fn masks(node: &mut Node, size: (u32, u32)) {
    node.mask = Some(Arc::new(Mask::from_fn(size.0, size.1, 0, |x, y| {
        if x > 3 && y > 2 { 255 } else { 0 }
    })));
    node.mask_transform = crate::Mapping2::Affine(glam::DAffine2::from_cols_array(
        &([1., 0., 0., 1., -2., -3.]),
    ));
    node.vector_mask = Some(VectorMask {
        path: Arc::new(Path::from_svg("M 13 10 L 40 10 L 40 35 L 13 35 Z").unwrap()),
        linked: false,
        transform: [1., 0., 0., 1., -11., -7.],
        empty_coverage: EmptyVectorCoverage::HideAll,
        ..Default::default()
    });
}

fn fixture_source(smart: bool, active_filter: bool) -> Document {
    let mut doc = Document::new(64, 48);
    let pixels = Arc::new(Raster::solid(32, 32, [1., 0., 0., 1.]));
    let mut node = if smart {
        let filters = vec![Filter::Invert, Filter::GaussianBlur { radius: 11.3 }];
        let styles = vec![
            FilterStyle {
                opacity: if active_filter { 1. } else { 0. },
                ..Default::default()
            },
            FilterStyle {
                enabled: false,
                ..Default::default()
            },
        ];
        let (cache, offset) = crate::smart::render_stack(&pixels, &filters, &styles, true);
        Node::new(
            1,
            "Original",
            NodeKind::Smart {
                editable: None,
                // Opaque retained provenance: this model test never decodes it.
                original_image: Some(Arc::new(crate::node::OriginalImage::new(
                    Arc::new(vec![11, 22, 33]),
                    [1; 32],
                    [2; 32],
                ))),
                source: pixels,
                filters,
                filter_styles: styles,
                filters_enabled: true,
                filter_mask: Some(SmartFilterMask::new(Arc::new(Mask::from_fn(
                    32,
                    32,
                    0,
                    |x, _| if x < 16 { 255 } else { 0 },
                )))),
                placement: crate::SmartPlacement::Legacy(Placement::at(11., 7.)),
                cache,
                offset,
            },
        )
    } else {
        Node::raster(1, "Original", pixels, Placement::at(11., 7.))
    };
    masks(&mut node, (32, 32));
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}

fn paper() -> Document {
    let mut doc = Document::new(128, 80);
    doc.nodes
        .push(Node::new(1, "Paper", NodeKind::Fill { rgba: [255; 4] }));
    doc.next_id = 2;
    doc
}

fn layout_target() -> (Editor, NodeId) {
    let mut target = Editor::new(paper(), None);
    let group = target
        .execute(Command::AddNode {
            node: Box::new(Node::group(0, "Layout")),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    crate::design_layout::enable(
        &mut target,
        group,
        crate::design_layout::Frame {
            padding: [4.; 4],
            ..Default::default()
        },
        (100., 60.),
    )
    .unwrap();
    (target, group)
}

fn expected_pixels(source: &Document) -> Vec<u8> {
    let mut expected = paper();
    expected.nodes.push(Node::raster(
        2,
        "Translated appearance",
        Arc::new(flatten(&source.composite_tree(), 0)),
        Placement::at(32., 16.),
    ));
    flatten(&expected.composite_tree(), 0).to_srgba8()
}

fn assert_world(actual: DAffine2, expected: DAffine2) {
    assert!(
        actual.abs_diff_eq(expected, 1e-8),
        "{actual:?} != {expected:?}"
    );
}

fn assert_resources(before: &Node, after: &Node) {
    assert!(Arc::ptr_eq(
        before.mask.as_ref().unwrap(),
        after.mask.as_ref().unwrap()
    ));
    assert_eq!(before.mask_enabled, after.mask_enabled);
    assert_eq!(before.mask_linked, after.mask_linked);
    assert_eq!(before.mask_properties, after.mask_properties);
    let (a, b) = (
        before.vector_mask.as_ref().unwrap(),
        after.vector_mask.as_ref().unwrap(),
    );
    assert!(Arc::ptr_eq(&a.path, &b.path));
    assert_eq!(
        (
            a.enabled,
            a.linked,
            a.inverted,
            a.properties,
            a.empty_coverage
        ),
        (
            b.enabled,
            b.linked,
            b.inverted,
            b.properties,
            b.empty_coverage
        )
    );
    if let (
        NodeKind::Smart {
            source: a,
            cache: ac,
            offset: ao,
            original_image: ai,
            filters: af,
            filter_styles: ast,
            filters_enabled: ae,
            filter_mask: Some(am),
            ..
        },
        NodeKind::Smart {
            source: b,
            cache: bc,
            offset: bo,
            original_image: bi,
            filters: bf,
            filter_styles: bst,
            filters_enabled: be,
            filter_mask: Some(bm),
            ..
        },
    ) = (&before.kind, &after.kind)
    {
        assert!(Arc::ptr_eq(a, b));
        assert!(Arc::ptr_eq(ac, bc));
        assert!(Arc::ptr_eq(ai.as_ref().unwrap(), bi.as_ref().unwrap()));
        assert_eq!((ao, af, ast, ae), (bo, bf, bst, be));
        assert!(Arc::ptr_eq(&am.pixels, &bm.pixels));
        assert_eq!(am, bm);
    } else if let (NodeKind::Raster { raster: a, .. }, NodeKind::Raster { raster: b, .. }) =
        (&before.kind, &after.kind)
    {
        assert!(Arc::ptr_eq(a, b));
    }
}

#[test]
fn fragment_masks_smart_design_placement_retains_visible_source_and_one_undo() {
    let source = fixture_source(true, false);
    let saved_source = source.clone();
    let fragment = Fragment::capture(&source, &[1]).unwrap();
    let saved_fragment = fragment.nodes.clone();
    let mut target =
        crate::project::ProjectEditor::new_project(crate::project::ProjectKind::Design, paper())
            .unwrap();
    let before = target.doc.clone();
    let history = target.history.len();
    let id = fragment
        .paste_into_project(&mut target, Slot::TOP, (32., 16.))
        .unwrap()[0];
    let after = target.doc.clone();
    let placed = after.node(id).unwrap();
    assert_eq!(
        crate::transform::local_to_document(placed)
            .unwrap()
            .require_affine("legacy fixture")
            .unwrap()
            .translation,
        dvec2(43., 23.)
    );
    assert_eq!(
        placed.vector_mask.as_ref().unwrap().transform,
        [1., 0., 0., 1., -11., -7.]
    );
    assert_eq!(placed.mask_transform, source.nodes[0].mask_transform);
    assert_resources(&source.nodes[0], placed);
    let pixels = flatten(&after.composite_tree(), 0).to_srgba8();
    assert_eq!(pixels, expected_pixels(&source));
    assert!(pixels.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
    assert_eq!(target.history.len(), history + 1);
    assert!(target.undo());
    assert_eq!(target.doc, before);
    assert!(target.redo());
    assert_eq!(target.doc, after);
    assert_eq!(source, saved_source);
    assert_eq!(fragment.nodes, saved_fragment);
}

#[test]
fn fragment_masks_linkage_and_enabled_states_do_not_change_placement_appearance() {
    for smart in [false, true] {
        for linked in 0..8 {
            for enabled in 0..8 {
                let mut source = fixture_source(smart, true);
                let node = &mut source.nodes[0];
                node.mask_linked = linked & 1 != 0;
                node.mask_enabled = enabled & 1 != 0;
                let vector = node.vector_mask.as_mut().unwrap();
                vector.linked = linked & 2 != 0;
                vector.enabled = enabled & 2 != 0;
                vector.inverted = enabled & 4 != 0;
                if let NodeKind::Smart {
                    filter_mask: Some(mask),
                    ..
                } = &mut node.kind
                {
                    mask.linked = linked & 4 != 0;
                    mask.enabled = enabled & 4 != 0;
                }
                let fragment = Fragment::capture(&source, &[1]).unwrap();
                let mut target = Editor::new(paper(), None);
                let id = fragment.paste(&mut target, Slot::TOP, (32., 16.)).unwrap()[0];
                let pasted = target.doc.node(id).unwrap();
                let translation = DAffine2::from_translation(dvec2(32., 16.));
                assert_world(
                    crate::transform::mask_to_document(pasted)
                        .unwrap()
                        .require_affine("legacy fixture")
                        .unwrap(),
                    translation
                        * crate::transform::mask_to_document(&source.nodes[0])
                            .unwrap()
                            .require_affine("legacy fixture")
                            .unwrap(),
                );
                assert_world(
                    crate::transform::vector_mask_to_document(pasted)
                        .unwrap()
                        .unwrap(),
                    translation
                        * crate::transform::vector_mask_to_document(&source.nodes[0])
                            .unwrap()
                            .unwrap(),
                );
                assert_resources(&source.nodes[0], pasted);
                assert_eq!(
                    flatten(&target.doc.composite_tree(), 0).to_srgba8(),
                    expected_pixels(&source),
                    "smart={smart}, linked={linked}, enabled={enabled}"
                );
            }
        }
    }
}

#[test]
fn fragment_masks_intrinsic_affines_and_later_ordinary_moves_are_independent() {
    for smart in [false, true] {
        let mut source = fixture_source(smart, true);
        let node = &mut source.nodes[0];
        node.mask_linked = false;
        node.mask_enabled = false;
        node.mask_properties = MaskProperties {
            density: 0.6,
            feather: 1.5,
        };
        node.vector_mask.as_mut().unwrap().properties = node.mask_properties;
        match &mut node.kind {
            NodeKind::Raster { placement, .. }
            | NodeKind::Smart {
                placement: crate::SmartPlacement::Legacy(placement),
                ..
            } => {
                *placement = Placement {
                    x: 11.,
                    y: 7.,
                    scale_x: 1.2,
                    scale_y: 0.8,
                    rotation: 17.,
                    flip_x: true,
                    ..Default::default()
                };
            }
            _ => unreachable!(),
        }
        if let NodeKind::Smart {
            filter_mask: Some(mask),
            offset,
            ..
        } = &mut node.kind
        {
            mask.linked = false;
            mask.enabled = false;
            mask.transform = crate::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
                1., 0.2, -0.1, 1., 2., -3.,
            ]));
            mask.properties = MaskProperties {
                density: 0.7,
                feather: 0.5,
            };
            *offset = (-2, -3);
        }
        let original = source.nodes[0].clone();
        let fragment = Fragment::capture(&source, &[1]).unwrap();
        let mut target = Editor::new(Document::new(128, 80), None);
        let id = fragment
            .paste(&mut target, Slot::TOP, (6.25, -2.5))
            .unwrap()[0];
        let pasted = target.doc.node(id).unwrap().clone();
        let translation = DAffine2::from_translation(dvec2(6.25, -2.5));
        assert_resources(&original, &pasted);
        assert_eq!(original.mask_transform, pasted.mask_transform);
        assert_eq!(original.vector_mask, pasted.vector_mask);
        assert_world(
            crate::transform::mask_to_document(&pasted)
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap(),
            translation
                * crate::transform::mask_to_document(&original)
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
        );
        if smart {
            assert_world(
                crate::smart_filter_mask::to_document(&pasted)
                    .unwrap()
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
                translation
                    * crate::smart_filter_mask::to_document(&original)
                        .unwrap()
                        .unwrap()
                        .require_affine("legacy fixture")
                        .unwrap(),
            );
        }
        target
            .execute(Command::TranslateNode { id, dx: 4., dy: 3. })
            .unwrap();
        let moved = target.doc.node(id).unwrap();
        assert_world(
            crate::transform::mask_to_document(moved)
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap(),
            crate::transform::mask_to_document(&pasted)
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap(),
        );
        assert_world(
            crate::transform::vector_mask_to_document(moved)
                .unwrap()
                .unwrap(),
            crate::transform::vector_mask_to_document(&pasted)
                .unwrap()
                .unwrap(),
        );
        if smart {
            assert_world(
                crate::smart_filter_mask::to_document(moved)
                    .unwrap()
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
                crate::smart_filter_mask::to_document(&pasted)
                    .unwrap()
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
            );
        }
    }
}

#[test]
fn fragment_masks_all_document_kinds_and_hidden_descendants_translate_once() {
    let mut source = Document::new(64, 48);
    source.nodes = vec![
        fixture_source(false, false).nodes.remove(0),
        Node::path(
            2,
            "Path",
            Arc::new(Path::from_svg("M 5 6 L 12 6 L 12 15 Z").unwrap()),
            PathStyle::default(),
            64,
            48,
        ),
        Node::text(
            3,
            "Text",
            crate::text::TextSpec {
                text: "A".into(),
                x: 5.,
                y: 6.,
                ..Default::default()
            },
            64,
            48,
        ),
        Node::strokes(
            4,
            "Strokes",
            Arc::new(StrokeSet {
                strokes: vec![Stroke {
                    points: vec![StrokePoint::new(5., 6.), StrokePoint::new(12., 15.)],
                    ..Stroke::new([255, 0, 0, 255], 2.)
                }],
                fills: Vec::new(),
            }),
            64,
            48,
        ),
        Node::new(
            5,
            "Fill",
            NodeKind::Fill {
                rgba: [255, 0, 0, 255],
            },
        ),
        Node::adjust(6, emulsion_raster::adjust::Adjustment::Invert),
        Node::group(7, "Inner"),
        Node::group(8, "Outer"),
    ];
    for node in &mut source.nodes {
        node.parent = match node.id {
            8 => None,
            7 => Some(8),
            _ => Some(7),
        };
        masks(node, (64, 48));
        node.mask_linked = false;
        node.mask_enabled = false;
        node.vector_mask.as_mut().unwrap().enabled = false;
    }
    source.node_mut(4).unwrap().visible = false;
    source.node_mut(3).unwrap().clip_to = Some(2);
    source.next_id = 9;
    source.validate().unwrap();
    let fragment = Fragment::capture(&source, &[8, 7, 3]).unwrap();
    assert_eq!(fragment.roots, vec![8]);
    let saved = fragment.nodes.clone();
    let mut target = Editor::new(Document::new(128, 80), None);
    let root = fragment
        .paste(&mut target, Slot::TOP, (100.25, -70.5))
        .unwrap()[0];
    let translation = DAffine2::from_translation(dvec2(100.25, -70.5));
    for original in &source.nodes {
        let pasted = target
            .doc
            .nodes
            .iter()
            .find(|node| node.name == original.name)
            .unwrap();
        assert_world(
            crate::transform::mask_to_document(pasted)
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap(),
            translation
                * crate::transform::mask_to_document(original)
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
        );
        assert_world(
            crate::transform::vector_mask_to_document(pasted)
                .unwrap()
                .unwrap(),
            translation
                * crate::transform::vector_mask_to_document(original)
                    .unwrap()
                    .unwrap(),
        );
        assert_resources(original, pasted);
        if original.id != 8 {
            assert!(target.doc.is_ancestor(root, pasted.id));
        }
        match &pasted.kind {
            NodeKind::Path { path, cache, .. } => {
                assert_eq!(path.subpaths[0].anchors[0].p, (105.25, -64.5));
                assert_eq!(cache.size(), (128, 80));
            }
            NodeKind::Text { spec, cache } => {
                assert_eq!((spec.x, spec.y), (105.25, -64.5));
                assert_eq!(cache.size(), (128, 80));
            }
            NodeKind::Strokes { strokes, cache } => {
                assert_eq!(
                    (
                        strokes.strokes[0].points[0].x,
                        strokes.strokes[0].points[0].y
                    ),
                    (105.25, -64.5)
                );
                assert_eq!(cache.size(), (128, 80));
                assert!(!pasted.visible);
            }
            _ => {}
        }
    }
    let text = target
        .doc
        .nodes
        .iter()
        .find(|node| node.name == "Text")
        .unwrap();
    let path = target
        .doc
        .nodes
        .iter()
        .find(|node| node.name == "Path")
        .unwrap();
    assert_eq!(text.clip_to, Some(path.id));
    assert_eq!(text.parent, path.parent);
    assert_eq!(fragment.nodes, saved);
    target.doc.validate().unwrap();
}

#[test]
fn fragment_masks_zero_offset_repeated_paste_and_component_dependencies() {
    let mut source = Editor::new(fixture_source(true, true), None);
    let instance = crate::design_components::create(&mut source, &[1], "Masked asset").unwrap();
    let fragment = Fragment::capture(&source.doc, &[instance]).unwrap();
    let saved = fragment.nodes.clone();
    let source_roots = crate::design_components::source_roots(&fragment.design);
    assert!(!source_roots.is_empty());
    let mut target = Editor::new(Document::new(128, 80), None);
    for offset in [(0., 0.), (32., 16.)] {
        let root = fragment.paste(&mut target, Slot::TOP, offset).unwrap()[0];
        let child = target.doc.children(Some(root))[0];
        let pasted = target.doc.node(child).unwrap();
        let original = source.doc.node(1).unwrap();
        assert_resources(original, pasted);
        assert_eq!(original.mask_transform, pasted.mask_transform);
        assert_eq!(original.vector_mask, pasted.vector_mask);
        assert_world(
            crate::transform::local_to_document(pasted)
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap(),
            DAffine2::from_translation(dvec2(offset.0, offset.1))
                * crate::transform::local_to_document(original)
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
        );
    }
    for root in crate::design_components::source_roots(&target.doc.design) {
        let hidden = target.doc.node(root).unwrap();
        assert!(!hidden.visible);
        let child = target.doc.node(target.doc.children(Some(root))[0]).unwrap();
        assert_eq!(child.kind, source.doc.node(1).unwrap().kind);
        assert_eq!(child.vector_mask, source.doc.node(1).unwrap().vector_mask);
    }
    assert_eq!(fragment.nodes, saved);
    target.doc.validate().unwrap();
}

#[test]
fn fragment_masks_invalid_preparation_and_unmovable_roots_leave_history_unchanged() {
    let source = fixture_source(true, false);
    let good = Fragment::capture(&source, &[1]).unwrap();
    let mut target = Editor::new(paper(), None);
    target
        .execute(Command::Rename {
            id: 1,
            name: "Existing history".into(),
        })
        .unwrap();
    let before = target.doc.clone();
    let revision = target.revision;
    let history = target.history.len();
    let mut cases = vec![
        ("NaN offset", good.clone(), (f64::NAN, 0.)),
        ("infinite offset", good.clone(), (0., f64::INFINITY)),
    ];
    let mut overflow = good.clone();
    if let NodeKind::Smart { placement, .. } = &mut overflow.nodes[0].kind {
        let crate::SmartPlacement::Legacy(placement) = placement else {
            panic!("legacy fixture");
        };
        placement.x = f64::MAX;
    }
    cases.push(("placement overflow", overflow, (f64::MAX, 0.)));
    for (component, label) in [
        (0, "raster mask world overflow"),
        (1, "vector mask world overflow"),
        (2, "filter mask world overflow"),
    ] {
        let mut overflow = good.clone();
        let node = &mut overflow.nodes[0];
        if let NodeKind::Smart {
            placement,
            filter_mask,
            ..
        } = &mut node.kind
        {
            let crate::SmartPlacement::Legacy(placement) = placement else {
                panic!("legacy fixture");
            };
            placement.x = f64::MAX / 2.;
            if component == 2 {
                if let crate::Mapping2::Affine(transform) =
                    &mut filter_mask.as_mut().unwrap().transform
                {
                    transform.translation.x = f64::MAX / 2.;
                } else {
                    panic!("legacy fixture");
                }
            }
        }
        if component == 0 {
            if let crate::Mapping2::Affine(transform) = &mut node.mask_transform {
                transform.translation.x = f64::MAX / 2.;
            } else {
                panic!("legacy fixture");
            }
        }
        if component == 1 {
            node.vector_mask.as_mut().unwrap().transform[4] = f64::MAX / 2.;
        }
        // Stored scalars stay finite while the composed mask world map overflows.
        cases.push((label, overflow, (f64::MAX / 2., 0.)));
    }
    for (label, node) in [
        (
            "text coordinate overflow",
            Node::text(1, "Text", crate::text::TextSpec::default(), 64, 48),
        ),
        (
            "path coordinate overflow",
            Node::path(
                1,
                "Path",
                Arc::new(Path::from_svg("M 1 2 L 3 4").unwrap()),
                PathStyle::default(),
                64,
                48,
            ),
        ),
        (
            "stroke coordinate overflow",
            Node::strokes(
                1,
                "Strokes",
                Arc::new(StrokeSet {
                    strokes: vec![Stroke {
                        points: vec![StrokePoint::new(f64::MAX, 2.)],
                        ..Stroke::new([0; 4], 1.)
                    }],
                    fills: Vec::new(),
                }),
                64,
                48,
            ),
        ),
    ] {
        let mut overflow = good.clone();
        overflow.nodes = vec![node];
        if let NodeKind::Path { path, .. } = &mut overflow.nodes[0].kind {
            let mut changed = (**path).clone();
            changed.subpaths[0].anchors[0].p.0 = f64::MAX;
            *path = Arc::new(changed);
        }
        cases.push((label, overflow, (f64::MAX, 0.)));
    }
    let mut broken = good.clone();
    broken.nodes[0].parent = Some(99);
    cases.push(("missing parent", broken, (32., 16.)));
    for (label, extra) in [
        ("unmovable empty group", Node::group(2, "Empty")),
        (
            "unmovable unmasked fill",
            Node::new(2, "Unmasked fill", NodeKind::Fill { rgba: [255; 4] }),
        ),
    ] {
        let mut mixed = good.clone();
        mixed.nodes.push(extra);
        mixed.roots.push(2);
        cases.push((label, mixed, (32., 16.)));
    }
    for (label, fragment, offset) in cases {
        let original = fragment.nodes.clone();
        assert!(
            fragment.paste(&mut target, Slot::TOP, offset).is_err(),
            "{label}"
        );
        assert_eq!(target.doc, before, "{label}");
        assert_eq!(target.revision, revision, "{label}");
        assert_eq!(target.history.len(), history, "{label}");
        assert!(!target.in_transaction(), "{label}");
        assert_eq!(fragment.nodes, original, "{label}");
    }
    assert!(
        good.paste(&mut target, Slot::top_of(Some(999)), (32., 16.))
            .is_err()
    );
    assert_eq!(target.doc, before);
    assert_eq!(target.revision, revision);
    assert_eq!(target.history.len(), history);
}

#[test]
fn fragment_masks_layout_postlude_rejection_is_atomic() {
    let (mut target, group) = layout_target();
    let source = fixture_source(true, false);
    let fragment = Fragment::capture(&source, &[1]).unwrap();
    let before = target.doc.clone();
    let history = target.history.len();
    let revision = target.revision;
    let error = fragment
        .paste(&mut target, Slot::top_of(Some(group)), (32., 16.))
        .unwrap_err();
    assert!(error.contains("unlinked mask"), "{error}");
    assert_eq!(target.doc, before);
    assert_eq!(target.history.len(), history);
    assert_eq!(target.revision, revision);
    assert!(!target.in_transaction());
    // The same layout still accepts ordinary linked artwork.
    let mut linked = source;
    linked.nodes[0].vector_mask.as_mut().unwrap().linked = true;
    Fragment::capture(&linked, &[1])
        .unwrap()
        .paste(&mut target, Slot::top_of(Some(group)), (32., 16.))
        .unwrap();
    assert_eq!(target.history.len(), history + 1);
}

#[test]
fn fragment_masks_layout_guard_includes_unlinked_group_masks_and_disabled_components() {
    for disabled in [false, true] {
        let (mut target, group) = layout_target();
        let mut source = fixture_source(false, false);
        let node = &mut source.nodes[0];
        node.parent = Some(2);
        node.vector_mask.as_mut().unwrap().linked = true;
        let mut ancestor = Node::group(2, "Masked group");
        masks(&mut ancestor, (64, 48));
        ancestor.vector_mask.as_mut().unwrap().enabled = !disabled;
        source.nodes.push(ancestor);
        source.next_id = 3;
        let fragment = Fragment::capture(&source, &[2]).unwrap();
        let before = target.doc.clone();
        let revision = target.revision;
        let history = target.history.len();
        let error = fragment
            .paste(&mut target, Slot::top_of(Some(group)), (32., 16.))
            .unwrap_err();
        assert!(error.contains("unlinked mask"), "{error}");
        assert_eq!(target.doc, before);
        assert_eq!(target.revision, revision);
        assert_eq!(target.history.len(), history);
    }
}

#[test]
fn fragment_masks_layout_guard_allows_unchanged_geometry_and_content_only_updates() {
    let (mut target, group) = layout_target();
    let mut source = fixture_source(false, false);
    source.nodes[0].mask = Some(Arc::new(Mask::white(32, 32)));
    source.nodes[0].mask_transform = crate::Mapping2::IDENTITY;
    source.nodes[0].mask_linked = false;
    source.nodes[0].vector_mask = None;
    let fragment = Fragment::capture(&source, &[1]).unwrap();
    // The requested placement already matches the first layout cell.
    let id = fragment
        .paste(&mut target, Slot::top_of(Some(group)), (-7., -3.))
        .unwrap()[0];
    let placed = target.doc.node(id).unwrap();
    assert_eq!(
        crate::transform::local_to_document(placed)
            .unwrap()
            .require_affine("legacy fixture")
            .unwrap()
            .translation,
        dvec2(4., 4.)
    );
    assert!(!placed.mask_linked);

    let mut doc = fixture_source(true, false);
    let original = doc.nodes.clone();
    if let NodeKind::Smart {
        filters_enabled,
        filter_styles,
        ..
    } = &mut doc.nodes[0].kind
    {
        *filters_enabled = false;
        filter_styles[0].opacity = 0.5;
    }
    assert!(check_placed_masks(&original, &HashMap::from([(1, 1)]), &doc).is_ok());
    let mut node = Node::text(
        2,
        "Text",
        crate::text::TextSpec {
            text: "Before".into(),
            ..Default::default()
        },
        64,
        48,
    );
    masks(&mut node, (64, 48));
    let original = vec![node.clone()];
    if let NodeKind::Text { spec, .. } = &mut node.kind {
        let mut changed = (**spec).clone();
        changed.text = "After".into();
        *spec = Arc::new(changed);
    }
    doc.nodes = vec![node];
    assert!(check_placed_masks(&original, &HashMap::from([(2, 2)]), &doc).is_ok());
}

#[test]
fn fragment_masks_vector_only_fill_and_adjustment_are_movable() {
    for kind in [
        NodeKind::Fill { rgba: [255; 4] },
        NodeKind::Adjust(emulsion_raster::adjust::Adjustment::Invert),
    ] {
        let mut source = Document::new(64, 48);
        let mut node = Node::new(1, "Vector only", kind);
        masks(&mut node, (64, 48));
        node.mask = None;
        node.vector_mask.as_mut().unwrap().enabled = false;
        source.nodes.push(node);
        source.next_id = 2;
        let mut target = Editor::new(Document::new(128, 80), None);
        let id = Fragment::capture(&source, &[1])
            .unwrap()
            .paste(&mut target, Slot::TOP, (32., 16.))
            .unwrap()[0];
        let pasted = target.doc.node(id).unwrap();
        assert!(pasted.mask.is_none());
        assert_world(
            crate::transform::vector_mask_to_document(pasted)
                .unwrap()
                .unwrap(),
            DAffine2::from_translation(dvec2(32., 16.))
                * crate::transform::vector_mask_to_document(&source.nodes[0])
                    .unwrap()
                    .unwrap(),
        );
    }
}

#[test]
fn fragment_masks_document_planes_retain_detail_outside_destination_canvas() {
    for linked in [false, true] {
        for enabled in [false, true] {
            let mut source = Document::new(64, 48);
            let mut node = Node::new(1, "Masked fill", NodeKind::Fill { rgba: [255; 4] });
            masks(&mut node, (64, 48));
            node.mask_transform = crate::Mapping2::IDENTITY;
            node.mask_linked = linked;
            node.mask_enabled = enabled;
            let vector = node.vector_mask.as_mut().unwrap();
            vector.linked = linked;
            vector.enabled = enabled;
            source.nodes.push(node);
            source.next_id = 2;
            let mut target = Editor::new(Document::new(64, 48), None);
            let id = Fragment::capture(&source, &[1])
                .unwrap()
                .paste(&mut target, Slot::TOP, (100.25, -70.5))
                .unwrap()[0];
            let placed = target.doc.node(id).unwrap();
            assert_resources(&source.nodes[0], placed);
            let translation = DAffine2::from_translation(dvec2(100.25, -70.5));
            assert_world(
                crate::transform::mask_to_document(placed)
                    .unwrap()
                    .require_affine("legacy fixture")
                    .unwrap(),
                translation
                    * crate::transform::mask_to_document(&source.nodes[0])
                        .unwrap()
                        .require_affine("legacy fixture")
                        .unwrap(),
            );
            assert_world(
                crate::transform::vector_mask_to_document(placed)
                    .unwrap()
                    .unwrap(),
                translation
                    * crate::transform::vector_mask_to_document(&source.nodes[0])
                        .unwrap()
                        .unwrap(),
            );
        }
    }
}
