//! Versioned profile/identity persistence, including history skipped by plain reads.
use emulsion_core::{Command, Document, Editor, Node, NodeKind, graph::Graph};
use emulsion_io::{ora, smart_source};
use emulsion_raster::{Placement, Raster, blend::BlendSpace};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MANIFEST: &str = "emulsion.json";
const HISTORY: &str = "history/graph.json";
fn document() -> Document {
    let mut doc = Document::new(3, 2);
    doc.nodes.push(Node::raster(
        1,
        "Background",
        Arc::new(Raster::solid(3, 2, [1.; 4])),
        Placement::default(),
    ));
    doc.nodes.push(Node::raster(
        2,
        "Paint",
        Arc::new(Raster::solid(3, 2, [0., 0., 1., 1.])),
        Placement::default(),
    ));
    doc.next_id = 3;
    doc
}
fn entry_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}
fn entry(path: &Path, name: &str) -> Vec<u8> {
    entry_bytes(&std::fs::read(path).unwrap(), name)
}
fn metadata(path: &Path, name: &str) -> Value {
    serde_json::from_slice(&entry(path, name)).unwrap()
}
fn rewrite(path: &Path, mut edit: impl FnMut(&str, &mut Value)) {
    let mut source = ZipArchive::new(Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut file = source.by_index(i).unwrap();
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == MANIFEST || name == HISTORY {
            let mut value = serde_json::from_slice(&bytes).unwrap();
            edit(&name, &mut value);
            bytes = serde_json::to_vec(&value).unwrap();
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&bytes).unwrap();
    }
    std::fs::write(path, out.finish().unwrap().into_inner()).unwrap();
}
fn assert_state(actual: &Document, expected: &Document) {
    assert_eq!(actual.blend_space, expected.blend_space);
    assert_eq!(actual.psd_background, expected.psd_background);
    assert_eq!(actual.nodes.len(), expected.nodes.len());
    actual.validate().unwrap();
}
fn reject_both(path: &Path, needle: &str) {
    let plain = ora::read(path)
        .expect_err("plain read must reject")
        .to_string();
    let full = ora::read_full(path)
        .err()
        .expect("history recovery must not hide invalid state")
        .to_string();
    assert!(plain.contains(needle), "{plain}");
    assert!(full.contains(needle), "{full}");
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
fn live_profile_target_and_dormant_target_roundtrip_as_v14() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.ora");
    for space in [
        BlendSpace::Linear,
        BlendSpace::Srgb,
        BlendSpace::PhotoshopSrgbV1,
    ] {
        for target in [None, Some(1)] {
            let mut doc = document();
            doc.blend_space = space;
            doc.psd_background = target;
            let required = if target.is_some() || space == BlendSpace::PhotoshopSrgbV1 {
                14
            } else {
                9
            };
            ora::write(&doc, &path).unwrap();
            let value = metadata(&path, MANIFEST);
            assert_eq!(value["version"], required);
            assert_eq!(value["blend_space"], serde_json::to_value(space).unwrap());
            if target.is_none() {
                assert!(value.get("psd_background").is_none());
            } else {
                assert_eq!(value["psd_background"], 1);
            }
            assert_state(&ora::read(&path).unwrap(), &doc);
            assert_state(&ora::read_full(&path).unwrap().doc, &doc);
            let stack = String::from_utf8(entry(&path, "stack.xml")).unwrap();
            assert_eq!(
                stack.contains("Appearance (editable layers in Emulsion)"),
                required == 14
            );
        }
    }
    assert_eq!(
        serde_json::to_string(&BlendSpace::Linear).unwrap(),
        "\"linear\""
    );
    assert_eq!(
        serde_json::to_string(&BlendSpace::Srgb).unwrap(),
        "\"srgb\""
    );
    assert_eq!(
        serde_json::to_string(&BlendSpace::PhotoshopSrgbV1).unwrap(),
        "\"photoshop-srgb-v1\""
    );
}

#[test]
fn history_only_and_working_only_state_require_v14_and_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.ora");
    let ordinary = document();
    for target_only in [false, true] {
        let mut feature = ordinary.clone();
        if target_only {
            feature.psd_background = Some(1);
        } else {
            feature.blend_space = BlendSpace::PhotoshopSrgbV1;
        }
        for history_only in [false, true] {
            let (live, mut graph) = if history_only {
                (&ordinary, Graph::new(feature.clone(), "Feature"))
            } else {
                (&feature, Graph::new(ordinary.clone(), "Ordinary"))
            };
            if history_only {
                graph.record(&ordinary, "Remove feature", false).unwrap();
            }
            ora::write_full(live, Some(&graph), &path).unwrap();
            assert_eq!(metadata(&path, MANIFEST)["version"], 14);
            assert_eq!(metadata(&path, HISTORY)["version"], 14);
            let reopened = ora::read_full(&path).unwrap();
            assert!(reopened.history_error.is_none());
            assert_state(&reopened.doc, live);
            assert_state(&ora::read(&path).unwrap(), live);
            let restored = reopened.graph.unwrap();
            for (actual, expected) in restored.commits().zip(graph.commits()) {
                assert_state(&actual.doc, &expected.doc);
            }
        }
    }
}

