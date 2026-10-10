//! Native Invert is editable, version-gated, and independent of PSD filterFX/FEid.
use emulsion_core::{Command, Document, Editor, Node, NodeKind, SmartFilterMask, graph::Graph};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_io::ora;
use emulsion_raster::{Mask, Placement, Raster, adjust::Adjustment};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MANIFEST: &str = "emulsion.json";
const HISTORY: &str = "history/graph.json";

fn document(invert: bool) -> Document {
    let mut doc = Document::new(8, 6);
    doc.source_depth = 16;
    doc.nodes.push(Node::smart(
        1,
        "Invert source",
        Arc::new(Raster::from_fn(3, 2, [0; 4], |x, y| {
            let a = if y == 0 { 32768 } else { 65535 };
            [x as u16 * 13000, a / 3, a / 2, a]
        })),
        if invert { vec![Filter::Invert] } else { vec![] },
        Placement::at(2.0, 3.0),
    ));
    doc.next_id = 2;
    doc
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
            let mut value = serde_json::from_slice(&bytes).unwrap();
            edit(&name, &mut value);
            bytes = serde_json::to_vec(&value).unwrap();
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

fn assert_pixels(actual: &Document, expected: &Document) {
    let NodeKind::Smart {
        source,
        cache,
        filters,
        filter_styles,
        filter_mask,
        placement,
        offset,
        ..
    } = &actual.nodes[0].kind
    else {
        panic!()
    };
    let NodeKind::Smart {
        source: original,
        cache: rendered,
        filters: stack,
        filter_styles: styles,
        filter_mask: mask,
        placement: original_placement,
        offset: original_offset,
        ..
    } = &expected.nodes[0].kind
    else {
        panic!()
    };
    assert_eq!(filters, stack);
    assert_eq!(filter_styles, styles);
    assert_eq!(placement, original_placement);
    assert_eq!(offset, original_offset);
    for (a, b) in [(source, original), (cache, rendered)] {
        assert_eq!(
            (a.width(), a.height(), a.fill()),
            (b.width(), b.height(), b.fill())
        );
        // Plain native reads decode a 16-bit straight-sRGB PNG; history keeps
        // exact linear planes. Allow only its one-step color quantization.
        for (actual, expected) in a
            .read_rect(a.bounds())
            .into_iter()
            .zip(b.read_rect(b.bounds()))
        {
            assert_eq!(actual[3], expected[3]);
            for ch in 0..3 {
                assert!(
                    actual[ch].abs_diff(expected[ch]) <= 2,
                    "{actual:?} != {expected:?}"
                );
            }
        }
    }
    assert_eq!(filter_mask.is_some(), mask.is_some());
    if let (Some(a), Some(b)) = (filter_mask, mask) {
        assert_eq!(a.enabled, b.enabled);
        assert_eq!(a.linked, b.linked);
        assert_eq!(a.transform, b.transform);
        assert_eq!(a.properties, b.properties);
        assert_eq!(a.pixels.to_gray8(), b.pixels.to_gray8());
    }
    let a = emulsion_core::smart_filter_mask::effective_pixels(&actual.nodes[0])
        .unwrap()
        .unwrap();
    let b = emulsion_core::smart_filter_mask::effective_pixels(&expected.nodes[0])
        .unwrap()
        .unwrap();
    for (actual, expected) in a
        .read_rect(a.bounds())
        .into_iter()
        .zip(b.read_rect(b.bounds()))
    {
        assert_eq!(actual[3], expected[3]);
        for ch in 0..3 {
            assert!(
                actual[ch].abs_diff(expected[ch]) <= 2,
                "{actual:?} != {expected:?}"
            );
        }
    }
    let (recomputed, _) = emulsion_core::smart::render_styled(source, filters, filter_styles);
    assert_eq!(
        cache.read_rect(cache.bounds()),
        recomputed.read_rect(recomputed.bounds())
    );
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
fn invert_live_hidden_and_zero_opacity_stacks_require_v14() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invert.ora");
    for visible in [false, true] {
        for opacity in [0.0, 1.0] {
            let mut doc = document(true);
            doc.nodes[0].visible = visible;
            Command::SetFilterStyles {
                id: 1,
                styles: vec![FilterStyle {
                    opacity,
                    ..Default::default()
                }],
            }
            .apply(&mut doc)
            .unwrap();
            ora::write(&doc, &path).unwrap();
            let manifest = entry(&path, MANIFEST);
            assert_eq!(manifest["version"], 14);
            assert_eq!(
                manifest["nodes"][0]["kind"]["filters"],
                json!([{"kind":"invert"}])
            );
            assert_pixels(&ora::read(&path).unwrap(), &doc);
            assert_pixels(&ora::read_full(&path).unwrap().doc, &doc);
        }
    }
    let mut ordinary = document(false);
    // The existing adjustment is a separate feature and remains legacy-readable.
    ordinary.nodes.push(Node::adjust(2, Adjustment::Invert));
    ordinary.next_id = 3;
    ora::write(&ordinary, &path).unwrap();
    assert_eq!(entry(&path, MANIFEST)["version"], 9);
    assert!(ora::read_full(&path).is_ok());
}

#[test]
fn invert_working_and_history_only_versions_retain_stack_and_mask() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.ora");
    let ordinary = document(false);
    let mut feature = document(true);
    Command::SetSmartFilterMask {
        id: 1,
        mask: Some(SmartFilterMask::new(Arc::new(Mask::from_gray8(
            3,
            2,
            &[0, 128, 255, 255, 128, 0],
        )))),
    }
    .apply(&mut feature)
    .unwrap();
    for history_only in [false, true] {
        let (live, mut graph) = if history_only {
            (&ordinary, Graph::new(feature.clone(), "Invert"))
        } else {
            (&feature, Graph::new(ordinary.clone(), "Source"))
        };
        if history_only {
            graph.record(&ordinary, "Remove Invert", false).unwrap();
        }
        ora::write_full(live, Some(&graph), &path).unwrap();
        assert_eq!(entry(&path, MANIFEST)["version"], 14);
        assert_eq!(entry(&path, HISTORY)["version"], 14);
        let reopened = ora::read_full(&path).unwrap();
        assert!(reopened.history_error.is_none());
        assert_pixels(&reopened.doc, live);
        assert_pixels(&ora::read(&path).unwrap(), live);
        for (actual, expected) in reopened.graph.unwrap().commits().zip(graph.commits()) {
            assert_pixels(&actual.doc, &expected.doc);
        }
    }
}

