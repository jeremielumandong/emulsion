//! Encoded originals are immutable content, shared across snapshots and invalidated by source edits.
use crate::node::{OriginalImage, SmartEditable};
use crate::{Command, Document, Editor, Node, NodeKind};
use emulsion_raster::{Placement, Raster};
use std::{collections::HashSet, sync::Arc};

fn original() -> Arc<OriginalImage> {
    Arc::new(OriginalImage::new(
        Arc::new(vec![42; 128]),
        [1; 32],
        [2; 32],
    ))
}

fn document() -> Document {
    let mut doc = Document::new(4, 4);
    let mut node = Node::smart(
        1,
        "Original",
        Arc::new(Raster::solid(4, 4, [0.2, 0.3, 0.4, 1.])),
        Vec::new(),
        Placement::default(),
    );
    let NodeKind::Smart { original_image, .. } = &mut node.kind else {
        unreachable!()
    };
    *original_image = Some(original());
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}

fn retained(doc: &Document) -> &Arc<OriginalImage> {
    let NodeKind::Smart {
        original_image: Some(original),
        ..
    } = &doc.nodes[0].kind
    else {
        panic!("expected retained encoded original")
    };
    original
}

#[test]
fn original_image_fields_are_read_only_and_equality_uses_descriptor_identity() {
    let doc = document();
    let original = retained(&doc);
    assert_eq!(original.encoded_sha256(), &[1; 32]);
    assert_eq!(original.source_sha256(), &[2; 32]);
    let mut detached = original.bytes().clone();
    Arc::make_mut(&mut detached).clear();
    assert_eq!(original.bytes().len(), 128);
    assert_eq!(doc.nodes[0].kind, doc.nodes[0].kind.clone());

    let mut changed = doc.clone();
    let NodeKind::Smart { original_image, .. } = &mut changed.nodes[0].kind else {
        unreachable!()
    };
    *original_image = Some(Arc::new((**original).clone()));
    assert_ne!(
        doc, changed,
        "a different descriptor must invalidate source sessions"
    );
    assert!(Arc::ptr_eq(original.bytes(), retained(&changed).bytes()));
    let NodeKind::Smart { original_image, .. } = &mut changed.nodes[0].kind else {
        unreachable!()
    };
    *original_image = None;
    assert_ne!(doc, changed);
}

#[test]
fn original_image_allocation_is_counted_once_across_layers_and_history() {
    let mut doc = document();
    let original = retained(&doc).clone();
    let allocation = (
        Arc::as_ptr(original.bytes()) as usize,
        original.bytes().capacity(),
    );
    let mut duplicate = doc.nodes[0].clone();
    duplicate.id = 2;
    // Even distinct descriptors sharing the same PNG allocation count it only once.
    let NodeKind::Smart { original_image, .. } = &mut duplicate.kind else {
        unreachable!()
    };
    *original_image = Some(Arc::new((*original).clone()));
    doc.nodes.push(duplicate);
    doc.next_id = 3;
    let mut planes = HashSet::new();
    let allocations = doc.buffers_once(&mut planes);
    assert_eq!(allocations.iter().filter(|a| **a == allocation).count(), 1);
    assert!(doc.clone().buffers_once(&mut planes).is_empty());

    let mut editor = Editor::new(doc, None);
    crate::photo_source::replace(&mut editor, 1, Arc::new(Raster::solid(4, 4, [1.; 4]))).unwrap();
    editor.execute(Command::RemoveNode { id: 2 }).unwrap();
    let mut planes = HashSet::new();
    assert!(!editor.doc.buffers_once(&mut planes).contains(&allocation));
    let retained_count = editor
        .history
        .steps()
        .flat_map(|step| step.before.buffers_once(&mut planes))
        .filter(|a| *a == allocation)
        .count();
    assert_eq!(retained_count, 1);
}

#[test]
fn original_image_survives_placement_filters_crop_and_undo() {
    let before = document();
    let original = retained(&before).clone();
    let mut editor = Editor::new(before.clone(), None);
    editor
        .execute(Command::SetPlacement {
            id: 1,
            placement: Placement::at(2., 3.),
        })
        .unwrap();
    editor
        .execute(Command::SetFilters {
            id: 1,
            filters: vec![crate::smart::Filter::FindEdges],
        })
        .unwrap();
    crate::photo_source::crop(&mut editor, 1, [0., 0., 2., 3.]).unwrap();
    assert!(Arc::ptr_eq(retained(&editor.doc), &original));
    editor.undo();
    editor.undo();
    editor.undo();
    assert_eq!(editor.doc, before);
    editor.redo();
    assert!(Arc::ptr_eq(retained(&editor.doc), &original));
}

#[test]
fn source_replacement_clears_editable_and_original_together_and_undo_restores_them() {
    for editable_source in [false, true] {
        let mut before = document();
        if editable_source {
            let NodeKind::Smart {
                editable,
                original_image,
                ..
            } = &mut before.nodes[0].kind
            else {
                unreachable!()
            };
            *editable = Some(SmartEditable::Svg {
                xml: Arc::from("<svg/>"),
            });
            *original_image = None;
        }
        before.validate().unwrap();
        let mut editor = Editor::new(before.clone(), None);
        crate::photo_source::replace(&mut editor, 1, Arc::new(Raster::solid(8, 8, [1.; 4])))
            .unwrap();
        assert!(matches!(
            &editor.doc.nodes[0].kind,
            NodeKind::Smart {
                editable: None,
                original_image: None,
                ..
            }
        ));
        editor.undo();
        assert_eq!(editor.doc, before);
        if !editable_source {
            assert!(Arc::ptr_eq(retained(&editor.doc), retained(&before)));
        }
        editor.redo();
        assert!(matches!(
            &editor.doc.nodes[0].kind,
            NodeKind::Smart {
                editable: None,
                original_image: None,
                ..
            }
        ));
    }
}

