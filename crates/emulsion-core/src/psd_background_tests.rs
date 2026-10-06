//! Explicit Background identity is independent of appearance and is snapshot state.
use crate::{Command, Dirty, Document, Editor, Node, NodeKind, command::Slot};
use emulsion_raster::{IRect, Placement, Raster, blend::BlendSpace};
use std::{collections::HashMap, sync::Arc};

fn document() -> Document {
    let mut doc = Document::new(4, 4);
    doc.nodes.push(Node::raster(
        1,
        "Ordinary name",
        Arc::new(Raster::solid(4, 4, [1.; 4])),
        Placement::default(),
    ));
    doc.nodes.push(Node::raster(
        2,
        "Background",
        Arc::new(Raster::solid(4, 4, [1., 0., 0., 1.])),
        Placement::default(),
    ));
    doc.next_id = 3;
    doc
}

#[test]
fn explicit_target_is_not_inferred_and_assignment_is_validated_atomically() {
    let mut doc = document();
    assert_eq!(doc.psd_background, None);
    assert_eq!(doc.composite_tree().knockout_background, None);
    for id in [2, 999] {
        let before = doc.clone();
        assert!(
            Command::SetPsdBackground { id: Some(id) }
                .apply(&mut doc)
                .is_err()
        );
        assert_eq!(doc, before);
    }
    Command::SetPsdBackground { id: Some(1) }
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.composite_tree().knockout_background, Some(1));
    let mut bad = doc.clone();
    bad.psd_background = Some(2);
    assert!(bad.validate().is_err());
    bad.psd_background = Some(999);
    assert!(bad.validate().is_err());
    bad.psd_background = Some(1);
    bad.nodes[0].kind = NodeKind::Fill { rgba: [255; 4] };
    assert!(bad.validate().is_err());
}

#[test]
fn assigning_identity_is_allowed_on_a_locked_eligible_raster() {
    let mut doc = document();
    doc.nodes[0].locked = true;
    doc.nodes[0].locks = crate::node::LayerLocks {
        transparency: true,
        pixels: true,
        position: true,
    };
    Command::SetPsdBackground { id: Some(1) }
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.psd_background, Some(1));
    Command::SetPsdBackground { id: None }
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.psd_background, None);
}

#[test]
fn target_only_edits_dirty_all_and_undo_redo_restore_identity() {
    let mut editor = Editor::new(document(), None);
    editor.take_dirty();
    editor
        .execute(Command::SetPsdBackground { id: Some(1) })
        .unwrap();
    assert!(matches!(editor.take_dirty(), Dirty::All));
    assert!(editor.uncommitted());
    assert_ne!(editor.doc, editor.committed);
    assert!(editor.undo());
    assert_eq!(editor.doc.psd_background, None);
    assert!(editor.redo());
    assert_eq!(editor.doc.psd_background, Some(1));
    editor
        .execute(Command::SetPsdBackground { id: None })
        .unwrap();
    assert_eq!(editor.doc.psd_background, None);
    editor.undo();
    assert_eq!(editor.doc.psd_background, Some(1));
}

#[test]
fn rename_visibility_pixels_locks_and_profile_changes_preserve_identity() {
    let mut doc = document();
    doc.psd_background = Some(1);
    for command in [
        Command::Rename {
            id: 1,
            name: "Renamed".into(),
        },
        Command::SetVisible {
            id: 1,
            visible: false,
        },
        Command::SetOpacity {
            id: 1,
            opacity: 0.2,
        },
        Command::ReplacePixels {
            id: 1,
            raster: Arc::new(Raster::solid(4, 4, [0., 1., 0., 0.5])),
            dirty: IRect::new(0, 0, 4, 4),
            label: "Paint".into(),
        },
        Command::SetLocked {
            id: 1,
            locked: true,
        },
        Command::SetBlendSpace {
            space: BlendSpace::PhotoshopSrgbV1,
        },
        Command::SetBlendSpace {
            space: BlendSpace::Srgb,
        },
        Command::SetBlendSpace {
            space: BlendSpace::Linear,
        },
    ] {
        command.apply(&mut doc).unwrap();
        assert_eq!(doc.psd_background, Some(1));
        doc.validate().unwrap();
    }
}

