use super::*;
use crate::{Node, NodeKind, fragment::Fragment, text::TextSpec};
fn setup() -> (Editor, NodeId, NodeId) {
    let mut e = Editor::new(Document::new(800, 600), None);
    let text = e
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Title",
                TextSpec {
                    text: "Original".into(),
                    x: 40.,
                    y: 60.,
                    ..Default::default()
                },
                800,
                600,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let root = create(&mut e, &[text], "Badge").unwrap();
    (e, root, text)
}
fn text_id(doc: &Document, root: NodeId) -> NodeId {
    doc.subtree(root)
        .into_iter()
        .find(|id| matches!(doc.node(*id).unwrap().kind, NodeKind::Text { .. }))
        .unwrap()
}
fn spec(doc: &Document, id: NodeId) -> TextSpec {
    match &doc.node(id).unwrap().kind {
        NodeKind::Text { spec, .. } => (**spec).clone(),
        _ => panic!(),
    }
}
fn words(e: &mut Editor, id: NodeId, words: &str) {
    let mut s = spec(&e.doc, id);
    s.text = words.into();
    e.execute(Command::SetText {
        id,
        spec: Box::new(s),
    })
    .unwrap();
}
#[test]
fn nested_dependencies_refresh_stable_ids_and_reject_cycles_atomically() {
    let (mut e, badge, text) = setup();
    let panel = create(&mut e, &[badge], "Panel").unwrap();
    let card = create(&mut e, &[panel], "Card").unwrap();
    let second = insert(&mut e, "Card", "Default", (250., 0.)).unwrap();
    let old_ids = e.doc.subtree(second);
    words(&mut e, text, "Published through nesting");
    let before = e.doc.clone();
    update(&mut e, badge, None).unwrap();
    assert_eq!(e.doc.subtree(second), old_ids);
    for root in [card, second] {
        assert_eq!(
            spec(&e.doc, text_id(&e.doc, root)).text,
            "Published through nesting"
        );
    }
    e.doc.validate().unwrap();
    e.undo();
    assert_eq!(e.doc, before);
    e.redo();
    let cycle = insert(&mut e, "Card", "Default", (0., 200.)).unwrap();
    e.execute(Command::MoveNode {
        id: cycle,
        slot: Slot::top_of(Some(badge)),
    })
    .unwrap();
    let before = e.doc.clone();
    let revision = e.revision;
    let history = e.history.len();
    assert!(update(&mut e, badge, None).unwrap_err().contains("acyclic"));
    assert_eq!(e.doc, before);
    assert_eq!(e.revision, revision);
    assert_eq!(e.history.len(), history);
}
#[test]
fn explicit_properties_survive_publish_and_reset_clears_them() {
    let (mut e, a, source_text) = setup();
    let b = insert(&mut e, "Badge", "Default", (250., 0.)).unwrap();
    let child = text_id(&e.doc, b);
    words(&mut e, child, "Local content");
    e.execute(Command::SetOpacity {
        id: child,
        opacity: 0.4,
    })
    .unwrap();
    e.execute(Command::SetVisible {
        id: child,
        visible: false,
    })
    .unwrap();
    set_overrides(
        &mut e,
        b,
        child,
        Overrides {
            content: true,
            geometry: true,
            opacity: true,
            visibility: true,
            ..Default::default()
        },
    )
    .unwrap();
    let position = (spec(&e.doc, child).x, spec(&e.doc, child).y);
    let mut published = spec(&e.doc, source_text);
    published.text = "New source".into();
    published.color = [255, 0, 0, 255];
    published.size = 64.;
    e.execute(Command::SetText {
        id: source_text,
        spec: Box::new(published),
    })
    .unwrap();
    let before = e.doc.clone();
    update(&mut e, a, None).unwrap();
    assert_eq!(text_id(&e.doc, b), child);
    let local = spec(&e.doc, child);
    assert_eq!(local.text, "Local content");
    assert_eq!(local.color, [255, 0, 0, 255]);
    assert_eq!(local.size, 64.);
    assert_eq!((local.x, local.y), position);
    assert_eq!(e.doc.node(child).unwrap().opacity, 0.4);
    assert!(!e.doc.node(child).unwrap().visible);
    e.undo();
    assert_eq!(e.doc, before);
    e.redo();
    let changed = e.doc.clone();
    reset(&mut e, b, None).unwrap();
    assert_eq!(text_id(&e.doc, b), child);
    assert_eq!(spec(&e.doc, child).text, "New source");
    assert_eq!(e.doc.node(child).unwrap().opacity, 1.);
    assert!(e.doc.node(child).unwrap().visible);
    assert!(e.doc.design.component_links[&b].overrides.is_empty());
    e.undo();
    assert_eq!(e.doc, changed);
}
#[test]
fn nested_override_clipboard_duplicate_and_import_keep_dependency_libraries() {
    let (mut e, badge, text) = setup();
    let outer = create(&mut e, &[badge], "Panel").unwrap();
    set_overrides(
        &mut e,
        badge,
        text,
        Overrides {
            content: true,
            ..Default::default()
        },
    )
    .unwrap();
    words(&mut e, text, "Nested override");
    let copy = e
        .execute(Command::DuplicateNode { id: outer })
        .unwrap()
        .unwrap();
    e.doc.validate().unwrap();
    let copy_text = text_id(&e.doc, copy);
    let copied_owner = owner(&e.doc, copy_text).unwrap();
    assert!(overrides_for(&e.doc, copied_owner, copy_text).content);
    let fragment = Fragment::capture(&e.doc, &[copy]).unwrap();
    let mut target = Editor::new(Document::new(800, 600), None);
    let pasted = fragment.paste(&mut target, Slot::TOP, (0., 0.)).unwrap()[0];
    target.doc.validate().unwrap();
    assert_eq!(target.doc.design.components.len(), 2);
    let pasted_text = text_id(&target.doc, pasted);
    let pasted_owner = owner(&target.doc, pasted_text).unwrap();
    assert!(overrides_for(&target.doc, pasted_owner, pasted_text).content);
    let mut imported = Editor::new(Document::new(800, 600), None);
    let inserted = import_and_insert(&mut imported, &e.doc, "Panel", "Default", (0., 0.)).unwrap();
    imported.doc.validate().unwrap();
    assert_eq!(imported.doc.design.components.len(), 2);
    assert!(owner(&imported.doc, text_id(&imported.doc, inserted)).is_some());
}
#[test]
fn appearance_overrides_are_independent_and_locked_updates_roll_back() {
    let (mut e, a, text) = setup();
    let b = insert(&mut e, "Badge", "Default", (250., 0.)).unwrap();
    let child = text_id(&e.doc, b);
    let mut custom = spec(&e.doc, child);
    custom.color = [0, 255, 0, 255];
    custom.size = 22.;
    e.execute(Command::SetText {
        id: child,
        spec: Box::new(custom),
    })
    .unwrap();
    set_overrides(
        &mut e,
        b,
        child,
        Overrides {
            appearance: true,
            ..Default::default()
        },
    )
    .unwrap();
    words(&mut e, text, "Changed content");
    e.execute(Command::SetOpacity {
        id: text,
        opacity: 0.2,
    })
    .unwrap();
    update(&mut e, a, None).unwrap();
    assert_eq!(spec(&e.doc, child).text, "Changed content");
    assert_eq!(spec(&e.doc, child).color, [0, 255, 0, 255]);
    assert_eq!(spec(&e.doc, child).size, 22.);
    assert_eq!(e.doc.node(child).unwrap().opacity, 0.2);
    e.execute(Command::SetLocked {
        id: child,
        locked: true,
    })
    .unwrap();
    words(&mut e, text, "Cannot publish");
    let before = e.doc.clone();
    let revision = e.revision;
    let history = e.history.len();
    assert!(update(&mut e, a, None).is_err());
    assert_eq!(e.doc, before);
    assert_eq!(e.revision, revision);
    assert_eq!(e.history.len(), history);
    assert!(reset(&mut e, b, None).is_err());
    assert_eq!(e.doc, before);
}
#[test]
fn legacy_links_bootstrap_members_and_invalid_refs_are_atomic() {
    let (mut e, a, text) = setup();
    let b = insert(&mut e, "Badge", "Default", (250., 0.)).unwrap();
    for link in e.doc.design.component_links.values_mut() {
        link.members.clear();
    }
    e.doc.validate().unwrap();
    words(&mut e, text, "Legacy update");
    update(&mut e, a, None).unwrap();
    assert_eq!(spec(&e.doc, text_id(&e.doc, b)).text, "Legacy update");
    assert!(!e.doc.design.component_links[&b].members.is_empty());
    let before = e.doc.clone();
    let mut design = e.doc.design.clone();
    design
        .component_links
        .get_mut(&b)
        .unwrap()
        .members
        .insert(999999, text);
    assert!(
        e.execute(Command::SetDesign {
            design: Box::new(design)
        })
        .is_err()
    );
    assert_eq!(e.doc, before);
}

