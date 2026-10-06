//! Native v15 state and archive-wide downgrade gates. No external codec inputs.
use emulsion_core::{Command, Document, Editor, Node, NodeKind, SmartFilterMask, graph::Graph};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_io::ora;
use emulsion_raster::{Mask, Placement, Raster};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
const MANIFEST: &str = "emulsion.json";
const HISTORY: &str = "history/graph.json";
fn document(empty: bool) -> Document {
    let mut doc = Document::new(16, 12);
    doc.source_depth = 16;
    doc.nodes.push(Node::smart(
        1,
        "Source",
        Arc::new(Raster::solid(6, 4, [0.2, 0.4, 0.1, 0.5])),
        if empty {
            vec![]
        } else {
            vec![Filter::GaussianBlur { radius: 2. }]
        },
        Placement::at(3., 2.),
    ));
    doc.next_id = 2;
    doc
}
fn disable(doc: &mut Document, root: bool) {
    if root {
        Command::SetFiltersEnabled {
            id: 1,
            enabled: false,
        }
        .apply(doc)
        .unwrap();
    } else {
        Command::SetFilterStyles {
            id: 1,
            styles: vec![FilterStyle {
                enabled: false,
                opacity: 0.,
                ..Default::default()
            }],
        }
        .apply(doc)
        .unwrap();
    }
}
fn assert_state(actual: &Document, expected: &Document) {
    let (
        NodeKind::Smart {
            filters: a,
            filter_styles: sa,
            filters_enabled: ea,
            source,
            cache,
            offset,
            filter_mask: ma,
            ..
        },
        NodeKind::Smart {
            filters: b,
            filter_styles: sb,
            filters_enabled: eb,
            filter_mask: mb,
            ..
        },
    ) = (&actual.nodes[0].kind, &expected.nodes[0].kind)
    else {
        panic!()
    };
    assert_eq!((a, sa, ea), (b, sb, eb));
    assert_eq!(
        ma.as_ref().map(|m| (
            m.enabled,
            m.linked,
            m.transform,
            m.properties,
            m.pixels.to_gray8()
        )),
        mb.as_ref().map(|m| (
            m.enabled,
            m.linked,
            m.transform,
            m.properties,
            m.pixels.to_gray8()
        ))
    );
    if !emulsion_core::smart::has_active_filters(a, sa, *ea) {
        assert!(Arc::ptr_eq(source, cache));
        assert_eq!(*offset, (0, 0));
        assert!(Arc::ptr_eq(
            source,
            &emulsion_core::smart_filter_mask::effective_pixels(&actual.nodes[0])
                .unwrap()
                .unwrap()
        ));
    }
}
fn raw_entry(path: &Path, name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

fn entry(path: &Path, name: &str) -> Value {
    let mut zip = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn rewrite(path: &Path, mut edit: impl FnMut(&str, &mut Value)) {
    let mut source = ZipArchive::new(Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut file = source.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == MANIFEST || name == HISTORY {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            let before = value.clone();
            edit(&name, &mut value);
            // Native history fingerprints bind the exact manifest bytes. Do
            // not reserialize an untouched entry merely because it is JSON.
            if value != before {
                bytes = serde_json::to_vec(&value).unwrap();
            }
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&bytes).unwrap();
    }
    std::fs::write(path, out.finish().unwrap().into_inner()).unwrap();
}

fn reject_both(path: &Path, needle: &str) {
    for error in [ora::read(path).err(), ora::read_full(path).err()] {
        let message = error
            .expect("must reject, never discard filter-bearing history")
            .to_string();
        assert!(message.contains(needle), "{message}");
    }
}

fn reject_manifest_both(path: &Path, details: &[&str]) {
    for error in [ora::read(path).err(), ora::read_full(path).err()] {
        let error = error.expect("must reject, never discard protected history");
        let message = match error {
            emulsion_io::IoError::Manifest(message) => message,
            other => panic!("expected manifest rejection, got {other}"),
        };
        for detail in details {
            assert!(
                message.contains(detail),
                "expected {detail:?}, got {message}"
            );
        }
    }
}

#[test]
fn live_hidden_empty_and_zero_opacity_disabled_states_require_v15() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("enabled.ora");
    for (empty, root) in [(true, true), (false, true), (false, false)] {
        for visible in [false, true] {
            let mut doc = document(empty);
            doc.nodes[0].visible = visible;
            doc.nodes[0].opacity = 0.;
            disable(&mut doc, root);
            ora::write(&doc, &path).unwrap();
            let metadata = entry(&path, MANIFEST);
            assert_eq!(metadata["version"], 15);
            if root {
                assert_eq!(metadata["nodes"][0]["kind"]["filters_enabled"], false);
            } else {
                assert_eq!(
                    metadata["nodes"][0]["kind"]["filter_styles"][0]["enabled"],
                    false
                );
            }
            assert_state(&ora::read(&path).unwrap(), &doc);
            assert_state(&ora::read_full(&path).unwrap().doc, &doc);
        }
    }
    let legacy = document(false);
    ora::write(&legacy, &path).unwrap();
    let metadata = entry(&path, MANIFEST);
    assert_eq!(metadata["version"], 9);
    assert!(
        metadata["nodes"][0]["kind"]
            .get("filters_enabled")
            .is_none()
    );
    assert_state(&ora::read(&path).unwrap(), &legacy);
}

#[test]
fn all_snapshot_locations_force_both_native_and_history_versions_and_reject_downgrades() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.ora");
    for root in [false, true] {
        for location in ["live", "working", "history"] {
            let ordinary = document(false);
            let mut feature = ordinary.clone();
            disable(&mut feature, root);
            for (outer, history) in [(15, 15), (14, 15), (15, 14), (14, 14)] {
                let mut graph = Graph::new(
                    if location == "history" {
                        feature.clone()
                    } else {
                        ordinary.clone()
                    },
                    "Base",
                );
                let live = if location == "history" {
                    &ordinary
                } else {
                    &feature
                };
                if location != "working" {
                    graph.record(live, "Current", false).unwrap();
                }
                ora::write_full(live, Some(&graph), &path).unwrap();
                assert_eq!(entry(&path, MANIFEST)["version"], 15);
                assert_eq!(entry(&path, HISTORY)["version"], 15);
                if outer == 15 && history == 15 {
                    let opened = ora::read_full(&path).unwrap();
                    assert!(opened.history_error.is_none());
                    assert_state(&opened.doc, live);
                    for (actual, expected) in opened.graph.unwrap().commits().zip(graph.commits()) {
                        assert_state(&actual.doc, &expected.doc);
                    }
                } else {
                    rewrite(&path, |name, value| {
                        value["version"] = json!(if name == MANIFEST { outer } else { history })
                    });
                    reject_both(&path, "version 15");
                }
            }
        }
    }
}

