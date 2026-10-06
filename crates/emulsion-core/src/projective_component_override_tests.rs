//! Geometry overrides retain maps without overwriting updated component appearance.
use super::*;
use crate::{Command, Document, Editor, Mapping2, SmartFilterMask, SmartPlacement};
use emulsion_raster::projective::Projective2;
use emulsion_raster::{Mask, Placement, Raster};
use std::sync::Arc;
fn smart() -> Node {
    let mut n = Node::smart(
        1,
        "Smart",
        Arc::new(Raster::solid(4, 4, [0.2, 0.4, 0.6, 1.])),
        vec![emulsion_filters::Filter::Invert],
        Placement::default(),
    );
    let NodeKind::Smart { filter_mask, .. } = &mut n.kind else {
        panic!()
    };
    *filter_mask = Some(SmartFilterMask::new(Arc::new(Mask::white(4, 4))));
    n
}
fn h() -> Projective2 {
    Projective2::from_row_major([1., 0., 0., 0., 1., 0., 0.01, 0., 1.]).unwrap()
}
#[test]
fn whole_geometry_restores_mapping_only_and_keeps_new_mask_payload_and_properties() {
    let mut old = smart();
    let NodeKind::Smart {
        placement,
        filter_mask,
        ..
    } = &mut old.kind
    else {
        panic!()
    };
    *placement = SmartPlacement::Projective(h());
    let m = filter_mask.as_mut().unwrap();
    m.transform = Mapping2::Affine(glam::DAffine2::from_translation(glam::dvec2(0.25, -0.5)));
    m.linked = false;
    old.mask_linked = false;
    old.mask_enabled = true;
    old.mask_properties.density = 0.2;
    let old_map = m.transform;
    let mut next = smart();
    let replacement = Arc::new(Mask::empty(4, 4, 0));
    let NodeKind::Smart { filter_mask, .. } = &mut next.kind else {
        panic!()
    };
    let m = filter_mask.as_mut().unwrap();
    m.pixels = replacement.clone();
    m.enabled = false;
    m.properties.density = 0.7;
    next.mask_enabled = false;
    next.mask_properties.density = 0.8;
    restore(
        &old,
        &mut next,
        Overrides {
            geometry: true,
            ..Default::default()
        },
        true,
    )
    .unwrap();
    let m = crate::smart_filter_mask::descriptor(&next).unwrap();
    assert!(Arc::ptr_eq(&m.pixels, &replacement));
    assert!(!m.enabled);
    assert_eq!(m.properties.density, 0.7);
    assert_eq!(m.transform, old_map);
    assert!(!m.linked);
    assert!(!next.mask_enabled);
    assert_eq!(next.mask_properties.density, 0.8);
    assert!(!next.mask_linked);
}
#[test]
fn incompatible_projective_override_bases_or_descriptors_refuse_atomically() {
    let mut old = smart();
    let NodeKind::Smart { placement, .. } = &mut old.kind else {
        panic!()
    };
    *placement = SmartPlacement::Projective(h());
    for dimensions in [false, true] {
        let mut next = smart();
        let NodeKind::Smart {
            source,
            cache,
            filter_mask,
            ..
        } = &mut next.kind
        else {
            panic!()
        };
        if dimensions {
            *source = Arc::new(Raster::solid(2, 2, [1.; 4]));
            *cache = source.clone();
        } else {
            *filter_mask = None;
        }
        let before = next.clone();
        assert!(
            restore(
                &old,
                &mut next,
                Overrides {
                    geometry: true,
                    ..Default::default()
                },
                true
            )
            .is_err()
        );
        assert_eq!(next, before);
    }
}
#[test]
fn definition_mask_updates_flow_through_geometry_only_projective_instance_override() {
    let mut editor = Editor::new(Document::new(100, 80), None);
    let original = editor
        .execute(Command::AddNode {
            node: Box::new(smart()),
            slot: crate::command::Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let original_group =
        crate::design_components::create(&mut editor, &[original], "Masked card").unwrap();
    let instance =
        crate::design_components::insert(&mut editor, "Masked card", "Default", (20., 10.))
            .unwrap();
    let child = editor.doc.children(Some(instance))[0];
    editor
        .execute(Command::TransformSmartProjective {
            id: child,
            delta: h(),
        })
        .unwrap();
    let flags = crate::design_components::overrides_for(&editor.doc, instance, child);
    assert!(flags.geometry);
    assert!(!flags.appearance);
    let placement = match &editor.doc.node(child).unwrap().kind {
        NodeKind::Smart { placement, .. } => *placement,
        _ => panic!(),
    };
    let black = Arc::new(Mask::empty(4, 4, 0));
    editor
        .execute(Command::SetSmartFilterMaskPixels {
            id: original,
            pixels: black.clone(),
        })
        .unwrap();
    crate::design_components::update(&mut editor, original_group, None).unwrap();
    let n = editor.doc.node(child).unwrap();
    let mask = crate::smart_filter_mask::descriptor(n).unwrap();
    assert!(Arc::ptr_eq(&mask.pixels, &black));
    assert_eq!(mask.pixels.fill(), 0);
    let NodeKind::Smart {
        placement: after,
        source,
        ..
    } = &n.kind
    else {
        panic!()
    };
    assert_eq!(*after, placement);
    let effective =
        crate::smart_filter_mask::effective_pixels_with_space(n, editor.doc.blend_space)
            .unwrap()
            .unwrap();
    let grid = crate::smart_support::output_grid(n).unwrap();
    assert_eq!(grid.offset, (0, 0));
    assert_eq!(grid.size, (source.width(), source.height()));
    assert_eq!((effective.width(), effective.height()), grid.size);
    assert_eq!(
        effective.read_rect(source.bounds()),
        source.read_rect(source.bounds())
    );
}
