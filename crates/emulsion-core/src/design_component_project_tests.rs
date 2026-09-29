use super::*;
use crate::{Command, Node, NodeKind, command::Slot, project::ProjectKind, text::TextSpec};
fn fixture() -> (ProjectEditor, NodeId, NodeId, PageId) {
    let mut p = ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap();
    let id = p
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Label",
                TextSpec {
                    text: "Source".into(),
                    size: 24.,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let instance = create(&mut p, &[id], "Card").unwrap();
    let source_page = p.active_page();
    p.add_page(Document::new(600, 400), "Other page".into(), 0.)
        .unwrap();
    let copy = insert_project(&mut p, source_page, "Card", "Default", (100., 0.)).unwrap();
    (p, instance, copy, source_page)
}
fn label(doc: &Document, root: NodeId) -> NodeId {
    doc.subtree(root)
        .into_iter()
        .find(|id| matches!(doc.node(*id).unwrap().kind, NodeKind::Text { .. }))
        .unwrap()
}
fn edit(p: &mut ProjectEditor, id: NodeId, text: &str, size: f32) {
    let NodeKind::Text { spec, .. } = &p.doc.node(id).unwrap().kind else {
        panic!()
    };
    let mut spec = (**spec).clone();
    spec.text = text.into();
    spec.size = size;
    p.execute(Command::SetText {
        id,
        spec: Box::new(spec),
    })
    .unwrap();
}
#[test]
fn project_component_publish_preserves_ids_overrides_and_one_undo_across_pages() {
    let (mut p, instance, copy, source_page) = fixture();
    let target_page = p.active_page();
    let child = label(&p.doc, copy);
    set_overrides(
        &mut p,
        copy,
        child,
        Overrides {
            content: true,
            ..Default::default()
        },
    )
    .unwrap();
    edit(&mut p, child, "Local copy", 24.);
    p.set_active_page(source_page).unwrap();
    let original = label(&p.doc, instance);
    edit(&mut p, original, "Published", 42.);
    let before: Vec<_> = p
        .page_list()
        .iter()
        .map(|m| (m.id, p.page(m.id).unwrap().doc.clone()))
        .collect();
    assert_eq!(publish_project(&mut p, instance).unwrap(), 2);
    let target = &p.page(target_page).unwrap().doc;
    assert_eq!(label(target, copy), child);
    let NodeKind::Text { spec, .. } = &target.node(child).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.text, "Local copy");
    assert_eq!(spec.size, 42.);
    let after: Vec<_> = p
        .page_list()
        .iter()
        .map(|m| (m.id, p.page(m.id).unwrap().doc.clone()))
        .collect();
    assert!(p.undo());
    for (id, doc) in &before {
        assert_eq!(&p.page(*id).unwrap().doc, doc);
    }
    assert!(p.redo());
    for (id, doc) in &after {
        assert_eq!(&p.page(*id).unwrap().doc, doc);
    }
    p.snapshot().unwrap().validate().unwrap();
}
#[test]
fn project_component_locked_remote_consumer_rejects_every_page_and_import_is_one_undo() {
    let (mut p, instance, copy, source_page) = fixture();
    let target_page = p.active_page();
    p.doc.node_mut(copy).unwrap().locked = true;
    p.set_active_page(source_page).unwrap();
    let id = label(&p.doc, instance);
    edit(&mut p, id, "New", 34.);
    let before = p.doc.clone();
    let other = p.page(target_page).unwrap().doc.clone();
    assert!(publish_project(&mut p, instance).is_err());
    assert_eq!(p.doc, before);
    assert_eq!(p.page(target_page).unwrap().doc, other);
}
#[test]
fn fine_component_font_override_keeps_size_while_source_color_updates() {
    let (mut p, instance, copy, source_page) = fixture();
    set_auto_overrides(&mut p, copy, false).unwrap();
    let page = p.active_page();
    let child = label(&p.doc, copy);
    set_overrides(
        &mut p,
        copy,
        child,
        Overrides {
            font_size: true,
            ..Default::default()
        },
    )
    .unwrap();
    edit(&mut p, child, "Local", 70.);
    p.set_active_page(source_page).unwrap();
    let id = label(&p.doc, instance);
    let NodeKind::Text { spec, .. } = &p.doc.node(id).unwrap().kind else {
        panic!()
    };
    let mut spec = (**spec).clone();
    spec.text = "Published".into();
    spec.color = [255, 0, 0, 255];
    spec.size = 32.;
    p.execute(Command::SetText {
        id,
        spec: Box::new(spec),
    })
    .unwrap();
    publish_project(&mut p, instance).unwrap();
    let NodeKind::Text { spec, .. } = &p.page(page).unwrap().doc.node(child).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.size, 70.);
    assert_eq!(spec.text, "Published");
    assert_eq!(spec.color, [255, 0, 0, 255]);
}

