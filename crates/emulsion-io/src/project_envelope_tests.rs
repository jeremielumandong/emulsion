//! Project version gates cover saved retired history, independently of pages.
use super::*;
use crate::native_relation::{LiveRelation, history_matches};
use emulsion_core::{
    Document, Node, NodeKind,
    drawing_guides::GuideKind,
    graph::{Commit, Graph},
    node::SmartEditable,
    project::ProjectEditor,
    storyboard::Panel,
};
use emulsion_raster::{Mask, Placement, Raster, blend::BlendSpace};
use serde_json::{Value, json};
use std::sync::Arc;

fn plain() -> Document {
    let mut doc = Document::new(2, 1);
    doc.nodes.push(Node::new(
        1,
        "Legacy drawing",
        NodeKind::Fill {
            rgba: [23, 45, 67, 255],
        },
    ));
    doc.next_id = 2;
    doc
}

fn smart() -> Document {
    let mut doc = Document::new(2, 1);
    doc.nodes.push(Node::smart(
        1,
        "Hidden Smart source",
        Arc::new(Raster::solid(2, 1, [0.25, 0.5, 0.75, 1.])),
        Vec::new(),
        Placement::default(),
    ));
    doc.nodes[0].visible = false;
    doc.next_id = 2;
    doc
}

fn opaque() -> Document {
    let mut doc = smart();
    if let NodeKind::Smart { editable, .. } = &mut doc.nodes[0].kind {
        *editable = Some(SmartEditable::Document {
            archive: Arc::new(b"opaque future source, deliberately not a ZIP".to_vec()),
            external: None,
        });
    }
    doc
}

fn disabled() -> Document {
    let mut doc = smart();
    if let NodeKind::Smart {
        filters_enabled, ..
    } = &mut doc.nodes[0].kind
    {
        *filters_enabled = false;
    }
    doc
}

fn projective() -> Document {
    let mut doc = smart();
    // A latent identity descriptor alone requires native v16 retention.
    doc.nodes[0].mask_transform = emulsion_core::mapping::Mapping2::Projective(
        emulsion_raster::projective::Projective2::IDENTITY,
    );
    doc
}

fn original() -> Document {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &[1, 27, 85, 1, 255, 67, 13, 0],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let bytes = Arc::new(bytes);
    let source = crate::original_image_png::png_source(&bytes).unwrap();
    let original = crate::original_image_data::capture(bytes, &source);
    let mut doc = smart();
    doc.nodes[0] = Node::smart(1, "Original", source, Vec::new(), Placement::default());
    if let NodeKind::Smart { original_image, .. } = &mut doc.nodes[0].kind {
        *original_image = Some(original);
    }
    doc
}

fn retired(documents: &[Document]) -> (Project, Vec<u64>) {
    let mut editor = ProjectEditor::new_project(ProjectKind::Storyboard, plain()).unwrap();
    let mut ids = Vec::new();
    for (index, doc) in documents.iter().enumerate() {
        ids.push(
            editor
                .insert_panels(
                    Some(1),
                    doc,
                    vec![(format!("Removed {index}"), Panel::new(0, 24))],
                    None,
                )
                .unwrap()[0],
        );
    }
    editor.create_board_version("Before removal").unwrap();
    editor.remove_pages(&ids).unwrap();
    (editor.snapshot().unwrap(), ids)
}

fn bytes(project: &Project) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    write_to(project, &mut out).unwrap();
    out.into_inner()
}

fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

fn envelope_version(bytes: &[u8]) -> u32 {
    let manifest: Value = serde_json::from_slice(&entry(bytes, "project.json")).unwrap();
    manifest["version"].as_u64().unwrap() as u32
}

fn rewrite(bytes: &[u8], mut edit: impl FnMut(&str, &mut Vec<u8>)) -> Vec<u8> {
    let mut input = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..input.len() {
        let mut file = input.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        edit(&name, &mut data);
        output
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        output.write_all(&data).unwrap();
    }
    output.finish().unwrap().into_inner()
}

fn assert_graph(before: &Graph, after: &Graph) {
    assert_eq!(before.head(), after.head());
    assert_eq!(before.branches(), after.branches());
    assert_eq!(before.len(), after.len());
    for (a, b) in before.commits().zip(after.commits()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.parents, b.parents);
        assert_eq!(a.name, b.name);
        assert_eq!(a.time, b.time);
        assert_eq!(a.auto, b.auto);
        assert_eq!(a.branch, b.branch);
        assert_eq!(history_matches(&a.doc, &b.doc), LiveRelation::Consistent);
    }
}