#[test]
fn original_image_cannot_coexist_with_an_editable_source() {
    let mut doc = document();
    doc.validate().unwrap();
    let NodeKind::Smart { editable, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *editable = Some(SmartEditable::Svg {
        xml: Arc::from("<svg/>"),
    });
    assert_eq!(
        doc.validate(),
        Err(crate::DocumentError::BadValue(
            1,
            "original PNG requires a raster-backed Smart source"
        ))
    );
}

#[test]
fn raw_development_clears_original_and_undo_restores_it() {
    let mut before = document();
    before.raw = Some(crate::raw::RawDocument {
        schema_version: 1,
        node_id: 1,
        source: "photo.dng".into(),
        source_sha256: "a".repeat(64),
        params: Default::default(),
        metadata: Default::default(),
    });
    before.raw_originals.push("photo.dng".into());
    let mut editor = Editor::new(before.clone(), None);
    editor
        .execute(Command::DevelopRaw {
            id: 1,
            raster: Arc::new(Raster::solid(4, 4, [0.4, 0.6, 0.8, 1.])),
            params: Box::new(crate::raw::DevelopParams {
                exposure: 1.,
                ..Default::default()
            }),
        })
        .unwrap();
    assert!(matches!(
        &editor.doc.nodes[0].kind,
        NodeKind::Smart {
            editable: None,
            original_image: None,
            ..
        }
    ));
    editor.undo();
    assert_eq!(editor.doc, before);
    assert!(Arc::ptr_eq(retained(&editor.doc), retained(&before)));
    editor.redo();
    assert!(matches!(
        &editor.doc.nodes[0].kind,
        NodeKind::Smart {
            original_image: None,
            ..
        }
    ));
}

#[test]
fn applying_a_layered_source_invalidates_original_and_is_undoable() {
    let before = document();
    let mut editor = Editor::new(before.clone(), None);
    crate::smart_source::apply(
        &mut editor,
        1,
        Arc::new(vec![1, 2, 3]),
        None,
        Arc::new(Raster::solid(4, 4, [1.; 4])),
    )
    .unwrap();
    assert!(matches!(
        &editor.doc.nodes[0].kind,
        NodeKind::Smart {
            original_image: None,
            editable: Some(SmartEditable::Document { .. }),
            ..
        }
    ));
    editor.undo();
    assert_eq!(editor.doc, before);
}

#[test]
fn converting_pixels_to_smart_starts_without_an_encoded_original() {
    let mut doc = Document::new(4, 4);
    doc.nodes.push(Node::raster(
        1,
        "Pixels",
        Arc::new(Raster::solid(4, 4, [1.; 4])),
        Placement::default(),
    ));
    doc.next_id = 2;
    Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
    assert!(matches!(
        &doc.nodes[0].kind,
        NodeKind::Smart {
            original_image: None,
            ..
        }
    ));
}

#[test]
fn explicit_equivalent_pixel_replacement_still_invalidates_original() {
    let before = document();
    let NodeKind::Smart { source, .. } = &before.nodes[0].kind else {
        unreachable!()
    };
    let same = source.clone();
    let mut editor = Editor::new(before.clone(), None);
    crate::photo_source::replace(&mut editor, 1, same).unwrap();
    assert!(matches!(
        editor.doc.nodes[0].kind,
        NodeKind::Smart {
            original_image: None,
            ..
        }
    ));
    editor.undo();
    assert_eq!(editor.doc, before);
    assert!(Arc::ptr_eq(retained(&editor.doc), retained(&before)));
    editor.redo();
    assert!(matches!(
        editor.doc.nodes[0].kind,
        NodeKind::Smart {
            original_image: None,
            ..
        }
    ));
}

#[test]
fn original_image_retains_identity_through_independent_masks_and_duplicate() {
    let before = document();
    let original = retained(&before).clone();
    let mut editor = Editor::new(before, None);
    for command in [
        Command::SetMask {
            id: 1,
            mask: Some(Arc::new(emulsion_raster::Mask::white(4, 4))),
        },
        Command::SetVectorMask {
            id: 1,
            mask: Some(crate::VectorMask::default()),
        },
        Command::SetSmartFilterMask {
            id: 1,
            mask: Some(crate::SmartFilterMask::new(Arc::new(
                emulsion_raster::Mask::white(4, 4),
            ))),
        },
        Command::SetPlacement {
            id: 1,
            placement: Placement {
                rotation: 17.,
                scale_x: 1.3,
                scale_y: 0.8,
                ..Default::default()
            },
        },
    ] {
        editor.execute(command).unwrap();
        assert!(Arc::ptr_eq(retained(&editor.doc), &original));
    }
    let duplicate = editor
        .execute(Command::DuplicateNode { id: 1 })
        .unwrap()
        .unwrap();
    let NodeKind::Smart {
        original_image: Some(copy),
        ..
    } = &editor.doc.node(duplicate).unwrap().kind
    else {
        panic!("duplicate lost original")
    };
    assert!(Arc::ptr_eq(copy, &original));
    editor.undo();
    assert!(Arc::ptr_eq(retained(&editor.doc), &original));
}
