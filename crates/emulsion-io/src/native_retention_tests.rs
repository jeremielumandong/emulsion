//! Adversarial native/retired admission regressions. These exercise the same
//! archive bytes through strict/report and document/full-reader boundaries.
use crate::{IoError, NativeFailureCode, ora, project};
use emulsion_core::drawing_guides::{DrawingGuides, GuideKind, GuideSet, Ruler};
use emulsion_core::{Document, Node, NodeKind, graph::Graph};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn doc() -> Document {
    let mut doc = Document::new(2, 1);
    doc.nodes.push(Node::new(
        1,
        "Fill",
        NodeKind::Fill {
            rgba: [1, 2, 3, 255],
        },
    ));
    doc.next_id = 2;
    doc
}
fn native(doc: &Document, graph: Option<&Graph>) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    ora::write_to(doc, graph, &mut out).unwrap();
    out.into_inner()
}
// Explicit supported protected headers isolate retention policy from a
// fixture's dynamically selected minimal writer version. Refresh hints so a
// header edit cannot accidentally turn a matching-fingerprint test into stale
// fingerprint coverage.
fn protected_headers(bytes: &[u8]) -> Vec<u8> {
    let bytes = json_edit(bytes, |_, root| {
        root["version"] = 13.into();
    });
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let manifest =
        ora::read_entry(&mut zip, "emulsion.json", ora::MAX_NATIVE_MANIFEST_BYTES).unwrap();
    let fingerprint = crate::history::fingerprint(&manifest);
    json_edit(&bytes, |name, root| {
        assert_eq!(root["version"], 13);
        if name == crate::history::GRAPH {
            if root["live"].is_string() {
                root["live"] = fingerprint.clone().into();
            }
            if let Some(working) = root.get_mut("working") {
                working["live"] = fingerprint.clone().into();
            }
        }
    })
}
fn protected_native(doc: &Document, graph: Option<&Graph>) -> Vec<u8> {
    protected_headers(&native(doc, graph))
}
fn rewrite(bytes: &[u8], mut edit: impl FnMut(&str, &mut Vec<u8>) -> bool) -> Vec<u8> {
    let mut input = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if edit(&name, &mut bytes) {
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&bytes).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}
fn json_edit(bytes: &[u8], mut edit: impl FnMut(&str, &mut serde_json::Value)) -> Vec<u8> {
    rewrite(bytes, |name, bytes| {
        if name == "emulsion.json" || name == crate::history::GRAPH {
            let mut value = serde_json::from_slice(bytes).unwrap();
            edit(name, &mut value);
            *bytes = serde_json::to_vec(&value).unwrap();
        }
        true
    })
}
fn each_doc(
    value: &mut serde_json::Value,
    name: &str,
    mut edit: impl FnMut(&mut serde_json::Value),
) {
    if name == "emulsion.json" {
        edit(value);
    } else {
        for commit in value["commits"].as_array_mut().unwrap() {
            edit(&mut commit["doc"]);
        }
        if let Some(working) = value.get_mut("working") {
            edit(&mut working["doc"]);
        }
    }
}
fn package() -> (Vec<u8>, u64, u64) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard::Panel;
    let mut editor = ProjectEditor::new_project(ProjectKind::Storyboard, doc()).unwrap();
    let blank = editor.storyboard().unwrap().blank_panel().unwrap();
    let id = editor
        .insert_panels(
            Some(1),
            &blank,
            vec![("Retired".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap()[0];
    let version = editor.create_board_version("Before removal").unwrap();
    editor.remove_page(id).unwrap();
    let mut out = Cursor::new(Vec::new());
    project::write_to(&editor.snapshot().unwrap(), &mut out).unwrap();
    (out.into_inner(), id, version)
}
fn nested(bytes: &[u8], id: u64, edit: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let name = format!("history/board/{id}.ora");
    let mut edit = Some(edit);
    rewrite(bytes, |entry, bytes| {
        if entry == name {
            *bytes = edit.take().unwrap()(bytes);
        }
        true
    })
}
fn cause(error: &IoError) -> &IoError {
    match error {
        IoError::ProjectEntry { source, .. } => cause(source),
        other => other,
    }
}
fn assert_native_code(error: IoError, expected: NativeFailureCode) {
    assert!(
        matches!(cause(&error), IoError::NativePreservation { code, .. } if *code == expected),
        "{error:?}"
    );
}

fn current_aids(document: &mut Document) {
    document.colors = (0..emulsion_core::document::MAX_PROJECT_COLORS)
        .map(|i| [i as u8, 255 - i as u8, (i * 3) as u8])
        .collect();
    document.drawing_guides = DrawingGuides {
        guides: vec![
            GuideKind::Off,
            GuideKind::Grid { size: 12.5 },
            GuideKind::Isometric { size: 24.25 },
            GuideKind::Perspective {
                points: vec![(-10.5, 2.25), (22.0, -30.0), (0.0, 40.0)],
            },
            GuideKind::Curvilinear {
                center: (-2.5, 3.5),
                radius: 15.25,
                five: true,
            },
        ],
        ruler: Some(Ruler {
            a: (-1.25, 0.5),
            b: (10.0, -4.0),
            enabled: false,
        }),
        sets: vec![
            GuideSet {
                name: "  Camera α  ".into(),
                guides: vec![GuideKind::Off, GuideKind::Grid { size: 5.0 }],
            },
            GuideSet {
                name: "Fish-eye".into(),
                guides: vec![GuideKind::Curvilinear {
                    center: (1.25, 2.5),
                    radius: 3.75,
                    five: false,
                }],
            },
        ],
        active_set: Some(1),
    };
    document.drawing_guides.validate().unwrap();
}

fn assert_aids(actual: &Document, expected: &Document) {
    assert_eq!(actual.colors, expected.colors);
    assert_eq!(actual.drawing_guides, expected.drawing_guides);
}

fn retired_graph(project: &emulsion_core::project::Project, id: u64) -> &Graph {
    &project.storyboard.as_ref().unwrap().versions.retired[&id]
}

fn assert_retired_graph(actual: &Graph, expected: &Graph, live: &Document) {
    assert_eq!(actual.len(), expected.len());
    assert_eq!(actual.head(), expected.head());
    assert_eq!(actual.branches(), expected.branches());
    for (actual_commit, expected_commit) in actual.commits().zip(expected.commits()) {
        assert_eq!(actual_commit.id, expected_commit.id);
        assert_eq!(actual_commit.parents, expected_commit.parents);
        assert_eq!(actual_commit.name, expected_commit.name);
        assert_eq!(actual_commit.time, expected_commit.time);
        assert_eq!(actual_commit.auto, expected_commit.auto);
        assert_eq!(actual_commit.branch, expected_commit.branch);
        assert_eq!(
            crate::native_relation::history_persistence_matches(
                &actual_commit.doc,
                &expected_commit.doc
            ),
            crate::native_relation::LiveRelation::Consistent
        );
        assert_aids(&actual.retired_document_at(actual_commit.id).unwrap(), live);
        if actual_commit.id == actual.head_branch().tip {
            assert_aids(&actual_commit.doc, live);
        } else {
            assert!(actual_commit.doc.colors.is_empty());
            assert_eq!(actual_commit.doc.drawing_guides, DrawingGuides::default());
        }
    }
}

fn assert_no_aid_history(bytes: &[u8], id: u64, commits: usize, expected_envelope: u32) {
    let mut outer = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let envelope: serde_json::Value =
        serde_json::from_slice(&ora::read_entry(&mut outer, "project.json", 1 << 20).unwrap())
            .unwrap();
    assert_eq!(
        envelope["version"], expected_envelope,
        "aid preservation must retain the envelope required by the fixture's content"
    );
    let bytes = ora::read_entry(&mut outer, &format!("history/board/{id}.ora"), 8 << 20).unwrap();
    let mut inner = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let history: serde_json::Value = serde_json::from_slice(
        &ora::read_entry(&mut inner, crate::history::GRAPH, 8 << 20).unwrap(),
    )
    .unwrap();
    assert!(
        history.get("working").is_none(),
        "aids must not invent working state"
    );
    let written_commits = history["commits"].as_array().unwrap();
    assert_eq!(written_commits.len(), commits);
    for commit in written_commits {
        assert!(commit["doc"].get("colors").is_none());
        assert!(commit["doc"].get("drawing_guides").is_none());
    }
}

#[test]
fn missing_retired_archive_requires_report_for_file_and_reader_apis() {
    let (bytes, id, version) = package();
    let entry = format!("history/board/{id}.ora");
    let bytes = rewrite(&bytes, |name, _| name != entry);
    let report = project::read_from_with_report(Cursor::new(&bytes))
        .unwrap()
        .report;
    assert_eq!(
        report.diagnostics,
        vec![project::ProjectReadDiagnostic {
            code: project::ProjectReadDiagnosticCode::MissingRetiredArchive,
            panel_id: id,
            entry,
            affected_version_ids: vec![version],
        }]
    );
    assert!(
        matches!(project::read_from(Cursor::new(&bytes)), Err(IoError::ProjectRecoveryRequired { report: actual }) if actual == report)
    );
    let path = std::env::temp_dir().join(format!(
        "emulsion-native-retention-missing-{}.emu",
        std::process::id()
    ));
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(project::read_with_report(&path).unwrap().report, report);
    assert!(matches!(
        project::read(&path),
        Err(IoError::ProjectRecoveryRequired { .. })
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn present_corrupt_retired_zip_is_never_missing_or_recovered() {
    let (bytes, id, _) = package();
    let bytes = nested(&bytes, id, |_| b"not a ZIP".to_vec());
    let error = project::read_from_with_report(Cursor::new(bytes))
        .err()
        .unwrap();
    assert!(matches!(cause(&error), IoError::Zip(_)));
}

#[test]
fn future_retired_header_retains_typed_too_new_cause() {
    let (bytes, id, _) = package();
    for entry in ["emulsion.json", crate::history::GRAPH] {
        let bytes = nested(&bytes, id, |bytes| {
            rewrite(bytes, |name, contents| {
                if name == entry {
                    let format = if entry == "emulsion.json" {
                        "emulsion"
                    } else {
                        "emulsion-history"
                    };
                    *contents = format!(r#"{{"format":"{format}","version":17}}"#).into_bytes();
                }
                true
            })
        });
        let error = project::read_from_with_report(Cursor::new(bytes))
            .err()
            .unwrap();
        assert!(matches!(cause(&error), IoError::TooNew(17)), "{error:?}");
    }
}

#[test]
fn known_legacy_retired_damage_recovers_only_with_report() {
    let (bytes, id, _) = package();
    let bytes = nested(&bytes, id, |bytes| {
        json_edit(bytes, |name, value| {
            value["version"] = 12.into();
            if name == crate::history::GRAPH {
                value["head"] = serde_json::Value::Null;
            }
        })
    });
    let opened = project::read_from_with_report(Cursor::new(&bytes)).unwrap();
    assert_eq!(
        opened.report.diagnostics[0].code,
        project::ProjectReadDiagnosticCode::OmittedLegacyRetiredArchive
    );
    assert!(matches!(
        project::read_from(Cursor::new(bytes)),
        Err(IoError::ProjectRecoveryRequired { .. })
    ));
}

#[test]
fn legacy_unknown_kind_is_standalone_recoverable_but_retired_indeterminate() {
    let document = doc();
    let bytes = native(&document, Some(&Graph::new(document.clone(), "Opened")));
    let bytes = json_edit(&bytes, |name, value| {
        value["version"] = 12.into();
        if name == crate::history::GRAPH {
            value["commits"][0]["doc"]["nodes"][0]["kind"]["type"] = "unknown-kind".into();
        }
    });
    let opened = ora::read_from(Cursor::new(&bytes)).unwrap();
    assert!(opened.graph.is_none());
    assert!(opened.history_error.is_some());
    let (package, id, _) = package();
    let package = nested(&package, id, |_| bytes);
    assert!(project::read_from_with_report(Cursor::new(package)).is_err());
}

#[test]
fn successful_wrong_kind_smart_fields_fail_before_ignored_field_deserialization() {
    let document = doc();
    let base = native(&document, Some(&Graph::new(document.clone(), "Opened")));
    for (field, value) in [
        ("editable", serde_json::Value::Null),
        ("source_document", serde_json::Value::Null),
        ("original_image", serde_json::Value::Null),
        ("filters", serde_json::json!([])),
        ("filter_styles", serde_json::json!([])),
        ("filters_enabled", true.into()),
        ("filter_mask", serde_json::Value::Null),
    ] {
        let bytes = json_edit(&base, |name, root| {
            each_doc(root, name, |doc| {
                doc["nodes"][0]["kind"][field] = value.clone();
            })
        });
        let error = ora::read_from(Cursor::new(&bytes)).err().unwrap();
        assert_native_code(error, NativeFailureCode::ProtectedFieldWrongKind);
        let path = std::env::temp_dir().join(format!(
            "emulsion-native-retention-{field}-{}.ora",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        assert_native_code(
            ora::read(&path).err().unwrap(),
            NativeFailureCode::ProtectedFieldWrongKind,
        );
        std::fs::remove_file(path).unwrap();
    }
    for field in ["source", "cache", "offset"] {
        let bytes = json_edit(&base, |name, root| {
            if name == crate::history::GRAPH {
                root["commits"][0]["doc"]["nodes"][0]["kind"][field] = serde_json::Value::Null;
            }
        });
        assert_native_code(
            ora::read_from(Cursor::new(bytes)).err().unwrap(),
            NativeFailureCode::ProtectedFieldWrongKind,
        );
    }
}

#[test]
fn protected_retired_live_aids_survive_graph_storage() {
    let (bytes, id, _) = package();
    let bytes = nested(&bytes, id, |bytes| {
        json_edit(&protected_headers(bytes), |name, root| {
            if name == "emulsion.json" {
                root["colors"] = serde_json::json!([[1, 2, 3]]);
            }
        })
    });
    let opened = project::read_from_with_report(Cursor::new(&bytes)).unwrap();
    assert!(opened.report.is_empty());
    let graph = &opened.project.storyboard.as_ref().unwrap().versions.retired[&id];
    assert_eq!(
        graph.commit(graph.head_branch().tip).unwrap().doc.colors,
        vec![[1, 2, 3]]
    );
    assert!(project::read_from(Cursor::new(bytes)).is_ok());
}

#[test]
fn forged_tip_fingerprint_cannot_replace_live_metadata_and_retired_rejects_it() {
    let mut document = doc();
    current_aids(&mut document);
    let base = protected_native(&document, Some(&Graph::new(document.clone(), "Opened")));
    let bytes = json_edit(&base, |name, root| {
        if name == crate::history::GRAPH {
            root["commits"][0]["doc"]["nodes"][0]["name"] = "forged historical name".into();
        }
    });
    let opened = ora::read_from(Cursor::new(&bytes)).unwrap();
    assert_eq!(opened.doc.nodes[0].name, "Fill");
    assert!(opened.graph.is_some());
    let (package, id, _) = package();
    let bytes = nested(&package, id, |_| bytes);
    assert_native_code(
        project::read_from_with_report(Cursor::new(bytes))
            .err()
            .unwrap(),
        NativeFailureCode::RetiredLiveNotRepresented,
    );
}

#[test]
fn standalone_history_restoration_overlays_live_drawing_aids() {
    let mut document = doc();
    document.colors = vec![[1, 2, 3]];
    let graph = Graph::new(document.clone(), "Opened");
    let opened = ora::read_from(Cursor::new(protected_native(&document, Some(&graph)))).unwrap();
    assert_eq!(opened.doc.colors, document.colors);
}

#[test]
fn protected_unselected_working_record_cannot_disappear_even_with_stale_hint() {
    let original = doc();
    let graph = Graph::new(original.clone(), "Opened");
    let mut working = original.clone();
    working.nodes[0].name = "unsaved work".into();
    let base = protected_native(&working, Some(&graph));
    let bytes = json_edit(&base, |name, root| {
        if name == crate::history::GRAPH {
            root["working"]["live"] = "stale fingerprint".into();
        }
    });
    assert_native_code(
        ora::read_from(Cursor::new(bytes)).err().unwrap(),
        NativeFailureCode::WorkingNotRepresented,
    );
}

#[test]
fn template_pack_reader_remains_strict_for_embedded_recovery_needed_project() {
    let (bytes, id, _) = package();
    let entry = format!("history/board/{id}.ora");
    let bytes = rewrite(&bytes, |name, _| name != entry);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let manifest = crate::template_pack::Manifest::new(
        crate::template_pack::Kind::Storyboard,
        "Recovery fixture".into(),
    );
    writer
        .start_file("emulsion-template.json", SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    writer
        .start_file("project.emu", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(&bytes).unwrap();
    let path = std::env::temp_dir().join(format!(
        "emulsion-retention-strict-pack-{}.emutemplate",
        std::process::id()
    ));
    std::fs::write(&path, writer.finish().unwrap().into_inner()).unwrap();
    assert!(matches!(
        crate::template_pack::read(&path),
        Err(IoError::ProjectRecoveryRequired { .. })
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn independent_protected_working_without_fingerprint_is_rejected() {
    let document = doc();
    let graph = Graph::new(document.clone(), "Opened");
    let mut working = document.clone();
    working.nodes[0].name = "independent working".into();
    let bytes = json_edit(&protected_native(&working, Some(&graph)), |name, root| {
        if name == crate::history::GRAPH {
            root["working"].as_object_mut().unwrap().remove("live");
        }
    });
    assert_native_code(
        ora::read_from(Cursor::new(bytes)).err().unwrap(),
        NativeFailureCode::WorkingNotRepresented,
    );
}

#[test]
fn opaque_future_nested_sources_survive_outer_version_nine_unopened() {
    use emulsion_core::node::SmartEditable;
    use std::sync::Arc;
    let original = doc();
    let future = json_edit(&native(&original, None), |_, root| {
        root["version"] = 99.into();
    });
    let mut outer = Document::new(2, 1);
    let source = Arc::new(emulsion_raster::Raster::solid(2, 1, [0.2, 0.4, 0.1, 1.0]));
    outer.nodes.push(Node::new(
        1,
        "Opaque source",
        NodeKind::Smart {
            editable: Some(SmartEditable::Document {
                archive: Arc::new(future.clone()),
                external: None,
            }),
            original_image: None,
            source: source.clone(),
            cache: source,
            filters: Vec::new(),
            filter_styles: Vec::new(),
            filters_enabled: true,
            filter_mask: None,
            offset: (0, 0),
            placement: Default::default(),
        },
    ));
    outer.next_id = 2;
    current_aids(&mut outer);
    let graph = Graph::new(outer.clone(), "Original opaque source");
    let bytes = json_edit(&native(&outer, Some(&graph)), |_, root| {
        root["version"] = 9.into();
    });
    let opened = ora::read_from(Cursor::new(&bytes)).unwrap();
    let NodeKind::Smart {
        editable: Some(SmartEditable::Document { archive, .. }),
        ..
    } = &opened.doc.nodes[0].kind
    else {
        panic!("lost opaque source");
    };
    assert_eq!(archive.as_slice(), future);
    let (package, id, _) = package();
    let package = nested(&package, id, |_| bytes);
    let opened = project::read_from_with_report(Cursor::new(package)).unwrap();
    assert!(opened.report.is_empty());
    let graph = &opened.project.storyboard.as_ref().unwrap().versions.retired[&id];
    let tip = graph.commit(graph.head_branch().tip).unwrap();
    assert_aids(&tip.doc, &outer);
    let NodeKind::Smart {
        editable: Some(SmartEditable::Document { archive, .. }),
        ..
    } = &tip.doc.nodes[0].kind
    else {
        panic!("lost retired opaque source");
    };
    assert_eq!(archive.as_slice(), future);
    let mut bytes = Cursor::new(Vec::new());
    project::write_to(&opened.project, &mut bytes).unwrap();
    // Retired opaque sources require envelope two even with native version nine.
    assert_no_aid_history(bytes.get_ref(), id, graph.len(), 2);
    let reopened = project::read_from(Cursor::new(bytes.into_inner())).unwrap();
    let graph = retired_graph(&reopened, id);
    let tip = &graph.commit(graph.head_branch().tip).unwrap().doc;
    assert_aids(tip, &outer);
    let NodeKind::Smart {
        editable: Some(SmartEditable::Document { archive, .. }),
        ..
    } = &tip.nodes[0].kind
    else {
        panic!("lost resaved opaque source");
    };
    assert_eq!(archive.as_slice(), future);
}

#[test]
fn equal_working_content_is_retained_with_stale_or_missing_hint() {
    let mut document = doc();
    current_aids(&mut document);
    let bytes = protected_native(&document, Some(&Graph::new(document.clone(), "Opened")));
    for hint in [Some("stale"), None] {
        let bytes = json_edit(&bytes, |name, root| {
            if name == crate::history::GRAPH {
                let mut working = serde_json::json!({"doc":root["commits"][0]["doc"]});
                if let Some(hint) = hint {
                    working["live"] = hint.into();
                }
                root["working"] = working;
            }
        });
        assert!(
            ora::read_from(Cursor::new(&bytes))
                .unwrap()
                .history_error
                .is_none()
        );
        let (package, id, _) = package();
        let package = nested(&package, id, |_| bytes);
        assert!(
            project::read_from_with_report(Cursor::new(package))
                .unwrap()
                .report
                .is_empty()
        );
    }
}

#[test]
fn raw_hidden_rgb_mismatch_blocks_forged_fingerprint_substitution() {
    use std::sync::Arc;
    let mut document = Document::new(2, 1);
    document.nodes.push(Node::raster(
        1,
        "Transparent",
        Arc::new(emulsion_raster::Raster::empty(2, 1, [0; 4])),
        Default::default(),
    ));
    document.next_id = 2;
    document.selection = Some(Arc::new(emulsion_raster::Mask::empty(2, 1, 128)));
    current_aids(&mut document);
    let graph = Graph::new(document.clone(), "Opened");
    let base = protected_native(&document, Some(&graph));
    let mut zip = ZipArchive::new(Cursor::new(&base)).unwrap();
    let live: serde_json::Value = serde_json::from_slice(
        &ora::read_entry(&mut zip, "emulsion.json", ora::MAX_NATIVE_MANIFEST_BYTES).unwrap(),
    )
    .unwrap();
    let source = live["nodes"][0]["kind"]["src"].as_str().unwrap();
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 17, 3, 0, 11, 2, 9, 0])
            .unwrap();
    }
    let bytes = rewrite(&base, |name, bytes| {
        if name == source {
            *bytes = png.clone();
        }
        true
    });
    let opened = ora::read_from(Cursor::new(&bytes)).unwrap();
    assert!(
        opened.doc.selection.is_none(),
        "raw PNG mismatch must leave the decoded live document in place"
    );
    assert!(opened.graph.is_some());
    let (package, id, _) = package();
    let package = nested(&package, id, |_| bytes);
    assert_native_code(
        project::read_from_with_report(Cursor::new(package))
            .err()
            .unwrap(),
        NativeFailureCode::RetiredLiveNotRepresented,
    );
}

#[test]
fn malformed_legacy_snapshot_shapes_are_not_positive_retired_recovery_evidence() {
    let original = doc();
    let base = native(&original, Some(&Graph::new(original.clone(), "Opened")));
    let values = [
        serde_json::Value::Null,
        true.into(),
        7.into(),
        "unknown".into(),
        serde_json::json!([]),
        serde_json::json!([{"source_document":"opaque", "projective":null}]),
    ];
    for location in ["kind", "node", "doc", "commit", "commits", "working"] {
        for value in &values {
            // A correctly shaped empty commit list is known legacy graph
            // damage, covered by the explicit recover-with-report case below.
            if location == "commits" && value.as_array().is_some_and(Vec::is_empty) {
                continue;
            }
            let bytes = json_edit(&base, |name, root| {
                root["version"] = 12.into();
                if name == crate::history::GRAPH {
                    match location {
                        "kind" => root["commits"][0]["doc"]["nodes"][0]["kind"] = value.clone(),
                        "node" => root["commits"][0]["doc"]["nodes"][0] = value.clone(),
                        "doc" => root["commits"][0]["doc"] = value.clone(),
                        "commit" => root["commits"][0] = value.clone(),
                        "commits" => root["commits"] = value.clone(),
                        _ => root["working"] = serde_json::json!({"doc":value}),
                    }
                }
            });
            let (package, id, _) = package();
            let package = nested(&package, id, |_| bytes);
            assert!(
                project::read_from_with_report(Cursor::new(package)).is_err(),
                "{location}: {value}"
            );
        }
    }
}

#[test]
fn empty_legacy_commit_list_recovers_with_report_and_bare_api_requires_it() {
    let original = doc();
    let bytes = json_edit(
        &native(&original, Some(&Graph::new(original.clone(), "Opened"))),
        |name, root| {
            root["version"] = 12.into();
            if name == crate::history::GRAPH {
                root["commits"] = serde_json::json!([]);
            }
        },
    );
    let standalone = ora::read_from(Cursor::new(&bytes)).unwrap();
    assert!(standalone.graph.is_none());
    assert!(standalone.history_error.is_some());
    let (package, id, version) = package();
    let package = nested(&package, id, |_| bytes);
    let report = project::read_from_with_report(Cursor::new(&package))
        .unwrap()
        .report;
    assert_eq!(
        report.diagnostics,
        vec![project::ProjectReadDiagnostic {
            code: project::ProjectReadDiagnosticCode::OmittedLegacyRetiredArchive,
            panel_id: id,
            entry: format!("history/board/{id}.ora"),
            affected_version_ids: vec![version],
        }]
    );
    assert!(
        matches!(project::read_from(Cursor::new(package)), Err(IoError::ProjectRecoveryRequired { report: actual }) if actual == report)
    );
}

#[test]
fn working_writer_retains_allocator_depth_and_info_changes_hidden_by_document_equality() {
    for mutate in [
        (|doc: &mut Document| doc.next_id = 99) as fn(&mut Document),
        |doc| doc.source_depth = 16,
        |doc| {
            doc.info = Some(emulsion_core::document::ImageInfo {
                model: "Retained camera".into(),
                ..Default::default()
            })
        },
    ] {
        let original = doc();
        let graph = Graph::new(original.clone(), "Opened");
        let mut working = original;
        mutate(&mut working);
        let bytes = protected_native(&working, Some(&graph));
        let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let history: serde_json::Value = serde_json::from_slice(
            &ora::read_entry(
                &mut zip,
                crate::history::GRAPH,
                ora::MAX_NATIVE_MANIFEST_BYTES,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            history.get("working").is_some(),
            "writer must retain changed historical metadata"
        );
        let opened = ora::read_from(Cursor::new(bytes)).unwrap();
        assert_eq!(opened.doc.next_id, working.next_id);
        assert_eq!(opened.doc.source_depth, working.source_depth);
        assert_eq!(opened.doc.info, working.info);
    }
}

#[test]
fn working_writer_retains_active_smart_cache_and_offset_without_authenticating_rendering() {
    use std::sync::Arc;
    let mut original = Document::new(2, 1);
    let source = Arc::new(emulsion_raster::Raster::solid(2, 1, [0.3, 0.1, 0.2, 1.0]));
    let filters = vec![emulsion_filters::Filter::Invert];
    let (cache, offset) = emulsion_core::smart::render_stack(&source, &filters, &[], true);
    original.nodes.push(Node::new(
        1,
        "Active Smart",
        NodeKind::Smart {
            editable: None,
            original_image: None,
            source,
            filters,
            filter_styles: Vec::new(),
            filters_enabled: true,
            filter_mask: None,
            placement: Default::default(),
            cache,
            offset,
        },
    ));
    original.next_id = 2;
    let graph = Graph::new(original.clone(), "Opened");
    for change_cache in [true, false] {
        let mut working = original.clone();
        let NodeKind::Smart { cache, offset, .. } = &mut working.nodes[0].kind else {
            unreachable!()
        };
        if change_cache {
            *cache = Arc::new(emulsion_raster::Raster::solid(2, 1, [0.2, 0.4, 0.1, 1.0]));
        } else {
            *offset = (1, 0);
        }
        // Invert itself honestly requests v14; do not lower its feature gate.
        let bytes = native(&working, Some(&graph));
        let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let history: serde_json::Value = serde_json::from_slice(
            &ora::read_entry(
                &mut zip,
                crate::history::GRAPH,
                ora::MAX_NATIVE_MANIFEST_BYTES,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(history["version"], 14);
        assert!(history.get("working").is_some());
        let opened = ora::read_from(Cursor::new(bytes)).unwrap();
        assert_eq!(
            crate::native_relation::history_persistence_matches(&opened.doc, &working),
            crate::native_relation::LiveRelation::Consistent
        );
    }
}

#[test]
fn retired_write_refuses_invalid_aids_before_touching_output() {
    let (bytes, id, _) = package();
    let mut project = project::read_from(Cursor::new(bytes)).unwrap();
    let mut protected = doc();
    protected.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
    // An earlier protected commit protects the complete archive even when the
    // current tip alone would emit the supported legacy format.
    let mut graph = Graph::new(protected.clone(), "Protected past");
    let mut tip = doc();
    tip.colors = vec![[1, 2, 3]; emulsion_core::document::MAX_PROJECT_COLORS + 1];
    graph
        .record(&tip, "Legacy-looking tip with live aids", false)
        .unwrap();
    project
        .storyboard
        .as_mut()
        .unwrap()
        .versions
        .retired
        .insert(id, graph);
    let mut out = WriteSpy::default();
    assert_native_code(
        project::write_to(&project, &mut out).err().unwrap(),
        NativeFailureCode::RetiredLiveInvalidColors,
    );
    assert_eq!((out.writes, out.seeks), (0, 0));
    let path = std::env::temp_dir().join(format!(
        "emulsion-retention-write-preserves-{}.emu",
        std::process::id()
    ));
    std::fs::write(&path, b"existing destination must survive").unwrap();
    assert_native_code(
        project::write(&project, &path).err().unwrap(),
        NativeFailureCode::RetiredLiveInvalidColors,
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"existing destination must survive"
    );
    std::fs::remove_file(path).unwrap();
    // Canonical protected graph-only storage remains saveable and readable.
    project
        .storyboard
        .as_mut()
        .unwrap()
        .versions
        .retired
        .insert(id, Graph::new(protected, "Protected canonical"));
    let mut out = Cursor::new(Vec::new());
    project::write_to(&project, &mut out).unwrap();
    assert!(
        project::read_from_with_report(Cursor::new(out.into_inner()))
            .unwrap()
            .report
            .is_empty()
    );
}

#[test]
fn retired_aids_round_trip_legacy_and_protected_headers_through_all_project_apis() {
    let (package, id, _) = package();
    let mut live = doc();
    let mut graph = Graph::new(live.clone(), "Root");
    live.nodes[0].name = "Main artwork".into();
    let main = graph.record(&live, "Main", false).unwrap();
    graph.create_branch("Alternative", 1).unwrap();
    graph.set_head("Alternative").unwrap();
    live.nodes[0].name = "Alternative artwork".into();
    graph.record(&live, "Alternative", false).unwrap();
    graph.set_head("main").unwrap();
    live = graph.commit(main).unwrap().doc.clone();
    current_aids(&mut live);
    for version in [12, 13, 14, 15] {
        let bytes = json_edit(&native(&live, Some(&graph)), |_, root| {
            root["version"] = version.into();
        });
        let bytes = nested(&package, id, |_| bytes);
        let mut opened = project::read_from_with_report(Cursor::new(&bytes)).unwrap();
        assert!(opened.report.is_empty(), "version {version}");
        assert_retired_graph(retired_graph(&opened.project, id), &graph, &live);
        assert_retired_graph(
            retired_graph(&project::read_from(Cursor::new(&bytes)).unwrap(), id),
            &graph,
            &live,
        );
        let path = std::env::temp_dir().join(format!(
            "emulsion-retired-aids-v{version}-{}.emu",
            std::process::id()
        ));
        std::fs::write(&path, &bytes).unwrap();
        assert_retired_graph(
            retired_graph(&project::read(&path).unwrap(), id),
            &graph,
            &live,
        );
        assert!(project::read_with_report(&path).unwrap().report.is_empty());
        for _ in 0..2 {
            project::write(&opened.project, &path).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            // Only the input headers changed; the fill/aid content rewrites
            // to its legacy minimum and still requires envelope one.
            assert_no_aid_history(&bytes, id, graph.len(), 1);
            opened = project::read_with_report(&path).unwrap();
            assert!(opened.report.is_empty());
            assert_retired_graph(retired_graph(&opened.project, id), &graph, &live);
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn many_retired_commits_keep_one_exact_large_aid_payload() {
    let (bytes, id, _) = package();
    let mut project = project::read_from(Cursor::new(bytes)).unwrap();
    let mut live = doc();
    let mut graph = Graph::new(live.clone(), "Root");
    for n in 1..64 {
        live.nodes[0].name = format!("Artwork {n}");
        graph.record(&live, format!("Version {n}"), false).unwrap();
    }
    current_aids(&mut live);
    // Large enough to expose per-commit copying, without approaching archive
    // budgets or allocating a manifest-sized fixture.
    live.drawing_guides.sets[0].name = "Perspective α ".repeat(4096);
    graph.set_retired_live_aids(live.colors.clone(), live.drawing_guides.clone());
    project
        .storyboard
        .as_mut()
        .unwrap()
        .versions
        .retired
        .insert(id, graph.clone());
    let mut bytes = Cursor::new(Vec::new());
    project::write_to(&project, &mut bytes).unwrap();
    assert_no_aid_history(bytes.get_ref(), id, graph.len(), 1);
    let reopened = project::read_from(Cursor::new(bytes.into_inner())).unwrap();
    let restored = retired_graph(&reopened, id);
    assert_retired_graph(restored, &graph, &live);
    assert_eq!(
        restored
            .commits()
            .map(|commit| commit
                .doc
                .drawing_guides
                .sets
                .iter()
                .map(|set| set.name.len())
                .sum::<usize>())
            .sum::<usize>(),
        live.drawing_guides
            .sets
            .iter()
            .map(|set| set.name.len())
            .sum::<usize>(),
        "retained aid allocation must not multiply by the history length"
    );
}

#[test]
fn latest_and_cleared_aids_survive_removal_collection_and_serialized_version_retrieval() {
    use emulsion_core::Command;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard_versions::Baseline;

    for clear in [false, true] {
        let mut session =
            ProjectEditor::new_project(ProjectKind::Storyboard, original_png_document(255))
                .unwrap();
        let id = session.duplicate_page(1).unwrap();
        session.set_active_page(id).unwrap();
        current_aids(&mut session.doc);
        session
            .execute(Command::Rename {
                id: 1,
                name: "Version artwork".into(),
            })
            .unwrap();
        let version = session.create_board_version("Before removal").unwrap();
        session
            .execute(Command::Rename {
                id: 1,
                name: "Latest artwork".into(),
            })
            .unwrap();
        session.commit("Latest artwork", false).unwrap();
        let commits = session.graph.len();
        if clear {
            session.doc.colors.clear();
            session.doc.drawing_guides = DrawingGuides::default();
        } else {
            session.doc.colors = vec![[98, 76, 54], [32, 10, 12]];
            session.doc.drawing_guides.sets[0].name = "Changed after the artwork commit".into();
        }
        let expected = session.doc.clone();
        assert!(session.commit("Aid-only change", false).is_none());
        session.remove_page(id).unwrap();
        for collected in [false, true] {
            if collected {
                // Public page edits expire the 100-step page undo retention,
                // exercising editor collection as well as undo-retained saves.
                for n in 0..101 {
                    session
                        .rename_page(1, format!("Remaining panel {n}"), 0.)
                        .unwrap();
                }
            }
            let snapshot = session.snapshot().unwrap();
            let graph = retired_graph(&snapshot, id);
            assert_eq!(graph.len(), commits);
            assert_aids(
                &graph.commit(graph.head_branch().tip).unwrap().doc,
                &expected,
            );
            let mut bytes = Cursor::new(Vec::new());
            project::write_to(&snapshot, &mut bytes).unwrap();
            // Clearing aids does not remove the protected original PNG resource.
            assert_no_aid_history(bytes.get_ref(), id, commits, 2);
            let opened = project::read_from_with_report(Cursor::new(bytes.into_inner())).unwrap();
            assert!(opened.report.is_empty());
            let reopened = ProjectEditor::open(opened.project, None).unwrap();
            let state = reopened.board_state(Baseline::Version(version)).unwrap();
            let historical = state.doc(id).unwrap();
            assert_eq!(historical.nodes[0].name, "Version artwork");
            assert_aids(historical, &expected);
            let path = std::env::temp_dir().join(format!(
                "emulsion-latest-retired-aids-{clear}-{collected}-{}.emu",
                std::process::id()
            ));
            project::write(&reopened.snapshot().unwrap(), &path).unwrap();
            let reopened = ProjectEditor::open(project::read(&path).unwrap(), None).unwrap();
            let state = reopened.board_state(Baseline::Version(version)).unwrap();
            assert_aids(state.doc(id).unwrap(), &expected);
            assert_eq!(state.doc(id).unwrap().nodes[0].name, "Version artwork");
            std::fs::remove_file(path).unwrap();
        }
    }
}

fn invalid_finite_guides() -> Vec<DrawingGuides> {
    use emulsion_core::drawing_guides::{MAX_GUIDE_SETS, MAX_GUIDES};
    let guide = GuideKind::Grid { size: 1.0 };
    let mut cases = vec![
        DrawingGuides {
            guides: vec![GuideKind::Off; MAX_GUIDES + 1],
            ..Default::default()
        },
        DrawingGuides {
            sets: vec![
                GuideSet {
                    name: "Set".into(),
                    guides: vec![guide.clone()]
                };
                MAX_GUIDE_SETS + 1
            ],
            ..Default::default()
        },
        DrawingGuides {
            sets: vec![GuideSet {
                name: "Set".into(),
                guides: vec![GuideKind::Off; MAX_GUIDES + 1],
            }],
            ..Default::default()
        },
        DrawingGuides {
            sets: vec![GuideSet {
                name: "  \t".into(),
                guides: vec![guide],
            }],
            ..Default::default()
        },
        DrawingGuides {
            active_set: Some(0),
            ..Default::default()
        },
    ];
    for guide in [
        GuideKind::Grid { size: 0.0 },
        GuideKind::Isometric { size: -1.0 },
        GuideKind::Perspective { points: vec![] },
        GuideKind::Perspective {
            points: vec![(0.0, 0.0); 4],
        },
        GuideKind::Curvilinear {
            center: (0.0, 0.0),
            radius: 0.0,
            five: false,
        },
    ] {
        cases.push(DrawingGuides {
            guides: vec![guide.clone()],
            ..Default::default()
        });
        cases.push(DrawingGuides {
            sets: vec![GuideSet {
                name: "Invalid saved geometry".into(),
                guides: vec![guide],
            }],
            ..Default::default()
        });
    }
    cases
}

#[test]
fn invalid_raw_retired_aids_are_typed_or_explicitly_recovered_before_sanitation() {
    let (package, id, version) = package();
    let mut document = doc();
    current_aids(&mut document);
    let base = native(&document, Some(&Graph::new(document.clone(), "Root")));
    let mut cases = vec![(
        "colors",
        serde_json::json!(vec![
            [1, 2, 3];
            emulsion_core::document::MAX_PROJECT_COLORS + 1
        ]),
        NativeFailureCode::RetiredLiveInvalidColors,
    )];
    cases.extend(invalid_finite_guides().into_iter().map(|guides| {
        (
            "drawing_guides",
            serde_json::to_value(guides).unwrap(),
            NativeFailureCode::RetiredLiveInvalidDrawingGuides,
        )
    }));
    for (field, value, code) in cases {
        for protected in [false, true] {
            let bytes = json_edit(&base, |name, root| {
                root["version"] = if protected { 13 } else { 12 }.into();
                if name == "emulsion.json" {
                    root[field] = value.clone();
                }
            });
            let bytes = nested(&package, id, |_| bytes);
            if protected {
                assert_native_code(
                    project::read_from_with_report(Cursor::new(&bytes))
                        .err()
                        .unwrap(),
                    code,
                );
                assert_native_code(project::read_from(Cursor::new(bytes)).err().unwrap(), code);
            } else {
                let opened = project::read_from_with_report(Cursor::new(&bytes)).unwrap();
                assert_eq!(
                    opened.report.diagnostics,
                    vec![project::ProjectReadDiagnostic {
                        code: project::ProjectReadDiagnosticCode::SanitizedLegacyRetiredAids,
                        panel_id: id,
                        entry: format!("history/board/{id}.ora"),
                        affected_version_ids: vec![version],
                    }]
                );
                let restored = retired_graph(&opened.project, id);
                let tip = &restored.commit(restored.head_branch().tip).unwrap().doc;
                if field == "colors" {
                    assert_eq!(
                        tip.colors,
                        vec![[1, 2, 3]; emulsion_core::document::MAX_PROJECT_COLORS]
                    );
                    assert_eq!(tip.drawing_guides, document.drawing_guides);
                    let path = std::env::temp_dir().join(format!(
                        "emulsion-retired-sanitized-aids-{}.emu",
                        std::process::id()
                    ));
                    std::fs::write(&path, &bytes).unwrap();
                    assert_eq!(
                        project::read_with_report(&path).unwrap().report,
                        opened.report
                    );
                    assert!(
                        matches!(project::read(&path), Err(IoError::ProjectRecoveryRequired { report }) if report == opened.report)
                    );
                    std::fs::remove_file(path).unwrap();
                } else {
                    assert_eq!(tip.colors, document.colors);
                    assert_eq!(tip.drawing_guides, DrawingGuides::default());
                }
                assert!(
                    matches!(project::read_from(Cursor::new(bytes)), Err(IoError::ProjectRecoveryRequired { report }) if report == opened.report)
                );
            }
        }
    }
}

#[derive(Default)]
struct WriteSpy {
    writes: usize,
    seeks: usize,
}
impl Write for WriteSpy {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes += 1;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Seek for WriteSpy {
    fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
        self.seeks += 1;
        Ok(0)
    }
}

#[test]
fn retired_invalid_geometry_and_counts_reject_before_any_stream_write_or_atomic_stage() {
    let (bytes, id, _) = package();
    let mut project = project::read_from(Cursor::new(bytes)).unwrap();
    let mut cases = invalid_finite_guides();
    for guide in [
        GuideKind::Grid { size: f64::NAN },
        GuideKind::Isometric {
            size: f64::INFINITY,
        },
        GuideKind::Perspective {
            points: vec![(f64::NEG_INFINITY, 0.0)],
        },
        GuideKind::Curvilinear {
            center: (0.0, f64::NAN),
            radius: 1.0,
            five: true,
        },
        GuideKind::Curvilinear {
            center: (0.0, 0.0),
            radius: f64::INFINITY,
            five: false,
        },
    ] {
        cases.push(DrawingGuides {
            guides: vec![guide],
            ..Default::default()
        });
    }
    for enabled in [false, true] {
        for (a, b) in [
            ((f64::NAN, 0.0), (1.0, 1.0)),
            ((0.0, 0.0), (1.0, f64::INFINITY)),
        ] {
            cases.push(DrawingGuides {
                ruler: Some(Ruler { a, b, enabled }),
                ..Default::default()
            });
        }
    }
    let path = std::env::temp_dir().join(format!(
        "emulsion-retired-invalid-guide-{}.emu",
        std::process::id()
    ));
    std::fs::write(&path, b"existing destination").unwrap();
    for protected in [false, true] {
        for guides in &cases {
            let mut document = doc();
            if protected {
                document.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
            }
            let mut graph = Graph::new(document, "Root");
            graph.set_retired_live_aids(vec![[1, 2, 3]], guides.clone());
            project
                .storyboard
                .as_mut()
                .unwrap()
                .versions
                .retired
                .insert(id, graph);
            let mut spy = WriteSpy::default();
            assert_native_code(
                project::write_to(&project, &mut spy).err().unwrap(),
                NativeFailureCode::RetiredLiveInvalidDrawingGuides,
            );
            assert_eq!((spy.writes, spy.seeks), (0, 0));
            assert_native_code(
                project::write(&project, &path).err().unwrap(),
                NativeFailureCode::RetiredLiveInvalidDrawingGuides,
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"existing destination");
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn absent_aids_remain_default_and_unknown_or_malformed_aids_never_default_cleanly() {
    let (package, id, _) = package();
    let document = doc();
    let base = protected_native(&document, Some(&Graph::new(document.clone(), "Root")));
    let absent = json_edit(&base, |name, root| {
        if name == "emulsion.json" {
            root.as_object_mut().unwrap().remove("colors");
            root.as_object_mut().unwrap().remove("drawing_guides");
        }
    });
    let absent = nested(&package, id, |_| absent);
    let opened = project::read_from(Cursor::new(absent)).unwrap();
    assert_aids(
        &retired_graph(&opened, id).retired_document_at(1).unwrap(),
        &document,
    );
    for value in [
        serde_json::json!({"guides":[{"kind":"future_guide"}]}),
        serde_json::json!({"guides":[{"kind":"grid","size":null}]}),
        serde_json::Value::Null,
        serde_json::json!("invalid drawing guides"),
    ] {
        let bytes = json_edit(&base, |name, root| {
            if name == "emulsion.json" {
                root["drawing_guides"] = value.clone();
            }
        });
        let bytes = nested(&package, id, |_| bytes);
        assert!(project::read_from_with_report(Cursor::new(bytes)).is_err());
    }
}

#[test]
fn aid_overlay_cannot_hide_independent_retired_working_with_stale_or_missing_hint() {
    let (package, id, _) = package();
    let mut document = doc();
    current_aids(&mut document);
    let base = protected_native(&document, Some(&Graph::new(document.clone(), "Root")));
    for hint in [Some("stale fingerprint"), None] {
        let bytes = json_edit(&base, |name, root| {
            if name == crate::history::GRAPH {
                let mut working = serde_json::json!({"doc":root["commits"][0]["doc"]});
                working["doc"]["nodes"][0]["name"] = "Independent working artwork".into();
                if let Some(hint) = hint {
                    working["live"] = hint.into();
                }
                root["working"] = working;
            }
        });
        let bytes = nested(&package, id, |_| bytes);
        assert_native_code(
            project::read_from_with_report(Cursor::new(bytes))
                .err()
                .unwrap(),
            NativeFailureCode::RetiredWorkingNotRepresented,
        );
    }
}

#[test]
fn aid_overlay_cannot_hide_a_changed_opaque_source() {
    use emulsion_core::node::SmartEditable;
    use std::sync::Arc;
    let mut document = doc();
    let source = Arc::new(emulsion_raster::Raster::solid(2, 1, [0.2, 0.4, 0.1, 1.0]));
    let archive = native(&document, None);
    document.nodes[0].kind = NodeKind::Smart {
        editable: Some(SmartEditable::Document {
            archive: Arc::new(archive.clone()),
            external: None,
        }),
        original_image: None,
        source: source.clone(),
        cache: source,
        filters: Vec::new(),
        filter_styles: Vec::new(),
        filters_enabled: true,
        filter_mask: None,
        offset: (0, 0),
        placement: Default::default(),
    };
    let graph = Graph::new(document.clone(), "Original opaque source");
    let changed = json_edit(&archive, |_, root| root["version"] = 99.into());
    let NodeKind::Smart { editable, .. } = &mut document.nodes[0].kind else {
        unreachable!()
    };
    *editable = Some(SmartEditable::Document {
        archive: Arc::new(changed),
        external: None,
    });
    current_aids(&mut document);
    let bytes = native(&document, Some(&graph));
    let (package, id, _) = package();
    let bytes = nested(&package, id, |_| bytes);
    assert_native_code(
        project::read_from_with_report(Cursor::new(bytes))
            .err()
            .unwrap(),
        NativeFailureCode::RetiredLiveNotRepresented,
    );
}

fn original_png_document(hidden_red: u8) -> Document {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[hidden_red, 0, 0, 0, 12, 24, 36, 255])
            .unwrap();
    }
    let png = std::sync::Arc::new(png);
    let source = crate::original_image_png::png_source(&png).unwrap();
    let original = crate::original_image_data::capture(png, &source);
    let mut node = Node::smart(1, "Original PNG", source, Vec::new(), Default::default());
    let NodeKind::Smart { original_image, .. } = &mut node.kind else {
        unreachable!()
    };
    *original_image = Some(original);
    let mut document = Document::new(2, 1);
    document.nodes.push(node);
    document.next_id = 2;
    document
}

#[test]
fn canonical_protected_v13_v14_v15_retired_writers_preserve_aids_and_original_bytes() {
    let (bytes, id, _) = package();
    for version in [13, 14, 15] {
        let mut document = original_png_document(255);
        if version == 14 {
            document.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
        }
        if version == 15 {
            let NodeKind::Smart {
                filters_enabled, ..
            } = &mut document.nodes[0].kind
            else {
                unreachable!()
            };
            *filters_enabled = false;
        }
        current_aids(&mut document);
        let graph = Graph::new(document.clone(), "Protected current aids");
        let mut project = project::read_from(Cursor::new(&bytes)).unwrap();
        project
            .storyboard
            .as_mut()
            .unwrap()
            .versions
            .retired
            .insert(id, graph.clone());
        for _ in 0..2 {
            let mut bytes = Cursor::new(Vec::new());
            project::write_to(&project, &mut bytes).unwrap();
            // These fixtures contain actual protected v13/v14/v15 features.
            assert_no_aid_history(bytes.get_ref(), id, graph.len(), 2);
            let mut outer = ZipArchive::new(Cursor::new(bytes.get_ref())).unwrap();
            let inner =
                ora::read_entry(&mut outer, &format!("history/board/{id}.ora"), 1 << 20).unwrap();
            let mut inner = ZipArchive::new(Cursor::new(inner)).unwrap();
            let manifest: serde_json::Value = serde_json::from_slice(
                &ora::read_entry(&mut inner, "emulsion.json", 1 << 20).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["version"], version);
            let opened = project::read_from_with_report(Cursor::new(bytes.into_inner())).unwrap();
            assert!(opened.report.is_empty());
            assert_retired_graph(retired_graph(&opened.project, id), &graph, &document);
            project = opened.project;
        }
    }
}

#[test]
fn aid_overlay_cannot_hide_different_original_resource_bytes() {
    let historical = original_png_document(255);
    let mut live = original_png_document(127);
    current_aids(&mut live);
    let bytes = native(&live, Some(&Graph::new(historical, "Original source")));
    let (package, id, _) = package();
    let bytes = nested(&package, id, |_| bytes);
    assert_native_code(
        project::read_from_with_report(Cursor::new(bytes))
            .err()
            .unwrap(),
        NativeFailureCode::RetiredLiveNotRepresented,
    );
}