#[test]
fn project_publish_preserves_unrelated_locked_families_and_destination_only_variants() {
    let (mut p, instance, copy, source_page) = fixture();
    let target_page = p.active_page();
    // A variant authored only on the destination must not be republished by an
    // unrelated update to the source page's Default variant.
    let local_label = label(&p.doc, copy);
    edit(&mut p, local_label, "Local variant source", 31.);
    update(&mut p, copy, Some("Destination only")).unwrap();
    let alternate = insert(&mut p, "Card", "Destination only", (200., 0.)).unwrap();
    reset(&mut p, copy, Some("Default")).unwrap();
    let alt_label = label(&p.doc, alternate);
    edit(&mut p, alt_label, "Unpublished alternate instance", 47.);
    p.execute(Command::SetLocked {
        id: alternate,
        locked: true,
    })
    .unwrap();
    p.set_active_page(source_page).unwrap();
    let button_member = p
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Independent",
                TextSpec {
                    text: "Button source".into(),
                    x: 250.,
                    y: 120.,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    create(&mut p, &[button_member], "Button").unwrap();
    p.set_active_page(target_page).unwrap();
    let unrelated = insert_project(&mut p, source_page, "Button", "Default", (0., 100.)).unwrap();
    let unrelated_label = label(&p.doc, unrelated);
    edit(&mut p, unrelated_label, "Unpublished button instance", 55.);
    p.execute(Command::SetLocked {
        id: unrelated,
        locked: true,
    })
    .unwrap();
    let unaffected_ids = p
        .doc
        .subtree(alternate)
        .into_iter()
        .chain(p.doc.subtree(unrelated))
        .chain(
            p.doc
                .subtree(p.doc.design.components["Card"].variants["Destination only"]),
        )
        .chain(
            p.doc
                .subtree(p.doc.design.components["Button"].variants["Default"]),
        )
        .collect::<HashSet<_>>();
    let unaffected = p
        .doc
        .nodes
        .iter()
        .filter(|n| unaffected_ids.contains(&n.id))
        .cloned()
        .collect::<Vec<_>>();
    p.set_active_page(source_page).unwrap();
    let original = label(&p.doc, instance);
    edit(&mut p, original, "Published Default", 36.);
    let before = p
        .page_list()
        .iter()
        .map(|m| (m.id, p.page(m.id).unwrap().doc.clone()))
        .collect::<Vec<_>>();
    publish_project(&mut p, instance).unwrap();
    let target = &p.page(target_page).unwrap().doc;
    assert_eq!(
        target
            .nodes
            .iter()
            .filter(|n| unaffected_ids.contains(&n.id))
            .cloned()
            .collect::<Vec<_>>(),
        unaffected
    );
    let updated = label(target, copy);
    let NodeKind::Text { spec, .. } = &target.node(updated).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.text, "Published Default");
    let after = p
        .page_list()
        .iter()
        .map(|m| (m.id, p.page(m.id).unwrap().doc.clone()))
        .collect::<Vec<_>>();
    assert!(p.undo());
    for (id, doc) in &before {
        assert_eq!(&p.page(*id).unwrap().doc, doc);
    }
    assert!(p.redo());
    for (id, doc) in &after {
        assert_eq!(&p.page(*id).unwrap().doc, doc);
    }
}