#[test]
fn malformed_flags_and_orphan_styles_fail_before_missing_source_planes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.ora");
    for field in ["root", "item", "orphan"] {
        for value in [json!(null), json!("false"), json!(0), json!([])] {
            let mut doc = document(false);
            disable(&mut doc, true);
            ora::write(&doc, &path).unwrap();
            rewrite(&path, |name, manifest| {
                if name == MANIFEST {
                    let kind = &mut manifest["nodes"][0]["kind"];
                    kind["src"] = json!("missing-source.png");
                    match field {
                        "root" => kind["filters_enabled"] = value.clone(),
                        "item" => kind["filter_styles"] = json!([{"enabled":value}]),
                        _ => kind["filter_styles"] = json!([{}, {"enabled":false}]),
                    }
                }
            });
            reject_both(
                &path,
                if field == "orphan" {
                    "Orphan"
                } else {
                    "boolean"
                },
            );
        }
    }
}

#[test]
fn malformed_legacy_history_with_enabled_metadata_is_never_silently_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.ora");
    for item in [false, true] {
        for value in [json!(false), json!(true), json!("invalid")] {
            let ordinary = document(false);
            let mut feature = ordinary.clone();
            disable(&mut feature, !item);
            let mut graph = Graph::new(feature, "Disabled");
            graph.record(&ordinary, "Current", false).unwrap();
            ora::write_full(&ordinary, Some(&graph), &path).unwrap();
            rewrite(&path, |name, metadata| {
                metadata["version"] = json!(9);
                if name == HISTORY {
                    let node = &mut metadata["commits"][0]["doc"]["nodes"][0];
                    node["parent"] = json!(false);
                    if item {
                        node["kind"]["filter_styles"][0]["enabled"] = value.clone();
                    } else {
                        node["kind"]["filters_enabled"] = value.clone();
                    }
                }
            });
            // Admission diagnoses version/type violations before the typed
            // history probe reaches the independently malformed parent.
            let details: &[&str] = match value {
                Value::Bool(false) => {
                    &["disabled Smart Filter state requires native and existing history version 15"]
                }
                Value::Bool(true) => &[
                    "Invalid protected history metadata (including Smart Filters and projective mappings):",
                    "invalid type: boolean `false`, expected u64",
                ],
                _ => &["invalid type: string \"invalid\", expected a boolean"],
            };
            reject_manifest_both(&path, details);
        }
    }
}

