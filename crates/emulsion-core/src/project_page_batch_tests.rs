use super::*;
use crate::{
    Command, Node, NodeKind,
    command::Slot,
    design_appearance::Appearance,
    design_interactions::Action,
    design_metadata::PageTransition,
    design_precision::{Settings, Unit},
    design_styles::SavedStyle,
    design_variables::{Property, Value},
    document::{Guide, ImageInfo},
};

fn fixture(count: usize) -> ProjectEditor {
    ProjectEditor::open(
        Project {
            kind: ProjectKind::Design,
            pages: (1..=count as u64)
                .map(|id| {
                    let doc = Document::new(64 + id as u32 % 13, 48 + id as u32 % 11);
                    ProjectPage {
                        meta: PageMeta {
                            id,
                            name: format!("Page {id}"),
                            bleed_mm: (id % 100) as f64,
                        },
                        graph: Graph::new(doc.clone(), "Opened"),
                        doc,
                    }
                })
                .collect(),
            active: 1,
            next_page_id: count as u64 + 1,
            storyboard: None,
        },
        Some("pages.emu".into()),
    )
    .unwrap()
}

fn ids(project: &ProjectEditor) -> Vec<PageId> {
    project.page_list().iter().map(|meta| meta.id).collect()
}

fn assert_document_eq(actual: &Document, expected: &Document) {
    assert_eq!(actual, expected);
    // These fields are intentionally excluded from artwork equality.
    assert_eq!(actual.colors, expected.colors);
    assert_eq!(actual.info, expected.info);
    assert_eq!(actual.source_depth, expected.source_depth);
    assert_eq!(actual.next_id, expected.next_id);
}

fn assert_graph_eq(actual: &Graph, expected: &Graph) {
    assert_eq!(actual.head(), expected.head());
    assert_eq!(actual.branches(), expected.branches());
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.commits().zip(expected.commits()) {
        assert_eq!(actual.id, expected.id);
        assert_eq!(actual.parents, expected.parents);
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.time, expected.time);
        assert_eq!(actual.auto, expected.auto);
        assert_eq!(actual.branch, expected.branch);
        assert_document_eq(&actual.doc, &expected.doc);
    }
}

fn assert_pages_eq(actual: &ProjectEditor, expected: &Project) {
    let actual = actual.snapshot().unwrap();
    assert_eq!(actual.kind, expected.kind);
    assert_eq!(actual.active, expected.active);
    assert_eq!(actual.pages.len(), expected.pages.len());
    for (actual, expected) in actual.pages.iter().zip(&expected.pages) {
        assert_eq!(actual.meta, expected.meta);
        assert_document_eq(&actual.doc, &expected.doc);
        assert_graph_eq(&actual.graph, &expected.graph);
    }
}

fn assert_rejected<T>(
    project: &mut ProjectEditor,
    operation: impl FnOnce(&mut ProjectEditor) -> Result<T, String>,
) {
    let before = project.snapshot().unwrap();
    let stamp = project.stamp();
    let state = (
        project.undo_pages.len(),
        project.redo_pages.len(),
        project.pages.len(),
        project.last_page_edit,
        project.is_modified(),
        project.can_redo(),
    );
    assert!(operation(project).is_err());
    assert_pages_eq(project, &before);
    assert_eq!(project.next_page_id, before.next_page_id);
    assert_eq!(project.stamp(), stamp);
    assert_eq!(
        state,
        (
            project.undo_pages.len(),
            project.redo_pages.len(),
            project.pages.len(),
            project.last_page_edit,
            project.is_modified(),
            project.can_redo(),
        )
    );
}