#[test]
fn invert_downgrade_gates_cover_live_working_and_historical_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("downgrade.ora");
    for location in ["live", "working", "history"] {
        for (outer, history) in [(13, 14), (14, 13), (13, 13)] {
            let ordinary = document(false);
            let feature = document(true);
            let mut graph = Graph::new(
                if location == "history" {
                    feature.clone()
                } else {
                    ordinary.clone()
                },
                "Source",
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
            rewrite(&path, |name, value| {
                value["version"] = json!(if name == MANIFEST { outer } else { history })
            });
            reject_both(&path, "version 14");
        }
    }
}

#[test]
fn unknown_v14_filters_and_malformed_downgraded_history_cannot_be_recovered_away() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("malformed.ora");
    for location in ["live", "working", "history"] {
        let ordinary = document(false);
        let feature = document(true);
        let mut graph = Graph::new(
            if location == "history" {
                feature.clone()
            } else {
                ordinary.clone()
            },
            "Source",
        );
        let live = if location == "history" {
            &ordinary
        } else {
            &feature
        };
        if location == "history" {
            graph.record(live, "Remove filter", false).unwrap();
        }
        ora::write_full(live, Some(&graph), &path).unwrap();
        rewrite(&path, |name, value| {
            let doc = match (name, location) {
                (MANIFEST, "live") => Some(value),
                (HISTORY, "working") => Some(&mut value["working"]["doc"]),
                (HISTORY, "history") => Some(&mut value["commits"][0]["doc"]),
                _ => None,
            };
            if let Some(doc) = doc {
                doc["nodes"][0]["kind"]["filters"][0]["kind"] = json!("future-invert");
            }
        });
        reject_both(&path, "unknown variant");
    }
    // The downgraded case fails at admission; the supported-version control
    // retains recognized Invert metadata and reaches the malformed parent.
    for version in [13, 14] {
        let ordinary = document(false);
        let mut graph = Graph::new(document(true), "Invert");
        graph.record(&ordinary, "Remove filter", false).unwrap();
        ora::write_full(&ordinary, Some(&graph), &path).unwrap();
        assert_eq!(entry(&path, MANIFEST)["version"], json!(14));
        assert_eq!(entry(&path, HISTORY)["version"], json!(14));
        assert_eq!(
            entry(&path, HISTORY)["commits"][0]["doc"]["nodes"][0]["kind"]["filters"][0]["kind"],
            json!("invert")
        );
        rewrite(&path, |name, value| {
            value["version"] = json!(version);
            if name == HISTORY {
                value["commits"][0]["doc"]["nodes"][0]["parent"] = json!(false);
            }
        });
        assert_eq!(entry(&path, MANIFEST)["version"], json!(version));
        assert_eq!(entry(&path, HISTORY)["version"], json!(version));
        let details: &[&str] = if version == 13 {
            &[
                "PSD-compatible compositing or Invert state requires native and existing history version 14",
            ]
        } else {
            &[
                "Invalid protected history metadata (including Smart Filters and projective mappings):",
                "invalid type: boolean `false`, expected u64",
            ]
        };
        reject_manifest_both(&path, details);
    }
}

#[test]
fn reopened_invert_source_can_be_replaced_undone_and_saved_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.ora");
    let original = document(true);
    ora::write(&original, &path).unwrap();
    let reopened = ora::read(&path).unwrap();
    let mut editor = Editor::new(reopened.clone(), None);
    let replacement = Arc::new(Raster::from_fn(3, 2, [0; 4], |x, _| {
        [30000, x as u16 * 14000, 10000, 50000]
    }));
    emulsion_core::photo_source::replace(&mut editor, 1, replacement).unwrap();
    let edited = editor.doc.clone();
    assert!(editor.undo());
    assert_eq!(editor.doc, reopened);
    assert!(editor.redo());
    assert_eq!(editor.doc, edited);
    ora::write_full(&editor.doc, Some(&editor.graph), &path).unwrap();
    assert_pixels(&ora::read_full(&path).unwrap().doc, &edited);
    assert_eq!(entry(&path, MANIFEST)["version"], 14);
}