#[test]
fn legacy_and_live_only_projects_keep_envelope_one() {
    for kind in [ProjectKind::Design, ProjectKind::Storyboard] {
        for doc in [plain(), opaque(), disabled(), projective()] {
            let project = ProjectEditor::new_project(kind, doc)
                .unwrap()
                .snapshot()
                .unwrap();
            assert_eq!(required_version(&project), 1);
            assert_eq!(envelope_version(&bytes(&project)), 1);
        }
    }
    let mut raster_mask = plain();
    raster_mask.nodes[0].mask_properties.density = 0.5;
    let mut vector_mask = plain();
    vector_mask.nodes[0].vector_mask = Some(Default::default());
    let mut filter_mask = smart();
    if let NodeKind::Smart { filter_mask, .. } = &mut filter_mask.nodes[0].kind {
        *filter_mask = Some(emulsion_core::SmartFilterMask::new(Arc::new(Mask::empty(
            2, 1, 255,
        ))));
    }
    for (version, doc) in [
        (9, plain()),
        (10, raster_mask),
        (11, vector_mask),
        (12, filter_mask),
    ] {
        assert_eq!(ora::required_version(&doc), version);
        let (project, _) = retired(&[doc]);
        assert_eq!(required_version(&project), 1);
        let encoded = bytes(&project);
        assert_eq!(envelope_version(&encoded), 1);
        assert!(read_from(Cursor::new(&encoded)).is_ok());
    }

    let mut aids = plain();
    aids.colors = vec![[1, 2, 3]];
    aids.drawing_guides
        .guides
        .push(GuideKind::Grid { size: 8. });
    let (mut project, ids) = retired(&[plain()]);
    project
        .storyboard
        .as_mut()
        .unwrap()
        .versions
        .retired
        .insert(ids[0], Graph::new(aids, "Existing live aids"));
    // The separately implemented existing-field aid repair does not change
    // the envelope predicate. Older readers may still discard these aids.
    assert_eq!(required_version(&project), 1);
    assert_eq!(envelope_version(&bytes(&project)), 1);
}

#[test]
fn protected_retired_features_select_two_and_round_trip_without_opening_sources() {
    let mut profile = plain();
    profile.blend_space = BlendSpace::PhotoshopSrgbV1;
    for (native_version, doc) in [
        (9, opaque()),
        (13, original()),
        (14, profile),
        (15, disabled()),
        (16, projective()),
    ] {
        assert_eq!(ora::required_version(&doc), native_version);
        let (project, ids) = retired(&[doc]);
        assert_eq!(required_version(&project), RETIRED_PRESERVATION_VERSION);
        let encoded = bytes(&project);
        assert_eq!(envelope_version(&encoded), 2);
        let nested = entry(&encoded, &retired_entry(ids[0]));
        let manifest: Value = serde_json::from_slice(&entry(&nested, "emulsion.json")).unwrap();
        assert_eq!(manifest["version"], native_version);
        let before = &project.storyboard.as_ref().unwrap().versions.retired[&ids[0]];
        let strict = read_from(Cursor::new(&encoded)).unwrap();
        let reported = read_from_with_report(Cursor::new(&encoded)).unwrap();
        assert!(reported.report.is_empty());
        for restored in [&strict, &reported.project] {
            assert_graph(
                before,
                &restored.storyboard.as_ref().unwrap().versions.retired[&ids[0]],
            );
            let again = bytes(restored);
            assert_eq!(envelope_version(&again), 2);
            let twice = read_from(Cursor::new(again)).unwrap();
            assert_graph(
                before,
                &twice.storyboard.as_ref().unwrap().versions.retired[&ids[0]],
            );
        }
    }
}