#[test]
fn forged_outer_and_history_downgrades_reject_all_state_locations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downgrade.ora");
    let ordinary = document();
    for target_only in [false, true] {
        let mut feature = ordinary.clone();
        if target_only {
            feature.psd_background = Some(1);
        } else {
            feature.blend_space = BlendSpace::PhotoshopSrgbV1;
        }
        for location in ["live", "history", "working"] {
            for (outer, history) in [(13, 14), (14, 13), (13, 13)] {
                let (live, mut graph) = if location == "history" {
                    (&ordinary, Graph::new(feature.clone(), "Feature"))
                } else {
                    (&feature, Graph::new(ordinary.clone(), "Ordinary"))
                };
                if location == "history" {
                    graph.record(&ordinary, "Reset", false).unwrap();
                }
                if location == "live" {
                    graph.record(&feature, "Feature", false).unwrap();
                }
                ora::write_full(live, Some(&graph), &path).unwrap();
                rewrite(&path, |name, value| {
                    value["version"] = json!(if name == MANIFEST { outer } else { history })
                });
                reject_both(&path, "version 14");
            }
        }
    }
}

#[test]
fn invalid_live_history_and_working_targets_are_not_pruned_or_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.ora");
    for location in ["live", "history", "working"] {
        for target in [json!(2), json!(999), json!("invalid")] {
            let ordinary = document();
            let mut doc = ordinary.clone();
            doc.psd_background = Some(1);
            let mut graph = Graph::new(doc.clone(), "Background");
            let live = if location == "history" {
                graph.record(&ordinary, "Clear", false).unwrap();
                ordinary.clone()
            } else if location == "working" {
                graph = Graph::new(ordinary.clone(), "Ordinary");
                doc.clone()
            } else {
                doc.clone()
            };
            ora::write_full(&live, Some(&graph), &path).unwrap();
            rewrite(&path, |name, value| {
                if location == "live" && name == MANIFEST {
                    value["psd_background"] = target.clone();
                }
                if location == "history" && name == HISTORY {
                    value["commits"][0]["doc"]["psd_background"] = target.clone();
                }
                if location == "working" && name == HISTORY {
                    value["working"]["doc"]["psd_background"] = target.clone();
                }
            });
            reject_both(
                &path,
                if target.is_string() {
                    "invalid"
                } else {
                    "Background target"
                },
            );
        }
    }
}

#[test]
fn malformed_downgraded_history_cannot_drop_hidden_new_features() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("malformed.ora");
    for field in ["psd_background", "blend_space"] {
        let doc = document();
        ora::write_full(&doc, Some(&Graph::new(doc.clone(), "Ordinary")), &path).unwrap();
        rewrite(&path, |name, value| {
            if name == HISTORY {
                value["commits"][0]["doc"][field] = if field == "psd_background" {
                    json!("bad id")
                } else {
                    json!("photoshop-srgb-v1")
                };
                value["commits"][0]["doc"]["nodes"][0]["id"] = json!("malformed unrelated node");
            }
        });
        assert_eq!(metadata(&path, MANIFEST)["version"], json!(9));
        assert_eq!(metadata(&path, HISTORY)["version"], json!(9));
        reject_manifest_both(
            &path,
            &[
                "Photoshop compositing or Invert state requires native and existing history version 14",
            ],
        );
    }
}

#[test]
fn hidden_role_is_still_versioned_and_invalid_order_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hidden.ora");
    let mut doc = document();
    doc.psd_background = Some(1);
    doc.nodes[0].visible = false;
    doc.nodes[0].name = "Unrelated name".into();
    ora::write(&doc, &path).unwrap();
    assert_eq!(ora::read(&path).unwrap().psd_background, Some(1));
    rewrite(&path, |name, value| {
        if name == MANIFEST {
            value["nodes"].as_array_mut().unwrap().swap(0, 1);
        }
    });
    reject_both(&path, "Background target");
}