#[test]
fn structural_edits_demote_and_undo_restores_target() {
    let mut before = document();
    before.psd_background = Some(1);
    for command in [
        Command::RemoveNode { id: 1 },
        Command::MoveNode {
            id: 1,
            slot: Slot::TOP,
        },
        Command::Group {
            ids: vec![1],
            name: "Group".into(),
        },
        Command::ConvertToSmart { id: 1 },
        Command::AddNode {
            node: Box::new(Node::group(0, "Inserted beneath")),
            slot: Slot {
                parent: None,
                index: 0,
            },
        },
    ] {
        let mut editor = Editor::new(before.clone(), None);
        editor.execute(command).unwrap();
        assert_eq!(editor.doc.psd_background, None);
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(editor.redo());
        assert_eq!(editor.doc.psd_background, None);
    }
}

#[test]
fn duplicate_and_fragment_paste_do_not_acquire_background_role() {
    let mut doc = document();
    doc.psd_background = Some(1);
    let duplicate = Command::DuplicateNode { id: 1 }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
    assert_ne!(duplicate, 1);
    assert_eq!(doc.psd_background, Some(1));
    let fragment = crate::fragment::Fragment::capture(&doc, &[1]).unwrap();
    let mut target = Editor::new(Document::new(4, 4), None);
    fragment.paste(&mut target, Slot::TOP, (0., 0.)).unwrap();
    assert_eq!(target.doc.psd_background, None);
}

#[test]
fn whole_page_copy_retains_role_and_source_only_document_does_not() {
    let mut doc = document();
    doc.psd_background = Some(1);
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    let solo = doc.solo(1).unwrap();
    assert_eq!(solo.psd_background, None);
    assert_eq!(solo.blend_space, doc.blend_space);
    let mut project =
        crate::project::ProjectEditor::new_project(crate::project::ProjectKind::Design, doc)
            .unwrap();
    let source = project.active_page();
    let before = project.page(source).unwrap().doc.clone();
    let ids = project.duplicate_pages(&[source]).unwrap();
    assert_eq!(
        project.page(ids[0]).unwrap().doc.psd_background,
        before.psd_background
    );
    assert_eq!(
        project.page(ids[0]).unwrap().doc.blend_space,
        before.blend_space
    );
}

#[test]
fn fingerprints_preserve_legacy_hashes_but_track_profile_and_dormant_identity() {
    use crate::storyboard_fingerprint::document_fingerprint as hash;
    let mut doc = document();
    let legacy = hash(&doc);
    doc.blend_space = BlendSpace::Srgb;
    assert_eq!(hash(&doc), legacy);
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    let profile = hash(&doc);
    assert_ne!(profile, legacy);
    doc.psd_background = Some(1);
    assert_ne!(hash(&doc), profile);
    doc.blend_space = BlendSpace::Linear;
    assert_ne!(hash(&doc), legacy);
}

#[test]
fn graph_canvas_merge_tracks_target_and_prunes_invalidated_role() {
    use crate::graph::{ConflictKey, MergeOutcome, Side, merge};
    let base = document();
    let mut theirs = base.clone();
    theirs.psd_background = Some(1);
    let MergeOutcome::Merged(merged) = merge(&base, &base, &theirs, &HashMap::new()).unwrap()
    else {
        panic!("uncontested role")
    };
    assert_eq!(merged.psd_background, Some(1));
    assert!(
        crate::graph::compare(&base, &theirs)
            .iter()
            .any(|row| row.label == "Photoshop Background")
    );
    let mut ours = base.clone();
    ours.blend_space = BlendSpace::PhotoshopSrgbV1;
    let choices = HashMap::from([(ConflictKey::Canvas, Side::Theirs)]);
    let MergeOutcome::Merged(merged) = merge(&base, &ours, &theirs, &choices).unwrap() else {
        panic!("resolved role")
    };
    assert_eq!(merged.psd_background, Some(1));
    assert_eq!(merged.blend_space, BlendSpace::Linear);
    let mut ours = base.clone();
    Command::RemoveNode { id: 1 }.apply(&mut ours).unwrap();
    let MergeOutcome::Merged(merged) = merge(&base, &ours, &theirs, &HashMap::new()).unwrap()
    else {
        panic!("deleted role")
    };
    assert_eq!(merged.psd_background, None);
}