fn rich_document(page: PageId) -> Document {
    let mut doc = Document::new(100 + page as u32, 80 + page as u32);
    doc.resolution = 300.;
    doc.source_depth = 16;
    doc.colors = vec![[17, 42, 80], [254, 192, 16]];
    doc.info = Some(ImageInfo {
        software: "Brand artwork".into(),
        ..Default::default()
    });
    doc.guides = vec![Guide {
        vertical: true,
        pos: 12.5,
    }];
    for (index, target) in [1, 3, 4, 99, u64::MAX].into_iter().enumerate() {
        let id = Command::AddNode {
            node: Box::new(Node::new(
                0,
                format!("Artwork {index}"),
                NodeKind::Fill {
                    rgba: [17, 42, 80, 255],
                },
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        doc.design
            .interactions
            .insert(id, vec![Action::Slide { page: target }]);
    }
    let node = &doc.nodes[0];
    doc.design.saved_styles.insert(
        "Brand ink".into(),
        SavedStyle {
            appearance: Appearance::capture(node),
        },
    );
    doc.design.style_links.insert(node.id, "Brand ink".into());
    doc.design
        .variables
        .insert("Brand color".into(), Value::Color([17, 42, 80, 255]));
    doc.design
        .variable_libraries
        .insert("Brand color".into(), "brand-library-1".into());
    doc.design.variable_bindings.insert(
        node.id,
        BTreeMap::from([(Property::Fill, "Brand color".into())]),
    );
    doc.design.precision = Settings {
        unit: Unit::Millimeters,
        origin: [2., 3.],
    };
    doc.design.speaker_notes = format!("Speaker notes for page {page}");
    doc.design.page_transition = PageTransition::Slide;
    doc.validate().unwrap();
    doc
}

fn rich_fixture() -> ProjectEditor {
    let mut project = fixture(5).snapshot().unwrap();
    for page in &mut project.pages {
        let mut doc = rich_document(page.meta.id);
        let mut graph = Graph::new(doc.clone(), "Opened");
        graph.create_branch("Alternate", 1).unwrap();
        graph.set_head("Alternate").unwrap();
        doc.nodes[0].name = "Alternate artwork".into();
        // A reference that only exists in graph history must also be preserved.
        doc.design
            .interactions
            .insert(doc.nodes[0].id, vec![Action::Slide { page: 77 }]);
        graph.record(&doc, "Alternate version", false).unwrap();
        graph.set_head("main").unwrap();
        doc = rich_document(page.meta.id);
        doc.nodes[0].name = "Main artwork".into();
        graph.record(&doc, "Main version", false).unwrap();
        doc.design
            .speaker_notes
            .push_str(" with uncommitted changes");
        page.doc = doc;
        page.graph = graph;
    }
    project.active = 3;
    ProjectEditor::open(project, Some("brand.emu".into())).unwrap()
}

#[test]
fn batch_selection_is_validated_and_normalized_in_document_order() {
    let project = fixture(5);
    assert_eq!(
        project.ordered_page_selection(&[5, 1, 3]).unwrap(),
        [1, 3, 5]
    );
    assert!(project.ordered_page_selection(&[]).is_err());
    assert!(project.ordered_page_selection(&[1, 1]).is_err());
    assert!(project.ordered_page_selection(&[1, 0]).is_err());
    assert!(project.ordered_page_selection(&[1, 99]).is_err());
    assert!(!project.can_undo());
}

#[test]
fn duplicate_batch_preserves_artwork_brand_metadata_graphs_and_internal_links() {
    let mut project = rich_fixture();
    let before = project.snapshot().unwrap();
    let before_stamp = project.stamp();
    let copies = project.duplicate_pages(&[3, 1]).unwrap();
    assert_eq!(copies, [6, 7]);
    assert_eq!(ids(&project), [1, 2, 3, 6, 7, 4, 5]);
    assert_eq!(project.active_page(), 7);
    assert!(project.is_modified());
    assert_eq!(project.undo_pages.len(), 1);
    let mapping = BTreeMap::from([
        (1, 6),
        (3, 7),
        (4, 4),
        (77, 77),
        (99, 99),
        (u64::MAX, u64::MAX),
    ]);
    for (source, copy) in [(1, 6), (3, 7)] {
        let original = &before.pages[source as usize - 1];
        let copied = project.page(copy).unwrap();
        let mut expected = original.doc.clone();
        expected.design.remap_pages(&mapping);
        assert_document_eq(&copied.doc, &expected);
        let mut graph = original.graph.clone();
        graph.remap_pages(&mapping);
        assert_graph_eq(&copied.graph, &graph);
        assert_eq!(copied.path, Some("brand.emu".into()));
        assert!(copied.history.is_empty());
        assert!(copied.uncommitted());
        assert_document_eq(&copied.committed, &graph.commit(1).unwrap().doc);
        let meta = project
            .page_list()
            .iter()
            .find(|meta| meta.id == copy)
            .unwrap();
        assert_eq!(meta.name, format!("{} copy", original.meta.name));
        assert_eq!(meta.bleed_mm, original.meta.bleed_mm);
    }
    for original in &before.pages {
        let current = project.page(original.meta.id).unwrap();
        assert_document_eq(&current.doc, &original.doc);
        assert_graph_eq(&current.graph, &original.graph);
    }
    let after = project.snapshot().unwrap();
    after.validate().unwrap();
    assert!(project.undo());
    assert_pages_eq(&project, &before);
    assert_eq!(project.stamp(), before_stamp);
    assert!(!project.is_modified());
    assert!(!project.can_undo());
    assert!(project.redo());
    assert_pages_eq(&project, &after);
    assert!(!project.can_redo());
}

#[test]
fn duplicate_batch_uses_first_copy_when_active_is_outside_selection_and_keeps_names_valid() {
    let mut project = fixture(5);
    project.layout[2].name = "é".repeat(200);
    project.set_active_page(2).unwrap();
    let copies = project.duplicate_pages(&[5, 3]).unwrap();
    assert_eq!(project.active_page(), copies[0]);
    assert_eq!(ids(&project), [1, 2, 3, 4, 5, 6, 7]);
    let name = &project.page_list()[5].name;
    assert_eq!(name.chars().count(), 200);
    assert!(name.ends_with(" copy"));
    project.snapshot().unwrap().validate().unwrap();
    assert!(project.undo());
    assert_eq!(project.active_page(), 2);
    let replacement = project.duplicate_pages(&[1]).unwrap();
    assert!(replacement[0] > copies[1]);
    assert!(!project.can_redo());
}

#[test]
fn duplicate_batch_accepts_all_pages_as_one_group() {
    let mut project = fixture(3);
    assert_eq!(project.duplicate_pages(&[3, 1, 2]).unwrap(), [4, 5, 6]);
    assert_eq!(ids(&project), [1, 2, 3, 4, 5, 6]);
    assert!(project.undo());
    assert_eq!(ids(&project), [1, 2, 3]);
    assert!(!project.can_undo());
}

#[test]
fn remove_batch_preserves_survivors_and_restores_all_selected_pages_with_one_undo() {
    let mut project = rich_fixture();
    let before = project.snapshot().unwrap();
    project.remove_pages(&[5, 1, 3]).unwrap();
    assert_eq!(ids(&project), [2, 4]);
    assert_eq!(project.active_page(), 4);
    assert_eq!(project.undo_pages.len(), 1);
    for id in [2, 4] {
        let original = &before.pages[id as usize - 1];
        assert_document_eq(&project.page(id).unwrap().doc, &original.doc);
        assert_graph_eq(&project.page(id).unwrap().graph, &original.graph);
    }
    assert!(project.page(1).is_none());
    let after = project.snapshot().unwrap();
    after.validate().unwrap();
    assert!(project.undo());
    assert_pages_eq(&project, &before);
    assert!(!project.is_modified());
    assert!(!project.can_undo());
    assert!(project.redo());
    assert_pages_eq(&project, &after);
}

#[test]
fn remove_batch_focuses_previous_survivor_at_end_and_preserves_unselected_active() {
    let mut project = fixture(5);
    project.set_active_page(4).unwrap();
    project.remove_pages(&[1, 4, 5]).unwrap();
    assert_eq!(ids(&project), [2, 3]);
    assert_eq!(project.active_page(), 3);
    assert!(project.undo());
    project.set_active_page(2).unwrap();
    project.remove_pages(&[1, 3, 4, 5]).unwrap();
    assert_eq!(ids(&project), [2]);
    assert_eq!(project.active_page(), 2);
    project.snapshot().unwrap().validate().unwrap();
    assert_rejected(&mut project, |p| p.remove_pages(&[2]));
}

#[test]
fn move_batch_uses_original_slots_for_front_end_and_discontiguous_groups() {
    for (selected, to, expected) in [
        (vec![4, 2], 0, vec![2, 4, 1, 3, 5]),
        (vec![4, 2], 5, vec![1, 3, 5, 2, 4]),
        (vec![4, 2], 3, vec![1, 3, 2, 4, 5]),
        (vec![4, 2], 2, vec![1, 2, 4, 3, 5]),
        (vec![1], 5, vec![2, 3, 4, 5, 1]),
        (vec![5], 0, vec![5, 1, 2, 3, 4]),
    ] {
        let mut project = rich_fixture();
        let before = project.snapshot().unwrap();
        project.move_pages(&selected, to).unwrap();
        assert_eq!(ids(&project), expected);
        assert_eq!(project.active_page(), before.active);
        for page in &before.pages {
            assert_document_eq(&project.page(page.meta.id).unwrap().doc, &page.doc);
            assert_graph_eq(&project.page(page.meta.id).unwrap().graph, &page.graph);
        }
        assert_eq!(project.undo_pages.len(), 1);
        let after = project.snapshot().unwrap();
        assert!(project.undo());
        assert_pages_eq(&project, &before);
        assert!(!project.can_undo());
        assert!(project.redo());
        assert_pages_eq(&project, &after);
    }
}

#[test]
fn move_batch_exhaustively_preserves_group_order_and_active_identity_for_every_slot() {
    let original: Vec<PageId> = (1..=5).collect();
    for bits in 1..32 {
        let selected: Vec<_> = original
            .iter()
            .copied()
            .filter(|id| bits & (1 << (id - 1)) != 0)
            .collect();
        for to in 0..=original.len() {
            let expected: Vec<_> = original[..to]
                .iter()
                .filter(|id| !selected.contains(id))
                .chain(selected.iter())
                .chain(original[to..].iter().filter(|id| !selected.contains(id)))
                .copied()
                .collect();
            for active in &original {
                let mut project = fixture(5);
                project.set_active_page(*active).unwrap();
                let reverse_selection: Vec<_> = selected.iter().copied().rev().collect();
                project.move_pages(&reverse_selection, to).unwrap();
                assert_eq!(ids(&project), expected);
                assert_eq!(project.active_page(), *active);
                assert_eq!(project.can_undo(), expected != original);
                assert_eq!(project.is_modified(), expected != original);
                project.snapshot().unwrap().validate().unwrap();
            }
        }
    }
}

#[test]
fn noop_batch_move_preserves_pending_redo_and_does_not_dirty_the_project() {
    let mut project = fixture(5);
    project.duplicate_pages(&[2, 3]).unwrap();
    assert!(project.undo());
    let stamp = project.stamp();
    for to in [1, 2, 3] {
        project.move_pages(&[3, 2], to).unwrap();
        assert_eq!(project.stamp(), stamp);
        assert!(!project.is_modified());
        assert!(!project.can_undo());
        assert!(project.can_redo());
    }
    for to in 0..=5 {
        project.move_pages(&[5, 3, 2, 1, 4], to).unwrap();
        assert_eq!(project.stamp(), stamp);
    }
    assert!(project.redo());
    assert_eq!(ids(&project), [1, 2, 3, 6, 7, 4, 5]);
}

#[test]
fn invalid_batch_operations_are_atomic_and_keep_existing_redo() {
    let mut project = fixture(5);
    project.duplicate_pages(&[1]).unwrap();
    assert!(project.undo());
    for selection in [vec![], vec![1, 1], vec![3, 99], vec![0], vec![6]] {
        assert_rejected(&mut project, |p| p.duplicate_pages(&selection));
        assert_rejected(&mut project, |p| p.remove_pages(&selection));
        assert_rejected(&mut project, |p| p.move_pages(&selection, 0));
    }
    assert_rejected(&mut project, |p| p.remove_pages(&[1, 2, 3, 4, 5]));
    assert_rejected(&mut project, |p| p.move_pages(&[1, 2], 6));
    assert_rejected(&mut project, |p| p.move_pages(&[1, 2], usize::MAX));
    assert!(project.redo());
}

#[test]
fn batch_operations_reject_transactions_and_nonproject_editors() {
    let mut project = fixture(5);
    project.begin("Unfinished artwork edit");
    assert_rejected(&mut project, |p| p.duplicate_pages(&[1, 2]));
    assert_rejected(&mut project, |p| p.remove_pages(&[1, 2]));
    assert_rejected(&mut project, |p| p.move_pages(&[1, 2], 5));
    assert_rejected(&mut project, |p| p.move_pages(&[1, 2], 0));
    assert!(project.in_transaction());
    project.end();
    let mut single: ProjectEditor = Editor::new(Document::new(10, 10), None).into();
    let stamp = single.stamp();
    assert!(single.duplicate_pages(&[1]).is_err());
    assert!(single.remove_pages(&[1]).is_err());
    assert!(single.move_pages(&[1], 0).is_err());
    assert_eq!(single.stamp(), stamp);
    assert!(!single.can_undo());
}

#[test]
fn duplicate_batch_checks_page_area_count_and_allocator_limits_before_mutation() {
    let mut area = fixture(3).snapshot().unwrap();
    for page in &mut area.pages {
        page.doc = Document::new(20_000, 15_000);
        page.graph = Graph::new(page.doc.clone(), "Large page");
    }
    let mut area = ProjectEditor::open(area, Some("large.emu".into())).unwrap();
    assert_rejected(&mut area, |p| p.duplicate_pages(&[1, 2]));
    assert_rejected(&mut area, |p| p.duplicate_pages(&[1]));

    let mut exact_area = fixture(2).snapshot().unwrap();
    for page in &mut exact_area.pages {
        page.doc = Document::new(20_000, 12_500);
        page.graph = Graph::new(page.doc.clone(), "Large page");
    }
    let mut exact_area = ProjectEditor::open(exact_area, None).unwrap();
    exact_area.duplicate_pages(&[1, 2]).unwrap();
    exact_area.snapshot().unwrap().validate().unwrap();
    assert_rejected(&mut exact_area, |p| p.duplicate_pages(&[1]));

    let mut count = fixture(MAX_PAGES);
    assert_rejected(&mut count, |p| p.duplicate_pages(&[1]));

    let mut allocator = fixture(3);
    allocator.next_page_id = u64::MAX - 2;
    assert_rejected(&mut allocator, |p| p.duplicate_pages(&[1, 2]));
    let copied = allocator.duplicate_pages(&[1]).unwrap();
    assert_eq!(copied, [u64::MAX - 2]);
    allocator.snapshot().unwrap().validate().unwrap();
    assert_rejected(&mut allocator, |p| p.duplicate_pages(&[1]));
}

#[test]
fn duplicate_batch_validates_every_copy_before_creating_any_page_or_history() {
    let mut project = fixture(3);
    project.pages.get_mut(&3).unwrap().doc.design.transition_ms = 0;
    assert_rejected(&mut project, |p| p.duplicate_pages(&[1, 3]));
}

#[test]
fn batch_layout_history_interleaves_with_content_edits_and_invalidates_new_branches() {
    let mut project = fixture(5);
    project.set_active_page(3).unwrap();
    let node = project
        .execute(Command::AddNode {
            node: Box::new(Node::new(0, "Before", NodeKind::Fill { rgba: [255; 4] })),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let copies = project.duplicate_pages(&[1, 3]).unwrap();
    project
        .execute(Command::Rename {
            id: node,
            name: "Copy edit".into(),
        })
        .unwrap();
    project.remove_pages(&copies).unwrap();
    project.move_pages(&[1, 3], 5).unwrap();
    assert!(project.undo());
    assert_eq!(ids(&project), [1, 2, 3, 4, 5]);
    assert!(project.undo());
    assert_eq!(project.active_page(), copies[1]);
    assert_eq!(project.doc.node(node).unwrap().name, "Copy edit");
    assert!(project.undo());
    assert_eq!(project.doc.node(node).unwrap().name, "Before");
    assert!(project.undo());
    assert_eq!(ids(&project), [1, 2, 3, 4, 5]);
    assert_eq!(project.active_page(), 3);
    assert!(project.undo());
    assert!(project.doc.nodes.is_empty());
    assert!(!project.can_undo());
    for _ in 0..5 {
        assert!(project.redo());
    }
    assert_eq!(ids(&project), [2, 4, 5, 1, 3]);
    assert!(!project.can_redo());
    assert!(project.undo());
    project.remove_pages(&[2, 4]).unwrap();
    assert!(!project.can_redo());
    assert!(!project.redo());
    project.snapshot().unwrap().validate().unwrap();
}

#[test]
fn remove_batch_preserves_grouped_content_history_when_deleted_pages_are_restored() {
    let mut project = rich_fixture();
    let before = project.snapshot().unwrap();
    let documents = [1, 3]
        .into_iter()
        .map(|id| {
            let mut doc = project.page(id).unwrap().doc.clone();
            doc.design.speaker_notes = "Shared publication".into();
            (id, doc)
        })
        .collect();
    project
        .commit_documents(documents, "Publish shared brand")
        .unwrap();
    let published = project.snapshot().unwrap();
    project.remove_pages(&[3, 1]).unwrap();
    let deleted = project.snapshot().unwrap();
    assert!(project.undo());
    assert_pages_eq(&project, &published);
    assert!(project.undo());
    assert_pages_eq(&project, &before);
    assert!(!project.can_undo());
    assert!(project.redo());
    assert_pages_eq(&project, &published);
    assert!(project.redo());
    assert_pages_eq(&project, &deleted);
    assert!(!project.can_redo());
}

#[test]
fn batch_operations_preserve_diagram_project_kind() {
    let mut snapshot = fixture(3).snapshot().unwrap();
    snapshot.kind = ProjectKind::Diagram;
    let mut project = ProjectEditor::open(snapshot, None).unwrap();
    project.duplicate_pages(&[1, 3]).unwrap();
    project.move_pages(&[4, 5], 0).unwrap();
    project.remove_pages(&[1, 3]).unwrap();
    assert_eq!(project.kind(), Some(ProjectKind::Diagram));
    assert_eq!(ids(&project), [4, 5, 2]);
    project.snapshot().unwrap().validate().unwrap();
}