#[test]
fn malformed_v15_disabled_history_is_rejected_without_a_downgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged-current.ora");
    for item in [false, true] {
        let ordinary = document(false);
        let mut feature = ordinary.clone();
        disable(&mut feature, !item);
        let mut graph = Graph::new(feature, "Disabled");
        graph.record(&ordinary, "Current", false).unwrap();
        ora::write_full(&ordinary, Some(&graph), &path).unwrap();
        assert_eq!(entry(&path, MANIFEST)["version"], json!(15));
        let history = entry(&path, HISTORY);
        assert_eq!(history["version"], json!(15));
        let kind = &history["commits"][0]["doc"]["nodes"][0]["kind"];
        assert_eq!(
            if item {
                &kind["filter_styles"][0]["enabled"]
            } else {
                &kind["filters_enabled"]
            },
            &json!(false)
        );
        rewrite(&path, |name, metadata| {
            if name == HISTORY {
                metadata["commits"][0]["doc"]["nodes"][0]["parent"] = json!(false);
            }
        });
        reject_manifest_both(
            &path,
            &[
                "Invalid protected history metadata (including Smart Filters and projective mappings):",
                "invalid type: boolean `false`, expected u64",
            ],
        );
    }
}

#[test]
fn direct_history_restore_canonicalizes_saved_expanded_bypass_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.ora");
    let active = document(false);
    let graph = Graph::new(active.clone(), "Expanded");
    let mut disabled = active.clone();
    disable(&mut disabled, true);
    ora::write_full(&disabled, Some(&graph), &path).unwrap();
    let manifest_before = raw_entry(&path, MANIFEST);
    let working_fingerprint = entry(&path, HISTORY)["working"]["live"].clone();
    assert!(working_fingerprint.is_string());
    rewrite(&path, |name, metadata| {
        if name == HISTORY {
            let old = metadata["commits"][0]["doc"]["nodes"][0]["kind"].clone();
            let working = &mut metadata["working"]["doc"]["nodes"][0]["kind"];
            working["cache"] = old["cache"].clone();
            working["offset"] = old["offset"].clone();
        }
    });
    assert_eq!(raw_entry(&path, MANIFEST), manifest_before);
    assert_eq!(
        entry(&path, HISTORY)["working"]["live"],
        working_fingerprint
    );
    let reopened = ora::read_full(&path).unwrap();
    assert!(reopened.history_error.is_none());
    assert_state(&reopened.doc, &disabled);
    let graph = reopened.graph.as_ref().expect("restored history graph");
    let historical = &graph.commits().next().unwrap().doc;
    let (
        NodeKind::Smart {
            source,
            cache,
            offset,
            ..
        },
        NodeKind::Smart {
            source: historical_source,
            cache: historical_cache,
            offset: historical_offset,
            ..
        },
    ) = (&reopened.doc.nodes[0].kind, &historical.nodes[0].kind)
    else {
        panic!()
    };
    // Only the exact working history snapshot shares its pool source with the
    // graph. A freshly rendered manifest would have a different source Arc.
    assert!(Arc::ptr_eq(source, historical_source));
    assert!(Arc::ptr_eq(source, cache));
    assert_eq!(*offset, (0, 0));
    assert!(!Arc::ptr_eq(source, historical_cache));
    assert_ne!(*historical_offset, (0, 0));
}

#[test]
fn dormant_mask_extent_and_nested_bytes_survive_reopen_edit_undo_resave() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("masks.ora");
    let mut nested = document(true);
    disable(&mut nested, true);
    let archive = emulsion_io::smart_source::encode(&nested).unwrap();
    let mut doc = document(false);
    let mut mask = SmartFilterMask::new(Arc::new(Mask::from_fn(18, 16, 255, |x, _| {
        if x < 9 { 0 } else { 128 }
    })));
    mask.enabled = false;
    mask.linked = false;
    mask.transform = emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
        1., 0.2, 0., 1., -5., -4.,
    ]));
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(mask),
    }
    .apply(&mut doc)
    .unwrap();
    if let NodeKind::Smart { editable, .. } = &mut doc.nodes[0].kind {
        *editable = Some(emulsion_core::node::SmartEditable::Document {
            archive: archive.clone(),
            external: None,
        });
    }
    disable(&mut doc, true);
    ora::write(&doc, &path).unwrap();
    let reopened = ora::read_full(&path).unwrap().doc;
    assert_state(&reopened, &doc);
    let NodeKind::Smart {
        editable:
            Some(emulsion_core::node::SmartEditable::Document {
                archive: retained, ..
            }),
        ..
    } = &reopened.nodes[0].kind
    else {
        panic!()
    };
    assert_eq!(retained.as_slice(), archive.as_slice());
    assert_state(
        &emulsion_io::smart_source::open(&reopened, 1).unwrap(),
        &nested,
    );
    let mut editor = Editor::new(reopened.clone(), None);
    editor
        .execute(Command::SetFiltersEnabled {
            id: 1,
            enabled: true,
        })
        .unwrap();
    let edited = editor.doc.clone();
    assert!(editor.undo());
    assert_state(&editor.doc, &reopened);
    assert!(editor.redo());
    assert_state(&editor.doc, &edited);
    ora::write_full(&editor.doc, Some(&editor.graph), &path).unwrap();
    assert_state(&ora::read_full(&path).unwrap().doc, &edited);
}