#[test]
fn project_insert_imports_missing_variant_without_refreshing_existing_artwork() {
    let (mut p, instance, copy, source_page) = fixture();
    let target_page = p.active_page();
    let child = label(&p.doc, copy);
    edit(&mut p, child, "Local unpublished", 57.);
    p.execute(Command::SetLocked {
        id: copy,
        locked: true,
    })
    .unwrap();
    let existing = p
        .doc
        .subtree(copy)
        .into_iter()
        .map(|id| p.doc.node(id).unwrap().clone())
        .collect::<Vec<_>>();
    p.set_active_page(source_page).unwrap();
    let child = label(&p.doc, instance);
    edit(&mut p, child, "New source variant", 41.);
    update(&mut p, instance, Some("New variant")).unwrap();
    p.set_active_page(target_page).unwrap();
    let before = p.doc.clone();
    let added = insert_project(&mut p, source_page, "Card", "New variant", (0., 120.)).unwrap();
    assert_eq!(
        p.doc
            .subtree(copy)
            .into_iter()
            .map(|id| p.doc.node(id).unwrap().clone())
            .collect::<Vec<_>>(),
        existing
    );
    let NodeKind::Text { spec, .. } = &p.doc.node(label(&p.doc, added)).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.text, "New source variant");
    assert_eq!(p.doc.design.components.len(), 1);
    let after = p.doc.clone();
    assert!(p.undo());
    assert_eq!(p.doc, before);
    assert!(p.redo());
    assert_eq!(p.doc, after);
    p.snapshot().unwrap().validate().unwrap();
}

#[test]
fn project_publish_refreshes_all_local_aliases_of_a_family() {
    let (mut p, instance, copy, source_page) = fixture();
    let target_page = p.active_page();
    let source = p.page(source_page).unwrap().doc.clone();
    let alias = import(&mut p, &source, "Card").unwrap();
    assert_ne!(alias, "Card");
    let duplicate = insert(&mut p, &alias, "Default", (0., 180.)).unwrap();
    let stable = [label(&p.doc, copy), label(&p.doc, duplicate)];
    p.set_active_page(source_page).unwrap();
    let child = label(&p.doc, instance);
    edit(&mut p, child, "Shared publication", 39.);
    publish_project(&mut p, instance).unwrap();
    let target = &p.page(target_page).unwrap().doc;
    assert_eq!([label(target, copy), label(target, duplicate)], stable);
    for child in stable {
        let NodeKind::Text { spec, .. } = &target.node(child).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Shared publication");
    }
    p.snapshot().unwrap().validate().unwrap();
}

#[test]
fn project_appearance_override_preserves_base_text_decorations() {
    let (mut p, instance, copy, source_page) = fixture();
    let target_page = p.active_page();
    let child = label(&p.doc, copy);
    let NodeKind::Text { spec, .. } = &p.doc.node(child).unwrap().kind else {
        panic!()
    };
    let mut spec = (**spec).clone();
    spec.underline = true;
    spec.strikethrough = true;
    p.execute(Command::SetText {
        id: child,
        spec: Box::new(spec),
    })
    .unwrap();
    set_overrides(
        &mut p,
        copy,
        child,
        Overrides {
            appearance: true,
            ..Default::default()
        },
    )
    .unwrap();
    p.set_active_page(source_page).unwrap();
    let original = label(&p.doc, instance);
    edit(&mut p, original, "Source", 42.);
    publish_project(&mut p, instance).unwrap();
    let NodeKind::Text { spec, .. } = &p.page(target_page).unwrap().doc.node(child).unwrap().kind
    else {
        panic!()
    };
    assert!(spec.underline && spec.strikethrough);
    assert_eq!(spec.size, 24.);
    assert!(spec.runs.is_empty());
}

#[test]
fn project_publish_propagates_nested_variants_and_destination_only_wrappers() {
    let (mut p, instance, copy, source_page) = fixture();
    let target_page = p.active_page();
    // A wrapper absent from the publishing page must still refresh its linked
    // child, then refresh all instances of that particular wrapper variant.
    let wrapper = create(&mut p, &[copy], "Local wrapper").unwrap();
    let second = insert(&mut p, "Local wrapper", "Default", (0., 150.)).unwrap();
    let stable = [label(&p.doc, wrapper), label(&p.doc, second)];
    p.set_active_page(source_page).unwrap();
    let original = label(&p.doc, instance);
    edit(&mut p, original, "Nested publication", 33.);
    publish_project(&mut p, instance).unwrap();
    let target = &p.page(target_page).unwrap().doc;
    assert_eq!([label(target, wrapper), label(target, second)], stable);
    for child in stable {
        let NodeKind::Text { spec, .. } = &target.node(child).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Nested publication");
    }
    assert!(!p.doc.design.components.contains_key("Local wrapper"));
    p.snapshot().unwrap().validate().unwrap();
}