#[test]
fn publishing_only_refreshes_dependent_variants_and_ignores_unrelated_locks() {
    let (mut e, badge, text) = setup();
    update(&mut e, badge, Some("Other")).unwrap();
    let other = insert(&mut e, "Badge", "Other", (250., 0.)).unwrap();
    let wrapper = create(&mut e, &[other], "Wrapper").unwrap();
    update(&mut e, wrapper, Some("Other wrapper")).unwrap();
    reset(&mut e, other, Some("Default")).unwrap();
    update(&mut e, wrapper, Some("Default wrapper")).unwrap();
    let unrelated = insert(&mut e, "Wrapper", "Other wrapper", (0., 250.)).unwrap();
    let unrelated_text = text_id(&e.doc, unrelated);
    words(&mut e, unrelated_text, "Unpublished local text");
    e.execute(Command::SetLocked {
        id: unrelated_text,
        locked: true,
    })
    .unwrap();
    reset(&mut e, badge, Some("Default")).unwrap();
    let text = if e.doc.node(text).is_some() {
        text
    } else {
        text_id(&e.doc, badge)
    };
    words(&mut e, text, "Default only");
    update(&mut e, badge, None).unwrap();
    assert_eq!(spec(&e.doc, unrelated_text).text, "Unpublished local text");
    e.doc.validate().unwrap();
}
#[test]
fn import_dependency_member_ids_do_not_collide_with_existing_page_ids() {
    let (mut source, badge, text) = setup();
    set_overrides(
        &mut source,
        badge,
        text,
        Overrides {
            content: true,
            ..Default::default()
        },
    )
    .unwrap();
    create(&mut source, &[badge], "Panel").unwrap();
    for padding in 1..12 {
        let mut target = Editor::new(Document::new(800, 600), None);
        for _ in 0..padding {
            target
                .execute(Command::AddNode {
                    node: Box::new(Node::text(
                        0,
                        "Existing",
                        TextSpec {
                            text: "Existing".into(),
                            ..Default::default()
                        },
                        800,
                        600,
                    )),
                    slot: Slot::TOP,
                })
                .unwrap();
        }
        let root =
            import_and_insert(&mut target, &source.doc, "Panel", "Default", (0., 0.)).unwrap();
        target.doc.validate().unwrap();
        let member = text_id(&target.doc, root);
        let group = owner(&target.doc, member).unwrap();
        assert!(
            overrides_for(&target.doc, group, member).content,
            "padding {padding}"
        );
    }
}

