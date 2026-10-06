//! Effective Smart grid migration regressions. Authored; not executed here.
use crate::{Command, Document, Editor, Mapping2, Node, NodeKind, SmartPlacement, VectorMask};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::composite::flatten;
use emulsion_raster::{Mask, Placement, Raster};
use glam::{DAffine2, dvec2};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Stack {
    Active,
    RootBypass,
    StagesBypass,
}
fn scene(stack: Stack, offset: (i32, i32), transformed: bool) -> Document {
    let source = Arc::new(Raster::from_fn(4, 4, [0; 4], |x, y| {
        [12000 + 1000 * x as u16, 8000 + 700 * y as u16, 6000, 65535]
    }));
    let placement = if transformed {
        Placement {
            rotation: 31.,
            flip_x: true,
            scale_x: 1.5,
            scale_y: 1.25,
            ..Placement::at(12., 11.)
        }
    } else {
        Placement::at(12., 11.)
    };
    let mut node = Node::smart(
        1,
        "Retained Smart",
        source,
        vec![Filter::BoxBlur { radius: 2. }],
        placement,
    );
    let NodeKind::Smart {
        filters_enabled,
        filter_styles,
        cache,
        offset: origin,
        ..
    } = &mut node.kind
    else {
        panic!()
    };
    match stack {
        Stack::Active => {}
        Stack::RootBypass => {
            *filters_enabled = false;
            *cache = Arc::new(Raster::solid(8, 8, [1., 0., 1., 1.]));
            *origin = offset;
        }
        Stack::StagesBypass => {
            *filter_styles = vec![FilterStyle {
                enabled: false,
                ..Default::default()
            }];
            *cache = Arc::new(Raster::solid(8, 8, [1., 0., 1., 1.]));
            *origin = offset;
        }
    }
    node.mask = Some(Arc::new(Mask::from_fn(6, 6, 255, |x, y| {
        if (x + y) % 3 == 0 { 96 } else { 255 }
    })));
    node.mask_transform = Mapping2::Affine(DAffine2::from_translation(dvec2(-1., 0.5)));
    node.mask_linked = false;
    node.vector_mask = Some(VectorMask {
        path: Arc::new(
            emulsion_raster::vector::Path::from_svg("M -3 -3 L 9 -3 L 9 9 L -3 9 Z").unwrap(),
        ),
        transform: [1., 0., 0., 1., 0.5, -0.5],
        linked: false,
        ..Default::default()
    });
    let mut doc = Document::new(40, 36);
    doc.nodes.push(node);
    doc.next_id = 2;
    doc.validate().unwrap();
    doc
}
fn source_and_placement(doc: &Document) -> (Arc<Raster>, Placement) {
    let NodeKind::Smart {
        source,
        placement: SmartPlacement::Legacy(p),
        ..
    } = &doc.nodes[0].kind
    else {
        panic!()
    };
    (source.clone(), *p)
}
fn expected_placement(doc: &Document) -> Placement {
    let (source, p) = source_and_placement(doc);
    let grid = crate::smart_support::output_grid(&doc.nodes[0]).unwrap();
    crate::smart::cache_placement(
        &p,
        (source.width(), source.height()),
        grid.size,
        grid.offset,
    )
}
fn assert_resources_restored(before: &Document, after: &Document) {
    assert_eq!(before, after);
    let NodeKind::Smart {
        source: a,
        cache: ac,
        offset: ao,
        ..
    } = &before.nodes[0].kind
    else {
        panic!()
    };
    let NodeKind::Smart {
        source: b,
        cache: bc,
        offset: bo,
        ..
    } = &after.nodes[0].kind
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(a, b));
    assert!(Arc::ptr_eq(ac, bc));
    assert_eq!(ao, bo);
    assert!(Arc::ptr_eq(
        before.nodes[0].mask.as_ref().unwrap(),
        after.nodes[0].mask.as_ref().unwrap()
    ));
}
#[test]
fn rasterize_uses_effective_grid_for_pixels_placement_and_both_component_maps() {
    for stack in [Stack::Active, Stack::RootBypass, Stack::StagesBypass] {
        for offset in [(-2, -2), (20, 17)] {
            for transformed in [false, true] {
                let before = scene(stack, offset, transformed);
                let n = &before.nodes[0];
                let grid = crate::smart_support::output_grid(n).unwrap();
                let expected_pixels =
                    crate::smart_filter_mask::effective_pixels_with_space(n, before.blend_space)
                        .unwrap()
                        .unwrap();
                let raster_map =
                    crate::composite_mask_cache::mapping_to_output(n.mask_transform, grid.offset)
                        .unwrap();
                let vector_map = crate::composite_mask_cache::mask_to_output(
                    n.vector_mask.as_ref().unwrap().transform,
                    grid.offset,
                )
                .to_cols_array();
                let expected = flatten(&before.composite_tree(), 0).to_srgba8();
                let mut editor = Editor::new(before.clone(), None);
                editor.execute(Command::Rasterize { id: 1 }).unwrap();
                let baked = &editor.doc.nodes[0];
                let NodeKind::Raster { raster, placement } = &baked.kind else {
                    panic!()
                };
                assert!(Arc::ptr_eq(raster, &expected_pixels));
                assert_eq!(*placement, expected_placement(&before));
                assert_eq!((raster.width(), raster.height()), grid.size);
                assert_eq!(baked.mask_transform, raster_map);
                assert_eq!(baked.vector_mask.as_ref().unwrap().transform, vector_map);
                assert!(!baked.mask_linked);
                assert!(!baked.vector_mask.as_ref().unwrap().linked);
                assert_eq!(
                    flatten(&editor.doc.composite_tree(), 0).to_srgba8(),
                    expected
                );
                assert!(editor.undo());
                assert_resources_restored(&before, &editor.doc);
            }
        }
    }
}
#[test]
fn apply_mask_bakes_coverage_on_effective_grid_and_retains_vector_world_mapping() {
    for stack in [Stack::Active, Stack::RootBypass, Stack::StagesBypass] {
        for enabled in [false, true] {
            let mut before = scene(stack, (20, 17), true);
            before.nodes[0].mask_enabled = enabled;
            let n = &before.nodes[0];
            let grid = crate::smart_support::output_grid(n).unwrap();
            let source =
                crate::smart_filter_mask::effective_pixels_with_space(n, before.blend_space)
                    .unwrap()
                    .unwrap();
            let coverage = before.raster_mask_for_inspection(n).unwrap().unwrap();
            let world = crate::transform::vector_mask_to_document(n)
                .unwrap()
                .unwrap();
            let mut editor = Editor::new(before.clone(), None);
            editor.execute(Command::ApplyLayerMask { id: 1 }).unwrap();
            let n = &editor.doc.nodes[0];
            let NodeKind::Raster { raster, placement } = &n.kind else {
                panic!()
            };
            assert_eq!((raster.width(), raster.height()), grid.size);
            assert_eq!(*placement, expected_placement(&before));
            for y in 0..raster.height() {
                for x in 0..raster.width() {
                    let cov = if enabled {
                        u32::from(coverage.get(x, y))
                    } else {
                        255
                    };
                    assert_eq!(
                        raster.get(x, y),
                        source
                            .get(x, y)
                            .map(|v| ((u32::from(v) * cov + 127) / 255) as u16)
                    );
                }
            }
            let actual = crate::transform::vector_mask_to_document(n)
                .unwrap()
                .unwrap();
            for p in [dvec2(0., 0.), dvec2(3., 2.)] {
                assert!((actual.transform_point2(p) - world.transform_point2(p)).length() < 1e-8);
            }
            assert!(n.mask.is_none());
            assert!(!n.vector_mask.as_ref().unwrap().linked);
            assert!(editor.undo());
            assert_resources_restored(&before, &editor.doc);
        }
    }
}
#[test]
fn bypass_vector_trace_matches_source_raster_geometry_for_rotated_reflected_content() {
    for stack in [Stack::RootBypass, Stack::StagesBypass] {
        let doc = scene(stack, (20, 17), true);
        let (source, placement) = source_and_placement(&doc);
        let mut reference = doc.clone();
        reference.nodes[0].kind = NodeKind::Raster {
            raster: source,
            placement,
        };
        let options = crate::design_vectors::trace::Options {
            alpha_only: true,
            ..Default::default()
        };
        assert_eq!(
            crate::design_vectors::trace::preview(&doc, 1, options).unwrap(),
            crate::design_vectors::trace::preview(&reference, 1, options).unwrap()
        );
    }
}
#[test]
fn bypass_effect_translation_uses_source_extent_even_when_raw_cache_is_displaced() {
    for stack in [Stack::RootBypass, Stack::StagesBypass] {
        let mut doc = scene(stack, (20, 17), true);
        doc.nodes[0]
            .styles
            .push(crate::styles::LayerStyle::DropShadow {
                color: [0; 3],
                opacity: 100.,
                angle: 0.,
                distance: 2.,
                size: 1.,
            });
        let mut normalized = doc.clone();
        let NodeKind::Smart {
            source,
            cache,
            offset,
            ..
        } = &mut normalized.nodes[0].kind
        else {
            panic!()
        };
        *cache = source.clone();
        *offset = (0, 0);
        assert!(crate::styles::render(&normalized, &normalized.nodes[0]).is_some());
        assert!(crate::styles::render(&doc, &doc.nodes[0]).is_some());
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&normalized.composite_tree(), 0).to_srgba8()
        );
    }
}

#[test]
fn first_mask_position_lock_exemption_uses_effective_grid_not_inactive_cache() {
    for stack in [Stack::RootBypass, Stack::StagesBypass] {
        let mut doc = scene(stack, (20, 17), false);
        doc.nodes[0].locks.position = true;
        let before = doc.clone();
        let mut misplaced = crate::SmartFilterMask::new(Arc::new(Mask::white(8, 8)));
        misplaced.transform = Mapping2::Affine(DAffine2::from_translation(dvec2(20., 17.)));
        assert!(matches!(
            Command::SetSmartFilterMask {
                id: 1,
                mask: Some(misplaced)
            }
            .apply(&mut doc),
            Err(crate::CommandError::Locked(1))
        ));
        assert_eq!(doc, before);
        let canonical = crate::SmartFilterMask::new(Arc::new(Mask::white(4, 4)));
        Command::SetSmartFilterMask {
            id: 1,
            mask: Some(canonical),
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(
            crate::smart_filter_mask::descriptor(&doc.nodes[0])
                .unwrap()
                .transform,
            Mapping2::IDENTITY
        );
    }
}
