use super::*;
use crate::{Command, Node, NodeKind, command::Slot, design_interactions::Action};

fn fixture(count: usize) -> ProjectEditor {
    ProjectEditor::open(
        Project {
            kind: ProjectKind::Design,
            pages: (1..=count as u64)
                .map(|id| {
                    let doc = Document::new(100, 80);
                    ProjectPage {
                        meta: PageMeta {
                            id,
                            name: format!("Page {id}"),
                            bleed_mm: 3.,
                        },
                        graph: Graph::new(doc.clone(), "Opened"),
                        doc,
                    }
                })
                .collect(),
            active: 1,
            next_page_id: count as u64 + 1,
        },
        Some("resize.emu".into()),
    )
    .unwrap()
}

fn preview(project: &ProjectEditor, width: u32, height: u32) -> Document {
    let mut doc = project.doc.clone();
    doc.width = width;
    doc.height = height;
    doc
}

fn add(editor: &mut Editor, name: &str) -> crate::NodeId {
    editor
        .execute(Command::AddNode {
            node: Box::new(Node::new(0, name, NodeKind::Fill { rgba: [255; 4] })),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
}

fn assert_document_eq(actual: &Document, expected: &Document) {
    assert_eq!(actual, expected);
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

#[derive(Debug, PartialEq)]
struct EditorState {
    id: PageId,
    revision: u64,
    last_edit_order: u64,
    history_len: usize,
    can_redo: bool,
}

#[derive(Debug, PartialEq)]
struct ProjectState {
    undo_pages: usize,
    redo_pages: usize,
    last_page_edit: u64,
    modified: bool,
    can_redo: bool,
    editors: Vec<EditorState>,
}

fn state(project: &ProjectEditor) -> ProjectState {
    ProjectState {
        undo_pages: project.undo_pages.len(),
        redo_pages: project.redo_pages.len(),
        last_page_edit: project.last_page_edit,
        modified: project.is_modified(),
        can_redo: project.can_redo(),
        editors: project
            .pages
            .iter()
            .map(|(id, editor)| EditorState {
                id: *id,
                revision: editor.revision,
                last_edit_order: editor.last_edit_order,
                history_len: editor.history.len(),
                can_redo: editor.history.can_redo(),
            })
            .collect(),
    }
}

fn assert_rejected(
    project: &mut ProjectEditor,
    doc: Document,
    make_copy: bool,
    name: Option<String>,
) {
    let before = project.snapshot().unwrap();
    let stamp = project.stamp();
    let state_before = state(project);
    assert!(project.apply_resized_page(doc, make_copy, name).is_err());
    assert_pages_eq(project, &before);
    assert_eq!(project.next_page_id, before.next_page_id);
    assert_eq!(project.stamp(), stamp);
    assert_eq!(state(project), state_before);
}

#[test]
fn resize_original_is_one_edit_preserving_page_metadata_graph_and_source_history() {
    let mut project = fixture(2);
    let node = add(&mut project, "Artwork");
    project.create_version("Approved original").unwrap();
    let before = project.snapshot().unwrap();
    let before_stamp = project.stamp();
    let history_len = project.history.len();
    let doc = preview(&project, 360, 640);
    assert_eq!(
        project
            .apply_resized_page(doc.clone(), false, Some("Ignored rename".into()))
            .unwrap(),
        1
    );
    assert_document_eq(&project.doc, &doc);
    assert_eq!(project.history.len(), history_len + 1);
    assert_eq!(project.history.steps().next().unwrap().name, "Resize page");
    assert_eq!(project.page_list()[0], before.pages[0].meta);
    assert_graph_eq(&project.graph, &before.pages[0].graph);
    assert_document_eq(&project.page(2).unwrap().doc, &before.pages[1].doc);
    assert!(project.undo_pages.is_empty());
    assert!(project.undo());
    assert_pages_eq(&project, &before);
    assert_eq!(project.stamp(), before_stamp);
    assert!(project.redo());
    assert_document_eq(&project.doc, &doc);
    assert!(project.undo());
    assert!(project.undo());
    assert!(
        project.doc.node(node).is_none(),
        "the earlier source edit remains undoable"
    );
}

fn graph_fixture() -> ProjectEditor {
    let mut project = fixture(2).snapshot().unwrap();
    let mut editor = Editor::new(Document::new(100, 80), None);
    // Navigation is a terminal click action, so each independent destination
    // belongs to its own object rather than an invalid sequence of slide jumps.
    for (name, page) in [
        ("Self navigation", 1),
        ("External navigation", 2),
        ("Deleted destination", 99),
        ("Unavailable destination", u64::MAX),
    ] {
        let node = add(&mut editor, name);
        editor
            .doc
            .design
            .interactions
            .insert(node, vec![Action::Slide { page }]);
    }
    editor.doc.validate().unwrap();
    editor.create_version("Navigation original").unwrap();
    editor.branch("Alternate").unwrap();
    let historical = add(&mut editor, "Historical-only destination");
    editor
        .doc
        .design
        .interactions
        .insert(historical, vec![Action::Slide { page: 77 }]);
    editor.doc.validate().unwrap();
    editor.create_version("Historical deleted target").unwrap();
    editor.checkout("main").unwrap();
    editor.doc.design.speaker_notes = "Uncommitted source notes".into();
    project.pages[0].doc = editor.doc;
    project.pages[0].graph = editor.graph;
    ProjectEditor::open(project, Some("resize.emu".into())).unwrap()
}

#[test]
fn resize_copy_preserves_graph_and_every_external_link_and_remaps_self_navigation() {
    let mut project = graph_fixture();
    let before = project.snapshot().unwrap();
    let before_stamp = project.stamp();
    let source_history_len = project.history.len();
    let mut doc = preview(&project, 360, 640);
    let preview_only = Command::AddNode {
        node: Box::new(Node::new(
            0,
            "Preview-only destination",
            NodeKind::Fill { rgba: [255; 4] },
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    doc.design
        .interactions
        .insert(preview_only, vec![Action::Slide { page: 88 }]);
    doc.validate().unwrap();
    let copy_id = project.apply_resized_page(doc.clone(), true, None).unwrap();
    assert_eq!(copy_id, 3);
    assert_eq!(
        project.page_list().iter().map(|m| m.id).collect::<Vec<_>>(),
        [1, 3, 2]
    );
    assert_eq!(project.active_page(), copy_id);
    assert_eq!(project.page_list()[1].name, "Page 1 resized");
    assert_eq!(project.page_list()[1].bleed_mm, 3.);
    let map = BTreeMap::from([
        (1, 3),
        (2, 2),
        (77, 77),
        (88, 88),
        (99, 99),
        (u64::MAX, u64::MAX),
    ]);
    doc.design.remap_pages(&map);
    let mut graph = before.pages[0].graph.clone();
    graph.remap_pages(&map);
    assert_document_eq(&project.doc, &doc);
    assert_graph_eq(&project.graph, &graph);
    let destinations = |doc: &Document| {
        doc.design
            .interactions
            .values()
            .flatten()
            .filter_map(|action| match action {
                Action::Slide { page } => Some(*page),
                _ => None,
            })
            .collect::<HashSet<_>>()
    };
    assert_eq!(
        destinations(&project.doc),
        HashSet::from([3, 2, 99, u64::MAX, 88])
    );
    assert!(
        project
            .graph
            .commits()
            .any(|commit| destinations(&commit.doc).contains(&77))
    );
    assert!(
        project
            .graph
            .commits()
            .all(|commit| !destinations(&commit.doc).contains(&1))
    );
    assert_document_eq(
        &project.committed,
        &graph.commit(graph.head_branch().base).unwrap().doc,
    );
    assert!(project.history.is_empty());
    assert_eq!(project.path, Some("resize.emu".into()));
    assert_eq!(project.page(1).unwrap().history.len(), source_history_len);
    assert_document_eq(&project.page(1).unwrap().doc, &before.pages[0].doc);
    assert_graph_eq(&project.page(1).unwrap().graph, &before.pages[0].graph);
    assert!(project.is_modified());
    project.snapshot().unwrap().validate().unwrap();
    let after = project.snapshot().unwrap();
    assert!(project.undo());
    assert_pages_eq(&project, &before);
    assert_eq!(project.stamp(), before_stamp);
    assert!(!project.is_modified());
    assert!(
        !project.can_undo(),
        "a copy's initial resize must not leak into project undo"
    );
    assert!(project.redo());
    assert_pages_eq(&project, &after);
    assert_eq!((project.doc.width, project.doc.height), (360, 640));
    assert!(!project.can_redo());
    assert!(project.undo());
    assert!(project.redo());
    assert_pages_eq(&project, &after);
}

#[test]
fn resized_copy_history_interleaves_with_earlier_source_and_later_copy_edits() {
    let mut project = fixture(2);
    let node = add(&mut project, "Source edit");
    let source = project.doc.clone();
    let resized = preview(&project, 360, 640);
    let copy = project
        .apply_resized_page(resized.clone(), true, None)
        .unwrap();
    project
        .execute(Command::Rename {
            id: node,
            name: "Copy edit".into(),
        })
        .unwrap();
    let edited = project.doc.clone();
    project.set_active_page(2).unwrap();
    add(&mut project, "Other page edit");
    assert!(project.undo());
    assert_eq!(project.active_page(), 2);
    assert!(project.doc.nodes.is_empty());
    assert!(project.undo());
    assert_eq!(project.active_page(), copy);
    assert_document_eq(&project.doc, &resized);
    assert!(project.undo());
    assert_eq!(project.active_page(), 1);
    assert!(project.page(copy).is_none());
    assert_document_eq(&project.doc, &source);
    assert!(project.undo());
    assert!(project.doc.nodes.is_empty());
    assert!(!project.can_undo());
    assert!(project.redo());
    assert_document_eq(&project.doc, &source);
    assert!(project.redo());
    assert_eq!(project.active_page(), copy);
    assert_document_eq(&project.doc, &resized);
    assert!(project.redo());
    assert_document_eq(&project.doc, &edited);
    assert!(project.redo());
    assert_eq!(project.active_page(), 2);
    assert_eq!(project.doc.nodes[0].name, "Other page edit");
    assert!(!project.can_redo());
}

#[test]
fn resize_noop_preserves_existing_redo_and_unchanged_copy_is_still_one_page_step() {
    let mut project = fixture(1);
    add(&mut project, "Undo me");
    assert!(project.undo());
    let before = state(&project);
    let doc = project.doc.clone();
    project
        .apply_resized_page(doc.clone(), false, None)
        .unwrap();
    assert_eq!(state(&project), before);
    assert!(project.can_redo());
    let copy = project.apply_resized_page(doc.clone(), true, None).unwrap();
    assert!(project.history.is_empty());
    assert!(!project.can_redo());
    assert!(project.undo());
    assert!(project.page(copy).is_none());
    assert!(project.redo());
    assert_document_eq(&project.doc, &doc);
}

#[test]
fn new_resize_after_undo_invalidates_old_copy_redo_and_never_reuses_its_id() {
    let mut project = fixture(1);
    let doc = preview(&project, 200, 160);
    let first = project.apply_resized_page(doc.clone(), true, None).unwrap();
    assert!(project.undo());
    let second = project.apply_resized_page(doc, true, None).unwrap();
    assert!(second > first);
    assert!(project.page(first).is_none());
    assert!(!project.pages.contains_key(&first));
    assert!(!project.can_redo());
    assert!(!project.redo());
}

#[test]
fn copied_names_are_trimmed_unicode_safe_and_validated_before_history_changes() {
    let mut project = fixture(1);
    project.layout[0].name = "é".repeat(200);
    let doc = preview(&project, 200, 160);
    let copy = project.apply_resized_page(doc.clone(), true, None).unwrap();
    assert_eq!(project.page_list()[1].name.chars().count(), 200);
    assert!(project.page_list()[1].name.ends_with(" resized"));
    assert!(project.undo());
    for name in [" ".into(), "Bad\nname".into(), "é".repeat(201)] {
        assert_rejected(&mut project, doc.clone(), true, Some(name));
    }
    let next = project
        .apply_resized_page(doc, true, Some("  Mobile campaign  ".into()))
        .unwrap();
    assert!(next > copy);
    assert_eq!(project.page_list()[1].name, "Mobile campaign");
    project.snapshot().unwrap().validate().unwrap();
}

#[test]
fn malformed_previews_are_atomic_and_preserve_existing_redo() {
    let mut project = fixture(2);
    add(&mut project, "Undo me");
    assert!(project.undo());
    for make_copy in [false, true] {
        for (width, height) in [(0, 80), (30_001, 1), (30_000, 30_000), (u32::MAX, u32::MAX)] {
            let doc = preview(&project, width, height);
            assert_rejected(&mut project, doc, make_copy, None);
        }
        let mut doc = preview(&project, 200, 160);
        doc.design.transition_ms = 0;
        assert_rejected(&mut project, doc, make_copy, None);
        let mut doc = preview(&project, 200, 160);
        doc.nodes = vec![Node::group(9, "One"), Node::group(9, "Duplicate")];
        assert_rejected(&mut project, doc, make_copy, None);
    }
    assert!(project.redo());
    assert_eq!(project.doc.nodes[0].name, "Undo me");
}

fn large_fixture() -> ProjectEditor {
    let mut project = fixture(3).snapshot().unwrap();
    for page in &mut project.pages {
        page.doc = Document::new(20_000, 15_000);
        page.graph = Graph::new(page.doc.clone(), "Large page");
    }
    ProjectEditor::open(project, Some("large.emu".into())).unwrap()
}

#[test]
fn resize_limits_use_resulting_project_area_with_exact_boundary_and_are_atomic() {
    let mut project = large_fixture();
    let doc = preview(&project, 20_000, 20_000);
    project.apply_resized_page(doc, false, None).unwrap();
    project.snapshot().unwrap().validate().unwrap();
    let doc = preview(&project, 1, 1);
    assert_rejected(&mut project, doc, true, None);
    assert!(project.undo());
    let doc = preview(&project, 20_000, 5_000);
    project.apply_resized_page(doc, true, None).unwrap();
    project.snapshot().unwrap().validate().unwrap();
    assert!(project.undo());
    let doc = preview(&project, 20_000, 5_001);
    assert_rejected(&mut project, doc, true, None);
    // 900 MP of other pages plus a 100 MP copy: replacement must exclude
    // only the active page, never one of the unrelated large originals.
    let doc = preview(&project, 20_000, 5_000);
    project.apply_resized_page(doc, true, None).unwrap();
    let doc = preview(&project, 20_000, 5_001);
    assert_rejected(&mut project, doc, false, None);
}

#[test]
fn page_limit_blocks_copies_but_allows_replacement() {
    let mut project = fixture(MAX_PAGES);
    let doc = preview(&project, 200, 160);
    assert_rejected(&mut project, doc.clone(), true, None);
    project.apply_resized_page(doc, false, None).unwrap();
    assert_eq!(project.page_list().len(), MAX_PAGES);
    assert!(project.undo());
    assert_eq!((project.doc.width, project.doc.height), (100, 80));
}

#[test]
fn resize_rejects_id_collisions_invalid_layout_and_allocator_exhaustion_atomically() {
    let mut project = fixture(2);
    for allocator in [0, 1, 2, u64::MAX] {
        project.next_page_id = allocator;
        for make_copy in [false, true] {
            let doc = preview(&project, 200, 160);
            assert_rejected(&mut project, doc, make_copy, None);
        }
    }
    project.next_page_id = u64::MAX - 2;
    let doc = preview(&project, 200, 160);
    assert_eq!(
        project.apply_resized_page(doc, true, None).unwrap(),
        u64::MAX - 2
    );
    project.snapshot().unwrap().validate().unwrap();
    let doc = preview(&project, 300, 160);
    assert_rejected(&mut project, doc.clone(), true, None);
    project.apply_resized_page(doc, false, None).unwrap();

    let mut project = fixture(2);
    let doc = preview(&project, 200, 160);
    project.active = 99;
    assert_rejected(&mut project, doc, true, None);

    let mut project = fixture(2);
    project.layout[1].id = 1;
    let doc = preview(&project, 200, 160);
    assert_rejected(&mut project, doc, true, None);
    let mut project = fixture(2);
    project.pages.get_mut(&2).unwrap().doc.design.transition_ms = 0;
    let doc = preview(&project, 200, 160);
    assert_rejected(&mut project, doc, false, None);
}

#[test]
fn resize_rejects_active_transactions_non_design_projects_and_plain_documents() {
    let mut project = fixture(1);
    project.begin("Unfinished");
    for make_copy in [false, true] {
        let doc = preview(&project, 200, 160);
        assert_rejected(&mut project, doc, make_copy, None);
    }
    assert!(project.in_transaction());
    project.cancel();
    project.kind = Some(ProjectKind::Diagram);
    for make_copy in [false, true] {
        let doc = preview(&project, 200, 160);
        assert_rejected(&mut project, doc, make_copy, None);
    }
    let mut single: ProjectEditor = Editor::new(Document::new(100, 80), None).into();
    let before = single.stamp();
    for make_copy in [false, true] {
        assert!(
            single
                .apply_resized_page(Document::new(200, 160), make_copy, None)
                .is_err()
        );
    }
    assert_eq!(single.stamp(), before);
    assert!(!single.can_undo());
}

#[test]
fn locked_foreground_cannot_change_even_in_a_resized_copy() {
    for lock in 0..4 {
        let mut project = fixture(1);
        let node = add(&mut project, "Foreground");
        let n = project.doc.node_mut(node).unwrap();
        match lock {
            0 => n.locked = true,
            1 => n.locks.position = true,
            2 => n.locks.pixels = true,
            _ => n.locks.transparency = true,
        }
        for make_copy in [false, true] {
            let mut doc = preview(&project, 200, 160);
            doc.node_mut(node).unwrap().opacity = 0.5;
            assert_rejected(&mut project, doc, make_copy, None);
        }
        // Unchanged locked foreground does not prevent a canvas-only resize.
        let doc = preview(&project, 200, 160);
        project.apply_resized_page(doc, false, None).unwrap();
        assert!(project.undo());
        assert_eq!((project.doc.width, project.doc.height), (100, 80));
    }
}

#[test]
fn copied_locked_background_can_resize_without_losing_pixels_roles_or_locks() {
    let mut source = Editor::new(Document::new(100, 80), None);
    let pixels = std::sync::Arc::new(emulsion_raster::Raster::solid(100, 80, [0.2, 0.4, 0.8, 1.]));
    let image = crate::design_background::replace_image(&mut source, pixels.clone()).unwrap();
    let background = source.doc.design.page_background.unwrap();
    let group = background.image.unwrap().group;
    source.doc.node_mut(group).unwrap().locked = true;
    source.doc.node_mut(image).unwrap().locks.position = true;
    let mut project = fixture(1).snapshot().unwrap();
    project.pages[0].doc = source.doc.clone();
    project.pages[0].graph = Graph::new(source.doc.clone(), "Locked original");
    let mut project = ProjectEditor::open(project, None).unwrap();
    let mut doc = preview(&project, 200, 160);
    let NodeKind::Raster { placement, .. } = &mut doc.node_mut(image).unwrap().kind else {
        panic!("expected a raster background");
    };
    placement.scale_x *= 2.;
    placement.scale_y *= 2.;
    let copy = project.apply_resized_page(doc.clone(), true, None).unwrap();
    assert_document_eq(&project.doc, &doc);
    assert_eq!(project.doc.design.page_background, Some(background));
    assert!(project.doc.node(group).unwrap().locked);
    assert!(project.doc.node(image).unwrap().locks.position);
    let NodeKind::Raster { raster, .. } = &project.doc.node(image).unwrap().kind else {
        panic!("expected a raster background");
    };
    assert!(std::sync::Arc::ptr_eq(raster, &pixels));
    assert!(project.undo());
    assert!(project.page(copy).is_none());
    assert_document_eq(&project.doc, &source.doc);
    assert!(project.redo());
    assert_document_eq(&project.doc, &doc);
}

#[test]
fn resize_rejects_locked_ancestor_changes_and_corrupt_historical_documents() {
    let mut project = fixture(1);
    let group = project
        .execute(Command::AddNode {
            node: Box::new(Node::group(0, "Protected group")),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let child = project
        .execute(Command::AddNode {
            node: Box::new(Node::new(0, "Child", NodeKind::Fill { rgba: [255; 4] })),
            slot: Slot::top_of(Some(group)),
        })
        .unwrap()
        .unwrap();
    project.doc.node_mut(group).unwrap().locked = true;
    for make_copy in [false, true] {
        let mut doc = preview(&project, 200, 160);
        doc.node_mut(child).unwrap().opacity = 0.5;
        assert_rejected(&mut project, doc, make_copy, None);
    }

    let mut project = fixture(1);
    project.graph = Graph::new(Document::new(0, 80), "Corrupt historical canvas");
    let doc = preview(&project, 200, 160);
    assert_rejected(&mut project, doc, true, None);
}

#[test]
fn expired_copy_creation_never_exposes_a_stale_resize_undo_step() {
    let mut project = fixture(1);
    let source = project.doc.clone();
    let graph = project.graph.clone();
    let resized = preview(&project, 360, 640);
    let copy = project
        .apply_resized_page(resized.clone(), true, None)
        .unwrap();
    for index in 0..MAX_PAGE_STEPS {
        project
            .rename_page(copy, format!("Renamed copy {index}"), 3.)
            .unwrap();
    }
    assert_eq!(project.undo_pages.len(), MAX_PAGE_STEPS);
    for _ in 0..MAX_PAGE_STEPS {
        assert!(project.undo());
        assert_document_eq(&project.page(copy).unwrap().doc, &resized);
    }
    assert_eq!(project.active_page(), copy);
    assert_eq!(project.page_list()[1].name, "Page 1 resized");
    assert_document_eq(&project.page(1).unwrap().doc, &source);
    assert_graph_eq(&project.graph, &graph);
    assert!(project.history.is_empty());
    assert!(!project.can_undo());
    assert!(!project.undo());
    assert_document_eq(&project.doc, &resized);
    for _ in 0..MAX_PAGE_STEPS {
        assert!(project.redo());
        assert_document_eq(&project.doc, &resized);
    }
    assert_eq!(
        project.page_list()[1].name,
        format!("Renamed copy {}", MAX_PAGE_STEPS - 1)
    );
    assert!(!project.can_redo());
    project.snapshot().unwrap().validate().unwrap();
}
