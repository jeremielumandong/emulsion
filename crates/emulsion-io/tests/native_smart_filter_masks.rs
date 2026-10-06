//! Dedicated Smart Filter mask persistence, resource validation and appearance exports.
use emulsion_core::{
    Command, Document, MaskProperties, Node, NodeKind, SmartFilterMask, VectorMask,
    command::Slot,
    graph::Graph,
    node::SmartEditable,
    project::{ProjectEditor, ProjectKind},
};
use emulsion_filters::Filter;
use emulsion_io::{export, lottie, ora, pptx, project_export, psd, smart_source};
use emulsion_raster::{Mask, Placement, Raster, composite::flatten};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MANIFEST: &str = "emulsion.json";
const HISTORY: &str = "history/graph.json";

fn fixture(active: bool) -> Document {
    let mut doc = Document::new(12, 10);
    doc.source_depth = 16;
    let source = Arc::new(Raster::from_srgba8(8, 6, &[210, 80, 35, 180].repeat(48)));
    let mut node = Node::smart(
        0,
        "Filtered photograph",
        source,
        if active {
            vec![Filter::GaussianBlur { radius: 1.0 }]
        } else {
            vec![]
        },
        Placement::at(2.0, 2.0),
    );
    let NodeKind::Smart { filter_mask, .. } = &mut node.kind else {
        unreachable!()
    };
    *filter_mask = Some(SmartFilterMask {
        pixels: Arc::new(Mask::from_fn(7, 5, 127, |x, y| {
            ((x * 37 + y * 11) % 256) as u8
        })),
        enabled: true,
        linked: false,
        transform: emulsion_core::mapping::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.125, -0.25, 1.0, -0.5, 0.75,
        ])),
        properties: MaskProperties {
            density: 0.65,
            feather: 1.25,
        },
    });
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    doc
}

fn mask(doc: &Document) -> &SmartFilterMask {
    let NodeKind::Smart {
        filter_mask: Some(mask),
        ..
    } = &doc.nodes[0].kind
    else {
        panic!("filter mask")
    };
    mask
}

fn mask_mut(doc: &mut Document) -> &mut SmartFilterMask {
    let NodeKind::Smart {
        filter_mask: Some(mask),
        ..
    } = &mut doc.nodes[0].kind
    else {
        panic!("filter mask")
    };
    mask
}