#[test]
fn project_outer_v1_and_each_native_page_gate_independently() {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.emu");
    let mut doc = document();
    doc.psd_background = Some(1);
    let project = ProjectEditor::new_project(ProjectKind::Design, doc)
        .unwrap()
        .snapshot()
        .unwrap();
    emulsion_io::project::write(&project, &path).unwrap();
    let outer: Value = serde_json::from_slice(&entry(&path, "project.json")).unwrap();
    assert_eq!(outer["version"], 1);
    let page = entry(&path, "pages/1.ora");
    let manifest: Value = serde_json::from_slice(&entry_bytes(&page, MANIFEST)).unwrap();
    assert_eq!(manifest["version"], 14);
    let reopened = emulsion_io::project::read(&path).unwrap();
    assert_eq!(reopened.pages[0].doc.psd_background, Some(1));
}

#[test]
fn nested_source_v14_is_independent_and_source_session_detects_target_only_changes() {
    use emulsion_core::node::SmartEditable;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host.ora");
    let mut source = document();
    source.blend_space = BlendSpace::PhotoshopSrgbV1;
    source.psd_background = Some(1);
    let archive = smart_source::encode(&source).unwrap();
    let mut host = Document::new(3, 2);
    let mut smart = Node::smart(
        1,
        "Nested",
        Arc::new(Raster::solid(3, 2, [1.; 4])),
        vec![],
        Placement::default(),
    );
    let NodeKind::Smart { editable, .. } = &mut smart.kind else {
        unreachable!()
    };
    *editable = Some(SmartEditable::Document {
        archive: archive.clone(),
        external: None,
    });
    host.nodes.push(smart);
    host.next_id = 2;
    ora::write(&host, &path).unwrap();
    assert_eq!(metadata(&path, MANIFEST)["version"], 9);
    let reopened = ora::read(&path).unwrap();
    let opened = smart_source::open(&reopened, 1).unwrap();
    assert_state(&opened, &source);
    let mut changed = opened.clone();
    changed.psd_background = None;
    assert!(!smart_source::same_document_contents(&opened, &changed));
    let mut editor = Editor::new(reopened, None);
    smart_source::apply(&mut editor, 1, &changed).unwrap();
    assert_eq!(
        smart_source::open(&editor.doc, 1).unwrap().psd_background,
        None
    );
    editor.undo();
    assert_eq!(
        smart_source::open(&editor.doc, 1).unwrap().psd_background,
        Some(1)
    );
    editor.redo();
    assert_eq!(
        smart_source::open(&editor.doc, 1).unwrap().psd_background,
        None
    );
}

#[test]
fn synthesized_source_and_selection_exports_clear_external_identity() {
    let mut doc = document();
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    doc.psd_background = Some(1);
    let (subset, _) =
        emulsion_io::selection_export::prepare(&doc, &[1], &Default::default()).unwrap();
    assert_eq!(subset.psd_background, None);
    assert_eq!(subset.blend_space, doc.blend_space);
    Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
    let source = smart_source::open(&doc, 1).unwrap();
    assert_eq!(source.psd_background, None);
    assert_eq!(source.blend_space, doc.blend_space);
    let NodeKind::Smart { editable, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *editable = Some(emulsion_core::node::SmartEditable::Svg {
        xml: Arc::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"3\" height=\"2\"><rect width=\"3\" height=\"2\" fill=\"red\"/></svg>",
        ),
    });
    let svg_source = smart_source::open(&doc, 1).unwrap();
    assert_eq!(svg_source.psd_background, None);
    assert_eq!(svg_source.blend_space, doc.blend_space);
}

#[test]
fn external_layer_adoption_remaps_external_role_without_name_inference() {
    let mut current = document();
    current.psd_background = Some(1);
    let mut external = document();
    external.nodes[0].name = "Paint".into();
    external.nodes[1].name = "Background".into();
    let adopted = emulsion_io::storyboard_external_edit::adopt_layers(&current, &external).unwrap();
    assert_eq!(adopted.psd_background, None);
    external.psd_background = Some(1);
    external.blend_space = BlendSpace::PhotoshopSrgbV1;
    let adopted = emulsion_io::storyboard_external_edit::adopt_layers(&current, &external).unwrap();
    assert_eq!(adopted.psd_background, Some(2));
    assert_eq!(adopted.blend_space, external.blend_space);
    adopted.validate().unwrap();
    let stacked =
        emulsion_io::storyboard_external_edit::stack_layers(&current, &external, "External")
            .unwrap();
    assert_eq!(stacked.psd_background, Some(1));
}

#[test]
fn native_target_topology_errors_precede_missing_source_allocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("topology.ora");
    let mut doc = document();
    doc.psd_background = Some(1);
    for case in ["parent", "clip", "kind", "duplicate"] {
        ora::write(&doc, &path).unwrap();
        rewrite(&path, |name, value| {
            if name != MANIFEST {
                return;
            }
            value["nodes"][0]["kind"]["src"] = json!("missing-source.png");
            match case {
                "parent" => value["nodes"][0]["parent"] = json!(2),
                "clip" => value["nodes"][0]["clip_to"] = json!(2),
                "kind" => value["nodes"][0]["kind"]["type"] = json!("smart"),
                "duplicate" => value["nodes"][1]["id"] = json!(1),
                _ => unreachable!(),
            }
        });
        reject_both(&path, "Background target");
    }
}