#[test]
fn importing_names_with_numeric_suffixes_does_not_rename_other_dependencies() {
    let (mut source, a, _) = setup();
    let text = source
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Other title",
                TextSpec {
                    text: "Second dependency".into(),
                    ..Default::default()
                },
                800,
                600,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let b = create(&mut source, &[text], "Badge (2)").unwrap();
    create(&mut source, &[a, b], "Pair").unwrap();
    let (mut target, _, _) = setup();
    let pair = import_and_insert(&mut target, &source.doc, "Pair", "Default", (0., 0.)).unwrap();
    target.doc.validate().unwrap();
    let names: HashSet<_> = target
        .doc
        .subtree(pair)
        .iter()
        .filter_map(|id| {
            target
                .doc
                .design
                .component_links
                .get(id)
                .map(|l| l.component.clone())
        })
        .collect();
    assert_eq!(names.len(), 3);
    assert_eq!(target.doc.design.components.len(), 4);
    let words: HashSet<_> = target
        .doc
        .subtree(pair)
        .iter()
        .filter_map(|id| match &target.doc.node(*id)?.kind {
            NodeKind::Text { spec, .. } => Some(spec.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        words,
        HashSet::from(["Original".into(), "Second dependency".into()])
    );
}

#[test]
fn root_visibility_override_survives_publish_and_reset_restores_visible_instance() {
    let (mut e, a, text) = setup();
    let b = insert(&mut e, "Badge", "Default", (250., 0.)).unwrap();
    set_overrides(
        &mut e,
        b,
        b,
        Overrides {
            visibility: true,
            ..Default::default()
        },
    )
    .unwrap();
    e.execute(Command::SetVisible {
        id: b,
        visible: false,
    })
    .unwrap();
    words(&mut e, text, "Published");
    update(&mut e, a, None).unwrap();
    assert!(!e.doc.node(b).unwrap().visible);
    assert!(overrides_for(&e.doc, b, b).visibility);
    let before = e.doc.clone();
    reset(&mut e, b, None).unwrap();
    assert!(e.doc.node(b).unwrap().visible);
    assert!(overrides_for(&e.doc, b, b).is_empty());
    for root in source_roots(&e.doc.design) {
        assert!(!e.doc.node(root).unwrap().visible);
    }
    e.undo();
    assert_eq!(e.doc, before);
}

#[test]
fn component_pixel_publish_detaches_raw_recipe_but_keeps_original_and_undo() {
    use crate::raw::{DevelopParams, RawDocument, RawMetadata};
    use emulsion_raster::{IRect, Placement, Raster};
    let mut e = Editor::new(Document::new(32, 32), None);
    let raw_node = e
        .execute(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "RAW",
                std::sync::Arc::new(Raster::solid(8, 8, [0.2, 0.3, 0.4, 1.])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    e.doc.raw = Some(RawDocument {
        schema_version: 1,
        node_id: raw_node,
        source: "original.dng".into(),
        source_sha256: "a".repeat(64),
        params: DevelopParams::default(),
        metadata: RawMetadata::default(),
    });
    let a = create(&mut e, &[raw_node], "Photo").unwrap();
    assert!(e.doc.raw.is_some());
    let b = insert(&mut e, "Photo", "Default", (10., 0.)).unwrap();
    let image = e.doc.children(Some(b))[0];
    e.execute(Command::ReplacePixels {
        id: image,
        raster: std::sync::Arc::new(Raster::solid(8, 8, [0.8, 0.1, 0.2, 1.])),
        dirty: IRect::new(0, 0, 8, 8),
        label: "Edit instance pixels".into(),
    })
    .unwrap();
    assert!(e.doc.raw.is_some());
    let before = e.doc.clone();
    update(&mut e, b, None).unwrap();
    assert!(e.doc.node(a).is_some());
    assert!(e.doc.raw.is_none());
    assert!(e.doc.raw_originals.contains(&"original.dng".into()));
    e.undo();
    assert_eq!(e.doc, before);
    assert!(e.doc.raw.is_some());
    e.redo();
    assert!(e.doc.raw.is_none());
}