fn remove_mask(doc: &mut Document) {
    let NodeKind::Smart { filter_mask, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *filter_mask = None;
}

fn entry(path: &Path, name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = vec![];
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

fn json_entry(path: &Path, name: &str) -> Value {
    serde_json::from_slice(&entry(path, name)).unwrap()
}

fn rewrite(path: &Path, mut edit: impl FnMut(&str, Vec<u8>) -> Option<Vec<u8>>) {
    let mut source = ZipArchive::new(Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut file = source.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).unwrap();
        if let Some(bytes) = edit(&name, bytes) {
            output
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            output.write_all(&bytes).unwrap();
        }
    }
    std::fs::write(path, output.finish().unwrap().into_inner()).unwrap();
}

fn assert_native_raster(actual: &Raster, expected: &Raster) {
    assert_eq!(
        (actual.width(), actual.height(), actual.fill()),
        (expected.width(), expected.height(), expected.fill())
    );
    assert_eq!(
        actual.read_rect(actual.bounds()),
        expected.read_rect(expected.bounds())
    );
}

fn assert_mask(actual: &SmartFilterMask, expected: &SmartFilterMask) {
    assert_eq!(actual.enabled, expected.enabled);
    assert_eq!(actual.linked, expected.linked);
    assert_eq!(actual.transform, expected.transform);
    assert_eq!(actual.properties, expected.properties);
    assert_eq!(
        (
            actual.pixels.width(),
            actual.pixels.height(),
            actual.pixels.fill()
        ),
        (
            expected.pixels.width(),
            expected.pixels.height(),
            expected.pixels.fill()
        )
    );
    assert_eq!(actual.pixels.to_gray8(), expected.pixels.to_gray8());
}

#[test]
fn native_filter_masks_roundtrip_live_working_history_and_shared_raw_planes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filter.ora");
    for active in [false, true] {
        for enabled in [false, true] {
            let mut original = fixture(active);
            mask_mut(&mut original).enabled = enabled;
            // Exercise reuse of the existing mask pool across component types.
            original.nodes[0].mask = Some(mask(&original).pixels.clone());
            let mut graph = Graph::new(original.clone(), "Original");
            let mut committed = original.clone();
            mask_mut(&mut committed).properties.density = 0.35;
            graph.record(&committed, "Filter density", false).unwrap();
            let mut working = committed.clone();
            mask_mut(&mut working).linked = true;
            for doc in [&original, &committed, &working] {
                for history in [None, Some(&graph)] {
                    ora::write_full(doc, history, &path).unwrap();
                    let reopened = ora::read_full(&path).unwrap();
                    assert!(reopened.history_error.is_none());
                    assert_mask(mask(&reopened.doc), mask(doc));
                    assert_eq!(
                        flatten(&reopened.doc.composite_tree(), 0).to_srgba8(),
                        flatten(&doc.composite_tree(), 0).to_srgba8()
                    );
                    let manifest = json_entry(&path, MANIFEST);
                    assert_eq!(manifest["version"], 12);
                    assert_ne!(
                        manifest["nodes"][0]["mask"],
                        manifest["nodes"][0]["kind"]["filter_mask"]["pixels"]
                    );
                    if let Some(restored) = reopened.graph {
                        let encoded = json_entry(&path, HISTORY);
                        assert_eq!(encoded["masks"].as_array().unwrap().len(), 1);
                        for (actual, expected) in restored.commits().zip(graph.commits()) {
                            assert_mask(mask(&actual.doc), mask(&expected.doc));
                            assert!(Arc::ptr_eq(
                                &mask(&actual.doc).pixels,
                                &mask(&reopened.doc).pixels
                            ));
                            assert!(Arc::ptr_eq(
                                actual.doc.nodes[0].mask.as_ref().unwrap(),
                                &mask(&reopened.doc).pixels
                            ));
                            let NodeKind::Smart {
                                source: actual_source,
                                cache: actual_cache,
                                offset: actual_offset,
                                ..
                            } = &actual.doc.nodes[0].kind
                            else {
                                unreachable!()
                            };
                            let NodeKind::Smart {
                                source: expected_source,
                                cache: expected_cache,
                                offset: expected_offset,
                                ..
                            } = &expected.doc.nodes[0].kind
                            else {
                                unreachable!()
                            };
                            assert_eq!(actual_offset, expected_offset);
                            for (actual, expected) in [
                                (actual_source, expected_source),
                                (actual_cache, expected_cache),
                            ] {
                                assert_eq!(
                                    (actual.width(), actual.height(), actual.fill()),
                                    (expected.width(), expected.height(), expected.fill())
                                );
                                assert_eq!(
                                    actual.read_rect(actual.bounds()),
                                    expected.read_rect(expected.bounds()),
                                    "history must preserve native premultiplied S and F samples exactly"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn filter_mask_version_matrix_includes_disabled_dormant_and_removed_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("versions.ora");
    let mut defaults = fixture(false);
    remove_mask(&mut defaults);
    let mut raster = defaults.clone();
    raster.nodes[0].mask_properties.density = 0.5;
    let mut vector = defaults.clone();
    vector.nodes[0].vector_mask = Some(VectorMask::default());
    let mut filter = fixture(false);
    mask_mut(&mut filter).enabled = false;
    mask_mut(&mut filter).pixels = Arc::new(Mask::empty(8, 6, 255));
    let default_graph = Graph::new(defaults.clone(), "Default");
    let mut filter_graph = Graph::new(filter.clone(), "Dormant filter mask");
    filter_graph
        .record(&defaults, "Delete filter mask", false)
        .unwrap();
    for (doc, graph, version) in [
        (&defaults, None, 9),
        (&raster, None, 10),
        (&vector, None, 11),
        (&filter, None, 12),
        (&filter, Some(&default_graph), 12),
        (&defaults, Some(&filter_graph), 12),
        (&vector, Some(&filter_graph), 12),
    ] {
        ora::write_full(doc, graph, &path).unwrap();
        assert_eq!(json_entry(&path, MANIFEST)["version"], version);
        if graph.is_some() {
            assert_eq!(json_entry(&path, HISTORY)["version"], version);
        }
        let reopened = ora::read_full(&path).unwrap();
        assert!(reopened.history_error.is_none());
    }
    // Missing optional field in an older file remains a mask-free Smart node.
    ora::write(&defaults, &path).unwrap();
    assert!(
        json_entry(&path, MANIFEST)["nodes"][0]["kind"]
            .get("filter_mask")
            .is_none()
    );
    assert!(matches!(
        &ora::read(&path).unwrap().nodes[0].kind,
        NodeKind::Smart {
            filter_mask: None,
            ..
        }
    ));
}

#[test]
fn filter_descriptors_reject_pre12_and_future_versions_before_schema_or_resources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("versions.ora");
    let doc = fixture(false);
    let graph = Graph::new(doc.clone(), "Filter mask");
    for target in [MANIFEST, HISTORY] {
        for version in [11, ora::FORMAT_VERSION + 1] {
            ora::write_full(&doc, Some(&graph), &path).unwrap();
            rewrite(&path, |name, bytes| {
                if name != target {
                    return Some(bytes);
                }
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = json!(version);
                if version > ora::FORMAT_VERSION {
                    value = json!({"version": version});
                }
                Some(serde_json::to_vec(&value).unwrap())
            });
            if version > ora::FORMAT_VERSION {
                // Unknown history versions may retain features absent from the
                // live snapshot. The v13 archive preflight must fail closed,
                // rather than silently dropping that future history.
                assert!(
                    matches!(ora::read_full(&path), Err(emulsion_io::IoError::TooNew(v)) if v == version)
                );
                assert!(
                    matches!(ora::read(&path), Err(emulsion_io::IoError::TooNew(v)) if v == version)
                );
            } else if target == MANIFEST {
                assert!(ora::read_full(&path).is_err());
            } else {
                let restored = ora::read_full(&path).unwrap();
                assert!(restored.graph.is_none());
                assert!(restored.history_error.is_some());
                assert_mask(mask(&restored.doc), mask(&doc));
            }
        }
    }
}

#[test]
fn malformed_filter_mask_metadata_resources_and_history_indices_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("malformed.ora");
    let doc = fixture(false);
    let graph = Graph::new(doc.clone(), "Disabled validation");
    for (field, value) in [
        ("width", json!(0)),
        ("width", json!(30001)),
        ("height", json!(9)),
        ("fill", json!(256)),
        ("enabled", json!("false")),
        ("linked", json!(0)),
        ("transform", json!([0, 0, 0, 0, 0, 0])),
        ("transform", json!([1, 0, 0, 1, null, 0])),
        ("properties", json!({"density": 1.1, "feather": 0})),
        ("properties", json!({"density": 1, "feather": 1001})),
        ("pixels", json!("../filter.png")),
        ("pixels", json!("emulsion/filter-mask-missing.png")),
    ] {
        ora::write(&doc, &path).unwrap();
        rewrite(&path, |name, bytes| {
            if name != MANIFEST {
                return Some(bytes);
            }
            let mut manifest: Value = serde_json::from_slice(&bytes).unwrap();
            manifest["nodes"][0]["kind"]["filter_mask"][field] = value.clone();
            Some(serde_json::to_vec(&manifest).unwrap())
        });
        assert!(ora::read(&path).is_err(), "accepted {field}: {value}");
    }
    for (field, value) in [
        ("pixels", json!(999)),
        ("width", json!(9)),
        ("fill", json!(0)),
    ] {
        ora::write_full(&doc, Some(&graph), &path).unwrap();
        rewrite(&path, |name, bytes| {
            if name != HISTORY {
                return Some(bytes);
            }
            let mut history: Value = serde_json::from_slice(&bytes).unwrap();
            history["commits"][0]["doc"]["nodes"][0]["kind"]["filter_mask"][field] = value.clone();
            Some(serde_json::to_vec(&history).unwrap())
        });
        let restored = ora::read_full(&path).unwrap();
        assert!(restored.history_error.is_some());
        assert_mask(mask(&restored.doc), mask(&doc));
    }
    for corrupt_instead_of_remove in [false, true] {
        ora::write(&doc, &path).unwrap();
        rewrite(&path, |name, bytes| {
            if !name.starts_with("emulsion/filter-mask-") {
                return Some(bytes);
            }
            corrupt_instead_of_remove.then(|| b"invalid PNG".to_vec())
        });
        assert!(ora::read(&path).is_err());
    }
}

#[test]
fn filter_masks_native_previews_generic_ora_flat_exports_and_psd_match_appearance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("appearance.ora");
    for filter_enabled in [false, true] {
        for raster_enabled in [false, true] {
            for vector_enabled in [false, true] {
                let mut doc = fixture(true);
                mask_mut(&mut doc).enabled = filter_enabled;
                doc.nodes[0].mask = Some(Arc::new(Mask::from_fn(8, 6, 255, |x, _| {
                    if x < 4 { 255 } else { 80 }
                })));
                doc.nodes[0].mask_enabled = raster_enabled;
                doc.nodes[0].vector_mask = Some(VectorMask {
                    enabled: vector_enabled,
                    properties: MaskProperties {
                        density: 0.4,
                        feather: 0.0,
                    },
                    ..VectorMask::empty(emulsion_core::EmptyVectorCoverage::HideAll)
                });
                let expected = flatten(&doc.composite_tree(), 0);
                ora::write(&doc, &path).unwrap();
                assert!(
                    String::from_utf8(entry(&path, "stack.xml"))
                        .unwrap()
                        .contains("Appearance (editable layers in Emulsion)")
                );
                for name in ["mergedimage.png", "Thumbnails/thumbnail.png"] {
                    assert_eq!(
                        image::load_from_memory(&entry(&path, name))
                            .unwrap()
                            .to_rgba8()
                            .into_raw(),
                        expected.to_srgba8()
                    );
                }
                // Layer resources contain the effective Smart pixels alone;
                // layer coverage is applied once by the named merged fallback.
                let pixels = emulsion_core::smart_filter_mask::effective_pixels(&doc.nodes[0])
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    image::load_from_memory(&entry(
                        &path,
                        &format!("data/node-{}.png", doc.nodes[0].id)
                    ))
                    .unwrap()
                    .to_rgba16()
                    .into_raw(),
                    pixels.to_srgba16()
                );
                rewrite(
                    &path,
                    |name, bytes| if name == MANIFEST { None } else { Some(bytes) },
                );
                // Generic ORA and PSD carry an 8-bit straight-alpha fallback.
                // Their decoder re-premultiplies those samples into native u16;
                // zero-alpha RGB disappears and tiny-alpha RGB can requantize.
                // Compare that exact representation, without a tolerance, while
                // the serialized preview-byte checks above remain independent.
                let decoded_fallback =
                    Raster::from_srgba8(doc.width, doc.height, &expected.to_srgba8());
                assert_native_raster(
                    &flatten(&ora::read(&path).unwrap().composite_tree(), 0),
                    &decoded_fallback,
                );
                for extension in ["png", "tiff"] {
                    let path = dir.path().join(format!("appearance.{extension}"));
                    export::export(
                        &doc,
                        &path,
                        export::ExportOptions {
                            depth: 16,
                            jpeg_quality: 92,
                        },
                    )
                    .unwrap();
                    assert_eq!(
                        image::open(&path).unwrap().to_rgba16().into_raw(),
                        expected.to_srgba16()
                    );
                }
                assert!(psd::needs_appearance_fallback(&doc));
                let psd_path = dir.path().join("appearance.psd");
                psd::write(&doc, &psd_path).unwrap();
                let serialized = ag_psd::read_psd(
                    &std::fs::read(&psd_path).unwrap(),
                    &ag_psd::psd::ReadOptions {
                        skip_thumbnail: Some(true),
                        // The PSD composite has its own white-matte encoding;
                        // inspect the exact editable fallback layer samples.
                        skip_composite_image_data: Some(true),
                        skip_linked_files_data: Some(true),
                        use_image_data: Some(true),
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(serialized.bits_per_channel, Some(8.0));
                let layers = serialized.children.as_ref().unwrap();
                assert_eq!(layers.len(), 1);
                let pixels = layers[0].image_data.as_ref().unwrap();
                assert_eq!((pixels.width, pixels.height), (doc.width, doc.height));
                assert_eq!(pixels.data, expected.to_srgba8());
                let restored = psd::read(&psd_path).unwrap();
                assert_eq!(restored.nodes.len(), 1);
                assert!(restored.nodes[0].name.contains("appearance"));
                assert_native_raster(&flatten(&restored.composite_tree(), 0), &decoded_fallback);
                assert!(project_export::svg(&doc).unwrap().1);
            }
        }
    }
}

#[test]
fn disabled_and_dormant_descriptors_still_choose_conservative_interchange() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dormant.ora");
    for active in [false, true] {
        for enabled in [false, true] {
            let mut doc = fixture(active);
            mask_mut(&mut doc).enabled = enabled;
            ora::write(&doc, &path).unwrap();
            assert!(
                String::from_utf8(entry(&path, "stack.xml"))
                    .unwrap()
                    .contains("Appearance (editable layers in Emulsion)")
            );
            assert!(psd::needs_appearance_fallback(&doc));
            let error = lottie::encode(&doc).unwrap_err().to_string();
            assert!(error.contains("masks"), "{error}");
        }
    }
}

#[test]
fn powerpoint_picture_uses_effective_filter_pixels_and_expanded_cache_placement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filter.pptx");
    let doc = fixture(true);
    let expected = emulsion_core::smart_filter_mask::effective_pixels(&doc.nodes[0])
        .unwrap()
        .unwrap();
    let NodeKind::Smart { offset, .. } = &doc.nodes[0].kind else {
        panic!("Smart fixture")
    };
    assert!(expected.width() > 8 && expected.height() > 6);
    // This fixture's source has identity scale/rotation at (2, 2). Its expanded
    // pixels must move by the cache origin, independently of the exporter helper.
    let expected_placement = Placement::at(2. + f64::from(offset.0), 2. + f64::from(offset.1));
    let project = ProjectEditor::new_project(ProjectKind::Design, doc)
        .unwrap()
        .snapshot()
        .unwrap();
    let report = pptx::write(&project, &[project.pages[0].meta.id], &path).unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("rendered filter appearance"))
    );
    let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let name = (0..zip.len())
        .find_map(|i| {
            let name = zip.by_index(i).unwrap().name().to_owned();
            name.starts_with("ppt/media/").then_some(name)
        })
        .unwrap();
    let mut png = vec![];
    zip.by_name(&name).unwrap().read_to_end(&mut png).unwrap();
    let encoded = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(
        (encoded.width(), encoded.height()),
        (expected.width(), expected.height())
    );
    assert_eq!(encoded.into_raw(), expected.to_srgba8());
    let restored = pptx::read(&path).unwrap();
    let slide = &restored.project.pages[0].doc;
    assert_eq!((slide.width, slide.height), (12, 10));
    assert_eq!(slide.nodes.len(), 2);
    assert!(matches!(
        slide.nodes[0].kind,
        NodeKind::Fill {
            rgba: [255, 255, 255, 255]
        }
    ));
    let NodeKind::Raster { raster, placement } = &slide.nodes[1].kind else {
        panic!("Smart appearance must reopen as a picture")
    };
    let decoded = Raster::from_srgba8(expected.width(), expected.height(), &expected.to_srgba8());
    assert_native_raster(raster, &decoded);
    assert_eq!(*placement, expected_placement);

    // PowerPoint's default slide background is white. Retain the whole-slide
    // equality check against an explicit decoded-picture-over-white reference,
    // rather than expecting the source document's transparent background.
    let mut reference = Document::new(12, 10);
    reference.nodes.push(Node::new(
        1,
        "White slide background",
        NodeKind::Fill { rgba: [255; 4] },
    ));
    reference.nodes.push(Node::raster(
        2,
        "Decoded Smart appearance",
        Arc::new(decoded),
        expected_placement,
    ));
    reference.next_id = 3;
    reference.validate().unwrap();
    assert_native_raster(
        &flatten(&slide.composite_tree(), 0),
        &flatten(&reference.composite_tree(), 0),
    );
}

#[test]
fn opaque_nested_source_versions_and_bytes_remain_independent_of_host() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("child.ora");
    let host_path = dir.path().join("host.ora");
    let child = fixture(false);
    for version in [12, ora::FORMAT_VERSION + 1] {
        ora::write(&child, &source_path).unwrap();
        if version > ora::FORMAT_VERSION {
            rewrite(&source_path, |name, bytes| {
                if name != MANIFEST {
                    return Some(bytes);
                }
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = json!(version);
                Some(serde_json::to_vec(&value).unwrap())
            });
        }
        let archive = Arc::new(std::fs::read(&source_path).unwrap());
        let mut host = fixture(false);
        remove_mask(&mut host);
        let NodeKind::Smart { editable, .. } = &mut host.nodes[0].kind else {
            unreachable!()
        };
        *editable = Some(SmartEditable::Document {
            archive: archive.clone(),
            external: None,
        });
        let graph = Graph::new(host.clone(), "Opaque child");
        ora::write_full(&host, Some(&graph), &host_path).unwrap();
        assert_eq!(json_entry(&host_path, MANIFEST)["version"], 9);
        assert_eq!(json_entry(&host_path, HISTORY)["version"], 9);
        let restored = ora::read_full(&host_path).unwrap();
        assert!(restored.history_error.is_none());
        for doc in std::iter::once(&restored.doc)
            .chain(restored.graph.as_ref().unwrap().commits().map(|c| &c.doc))
        {
            let NodeKind::Smart {
                editable:
                    Some(SmartEditable::Document {
                        archive: actual, ..
                    }),
                ..
            } = &doc.nodes[0].kind
            else {
                panic!("nested source")
            };
            assert_eq!(actual.as_slice(), archive.as_slice());
        }
        if version == 12 {
            assert_mask(
                mask(&smart_source::open(&restored.doc, restored.doc.nodes[0].id).unwrap()),
                mask(&child),
            );
        } else {
            assert!(
                matches!(smart_source::open(&restored.doc, restored.doc.nodes[0].id), Err(emulsion_io::IoError::TooNew(v)) if v == version)
            );
        }
    }
}

#[test]
fn filter_mask_history_reopens_with_precise_merge_and_undo_redo() {
    use emulsion_core::{Editor, graph::MergeOutcome};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("branches.ora");
    let doc = fixture(true);
    let id = doc.nodes[0].id;
    let mut editor = Editor::new(doc, None);
    editor.branch("filter-mask").unwrap();
    editor
        .execute(Command::SetSmartFilterMaskProperties {
            id,
            properties: MaskProperties {
                density: 0.25,
                feather: 2.0,
            },
        })
        .unwrap();
    editor.commit("Change filter mask", false);
    editor.checkout("main").unwrap();
    editor
        .execute(Command::Rename {
            id,
            name: "Renamed photograph".into(),
        })
        .unwrap();
    editor.commit("Rename photograph", false);
    ora::write_full(&editor.doc, Some(&editor.graph), &path).unwrap();
    let reopened = ora::read_full(&path).unwrap();
    assert!(reopened.history_error.is_none());
    let mut restored = Editor::with_graph(reopened.doc, Some(path), reopened.graph.unwrap());
    let MergeOutcome::Merged(merged) = restored.merge("filter-mask", &Default::default()).unwrap()
    else {
        panic!("unrelated metadata and Smart Filter mask edits must merge");
    };
    assert_eq!(merged.nodes[0].name, "Renamed photograph");
    assert_eq!(mask(&merged).properties.density, 0.25);
    let before = restored.doc.clone();
    restored
        .execute(Command::SetSmartFilterMaskEnabled { id, enabled: false })
        .unwrap();
    let after = restored.doc.clone();
    assert!(!mask(&after).enabled);
    restored.undo();
    assert_eq!(restored.doc, before);
    restored.redo();
    assert_eq!(restored.doc, after);
}

#[test]
fn invalid_live_or_saved_filter_masks_cannot_replace_an_existing_archive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unchanged.ora");
    let valid = fixture(false);
    for (transform, properties) in [
        ([0.0; 6], MaskProperties::default()),
        (
            [1.0, 0.0, 0.0, 1.0, f64::NAN, 0.0],
            MaskProperties::default(),
        ),
        (
            emulsion_core::node::default_mask_transform(),
            MaskProperties {
                density: f32::NAN,
                feather: 0.0,
            },
        ),
        (
            emulsion_core::node::default_mask_transform(),
            MaskProperties {
                density: 1.0,
                feather: f32::INFINITY,
            },
        ),
    ] {
        let mut invalid = valid.clone();
        let mask = mask_mut(&mut invalid);
        mask.enabled = false;
        mask.transform =
            emulsion_core::mapping::Mapping2::Affine(glam::DAffine2::from_cols_array(&transform));
        mask.properties = properties;
        std::fs::write(&path, b"previous file").unwrap();
        assert!(ora::write(&invalid, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous file");
        let graph = Graph::new(invalid, "Invalid disabled filter mask");
        assert!(ora::write_full(&valid, Some(&graph), &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous file");
    }
}