#[test]
fn standalone_feature_downgrade_and_unknown_versions_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("standalone.ora");
    for target_only in [false, true] {
        let mut doc = document();
        if target_only {
            doc.psd_background = Some(1);
        } else {
            doc.blend_space = BlendSpace::PhotoshopSrgbV1;
        }
        ora::write(&doc, &path).unwrap();
        rewrite(&path, |_, value| value["version"] = json!(13));
        reject_both(&path, "version 14");
    }
    ora::write(&document(), &path).unwrap();
    let future_version = ora::FORMAT_VERSION + 1;
    rewrite(&path, |_, value| value["version"] = json!(future_version));
    assert!(matches!(
        ora::read(&path),
        Err(emulsion_io::IoError::TooNew(version)) if version == future_version
    ));
    assert!(matches!(
        ora::read_full(&path),
        Err(emulsion_io::IoError::TooNew(version)) if version == future_version
    ));
}

#[test]
fn malformed_v14_history_profiles_are_rejected_even_by_plain_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile-tokens.ora");
    let doc = document();
    for profile in [Some(json!("unknown-profile")), Some(json!(12)), None] {
        let mut featured = doc.clone();
        featured.psd_background = Some(1);
        let mut graph = Graph::new(featured, "Background");
        graph.record(&doc, "Clear", false).unwrap();
        ora::write_full(&doc, Some(&graph), &path).unwrap();
        rewrite(&path, |name, value| {
            if name == HISTORY {
                let snapshot = value["commits"][0]["doc"].as_object_mut().unwrap();
                if let Some(profile) = &profile {
                    snapshot.insert("blend_space".into(), profile.clone());
                } else {
                    snapshot.remove("blend_space");
                }
            }
        });
        assert_eq!(metadata(&path, MANIFEST)["version"], json!(14));
        assert_eq!(metadata(&path, HISTORY)["version"], json!(14));
        let details: &[&str] = match profile {
            Some(Value::Number(_)) => &["invalid type: integer `12`, expected a string"],
            Some(_) => &[
                "Invalid protected history metadata (including Smart Filters and projective mappings):",
                "unknown variant `unknown-profile`, expected one of `linear`, `srgb`, `photoshop-srgb-v1`",
            ],
            None => &[
                "Invalid protected history metadata (including Smart Filters and projective mappings):",
                "missing field `blend_space`",
            ],
        };
        reject_manifest_both(&path, details);
    }
}

#[test]
fn external_adoption_profile_rule_preserves_legacy_and_transfers_photoshop_scope() {
    use BlendSpace::{Linear, PhotoshopSrgbV1, Srgb};
    for (current_space, external_space, expected) in [
        // Legacy-to-legacy adoption keeps the current document's old behavior.
        (Linear, Linear, Linear),
        (Linear, Srgb, Linear),
        (Srgb, Linear, Srgb),
        (Srgb, Srgb, Srgb),
        // Entering the versioned profile adopts the complete external scope.
        (Linear, PhotoshopSrgbV1, PhotoshopSrgbV1),
        (Srgb, PhotoshopSrgbV1, PhotoshopSrgbV1),
        // Leaving the versioned profile adopts the external legacy profile.
        (PhotoshopSrgbV1, Linear, Linear),
        (PhotoshopSrgbV1, Srgb, Srgb),
        (PhotoshopSrgbV1, PhotoshopSrgbV1, PhotoshopSrgbV1),
    ] {
        let mut current = document();
        current.blend_space = current_space;
        current.psd_background = Some(1);
        let mut external = document();
        external.blend_space = external_space;
        external.psd_background = Some(1);
        external.nodes[0].name = "Paint".into();
        external.nodes[1].name = "Background".into();
        let adopted =
            emulsion_io::storyboard_external_edit::adopt_layers(&current, &external).unwrap();
        assert_eq!(
            adopted.blend_space, expected,
            "{current_space:?} to {external_space:?}"
        );
        assert_eq!(
            adopted.psd_background,
            Some(2),
            "external target is explicitly remapped"
        );
        adopted.validate().unwrap();
        let stacked =
            emulsion_io::storyboard_external_edit::stack_layers(&current, &external, "External")
                .unwrap();
        assert_eq!(stacked.blend_space, current_space);
        assert_eq!(stacked.psd_background, current.psd_background);
    }
}
