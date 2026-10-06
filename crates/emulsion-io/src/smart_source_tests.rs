use super::*;
use emulsion_core::{Command, command::Slot};
use emulsion_raster::Raster;
fn smart() -> (Editor, NodeId) {
    let mut e = Editor::new(Document::new(40, 30), None);
    let id = e
        .execute(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Placed",
                Arc::new(Raster::solid(20, 10, [1., 0., 0., 1.])),
                Placement::at(7., 9.),
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    e.execute(Command::ConvertToSmart { id }).unwrap();
    (e, id)
}
fn temp(name: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "emulsion-smart-source-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}
#[test]
fn nested_smart_source_archives_round_trip_and_restore_parent_undo() {
    let (mut outer, id) = smart();
    let (mut inner, inner_id) = smart();
    let mut deepest = open(&inner.doc, inner_id).unwrap();
    deepest.nodes[0].name = "Nested original".into();
    apply(&mut inner, inner_id, &deepest).unwrap();
    let before = outer.doc.clone();
    apply(&mut outer, id, &inner.doc).unwrap();
    let bytes = encode(&outer.doc).unwrap();
    let reopened = crate::ora::read_from(Cursor::new(bytes.as_slice()))
        .unwrap()
        .doc;
    let child = open(&reopened, id).unwrap();
    let leaf = open(&child, inner_id).unwrap();
    assert_eq!(leaf.nodes[0].name, "Nested original");
    assert!(matches!(
        child.node(inner_id).unwrap().kind,
        NodeKind::Smart {
            editable: Some(SmartEditable::Document { .. }),
            ..
        }
    ));
    outer.undo();
    assert_eq!(outer.doc, before);
    outer.redo();
    assert_eq!(
        open(&outer.doc, id).unwrap().nodes.len(),
        inner.doc.nodes.len()
    );
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
    assert_eq!(
        (0..zip.len())
            .filter(|i| zip.by_index(*i).unwrap().name().starts_with("sources/"))
            .count(),
        1
    );
}
#[test]
fn linked_source_refresh_conflicts_and_explicit_native_write_keep_original_safe() {
    let (mut e, id) = smart();
    let path = temp("source.ora");
    let mut source = open(&e.doc, id).unwrap();
    crate::ora::write(&source, &path).unwrap();
    let initial = std::fs::read(&path).unwrap();
    relink(&mut e, id, &path, true).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), initial);
    assert!(!refresh(&mut e, id, false).unwrap());
    source.nodes[0].name = "External edit".into();
    crate::ora::write(&source, &path).unwrap();
    let before = e.doc.clone();
    assert!(refresh(&mut e, id, false).unwrap());
    assert_eq!(open(&e.doc, id).unwrap().nodes[0].name, "External edit");
    e.undo();
    assert_eq!(e.doc, before);
    e.redo();
    let mut local = open(&e.doc, id).unwrap();
    local.nodes[0].name = "Local edit".into();
    apply(&mut e, id, &local).unwrap();
    source.nodes[0].name = "Conflicting external edit".into();
    crate::ora::write(&source, &path).unwrap();
    let external = std::fs::read(&path).unwrap();
    let doc = e.doc.clone();
    assert!(refresh(&mut e, id, false).is_err());
    assert!(write_linked(&mut e, id).is_err());
    assert_eq!(doc, e.doc);
    assert_eq!(external, std::fs::read(&path).unwrap());
    assert!(refresh(&mut e, id, true).unwrap());
    let mut local = open(&e.doc, id).unwrap();
    local.nodes[0].name = "Explicit write".into();
    apply(&mut e, id, &local).unwrap();
    write_linked(&mut e, id).unwrap();
    assert_eq!(
        crate::ora::read(&path).unwrap().nodes[0].name,
        "Explicit write"
    );
    assert!(
        !emulsion_core::smart_source::link(&e.doc, id)
            .unwrap()
            .locally_modified
    );
    std::fs::remove_file(&path).unwrap();
    let before = e.doc.clone();
    assert!(refresh(&mut e, id, false).is_err());
    assert_eq!(before, e.doc);
    assert!(open(&e.doc, id).is_ok());
}
#[test]
fn smart_source_save_as_locks_and_link_round_trip() {
    let (mut e, id) = smart();
    let path = temp("new.ora");
    save_as(&mut e, id, &path).unwrap();
    let bytes = encode(&e.doc).unwrap();
    let doc = crate::ora::read_from(Cursor::new(bytes.as_slice()))
        .unwrap()
        .doc;
    assert_eq!(
        emulsion_core::smart_source::link(&doc, id),
        emulsion_core::smart_source::link(&e.doc, id)
    );
    let before = e.doc.clone();
    assert!(save_as(&mut e, id, &path).is_err());
    assert_eq!(e.doc, before);
    e.execute(Command::SetLocked { id, locked: true }).unwrap();
    let before = e.doc.clone();
    let source = open(&e.doc, id).unwrap();
    assert!(apply(&mut e, id, &source).is_err());
    assert_eq!(e.doc, before);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn native_source_history_uses_shared_resources_and_detects_corruption() {
    let (mut e, id) = smart();
    let mut source = open(&e.doc, id).unwrap();
    source.nodes[0].name = "Edited source".into();
    apply(&mut e, id, &source).unwrap();
    e.create_version("Source saved").unwrap();
    for index in 0..6 {
        e.execute(Command::Rename {
            id,
            name: format!("Renamed {index}"),
        })
        .unwrap();
        e.create_version(format!("Rename {index}")).unwrap();
    }
    let path = temp("history.ora");
    crate::ora::write_full(&e.doc, Some(&e.graph), &path).unwrap();
    let opened = crate::ora::read_full(&path).unwrap();
    assert!(opened.history_error.is_none());
    assert!(open(&opened.doc, id).is_ok());
    let file = std::fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    assert!(
        (0..zip.len())
            .filter(|i| zip
                .by_index(*i)
                .unwrap()
                .name()
                .starts_with("history/sources/"))
            .count()
            == 1
    );
    let mut damaged = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).unwrap();
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name.starts_with("sources/") {
            bytes[0] ^= 1;
        }
        damaged
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        use std::io::Write;
        damaged.write_all(&bytes).unwrap();
    }
    let damaged = damaged.finish().unwrap();
    assert!(crate::ora::read_from(damaged).is_err());
    std::fs::remove_file(path).unwrap();
}