#[test]
fn future_native_and_history_versions_are_rejected_for_enabled_documents_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.ora");
    let doc = document(true);
    let graph = Graph::new(doc.clone(), "Base");
    for target in [MANIFEST, HISTORY] {
        ora::write_full(&doc, Some(&graph), &path).unwrap();
        rewrite(&path, |name, metadata| {
            if name == target {
                metadata["version"] = json!(if target == MANIFEST {
                    ora::FORMAT_VERSION + 1
                } else {
                    emulsion_io::history::HISTORY_VERSION + 1
                });
            }
        });
        assert!(ora::read(&path).is_err());
        assert!(ora::read_full(&path).is_err());
    }
}

#[test]
fn nested_v15_bytes_are_opaque_until_opened_and_missing_style_defaults_stay_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let nested_path = dir.path().join("nested.ora");
    let outer_path = dir.path().join("outer.ora");
    let mut nested = document(true);
    disable(&mut nested, true);
    ora::write(&nested, &nested_path).unwrap();
    rewrite(&nested_path, |_, value| value["version"] = json!(14));
    let archive = Arc::new(std::fs::read(&nested_path).unwrap());
    let mut outer = document(true);
    if let NodeKind::Smart { editable, .. } = &mut outer.nodes[0].kind {
        *editable = Some(emulsion_core::node::SmartEditable::Document {
            archive: archive.clone(),
            external: None,
        });
    }
    ora::write(&outer, &outer_path).unwrap();
    assert_eq!(entry(&outer_path, MANIFEST)["version"], 9);
    let opened = ora::read_full(&outer_path).unwrap();
    let NodeKind::Smart {
        editable:
            Some(emulsion_core::node::SmartEditable::Document {
                archive: retained, ..
            }),
        ..
    } = &opened.doc.nodes[0].kind
    else {
        panic!()
    };
    assert_eq!(retained.as_slice(), archive.as_slice());
    assert!(
        emulsion_io::smart_source::open(&opened.doc, 1)
            .unwrap_err()
            .to_string()
            .contains("version 15")
    );
    let enabled = document(false);
    ora::write(&enabled, &outer_path).unwrap();
    rewrite(&outer_path, |_, value| {
        value["nodes"][0]["kind"]
            .as_object_mut()
            .unwrap()
            .remove("filter_styles");
    });
    let legacy = ora::read(&outer_path).unwrap();
    let NodeKind::Smart {
        filters_enabled,
        filter_styles,
        ..
    } = &legacy.nodes[0].kind
    else {
        panic!()
    };
    assert!(*filters_enabled && filter_styles.is_empty());
}

#[test]
fn enabled_metadata_cannot_hide_behind_unknown_historical_kind_tags() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unknown-kind.ora");
    for item in [false, true] {
        let ordinary = document(false);
        let mut feature = ordinary.clone();
        disable(&mut feature, !item);
        let mut graph = Graph::new(feature, "Disabled");
        graph.record(&ordinary, "Current", false).unwrap();
        ora::write_full(&ordinary, Some(&graph), &path).unwrap();
        rewrite(&path, |name, value| {
            value["version"] = json!(9);
            if name == HISTORY {
                value["commits"][0]["doc"]["nodes"][0]["kind"]["type"] =
                    json!("unknown-smart-kind");
            }
        });
        reject_both(&path, "version 15");
    }
}

#[test]
fn explicit_true_metadata_keeps_ignored_history_corruption_strict_but_legacy_still_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ignored-field.ora");
    for marker in ["absent", "root", "item"] {
        for corruption in ["cache", "type"] {
            let doc = document(false);
            let graph = Graph::new(doc.clone(), "Base");
            ora::write_full(&doc, Some(&graph), &path).unwrap();
            rewrite(&path, |name, value| {
                if name == HISTORY {
                    let kind = &mut value["commits"][0]["doc"]["nodes"][0]["kind"];
                    match marker {
                        "root" => kind["filters_enabled"] = json!(true),
                        "item" => kind["filter_styles"] = json!([{"enabled":true}]),
                        _ => {}
                    }
                    kind[corruption] = json!(if corruption == "type" {
                        "unknown-smart-kind"
                    } else {
                        "invalid-plane-id"
                    });
                }
            });
            let opened = ora::read_full(&path);
            if marker == "absent" {
                let recovered =
                    opened.expect("legacy histories without new metadata remain recoverable");
                assert!(recovered.history_error.is_some());
                assert!(recovered.graph.is_none());
            } else {
                assert!(
                    opened.is_err(),
                    "explicit {marker} state must not be silently dropped for bad {corruption}"
                );
            }
        }
    }
}