#[test]
fn every_retained_commit_gates_even_off_branch_and_unreferenced_by_board_versions() {
    let mut graph = Graph::new(plain(), "Root");
    graph.create_branch("Alternative", 1).unwrap();
    graph.set_head("Alternative").unwrap();
    let source = opaque();
    let protected_id = graph.record(&source, "Hidden source", false).unwrap();
    graph.set_head("main").unwrap();
    let mut tip = plain();
    tip.nodes[0].name = "Legacy tip".into();
    graph.record(&tip, "No source here", false).unwrap();
    // Graph::from_parts also permits a saved detached commit. The gate must
    // inspect its map, rather than just walk ancestry or branch tips.
    let mut commits: Vec<_> = graph.commits().cloned().collect();
    commits.push(Commit {
        id: 4,
        parents: vec![1],
        name: "Detached protected state".into(),
        time: 17,
        auto: false,
        branch: "main".into(),
        doc: disabled(),
    });
    let graph = Graph::from_parts(commits, graph.branches().clone(), graph.head().into()).unwrap();
    let (mut project, ids) = retired(&[plain()]);
    let board = project.storyboard.as_mut().unwrap();
    assert!(
        board
            .versions
            .list
            .iter()
            .all(|version| version.pages[&ids[0]] != protected_id)
    );
    board.versions.retired.insert(ids[0], graph.clone());
    assert_eq!(required_version(&project), 2);
    let restored = read_from(Cursor::new(bytes(&project))).unwrap();
    assert_graph(
        &graph,
        &restored.storyboard.as_ref().unwrap().versions.retired[&ids[0]],
    );

    // Isolate the detached-commit case from the protected alternative branch.
    let mut commits: Vec<_> = graph.commits().cloned().collect();
    commits
        .iter_mut()
        .find(|commit| commit.id == protected_id)
        .unwrap()
        .doc = plain();
    let detached_only =
        Graph::from_parts(commits, graph.branches().clone(), graph.head().into()).unwrap();
    project
        .storyboard
        .as_mut()
        .unwrap()
        .versions
        .retired
        .insert(ids[0], detached_only.clone());
    assert_eq!(required_version(&project), 2);
    let restored = read_from(Cursor::new(bytes(&project))).unwrap();
    assert_graph(
        &detached_only,
        &restored.storyboard.as_ref().unwrap().versions.retired[&ids[0]],
    );
}

#[test]
fn mixed_project_keeps_legacy_recovery_report_and_protected_failure_policy() {
    let (project, ids) = retired(&[plain(), opaque()]);
    let encoded = bytes(&project);
    assert_eq!(envelope_version(&encoded), 2);
    let restored = read_from_with_report(Cursor::new(&encoded)).unwrap();
    assert!(restored.report.is_empty());
    for id in &ids {
        assert_graph(
            &project.storyboard.as_ref().unwrap().versions.retired[id],
            &restored
                .project
                .storyboard
                .as_ref()
                .unwrap()
                .versions
                .retired[id],
        );
    }
    let corrupt_graph = |bytes: &[u8]| {
        rewrite(bytes, |name, data| {
            if name == crate::history::GRAPH {
                let mut value: Value = serde_json::from_slice(data).unwrap();
                value["head"] = Value::Null;
                *data = serde_json::to_vec(&value).unwrap();
            }
        })
    };
    for (index, id) in ids.iter().enumerate() {
        let damaged = rewrite(&encoded, |name, data| {
            if name == retired_entry(*id) {
                *data = corrupt_graph(data);
            }
        });
        assert!(read_from(Cursor::new(&damaged)).is_err());
        let recovered = read_from_with_report(Cursor::new(damaged));
        if index == 0 {
            let recovered = recovered.unwrap();
            assert_eq!(
                recovered.report.diagnostics[0].code,
                ProjectReadDiagnosticCode::OmittedLegacyRetiredArchive
            );
            assert!(
                recovered
                    .project
                    .storyboard
                    .as_ref()
                    .unwrap()
                    .versions
                    .retired
                    .contains_key(&ids[1])
            );
        } else {
            assert!(recovered.is_err());
        }
    }
}