fn smart_with_original() -> (Editor, NodeId, Arc<emulsion_core::node::OriginalImage>) {
    use image::ImageEncoder;
    let (mut editor, id) = smart();
    let mut bytes = Vec::new();
    // The transparent texel has hidden RGB that must survive a no-op source session.
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[255, 0, 0, 255, 31, 47, 63, 0],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let bytes = Arc::new(bytes);
    let decoded = crate::original_image_png::png_source(&bytes).unwrap();
    let original = crate::original_image_data::capture(bytes, &decoded);
    let NodeKind::Smart {
        source,
        cache,
        original_image,
        ..
    } = &mut editor.doc.node_mut(id).unwrap().kind
    else {
        unreachable!()
    };
    *source = decoded.clone();
    *cache = decoded;
    *original_image = Some(original.clone());
    editor.doc.validate().unwrap();
    (editor, id, original)
}

#[test]
fn raster_original_source_open_apply_noop_preserves_bytes_revision_and_history() {
    let (mut editor, id, original) = smart_with_original();
    let before = editor.doc.clone();
    let revision = editor.revision;
    let history = editor.history.len();
    let source = open(&editor.doc, id).unwrap();
    apply(&mut editor, id, &source).unwrap();
    assert_eq!(editor.doc, before);
    assert_eq!(editor.revision, revision);
    assert_eq!(editor.history.len(), history);
    let NodeKind::Smart {
        original_image: Some(retained),
        editable: None,
        ..
    } = &editor.doc.node(id).unwrap().kind
    else {
        panic!("no-op promoted source")
    };
    assert!(Arc::ptr_eq(retained, &original));
    assert!(Arc::ptr_eq(retained.bytes(), original.bytes()));
}

#[test]
fn raster_original_source_selection_only_noop_preserves_original() {
    let (mut editor, id, original) = smart_with_original();
    let before = editor.doc.clone();
    let revision = editor.revision;
    let history = editor.history.len();
    let mut source = open(&editor.doc, id).unwrap();
    source.selection = Some(Arc::new(emulsion_raster::Mask::empty(2, 1, 127)));
    apply(&mut editor, id, &source).unwrap();
    assert_eq!(editor.doc, before);
    assert_eq!(editor.revision, revision);
    assert_eq!(editor.history.len(), history);
    let NodeKind::Smart {
        original_image: Some(retained),
        editable: None,
        ..
    } = &editor.doc.node(id).unwrap().kind
    else {
        panic!("selection promoted source")
    };
    assert!(Arc::ptr_eq(retained, &original));
}

#[test]
fn raster_original_source_real_edits_promote_to_document_and_undo_restores_original() {
    for edit in ["name", "pixels", "layer"] {
        let (mut editor, id, original) = smart_with_original();
        let before = editor.doc.clone();
        let revision = editor.revision;
        let history = editor.history.len();
        let mut source = open(&editor.doc, id).unwrap();
        match edit {
            "name" => source.nodes[0].name = "Edited source name".into(),
            "pixels" => {
                source.nodes[0].kind = NodeKind::Raster {
                    raster: Arc::new(Raster::solid(2, 1, [0., 1., 0., 1.])),
                    placement: Placement::default(),
                }
            }
            "layer" => {
                let layer_id = source.alloc_id();
                source.nodes.push(Node::new(
                    layer_id,
                    "Added layer",
                    NodeKind::Fill {
                        rgba: [0, 0, 255, 255],
                    },
                ));
            }
            _ => unreachable!(),
        }
        apply(&mut editor, id, &source).unwrap();
        assert!(editor.revision > revision, "{edit}");
        assert_eq!(editor.history.len(), history + 1, "{edit}");
        let NodeKind::Smart {
            original_image: None,
            editable:
                Some(SmartEditable::Document {
                    archive,
                    external: None,
                }),
            ..
        } = &editor.doc.node(id).unwrap().kind
        else {
            panic!("{edit} did not promote source")
        };
        assert!(!archive.is_empty());
        let archive = archive.clone();
        let restored_source = open(&editor.doc, id).unwrap();
        match edit {
            "name" => assert_eq!(restored_source.nodes[0].name, "Edited source name"),
            "pixels" => assert_eq!(
                flatten(&restored_source.composite_tree(), 0).get(0, 0),
                [0, 65535, 0, 65535]
            ),
            "layer" => assert_eq!(restored_source.nodes.len(), 2),
            _ => unreachable!(),
        }
        editor.undo();
        assert_eq!(editor.doc, before, "{edit}");
        let NodeKind::Smart {
            original_image: Some(retained),
            editable: None,
            ..
        } = &editor.doc.node(id).unwrap().kind
        else {
            panic!("{edit} undo lost original")
        };
        assert!(Arc::ptr_eq(retained, &original));
        editor.redo();
        assert!(
            matches!(&editor.doc.node(id).unwrap().kind,
                NodeKind::Smart { original_image: None, editable: Some(SmartEditable::Document { archive: current, .. }), .. }
                    if Arc::ptr_eq(current, &archive)
            ),
            "{edit}"
        );
    }
}
