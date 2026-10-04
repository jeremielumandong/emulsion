use crate::smart_filter_mask::{
    descriptor, effective_pixels, for_inspection, pad_to_cache, to_document,
};
use crate::{Command, Document, Editor, MaskProperties, Node, NodeKind, SmartFilterMask};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::{BlendMode, IRect, Mask, Placement, Raster};
use glam::{DAffine2, dvec2};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(16, 12);
    let source = Arc::new(Raster::from_fn(5, 3, [123, 456, 789, 1000], |x, y| {
        [x as u16 * 3000, y as u16 * 2000, 1200, 20000]
    }));
    let filtered = Arc::new(Raster::from_fn(9, 7, [0; 4], |x, y| {
        [1000 + x as u16 * 2000, 1500 + y as u16 * 3000, 7000, 40000]
    }));
    doc.nodes.push(Node::new(
        1,
        "Stack",
        NodeKind::Smart {
            editable: None,
            source,
            filters: vec![Filter::GaussianBlur { radius: 1. }],
            filter_styles: vec![FilterStyle::default()],
            filter_mask: None,
            placement: Placement::at(3., 3.),
            cache: filtered,
            offset: (-2, -2),
        },
    ));
    doc.next_id = 2;
    doc
}
fn mask(doc: &mut Document, coverage: u8) {
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(SmartFilterMask::new(Arc::new(Mask::empty(5, 3, coverage)))),
    }
    .apply(doc)
    .unwrap();
}
fn raw(doc: &Document) -> Arc<Raster> {
    match &doc.nodes[0].kind {
        NodeKind::Smart { cache, .. } => cache.clone(),
        _ => panic!(),
    }
}
fn source(doc: &Document) -> Arc<Raster> {
    match &doc.nodes[0].kind {
        NodeKind::Smart { source, .. } => source.clone(),
        _ => panic!(),
    }
}
fn pixels(doc: &Document) -> Vec<u8> {
    emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8()
}
fn close(a: DAffine2, b: DAffine2) {
    for (x, y) in a.to_cols_array().into_iter().zip(b.to_cols_array()) {
        assert!((x - y).abs() < 1e-8, "{a:?} != {b:?}");
    }
}