#[test]
fn supported_envelopes_share_all_read_boundaries_and_future_header_precedes_schema() {
    let project = ProjectEditor::new_project(ProjectKind::Design, plain())
        .unwrap()
        .snapshot()
        .unwrap();
    let original = bytes(&project);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("header.emu");
    for version in [1, 2] {
        let encoded = rewrite(&original, |name, data| {
            if name == "project.json" {
                let mut value: Value = serde_json::from_slice(data).unwrap();
                value["version"] = version.into();
                *data = serde_json::to_vec(&value).unwrap();
            }
        });
        std::fs::write(&path, &encoded).unwrap();
        assert!(read(&path).is_ok());
        assert!(read_with_report(&path).unwrap().report.is_empty());
        assert!(read_from(Cursor::new(&encoded)).is_ok());
        assert!(
            read_from_with_report(Cursor::new(&encoded))
                .unwrap()
                .report
                .is_empty()
        );
        assert!(cover(&path).is_ok());
        assert_eq!(envelope_version(&bytes(&read(&path).unwrap())), 1);
    }
    for header in [
        json!({"version": 3, "kind": "future"}),
        json!({"version": u64::MAX}),
    ] {
        let expected = header["version"].as_u64().unwrap().min(u64::from(u32::MAX)) as u32;
        let encoded = rewrite(&original, |name, data| {
            if name == "project.json" {
                *data = serde_json::to_vec(&header).unwrap();
            }
        });
        std::fs::write(&path, &encoded).unwrap();
        assert!(matches!(read(&path), Err(IoError::TooNew(v)) if v == expected));
        assert!(matches!(read_with_report(&path), Err(IoError::TooNew(v)) if v == expected));
        assert!(
            matches!(read_from(Cursor::new(&encoded)), Err(IoError::TooNew(v)) if v == expected)
        );
        assert!(matches!(
            read_from_with_report(Cursor::new(&encoded)),
            Err(IoError::TooNew(v)) if v == expected
        ));
        assert!(matches!(cover(&path), Err(IoError::TooNew(v)) if v == expected));
    }
    for header in [
        "{}",
        "{\"version\":0}",
        "{\"version\":-1}",
        "{\"version\":1.5}",
        "{\"version\":\"2\"}",
        "{\"version\":null}",
        "{\"version\":2",
        "{\"version\":2,\"kind\":\"future\"}",
    ] {
        let encoded = rewrite(&original, |name, data| {
            if name == "project.json" {
                *data = header.as_bytes().to_vec();
            }
        });
        assert!(
            matches!(read_from(Cursor::new(&encoded)), Err(IoError::Manifest(_))),
            "{header}"
        );
        assert!(
            matches!(
                read_from_with_report(Cursor::new(&encoded)),
                Err(IoError::Manifest(_))
            ),
            "{header}"
        );
    }
}

#[test]
fn failed_protected_retired_save_does_not_replace_destination_or_leave_staging() {
    let (mut project, ids) = retired(&[opaque()]);
    let mut invalid = opaque();
    if let NodeKind::Smart { editable, .. } = &mut invalid.nodes[0].kind {
        *editable = Some(SmartEditable::Document {
            archive: Arc::new(Vec::new()),
            external: None,
        });
    }
    project
        .storyboard
        .as_mut()
        .unwrap()
        .versions
        .retired
        .insert(ids[0], Graph::new(invalid, "Invalid source"));
    assert_eq!(required_version(&project), 2);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unchanged.emu");
    std::fs::write(&path, b"existing destination").unwrap();
    assert!(write(&project, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing destination");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn undo_redo_and_version_deletion_follow_saved_retirement_without_mutating_history() {
    let source = opaque();
    let mut editor = ProjectEditor::new_project(ProjectKind::Storyboard, source.clone()).unwrap();
    editor
        .insert_panels(
            Some(1),
            &plain(),
            vec![("Live".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap();
    let version = editor.create_board_version("Protected panel").unwrap();
    editor.remove_page(1).unwrap();
    let retired = editor.snapshot().unwrap();
    let graph = &retired.storyboard.as_ref().unwrap().versions.retired[&1];
    for _ in 0..3 {
        assert_eq!(required_version(&retired), 2);
    }
    assert_eq!(envelope_version(&bytes(&retired)), 2);
    assert!(editor.undo());
    let live = editor.snapshot().unwrap();
    assert!(
        live.storyboard
            .as_ref()
            .unwrap()
            .versions
            .retired
            .is_empty()
    );
    assert_eq!(required_version(&live), 1);
    assert_eq!(envelope_version(&bytes(&live)), 1);
    assert_graph(graph, &editor.page(1).unwrap().graph);
    let NodeKind::Smart {
        source: before,
        editable: Some(SmartEditable::Document { archive: a, .. }),
        ..
    } = &source.nodes[0].kind
    else {
        panic!("source")
    };
    let NodeKind::Smart {
        source: after,
        editable: Some(SmartEditable::Document { archive: b, .. }),
        ..
    } = &editor.page(1).unwrap().doc.nodes[0].kind
    else {
        panic!("source")
    };
    assert!(Arc::ptr_eq(before, after));
    assert!(Arc::ptr_eq(a, b));
    assert!(editor.redo());
    let again = editor.snapshot().unwrap();
    let again_graph = &again.storyboard.as_ref().unwrap().versions.retired[&1];
    assert_graph(graph, again_graph);
    assert_eq!(envelope_version(&bytes(&again)), 2);
    let mut before = graph.clone();
    let mut after = again_graph.clone();
    assert_eq!(
        before.record(&plain(), "Next", false),
        after.record(&plain(), "Next", false)
    );
    editor.delete_board_version(version).unwrap();
    let forgotten = editor.snapshot().unwrap();
    assert!(
        forgotten
            .storyboard
            .as_ref()
            .unwrap()
            .versions
            .retired
            .is_empty()
    );
    assert_eq!(required_version(&forgotten), 1);
}
