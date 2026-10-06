//! Projective preview no-ops must retain unnormalized valid RAW provenance.
use crate::{Command, Document, Editor, Node, NodeKind};
use emulsion_raster::projective::Projective2;
use emulsion_raster::{Mask, Placement, Raster};
use std::sync::Arc;
fn editor() -> Editor {
    let mut doc = Document::new(12, 10);
    doc.nodes.push(Node::smart(
        1,
        "RAW",
        Arc::new(Raster::solid(4, 4, [1.; 4])),
        vec![],
        Placement::default(),
    ));
    doc.next_id = 2;
    doc.selection = Some(Arc::new(Mask::empty(12, 10, 128)));
    let mut editor = Editor::new(doc, None);
    editor
        .execute(Command::Rename {
            id: 1,
            name: "Redo target".into(),
        })
        .unwrap();
    assert!(editor.undo());
    editor.doc.raw = Some(crate::raw::RawDocument {
        schema_version: 1,
        node_id: 1,
        source: "original.raw".into(),
        source_sha256: "ab".repeat(32),
        params: Default::default(),
        metadata: Default::default(),
    });
    editor.doc.raw_originals.clear();
    editor.doc.validate().unwrap();
    editor.take_dirty();
    editor
}
fn identity() -> Command {
    Command::TransformSmartProjective {
        id: 1,
        delta: Projective2::IDENTITY,
    }
}
fn check(editor: &Editor, before: &Document, revision: u64, history: usize) {
    assert_eq!(&editor.doc, before);
    assert!(editor.doc.raw_originals.is_empty());
    assert_eq!(editor.revision, revision);
    assert_eq!(editor.history.len(), history);
    assert!(editor.history.can_redo());
    assert!(Arc::ptr_eq(
        editor.doc.selection.as_ref().unwrap(),
        before.selection.as_ref().unwrap()
    ));
    assert_eq!(editor.doc.next_id, before.next_id);
}
#[test]
fn identity_execute_identity_preview_and_untouched_cancel_do_not_normalize_raw_protection() {
    for action in 0..4 {
        let mut editor = editor();
        let before = editor.doc.clone();
        let revision = editor.revision;
        let history = editor.history.len();
        match action {
            0 => {
                editor.execute(identity()).unwrap();
            }
            1 => {
                editor.begin_preview("Perspective").unwrap();
                editor.preview(identity()).unwrap();
                editor.commit_preview();
            }
            2 => {
                editor.begin_preview("Perspective").unwrap();
                editor.cancel_preview();
            }
            _ => {
                editor.begin_preview("Perspective").unwrap();
                editor.preview(identity()).unwrap();
                editor.cancel_preview();
            }
        }
        check(&editor, &before, revision, history);
        assert_eq!(editor.take_dirty(), crate::Dirty::Nothing);
        assert!(!editor.in_transaction());
    }
}
#[test]
fn meaningful_projection_returned_to_identity_or_cancelled_restores_exact_raw_baseline() {
    for return_to_identity in [false, true] {
        let mut editor = editor();
        let before = editor.doc.clone();
        let revision = editor.revision;
        let history = editor.history.len();
        editor.begin_preview("Perspective").unwrap();
        let delta = Projective2::from_row_major([1., 0., 0., 0., 1., 0., 0.125, 0., 1.]).unwrap();
        editor
            .preview(Command::TransformSmartProjective { id: 1, delta })
            .unwrap();
        assert_ne!(editor.doc, before);
        assert!(!editor.doc.raw_originals.is_empty());
        if return_to_identity {
            editor.preview(identity()).unwrap();
            check(&editor, &before, revision, history);
            editor.commit_preview();
        } else {
            editor.cancel_preview();
        }
        check(&editor, &before, revision, history);
        assert_eq!(editor.take_dirty(), crate::Dirty::All);
    }
}
#[test]
fn cancelling_actual_raw_relink_or_source_edit_keeps_monotonic_file_protection() {
    for relink in [false, true] {
        let mut editor = editor();
        if !relink {
            let NodeKind::Smart {
                source, placement, ..
            } = &editor.doc.nodes[0].kind
            else {
                panic!()
            };
            editor.doc.nodes[0].kind = NodeKind::Raster {
                raster: source.clone(),
                placement: placement.require_legacy("test").unwrap(),
            };
        }
        let original = editor.doc.raw.as_ref().unwrap().source.clone();
        let revision = editor.revision;
        editor.begin_preview("Actual RAW edit").unwrap();
        if relink {
            editor
                .preview(Command::RelinkRaw {
                    source: "replacement.raw".into(),
                })
                .unwrap();
        } else {
            editor
                .preview(Command::ReplacePixels {
                    id: 1,
                    raster: Arc::new(Raster::solid(4, 4, [0.5; 4])),
                    dirty: emulsion_raster::IRect::new(0, 0, 4, 4),
                    label: "Preview source edit".into(),
                })
                .unwrap();
        }
        editor.cancel_preview();
        assert_eq!(editor.revision, revision);
        assert_eq!(editor.doc.raw.as_ref().unwrap().source, original);
        assert!(editor.doc.raw_originals.contains(&original));
        if relink {
            assert!(editor.doc.raw_originals.contains(&"replacement.raw".into()));
        }
    }
}