#[test]
fn smart_filter_mix_exact_rgba16_and_finite_source_fill() {
    for coverage in [0, 1, 64, 127, 128, 254, 255] {
        let mut doc = document();
        mask(&mut doc, coverage);
        let full = raw(&doc);
        let original = source(&doc);
        let out = effective_pixels(&doc.nodes[0]).unwrap();
        for y in 0..7 {
            for x in 0..9 {
                let s = if (2..7).contains(&x) && (2..5).contains(&y) {
                    original.get(x - 2, y - 2)
                } else {
                    [0; 4]
                };
                let f = full.get(x, y);
                let m = u32::from(coverage);
                let expected: [u16; 4] = std::array::from_fn(|i| {
                    ((u32::from(s[i]) * (255 - m) + u32::from(f[i]) * m + 127) / 255) as u16
                });
                assert_eq!(out.get(x, y), expected);
                assert!(out.get(x, y)[..3].iter().all(|v| *v <= out.get(x, y)[3]));
            }
        }
    }
}
#[test]
fn smart_filter_identity_fast_paths_preserve_raw_arc_and_dormant_descriptor() {
    let mut doc = document();
    let full = raw(&doc);
    assert!(Arc::ptr_eq(
        &effective_pixels(&doc.nodes[0]).unwrap(),
        &full
    ));
    mask(&mut doc, 255);
    assert!(Arc::ptr_eq(
        &effective_pixels(&doc.nodes[0]).unwrap(),
        &full
    ));
    mask(&mut doc, 0);
    Command::SetSmartFilterMaskEnabled {
        id: 1,
        enabled: false,
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(
        &effective_pixels(&doc.nodes[0]).unwrap(),
        &full
    ));
    Command::SetSmartFilterMaskEnabled {
        id: 1,
        enabled: true,
    }
    .apply(&mut doc)
    .unwrap();
    Command::SetSmartFilterMaskProperties {
        id: 1,
        properties: MaskProperties {
            density: 0.,
            feather: 1000.,
        },
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(
        &effective_pixels(&doc.nodes[0]).unwrap(),
        &full
    ));
    let before = descriptor(&doc.nodes[0]).unwrap().clone();
    Command::SetFilters {
        id: 1,
        filters: vec![],
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(descriptor(&doc.nodes[0]), Some(&before));
    assert!(Arc::ptr_eq(
        &effective_pixels(&doc.nodes[0]).unwrap(),
        &raw(&doc)
    ));
    Command::SetFilters {
        id: 1,
        filters: vec![Filter::FindEdges],
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(descriptor(&doc.nodes[0]), Some(&before));
}
#[test]
fn smart_filter_mask_caches_edits_without_rerendering_stack_or_content_placement() {
    let mut doc = document();
    mask(&mut doc, 128);
    let full = raw(&doc);
    let first = effective_pixels(&doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &effective_pixels(&doc.nodes[0]).unwrap()
    ));
    Command::SetPlacement {
        id: 1,
        placement: Placement::at(7., 9.),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &effective_pixels(&doc.nodes[0]).unwrap()
    ));
    Command::TransformNodes {
        ids: vec![1],
        transform: DAffine2::from_scale_angle_translation(dvec2(1.3, 1.3), 0.31, dvec2(3., 2.))
            .to_cols_array(),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &effective_pixels(&doc.nodes[0]).unwrap()
    ));
    Command::SetSmartFilterMaskPixels {
        id: 1,
        pixels: Arc::new(Mask::empty(5, 3, 64)),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(!Arc::ptr_eq(
        &first,
        &effective_pixels(&doc.nodes[0]).unwrap()
    ));
    assert!(Arc::ptr_eq(&full, &raw(&doc)));
    Command::SetSmartFilterMaskProperties {
        id: 1,
        properties: MaskProperties {
            density: 0.5,
            feather: 2.,
        },
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(&full, &raw(&doc)));
}
#[test]
fn smart_filter_mask_properties_use_shared_intrinsic_projection_and_ignore_enable_for_inspection() {
    let mut doc = document();
    mask(&mut doc, 0);
    let mut component = descriptor(&doc.nodes[0]).unwrap().clone();
    component.pixels = Arc::new(Mask::from_fn(5, 3, 0, |x, _| if x == 2 { 255 } else { 0 }));
    component.transform =
        DAffine2::from_scale_angle_translation(dvec2(1.8, 0.75), 0.2, dvec2(-1.3, 1.2))
            .to_cols_array();
    component.properties = MaskProperties {
        density: 0.65,
        feather: 2.,
    };
    component.enabled = false;
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(component.clone()),
    }
    .apply(&mut doc)
    .unwrap();
    let expected = crate::composite_mask_cache::derive_raster_mask(
        &component.pixels,
        component.properties,
        component.transform,
        crate::composite_mask_cache::MaskGrid {
            width: 9,
            height: 7,
            offset: (-2, -2),
        },
    );
    let inspected = for_inspection(&doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(&expected, &inspected));
    assert!(Arc::ptr_eq(
        &raw(&doc),
        &effective_pixels(&doc.nodes[0]).unwrap()
    ));
    Command::SetSmartFilterMaskEnabled {
        id: 1,
        enabled: true,
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(
        &inspected,
        &for_inspection(&doc.nodes[0]).unwrap()
    ));
}
#[test]
fn smart_filter_stack_mask_gates_after_noncommuting_styled_filters() {
    let mut doc = document();
    let filters = vec![Filter::GaussianBlur { radius: 1.2 }, Filter::FindEdges];
    let styles = vec![
        FilterStyle {
            opacity: 0.65,
            blend: BlendMode::Screen,
        },
        FilterStyle {
            opacity: 0.4,
            blend: BlendMode::Multiply,
        },
    ];
    Command::SetFilterStack {
        id: 1,
        filters: filters.clone(),
        styles: styles.clone(),
    }
    .apply(&mut doc)
    .unwrap();
    mask(&mut doc, 128);
    let full = raw(&doc);
    let original = source(&doc);
    let offset = match &doc.nodes[0].kind {
        NodeKind::Smart { offset, .. } => *offset,
        _ => unreachable!(),
    };
    let out = effective_pixels(&doc.nodes[0]).unwrap();
    for y in 0..full.height() {
        for x in 0..full.width() {
            let (sx, sy) = (
                i64::from(x) + i64::from(offset.0),
                i64::from(y) + i64::from(offset.1),
            );
            let s = if sx >= 0
                && sy >= 0
                && sx < i64::from(original.width())
                && sy < i64::from(original.height())
            {
                original.get(sx as u32, sy as u32)
            } else {
                [0; 4]
            };
            let expected: [u16; 4] = std::array::from_fn(|i| {
                ((u32::from(s[i]) * 127 + u32::from(full.get(x, y)[i]) * 128 + 127) / 255) as u16
            });
            assert_eq!(out.get(x, y), expected);
        }
    }
    let descriptor = descriptor(&doc.nodes[0]).unwrap().clone();
    Command::SetFilterStack {
        id: 1,
        filters: filters.into_iter().rev().collect(),
        styles: styles.into_iter().rev().collect(),
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(
        crate::smart_filter_mask::descriptor(&doc.nodes[0]),
        Some(&descriptor)
    );
    assert_ne!(full.to_srgba8(), raw(&doc).to_srgba8());
}
#[test]
fn smart_filter_mask_rasterize_preserves_both_ordinary_masks_and_exact_appearance() {
    for raster_enabled in [false, true] {
        for vector_enabled in [false, true] {
            let mut doc = document();
            mask(&mut doc, 64);
            doc.nodes[0].mask = Some(Arc::new(Mask::from_fn(5, 3, 255, |x, _| {
                if x < 2 { 0 } else { 128 }
            })));
            doc.nodes[0].mask_enabled = raster_enabled;
            doc.nodes[0].mask_properties = MaskProperties {
                density: 0.7,
                feather: 1.2,
            };
            doc.nodes[0].mask_transform =
                DAffine2::from_translation(dvec2(0.2, -0.7)).to_cols_array();
            let mut vector = crate::VectorMask::empty(crate::EmptyVectorCoverage::HideAll);
            vector.enabled = vector_enabled;
            vector.properties.density = 0.3;
            doc.nodes[0].vector_mask = Some(vector);
            let before = pixels(&doc);
            let canvas = IRect::new(0, 0, doc.width as i32, doc.height as i32);
            let before_native =
                emulsion_raster::composite::flatten(&doc.composite_tree(), 0).read_rect(canvas);
            let ordinary = doc.nodes[0].mask.clone().unwrap();
            let vector = doc.nodes[0].vector_mask.clone().unwrap();
            let effective = effective_pixels(&doc.nodes[0]).unwrap();
            let mut editor = Editor::new(doc, None);
            editor.execute(Command::Rasterize { id: 1 }).unwrap();
            assert_eq!(pixels(&editor.doc), before);
            assert_eq!(
                emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0)
                    .read_rect(canvas),
                before_native
            );
            assert!(Arc::ptr_eq(
                editor.doc.nodes[0].mask.as_ref().unwrap(),
                &ordinary
            ));
            assert!(Arc::ptr_eq(
                &editor.doc.nodes[0].vector_mask.as_ref().unwrap().path,
                &vector.path
            ));
            assert!(
                matches!(&editor.doc.nodes[0].kind,NodeKind::Raster{raster,..} if Arc::ptr_eq(raster,&effective))
            );
            assert!(editor.undo());
            assert!(descriptor(&editor.doc.nodes[0]).is_some());
            assert!(editor.redo());
            assert_eq!(pixels(&editor.doc), before);
            assert_eq!(
                emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0)
                    .read_rect(canvas),
                before_native
            );
        }
    }
}
#[test]
fn smart_filter_apply_ordinary_mask_consumes_stack_once_and_keeps_vector() {
    let mut doc = document();
    mask(&mut doc, 0);
    doc.nodes[0].mask = Some(Arc::new(Mask::empty(5, 3, 128)));
    doc.nodes[0].vector_mask = Some(crate::VectorMask::empty(
        crate::EmptyVectorCoverage::RevealAll,
    ));
    let before = pixels(&doc);
    Command::ApplyLayerMask { id: 1 }.apply(&mut doc).unwrap();
    assert_eq!(pixels(&doc), before);
    assert!(doc.nodes[0].mask.is_none());
    assert!(doc.nodes[0].vector_mask.is_some());
    assert!(matches!(doc.nodes[0].kind, NodeKind::Raster { .. }));
}
#[test]
fn smart_filter_mask_linked_and_unlinked_geometry_and_source_substitution() {
    for linked in [false, true] {
        let mut doc = document();
        mask(&mut doc, 128);
        Command::SetSmartFilterMaskLinked { id: 1, linked }
            .apply(&mut doc)
            .unwrap();
        let world = to_document(&doc.nodes[0]).unwrap();
        let plane = descriptor(&doc.nodes[0]).unwrap().pixels.clone();
        let translation = DAffine2::from_translation(dvec2(4., -2.));
        Command::TranslateNode {
            id: 1,
            dx: 4.,
            dy: -2.,
        }
        .apply(&mut doc)
        .unwrap();
        close(
            to_document(&doc.nodes[0]).unwrap(),
            if linked { translation * world } else { world },
        );
        let before_crop = to_document(&doc.nodes[0]).unwrap();
        Command::Crop {
            rect: IRect::new(2, 1, 12, 10),
            rotation: 0.,
        }
        .apply(&mut doc)
        .unwrap();
        close(
            to_document(&doc.nodes[0]).unwrap(),
            DAffine2::from_translation(dvec2(-2., -1.)) * before_crop,
        );
        let world = to_document(&doc.nodes[0]).unwrap();
        let mut editor = Editor::new(doc, None);
        crate::photo_source::replace(
            &mut editor,
            1,
            Arc::new(Raster::solid(7, 8, [0.2, 0.3, 0.4, 1.])),
        )
        .unwrap();
        close(to_document(&editor.doc.nodes[0]).unwrap(), world);
        assert!(Arc::ptr_eq(
            &descriptor(&editor.doc.nodes[0]).unwrap().pixels,
            &plane
        ));
    }
}
#[test]
fn smart_filter_padding_preserves_coverage_raw_properties_and_world_mapping() {
    let mut doc = document();
    mask(&mut doc, 0);
    let mut original = descriptor(&doc.nodes[0]).unwrap().clone();
    original.pixels = Arc::new(Mask::from_fn(5, 3, 0, |x, y| ((x + y) * 30) as u8));
    original.properties = MaskProperties {
        density: 0.8,
        feather: 2.,
    };
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(original.clone()),
    }
    .apply(&mut doc)
    .unwrap();
    let before = for_inspection(&doc.nodes[0]).unwrap().to_gray8();
    let world = to_document(&doc.nodes[0]).unwrap();
    let padded = pad_to_cache(&doc.nodes[0]).unwrap();
    assert_eq!((padded.pixels.width(), padded.pixels.height()), (9, 7));
    assert_eq!(padded.properties, original.properties);
    for y in 0..3 {
        for x in 0..5 {
            assert_eq!(padded.pixels.get(x + 2, y + 2), original.pixels.get(x, y));
        }
    }
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(padded),
    }
    .apply(&mut doc)
    .unwrap();
    let padded_world = to_document(&doc.nodes[0]).unwrap();
    close(
        padded_world * DAffine2::from_translation(dvec2(2., 2.)),
        world,
    );
    assert_eq!(for_inspection(&doc.nodes[0]).unwrap().to_gray8(), before);
    let next = pad_to_cache(&doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(
        &next.pixels,
        &descriptor(&doc.nodes[0]).unwrap().pixels
    ));
}
#[test]
fn smart_filter_padding_rejects_extreme_inverse_extent_without_mutation() {
    let mut doc = document();
    mask(&mut doc, 0);
    Command::SetSmartFilterMaskTransform {
        id: 1,
        transform: DAffine2::from_scale(dvec2(0.001, 0.001)).to_cols_array(),
    }
    .apply(&mut doc)
    .unwrap();
    let before = doc.clone();
    assert!(pad_to_cache(&doc.nodes[0]).is_err());
    assert_eq!(doc, before);
}
#[test]
fn smart_filter_mask_validation_missing_targets_locks_readonly_are_atomic() {
    let mut doc = document();
    mask(&mut doc, 128);
    let before = doc.clone();
    for properties in [
        MaskProperties {
            density: f32::NAN,
            feather: 0.,
        },
        MaskProperties {
            density: 1.1,
            feather: 0.,
        },
        MaskProperties {
            density: 1.,
            feather: 1001.,
        },
    ] {
        assert!(
            Command::SetSmartFilterMaskProperties { id: 1, properties }
                .apply(&mut doc)
                .is_err()
        );
        assert_eq!(doc, before);
    }
    for transform in [[0.; 6], [f64::NAN; 6], [f64::INFINITY; 6]] {
        assert!(
            Command::SetSmartFilterMaskTransform { id: 1, transform }
                .apply(&mut doc)
                .is_err()
        );
        assert_eq!(doc, before);
    }
    assert!(
        Command::SetSmartFilterMaskPixels {
            id: 1,
            pixels: Arc::new(Mask::empty(30001, 1, 0))
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    doc.nodes[0].locks.pixels = true;
    doc.nodes[0].locks.transparency = true;
    Command::SetSmartFilterMaskPixels {
        id: 1,
        pixels: Arc::new(Mask::empty(5, 3, 64)),
    }
    .apply(&mut doc)
    .unwrap();
    doc.nodes[0].locks.position = true;
    let before = doc.clone();
    assert!(
        Command::SetSmartFilterMaskLinked {
            id: 1,
            linked: false
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    let mut replacement = descriptor(&doc.nodes[0]).unwrap().clone();
    replacement.transform[4] = 1.;
    assert!(
        Command::SetSmartFilterMask {
            id: 1,
            mask: Some(replacement)
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    doc.nodes[0].locked = true;
    assert!(
        Command::SetSmartFilterMaskEnabled {
            id: 1,
            enabled: false
        }
        .apply(&mut doc)
        .is_err()
    );
    // Isolate read-only policy from the independent layer-lock checks above.
    doc.nodes[0].locked = false;
    doc.nodes[0].locks = Default::default();
    let mut editor = Editor::new(doc.clone(), None);
    editor.set_read_only(true);
    assert!(
        editor
            .execute(Command::SetSmartFilterMask { id: 1, mask: None })
            .is_err()
    );
    assert_eq!(editor.doc, doc);
    assert!(editor.history.is_empty());
    editor.set_read_only(false);
    editor
        .execute(Command::SetSmartFilterMask { id: 1, mask: None })
        .unwrap();
    assert!(descriptor(&editor.doc.nodes[0]).is_none());
    assert_eq!(editor.history.len(), 1);
    let mut missing = document();
    assert!(
        Command::SetSmartFilterMaskPixels {
            id: 1,
            pixels: Arc::new(Mask::empty(5, 3, 0))
        }
        .apply(&mut missing)
        .is_err()
    );
    missing.nodes[0].kind = NodeKind::Raster {
        raster: source(&document()),
        placement: Placement::default(),
    };
    assert!(
        Command::SetSmartFilterMask {
            id: 1,
            mask: Some(SmartFilterMask::new(Arc::new(Mask::empty(5, 3, 0))))
        }
        .apply(&mut missing)
        .is_err()
    );
}
#[test]
fn smart_filter_mask_undo_noop_and_conversion_restore_exact_snapshot() {
    let mut doc = document();
    mask(&mut doc, 64);
    let mut editor = Editor::new(doc, None);
    let before = editor.doc.clone();
    let existing = descriptor(&before.nodes[0]).unwrap().clone();
    editor
        .execute(Command::SetSmartFilterMask {
            id: 1,
            mask: Some(existing),
        })
        .unwrap();
    assert!(editor.history.is_empty());
    editor
        .execute(Command::SetSmartFilterMask { id: 1, mask: None })
        .unwrap();
    assert_eq!(editor.history.len(), 1);
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
    assert!(editor.redo());
    assert!(descriptor(&editor.doc.nodes[0]).is_none());
    assert!(editor.undo());
    editor.execute(Command::ConvertToLayers { id: 1 }).unwrap();
    assert!(
        matches!(&editor.doc.nodes[0].kind,NodeKind::Raster{raster,..} if Arc::ptr_eq(raster,&source(&before)))
    );
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
}
#[test]
fn smart_filter_publication_preserves_mask_and_exact_rendered_styles() {
    let mut doc = document();
    mask(&mut doc, 64);
    let original = descriptor(&doc.nodes[0]).unwrap().clone();
    let filters = vec![Filter::FindEdges];
    let styles = vec![FilterStyle {
        opacity: 0.23,
        blend: BlendMode::Screen,
    }];
    let (cache, offset) = crate::smart::render_styled(&source(&doc), &filters, &styles);
    Command::SetSmartCache {
        id: 1,
        filters,
        styles: styles.clone(),
        cache: cache.clone(),
        offset,
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(descriptor(&doc.nodes[0]), Some(&original));
    assert!(
        matches!(&doc.nodes[0].kind,NodeKind::Smart{filter_styles,cache:stored,..} if *filter_styles==styles && Arc::ptr_eq(stored,&cache))
    );
    let before = doc.clone();
    assert!(
        Command::SetSmartCache {
            id: 1,
            filters: vec![Filter::FindEdges],
            styles: vec![],
            cache,
            offset
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
}
#[test]
fn smart_filter_mask_effects_bounds_and_fingerprint_track_effective_alpha() {
    let mut doc = document();
    mask(&mut doc, 255);
    doc.nodes[0].styles = vec![crate::styles::LayerStyle::DropShadow {
        color: [0; 3],
        opacity: 100.,
        angle: 90.,
        distance: 2.,
        size: 1.,
    }];
    let first = crate::styles::render(&doc, &doc.nodes[0]).unwrap();
    let bounds = crate::geometry::node_bounds(&doc, 1).unwrap();
    let fingerprint = crate::storyboard_fingerprint::document_fingerprint(&doc);
    Command::SetSmartFilterMaskPixels {
        id: 1,
        pixels: Arc::new(Mask::empty(5, 3, 0)),
    }
    .apply(&mut doc)
    .unwrap();
    let next = crate::styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(&first, &next));
    assert_ne!(bounds, crate::geometry::node_bounds(&doc, 1).unwrap());
    assert_ne!(
        fingerprint,
        crate::storyboard_fingerprint::document_fingerprint(&doc)
    );
    assert!(
        crate::graph::compare(&document(), &doc)
            .iter()
            .any(|row| row.label.contains("Smart Filter mask"))
    );
}

#[test]
fn smart_filter_mask_duplicate_and_history_share_raw_planes_until_edit() {
    let mut doc = document();
    mask(&mut doc, 128);
    let original = descriptor(&doc.nodes[0]).unwrap().pixels.clone();
    let original_effective = effective_pixels(&doc.nodes[0]).unwrap();
    let id = Command::DuplicateNode { id: 1 }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(
        &descriptor(doc.node(id).unwrap()).unwrap().pixels,
        &original
    ));
    assert!(Arc::ptr_eq(
        &effective_pixels(doc.node(id).unwrap()).unwrap(),
        &original_effective
    ));
    Command::SetSmartFilterMaskPixels {
        id,
        pixels: Arc::new(Mask::empty(5, 3, 32)),
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(
        &descriptor(doc.node(1).unwrap()).unwrap().pixels,
        &original
    ));
    assert!(!Arc::ptr_eq(
        &effective_pixels(doc.node(id).unwrap()).unwrap(),
        &original_effective
    ));
    Command::RemoveNode { id: 1 }.apply(&mut doc).unwrap();
    assert!(effective_pixels(doc.node(id).unwrap()).is_some());
}
#[test]
fn smart_filter_mask_image_resize_rotation_and_disabled_world_mapping() {
    for linked in [false, true] {
        for enabled in [false, true] {
            let mut doc = document();
            mask(&mut doc, 128);
            Command::SetSmartFilterMaskLinked { id: 1, linked }
                .apply(&mut doc)
                .unwrap();
            Command::SetSmartFilterMaskEnabled { id: 1, enabled }
                .apply(&mut doc)
                .unwrap();
            let original = descriptor(&doc.nodes[0]).unwrap().clone();
            let world = to_document(&doc.nodes[0]).unwrap();
            Command::ImageSize {
                width: 32,
                height: 24,
            }
            .apply(&mut doc)
            .unwrap();
            close(
                to_document(&doc.nodes[0]).unwrap(),
                DAffine2::from_scale(dvec2(2., 2.)) * world,
            );
            assert_eq!(descriptor(&doc.nodes[0]), Some(&original));
            // Image-wide rotation transforms disabled and unlinked descriptors too.
            let before = to_document(&doc.nodes[0]).unwrap();
            Command::RotateImage { degrees: 90. }
                .apply(&mut doc)
                .unwrap();
            assert_ne!(to_document(&doc.nodes[0]).unwrap(), before);
            assert_eq!(descriptor(&doc.nodes[0]), Some(&original));
        }
    }
}
#[test]
fn smart_filter_mask_pixel_edit_batch_failure_and_modal_guard_do_not_publish() {
    let mut doc = document();
    mask(&mut doc, 128);
    let mut editor = Editor::new(doc, None);
    let before = editor.doc.clone();
    let revision = editor.revision;
    let commands = [
        Command::SetSmartFilterMaskPixels {
            id: 1,
            pixels: Arc::new(Mask::empty(5, 3, 0)),
        },
        Command::SetSmartFilterMaskProperties {
            id: 1,
            properties: MaskProperties {
                density: 2.,
                feather: 0.,
            },
        },
    ];
    assert!(editor.execute_commands("Paint", &commands).is_err());
    assert_eq!(editor.doc, before);
    assert_eq!(editor.revision, revision);
    assert!(editor.history.is_empty());
    editor.begin_preview("Transform mask").unwrap();
    assert!(
        editor
            .execute(Command::SetSmartFilterMask { id: 1, mask: None })
            .is_err()
    );
    assert_eq!(editor.doc, before);
    editor.cancel_preview();
}

#[test]
fn smart_filter_mask_plane_accounting_deduplicates_shared_node_and_history_buffers() {
    let mut doc = document();
    mask(&mut doc, 128);
    Command::SetSmartFilterMaskPixels {
        id: 1,
        pixels: Arc::new(Mask::from_fn(5, 3, 0, |x, y| (x + y) as u8)),
    }
    .apply(&mut doc)
    .unwrap();
    let before = doc.buffers();
    let raw = descriptor(&doc.nodes[0]).unwrap().pixels.clone();
    for allocation in raw.buffer_allocations() {
        assert!(before.contains(&allocation));
    }
    Command::DuplicateNode { id: 1 }.apply(&mut doc).unwrap();
    let mut after = doc.buffers();
    let mut before = before;
    after.sort_unstable();
    before.sort_unstable();
    assert_eq!(after, before);
    let mut identities = std::collections::HashSet::new();
    assert!(!doc.buffers_once(&mut identities).is_empty());
    assert!(doc.buffers_once(&mut identities).is_empty());
}

#[test]
fn smart_filter_position_lock_allows_default_cache_grid_add_but_not_arbitrary_geometry() {
    let mut doc = document();
    doc.nodes[0].locks.position = true;
    let mut descriptor = SmartFilterMask::new(Arc::new(Mask::empty(9, 7, 0)));
    descriptor.transform = [1., 0., 0., 1., -2., -2.];
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(descriptor.clone()),
    }
    .apply(&mut doc)
    .unwrap();
    descriptor.transform[4] -= 1.;
    let before = doc.clone();
    assert!(
        Command::SetSmartFilterMask {
            id: 1,
            mask: Some(descriptor.clone())
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
    Command::SetSmartFilterMask { id: 1, mask: None }
        .apply(&mut doc)
        .unwrap();
    let before = doc.clone();
    assert!(
        Command::SetSmartFilterMask {
            id: 1,
            mask: Some(descriptor)
        }
        .apply(&mut doc)
        .is_err()
    );
    assert_eq!(doc, before);
}

#[test]
fn smart_filter_projected_white_alias_does_not_warm_retain_raw_filtered_source() {
    for moved in [false, true] {
        let mut doc = document();
        let mut mask = SmartFilterMask::new(Arc::new(Mask::from_fn(5, 3, 255, |x, y| {
            if x == 2 && y == 1 { 0 } else { 255 }
        })));
        if moved {
            mask.transform[4] = 1000.;
        } else {
            mask.properties.density = 0.0001;
        }
        Command::SetSmartFilterMask {
            id: 1,
            mask: Some(mask),
        }
        .apply(&mut doc)
        .unwrap();
        let full = raw(&doc);
        let weak = Arc::downgrade(&full);
        let count = Arc::strong_count(&full);
        let coverage = for_inspection(&doc.nodes[0]).unwrap();
        assert_eq!(coverage.fill(), 255);
        assert_eq!(coverage.tile_count(), 0);
        let effective = effective_pixels(&doc.nodes[0]).unwrap();
        assert!(Arc::ptr_eq(&effective, &full));
        assert_eq!(Arc::strong_count(&full), count + 1);
        drop(effective);
        assert_eq!(Arc::strong_count(&full), count);
        drop(doc);
        drop(full);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn smart_filter_component_publish_preserves_mask_world_after_instance_alignment() {
    use crate::design_components;
    for (source_position, instance_offset) in [
        ((0., 0.), (100., 30.)),
        ((17., 11.), (170., -5.)),
        ((201., 73.), (-160., 20.)),
    ] {
        for linked in [false, true] {
            for enabled in [false, true] {
                for fill in [0, 255] {
                    let mut editor = Editor::new(Document::new(500, 240), None);
                    let source = Arc::new(Raster::from_fn(14, 10, [0; 4], |x, y| {
                        [(x * 1800) as u16, (y * 1800) as u16, 3000, 40000]
                    }));
                    let mut node = Node::smart(
                        0,
                        "Smart",
                        source,
                        vec![Filter::FindEdges],
                        Placement::at(source_position.0, source_position.1),
                    );
                    let initial =
                        SmartFilterMask::new(Arc::new(Mask::from_fn(14, 10, fill, |x, _| {
                            if x < 7 { 0 } else { 255 }
                        })));
                    let NodeKind::Smart { filter_mask, .. } = &mut node.kind else {
                        unreachable!()
                    };
                    *filter_mask = Some(initial);
                    let original_id = editor
                        .execute(Command::AddNode {
                            node: Box::new(node),
                            slot: crate::command::Slot::TOP,
                        })
                        .unwrap()
                        .unwrap();
                    let original_group =
                        design_components::create(&mut editor, &[original_id], "Masked card")
                            .unwrap();
                    let instance = design_components::insert(
                        &mut editor,
                        "Masked card",
                        "Default",
                        instance_offset,
                    )
                    .unwrap();
                    let instance_child = editor.doc.children(Some(instance))[0];
                    editor
                        .execute(Command::SetSmartFilterMaskLinked {
                            id: instance_child,
                            linked,
                        })
                        .unwrap();
                    editor
                        .execute(Command::SetSmartFilterMaskEnabled {
                            id: instance_child,
                            enabled,
                        })
                        .unwrap();
                    // An actual appearance edit on the instance creates the override.
                    editor
                        .execute(Command::SetSmartFilterMaskPixels {
                            id: instance_child,
                            pixels: Arc::new(Mask::from_fn(14, 10, fill, |x, y| {
                                if x < 5 && y < 8 { 0 } else { 255 }
                            })),
                        })
                        .unwrap();
                    assert!(
                        design_components::overrides_for(&editor.doc, instance, instance_child)
                            .appearance
                    );
                    let old_world = to_document(editor.doc.node(instance_child).unwrap()).unwrap();
                    let raw = descriptor(editor.doc.node(instance_child).unwrap())
                        .unwrap()
                        .pixels
                        .clone();
                    for publication in 0..2 {
                        // An unrelated layer-opacity update must not move mask details.
                        editor
                            .execute(Command::SetOpacity {
                                id: original_id,
                                opacity: if publication == 0 { 0.8 } else { 0.6 },
                            })
                            .unwrap();
                        // Retain the instance's opacity to make its full pixel oracle
                        // independent of the deliberately unrelated source update.
                        editor
                            .execute(Command::SetOpacity {
                                id: instance_child,
                                opacity: 0.75,
                            })
                            .unwrap();
                        let expected_pixels =
                            emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0)
                                .read_rect(IRect::new(0, 0, 500, 240));
                        let snapshot = editor.doc.clone();
                        design_components::update(&mut editor, original_group, None).unwrap();
                        let child = editor.doc.node(instance_child).unwrap();
                        close(to_document(child).unwrap(), old_world);
                        assert!(Arc::ptr_eq(&descriptor(child).unwrap().pixels, &raw));
                        assert_eq!(
                            (
                                descriptor(child).unwrap().linked,
                                descriptor(child).unwrap().enabled
                            ),
                            (linked, enabled)
                        );
                        assert_eq!(
                            emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0)
                                .read_rect(IRect::new(0, 0, 500, 240)),
                            expected_pixels
                        );
                        assert!(editor.undo());
                        assert_eq!(editor.doc, snapshot);
                        assert!(editor.redo());
                        close(
                            to_document(editor.doc.node(instance_child).unwrap()).unwrap(),
                            old_world,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn smart_filter_coverage_has_independent_density_affine_and_cache_origin_oracle() {
    let mut doc = document();
    let mut component = SmartFilterMask::new(Arc::new(Mask::from_pixels(2, 1, 255, &[0, 255])));
    component.properties.density = 0.5;
    component.transform = [2., 0., 0., 1., -1., 0.];
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(component),
    }
    .apply(&mut doc)
    .unwrap();
    // Density changes native values to [128,255] before horizontal 2x affine.
    // Cache origin -2 puts output x=0..4 at intrinsic centres -0.25..1.75.
    // Bilinear mixing with white outside gives these hand-computed byte values.
    let expected: [u8; 5] = [223, 160, 160, 223, 255];
    let coverage = for_inspection(&doc.nodes[0]).unwrap();
    assert_eq!(
        (0..5).map(|x| coverage.get(x, 2)).collect::<Vec<_>>(),
        expected
    );
    let filtered = raw(&doc);
    let original = source(&doc);
    let actual = effective_pixels(&doc.nodes[0]).unwrap();
    for (x, m) in expected.into_iter().enumerate() {
        let m = u32::from(m);
        let s = if x >= 2 {
            original.get((x - 2) as u32, 0)
        } else {
            [0; 4]
        };
        let f = filtered.get(x as u32, 2);
        let expected: [u16; 4] = std::array::from_fn(|i| {
            ((u32::from(s[i]) * (255 - m) + u32::from(f[i]) * m + 127) / 255) as u16
        });
        assert_eq!(actual.get(x as u32, 2), expected);
    }
}

#[test]
fn smart_filter_component_source_size_publication_preserves_instance_mask_world() {
    use crate::design_components;
    for linked in [false, true] {
        let mut editor = Editor::new(Document::new(400, 200), None);
        let mut node = Node::smart(
            0,
            "Image",
            Arc::new(Raster::solid(14, 10, [0.4, 0.2, 0.1, 0.7])),
            vec![Filter::FindEdges],
            Placement {
                rotation: 15.,
                scale_x: 1.2,
                scale_y: 0.8,
                ..Placement::at(18., 23.)
            },
        );
        let NodeKind::Smart { filter_mask, .. } = &mut node.kind else {
            unreachable!()
        };
        *filter_mask = Some(SmartFilterMask::new(Arc::new(Mask::white(14, 10))));
        let source_id = editor
            .execute(Command::AddNode {
                node: Box::new(node),
                slot: crate::command::Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let source_group =
            design_components::create(&mut editor, &[source_id], "Image component").unwrap();
        let instance =
            design_components::insert(&mut editor, "Image component", "Default", (160., 35.))
                .unwrap();
        let child = editor.doc.children(Some(instance))[0];
        editor
            .execute(Command::SetSmartFilterMaskLinked { id: child, linked })
            .unwrap();
        editor
            .execute(Command::SetSmartFilterMaskPixels {
                id: child,
                pixels: Arc::new(Mask::from_fn(14, 10, 255, |x, y| {
                    if x < 5 && y < 6 { 0 } else { 255 }
                })),
            })
            .unwrap();
        let world = to_document(editor.doc.node(child).unwrap()).unwrap();
        let raw = descriptor(editor.doc.node(child).unwrap())
            .unwrap()
            .pixels
            .clone();
        crate::photo_source::replace(
            &mut editor,
            source_id,
            Arc::new(Raster::solid(8, 5, [0.2, 0.3, 0.1, 0.7])),
        )
        .unwrap();
        let before = editor.doc.clone();
        design_components::update(&mut editor, source_group, None).unwrap();
        let node = editor.doc.node(child).unwrap();
        close(to_document(node).unwrap(), world);
        assert!(Arc::ptr_eq(&descriptor(node).unwrap().pixels, &raw));
        assert!(
            matches!(&node.kind,NodeKind::Smart{source,..} if(source.width(),source.height())==(8,5))
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(editor.redo());
        close(to_document(editor.doc.node(child).unwrap()).unwrap(), world);
    }
}

#[test]
fn smart_filter_unlinked_blur_mask_component_publish_does_not_reflow_row_sibling() {
    use crate::{command::Slot, design_components, design_layout};
    let mut editor = Editor::new(Document::new(480, 220), None);
    let mut node = Node::smart(
        0,
        "Blurred image",
        Arc::new(Raster::solid(14, 10, [0.4, 0.2, 0.1, 0.7])),
        vec![Filter::GaussianBlur { radius: 2. }],
        Placement::at(24., 20.),
    );
    let NodeKind::Smart {
        cache,
        offset,
        filter_mask,
        ..
    } = &mut node.kind
    else {
        unreachable!()
    };
    let w = cache.width();
    let h = cache.height();
    *filter_mask = Some(SmartFilterMask {
        pixels: Arc::new(Mask::from_fn(
            w,
            h,
            0,
            |x, _| if x >= w / 2 { 255 } else { 0 },
        )),
        transform: [1., 0., 0., 1., f64::from(offset.0), f64::from(offset.1)],
        ..SmartFilterMask::new(Arc::new(Mask::white(1, 1)))
    });
    let image = editor
        .execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let sibling = editor
        .execute(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Sibling",
                Arc::new(Raster::solid(18, 12, [0.1, 0.3, 0.2, 0.8])),
                Placement::at(80., 20.),
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let group = editor
        .execute(Command::Group {
            ids: vec![image, sibling],
            name: "Responsive row".into(),
        })
        .unwrap()
        .unwrap();
    design_layout::enable(
        &mut editor,
        group,
        design_layout::Frame {
            flow: design_layout::Flow::Row,
            wrap: false,
            padding: [8.; 4],
            gap: 11.,
            ..Default::default()
        },
        (150., 60.),
    )
    .unwrap();
    let original = design_components::create(&mut editor, &[group], "Masked row").unwrap();
    let instance =
        design_components::insert(&mut editor, "Masked row", "Default", (190., 40.)).unwrap();
    let children = editor.doc.children(Some(instance));
    let image = children
        .iter()
        .copied()
        .find(|id| matches!(editor.doc.node(*id).unwrap().kind, NodeKind::Smart { .. }))
        .unwrap();
    let sibling = children
        .iter()
        .copied()
        .find(|id| matches!(editor.doc.node(*id).unwrap().kind, NodeKind::Raster { .. }))
        .unwrap();
    let original_image = editor
        .doc
        .children(Some(original))
        .into_iter()
        .find(|id| matches!(editor.doc.node(*id).unwrap().kind, NodeKind::Smart { .. }))
        .unwrap();
    let raw = descriptor(editor.doc.node(image).unwrap())
        .unwrap()
        .pixels
        .clone();
    let edited = Arc::new(raw.write_rect(IRect::new((w / 2 + 1) as i32, 2, 2, 2), &[0; 4]));
    editor
        .execute(Command::SetSmartFilterMaskPixels {
            id: image,
            pixels: edited,
        })
        .unwrap();
    editor
        .execute(Command::SetSmartFilterMaskLinked {
            id: image,
            linked: false,
        })
        .unwrap();
    editor
        .execute(Command::SetOpacity {
            id: image,
            opacity: 0.65,
        })
        .unwrap();
    let before_image = editor.doc.node(image).unwrap().clone();
    let before_sibling = editor.doc.node(sibling).unwrap().clone();
    let world = to_document(&before_image).unwrap();
    let mut settled = editor.doc.clone();
    design_layout::reflow(&mut settled).unwrap();
    assert_eq!(
        settled, editor.doc,
        "fixture is a settled responsive layout"
    );
    editor
        .execute(Command::SetOpacity {
            id: original_image,
            opacity: 0.8,
        })
        .unwrap();
    let before = editor.doc.clone();
    let expected = emulsion_raster::composite::flatten(&before.composite_tree(), 0)
        .read_rect(IRect::new(0, 0, 480, 220));
    design_components::update(&mut editor, original, None).unwrap();
    close(to_document(editor.doc.node(image).unwrap()).unwrap(), world);
    assert_eq!(editor.doc.node(image).unwrap().kind, before_image.kind);
    assert_eq!(editor.doc.node(sibling).unwrap().kind, before_sibling.kind);
    assert_eq!(
        emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0)
            .read_rect(IRect::new(0, 0, 480, 220)),
        expected
    );
    let mut settled = editor.doc.clone();
    design_layout::reflow(&mut settled).unwrap();
    assert_eq!(
        settled, editor.doc,
        "publication leaves a settled responsive layout"
    );
    assert!(editor.undo());
    assert_eq!(editor.doc, before);
}
