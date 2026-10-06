//! Native vector-mask persistence and conservative appearance export contracts.
use base64::Engine as _;
use emulsion_core::{
    Command, Document, EmptyVectorCoverage, MaskProperties, Node, NodeKind, VectorMask,
    command::Slot, graph::Graph,
};
use emulsion_io::{export, ora, project_export, psd};
use emulsion_raster::{
    Mask, Placement, Raster,
    composite::flatten,
    vector::{Anchor, Path as VectorPath, PathStyle, SubPath},
};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MANIFEST: &str = "emulsion.json";
const HISTORY: &str = "history/graph.json";

fn add(doc: &mut Document, node: Node) {
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(doc)
    .unwrap();
}

fn fixture() -> Document {
    let mut doc = Document::new(16, 12);
    let mut node = Node::raster(
        0,
        "Both components",
        Arc::new(Raster::solid(16, 12, [0.7, 0.2, 0.1, 1.0])),
        Placement::default(),
    );
    node.mask = Some(Arc::new(Mask::from_fn(16, 12, 0, |x, y| {
        if x >= 2 && y < 10 { 255 } else { 0 }
    })));
    node.mask_linked = false;
    node.mask_transform =
        emulsion_core::mapping::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.0, 0.125, 1.0, -0.25, 0.5,
        ]));
    node.mask_properties = MaskProperties {
        density: 0.65,
        feather: 1.0,
    };
    node.vector_mask = Some(VectorMask {
        // Open geometry and off-canvas anchors are retained exactly; fill is
        // implicitly closed by the renderer without changing editable geometry.
        path: Arc::new(
            VectorPath::from_svg("M -3.125 1.25 C 2.125 -2.5 11.75 2.25 13.125 8.75 L 3.25 13.5")
                .unwrap(),
        ),
        enabled: true,
        linked: false,
        inverted: true,
        transform: [1.0, 0.125, -0.2, 0.9, 0.75, -0.5],
        properties: MaskProperties {
            density: 0.75,
            feather: 1.25,
        },
        empty_coverage: EmptyVectorCoverage::HideAll,
    });
    add(&mut doc, node);
    doc
}

fn default_document() -> Document {
    let mut doc = fixture();
    doc.nodes[0].vector_mask = None;
    doc.nodes[0].mask_properties = MaskProperties::default();
    doc
}

fn entry(path: &Path, name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

fn manifest(path: &Path, name: &str) -> Value {
    serde_json::from_slice(&entry(path, name)).unwrap()
}

fn rewrite(path: &Path, mut edit: impl FnMut(&str, Vec<u8>) -> Option<Vec<u8>>) {
    let mut source = ZipArchive::new(Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut file = source.by_index(i).unwrap();
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if let Some(bytes) = edit(&name, bytes) {
            output
                .start_file(&name, SimpleFileOptions::default())
                .unwrap();
            output.write_all(&bytes).unwrap();
        }
    }
    std::fs::write(path, output.finish().unwrap().into_inner()).unwrap();
}

fn assert_state(actual: &Document, expected: &Document) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    assert_eq!(actual.nodes.len(), expected.nodes.len());
    for (a, e) in actual.nodes.iter().zip(&expected.nodes) {
        assert_eq!(a.vector_mask, e.vector_mask);
        assert_eq!(a.mask_enabled, e.mask_enabled);
        assert_eq!(a.mask_linked, e.mask_linked);
        assert_eq!(a.mask_transform, e.mask_transform);
        assert_eq!(a.mask_properties, e.mask_properties);
        assert_eq!(
            a.mask.as_ref().map(|m| (m.width(), m.height(), m.fill())),
            e.mask.as_ref().map(|m| (m.width(), m.height(), m.fill()))
        );
        assert_eq!(
            a.mask.as_ref().map(|m| m.to_gray8()),
            e.mask.as_ref().map(|m| m.to_gray8())
        );
    }
    assert_eq!(
        flatten(&actual.composite_tree(), 0).to_srgba8(),
        flatten(&expected.composite_tree(), 0).to_srgba8()
    );
}

#[test]
fn native_vector_masks_roundtrip_all_enable_combinations_and_exact_working_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("both.ora");
    for raster_enabled in [false, true] {
        for vector_enabled in [false, true] {
            let mut original = fixture();
            original.nodes[0].mask_enabled = raster_enabled;
            original.nodes[0].vector_mask.as_mut().unwrap().enabled = vector_enabled;
            let mut graph = Graph::new(original.clone(), "Original components");
            let mut committed = original.clone();
            committed.nodes[0]
                .vector_mask
                .as_mut()
                .unwrap()
                .properties
                .density = 0.5;
            graph.record(&committed, "Vector density", false).unwrap();
            let mut working = committed.clone();
            working.nodes[0].vector_mask.as_mut().unwrap().linked = true;
            working.nodes[0].vector_mask.as_mut().unwrap().inverted = false;
            for doc in [&original, &committed, &working] {
                for history in [None, Some(&graph)] {
                    ora::write_full(doc, history, &path).unwrap();
                    let reopened = ora::read_full(&path).unwrap();
                    assert!(reopened.history_error.is_none());
                    assert_state(&reopened.doc, doc);
                    if let Some(restored) = reopened.graph {
                        for (actual, expected) in restored.commits().zip(graph.commits()) {
                            assert_state(&actual.doc, &expected.doc);
                            assert!(Arc::ptr_eq(
                                &actual.doc.nodes[0].vector_mask.as_ref().unwrap().path,
                                &reopened.doc.nodes[0].vector_mask.as_ref().unwrap().path,
                            ));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn native_vector_masks_version_matrix_includes_empty_disabled_and_removed_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("versions.ora");
    let defaults = default_document();
    let mut raster = defaults.clone();
    raster.nodes[0].mask_properties.density = 0.4;
    let mut vector = defaults.clone();
    vector.nodes[0].vector_mask = Some(VectorMask {
        enabled: false,
        ..VectorMask::empty(EmptyVectorCoverage::HideAll)
    });
    let default_graph = Graph::new(defaults.clone(), "Legacy");
    let mut raster_graph = Graph::new(raster.clone(), "Raster properties");
    raster_graph
        .record(&defaults, "Reset raster properties", false)
        .unwrap();
    let mut vector_graph = Graph::new(vector.clone(), "Disabled empty vector");
    vector_graph
        .record(&defaults, "Remove vector component", false)
        .unwrap();
    for (doc, graph, native_version, history_version) in [
        (&defaults, None, 9, None),
        (&defaults, Some(&default_graph), 9, Some(9)),
        (&raster, None, 10, None),
        (&raster, Some(&default_graph), 10, Some(10)),
        (&defaults, Some(&raster_graph), 10, Some(10)),
        (&vector, None, 11, None),
        (&vector, Some(&default_graph), 11, Some(11)),
        (&defaults, Some(&vector_graph), 11, Some(11)),
        (&raster, Some(&vector_graph), 11, Some(11)),
    ] {
        ora::write_full(doc, graph, &path).unwrap();
        assert_eq!(manifest(&path, MANIFEST)["version"], native_version);
        if let Some(version) = history_version {
            assert_eq!(manifest(&path, HISTORY)["version"], version);
        }
        let reopened = ora::read_full(&path).unwrap();
        assert!(reopened.history_error.is_none());
        assert_state(&reopened.doc, doc);
    }
    assert_eq!(ora::FORMAT_VERSION, 16);
    assert_eq!(emulsion_io::history::HISTORY_VERSION, 16);
}

#[test]
fn native_vector_masks_empty_base_and_inversion_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.ora");
    for base in [EmptyVectorCoverage::RevealAll, EmptyVectorCoverage::HideAll] {
        for inverted in [false, true] {
            let mut doc = default_document();
            doc.nodes[0].mask = None;
            doc.nodes[0].vector_mask = Some(VectorMask {
                inverted,
                ..VectorMask::empty(base)
            });
            let expected = if (base == EmptyVectorCoverage::RevealAll) != inverted {
                255
            } else {
                0
            };
            assert!(
                flatten(&doc.composite_tree(), 0)
                    .to_srgba8()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[3] == expected)
            );
            ora::write(&doc, &path).unwrap();
            assert_state(&ora::read(&path).unwrap(), &doc);
        }
    }
}

#[test]
fn native_vector_masks_share_path_blobs_and_live_history_arcs_without_matching_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.ora");
    let mut doc = fixture();
    let geometry = doc.nodes[0].vector_mask.as_ref().unwrap().path.clone();
    let mut outline = Node::path(
        0,
        "Same editable geometry",
        geometry,
        PathStyle::default(),
        doc.width,
        doc.height,
    );
    outline.visible = false;
    add(&mut doc, outline);
    let mut graph = Graph::new(doc.clone(), "Shared path");
    doc.nodes[0].vector_mask.as_mut().unwrap().enabled = false;
    graph.record(&doc, "Disable vector", false).unwrap();
    ora::write_full(&doc, Some(&graph), &path).unwrap();
    let live = manifest(&path, MANIFEST);
    assert_eq!(
        live["nodes"][0]["vector_mask"]["path"],
        live["nodes"][1]["kind"]["path"]
    );
    let zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(
        zip.file_names()
            .filter(|n| n.starts_with("emulsion/paths/"))
            .count(),
        1
    );
    drop(zip);
    // Force the reader to keep the decoded live document rather than taking
    // a cloned matching history tip, which would conceal reader-pool mistakes.
    rewrite(&path, |name, bytes| {
        if name != MANIFEST {
            return Some(bytes);
        }
        let mut live: Value = serde_json::from_slice(&bytes).unwrap();
        live["nodes"][0]["name"] = json!("Externally renamed live layer");
        Some(serde_json::to_vec(&live).unwrap())
    });
    let reopened = ora::read_full(&path).unwrap();
    assert!(reopened.history_error.is_none());
    assert_eq!(reopened.doc.nodes[0].name, "Externally renamed live layer");
    let mask_path = &reopened.doc.nodes[0].vector_mask.as_ref().unwrap().path;
    let NodeKind::Path {
        path: content_path, ..
    } = &reopened.doc.nodes[1].kind
    else {
        panic!("path layer")
    };
    assert!(Arc::ptr_eq(mask_path, content_path));
    for commit in reopened.graph.unwrap().commits() {
        assert!(Arc::ptr_eq(
            mask_path,
            &commit.doc.nodes[0].vector_mask.as_ref().unwrap().path
        ));
    }
}

#[test]
fn legacy_native_and_history_files_default_to_no_vector_component() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.ora");
    let doc = default_document();
    let graph = Graph::new(doc.clone(), "Legacy mask");
    for version in 1..=10 {
        ora::write_full(&doc, Some(&graph), &path).unwrap();
        rewrite(&path, |name, bytes| {
            if ![MANIFEST, HISTORY].contains(&name) {
                return Some(bytes);
            }
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["version"] = json!(version);
            Some(serde_json::to_vec(&value).unwrap())
        });
        let reopened = ora::read_full(&path).unwrap();
        assert!(
            reopened.history_error.is_none(),
            "v{version}: {:?}",
            reopened.history_error
        );
        assert_state(&reopened.doc, &doc);
        assert!(reopened.doc.nodes.iter().all(|n| n.vector_mask.is_none()));
        assert!(
            reopened.graph.unwrap().commits().all(|c| c
                .doc
                .nodes
                .iter()
                .all(|n| n.vector_mask.is_none()))
        );
    }
}

#[test]
fn malformed_vector_masks_fail_live_reads_and_isolate_damaged_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("malformed.ora");
    let doc = fixture();
    let graph = Graph::new(doc.clone(), "Valid vector");
    let invalid_fields = [
        ("transform", json!([0, 0, 0, 0, 0, 0])),
        ("transform", json!([1, 0, 0, 1, null, 0])),
        // Finite coefficients are insufficient when the supported intrinsic
        // coordinate envelope maps to infinity. Large translation alone is valid.
        ("transform", json!([1e300, 0, 0, 1, 0, 0])),
        ("properties", json!({"density": -0.1, "feather": 0})),
        ("properties", json!({"density": 1.1, "feather": 0})),
        ("properties", json!({"density": 0.5, "feather": -1})),
        (
            "properties",
            json!({"density": 0.5, "feather": emulsion_core::MAX_MASK_FEATHER + 1.0}),
        ),
        ("empty_coverage", json!("unknown")),
        ("path", json!("../../outside.bin")),
        ("path", json!("emulsion/paths/missing.bin")),
        (
            "path",
            json!({"subpaths":[{"closed":false,"anchors":[{"p":[1e10,0],"h_in":[0,0],"h_out":[0,0],"smooth":false}]}]}),
        ),
    ];
    for (field, bad) in invalid_fields {
        for target in [MANIFEST, HISTORY] {
            ora::write_full(&doc, Some(&graph), &path).unwrap();
            rewrite(&path, |name, bytes| {
                if name != target {
                    return Some(bytes);
                }
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                let node = if name == MANIFEST {
                    &mut value["nodes"][0]
                } else {
                    &mut value["commits"][0]["doc"]["nodes"][0]
                };
                node["vector_mask"][field] = bad.clone();
                Some(serde_json::to_vec(&value).unwrap())
            });
            if target == MANIFEST {
                assert!(
                    ora::read_full(&path).is_err(),
                    "accepted invalid live {field}: {bad}"
                );
            } else {
                let reopened = ora::read_full(&path).unwrap();
                assert!(
                    reopened.history_error.is_some(),
                    "accepted invalid history {field}: {bad}"
                );
                assert!(reopened.graph.is_none());
                assert_state(&reopened.doc, &doc);
            }
        }
    }
}

#[test]
fn large_vector_affine_translations_preserve_visible_coverage_native_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("large-affine.ora");
    for translation in [-1e10, 1e10] {
        let mut far = fixture();
        far.nodes[0].mask = None;
        let x = (2. - translation) / 16.;
        let path = Arc::new(VectorPath {
            subpaths: vec![SubPath {
                anchors: [(x, 2.), (x + 0.75, 2.), (x + 0.75, 10.), (x, 10.)]
                    .map(Anchor::corner)
                    .to_vec(),
                closed: true,
            }],
        });
        far.nodes[0].vector_mask = Some(VectorMask {
            path: path.clone(),
            transform: [1., 0., 0., 1., translation, 0.],
            linked: false,
            ..Default::default()
        });
        far.validate().unwrap();
        assert!(
            far.vector_mask_for_inspection(&far.nodes[0])
                .unwrap()
                .unwrap()
                .to_gray8()
                .iter()
                .all(|value| *value == 0)
        );
        let mut visible = far.clone();
        Command::SetVectorMaskTransform {
            id: visible.nodes[0].id,
            transform: [16., 0., 0., 1., translation, 0.],
        }
        .apply(&mut visible)
        .unwrap();
        assert!(Arc::ptr_eq(
            &visible.nodes[0].vector_mask.as_ref().unwrap().path,
            &path
        ));
        let expected = Mask::from_fn(16, 12, 0, |x, y| {
            if (2..14).contains(&x) && (2..10).contains(&y) {
                255
            } else {
                0
            }
        });
        assert_eq!(
            visible
                .vector_mask_for_inspection(&visible.nodes[0])
                .unwrap()
                .unwrap()
                .to_gray8(),
            expected.to_gray8()
        );
        let mut graph = Graph::new(far.clone(), "Far off canvas");
        graph
            .record(&visible, "Reveal authored path", false)
            .unwrap();
        let mut working = visible.clone();
        working.nodes[0].vector_mask.as_mut().unwrap().inverted = true;
        for doc in [&far, &visible, &working] {
            for history in [None, Some(&graph)] {
                ora::write_full(doc, history, &file).unwrap();
                let reopened = ora::read_full(&file).unwrap();
                assert!(reopened.history_error.is_none());
                assert_eq!(reopened.graph.is_some(), history.is_some());
                assert_state(&reopened.doc, doc);
                if let Some(restored) = reopened.graph {
                    assert_eq!(restored.len(), graph.len());
                    for (actual, expected) in restored.commits().zip(graph.commits()) {
                        assert_state(&actual.doc, &expected.doc);
                        assert!(Arc::ptr_eq(
                            &actual.doc.nodes[0].vector_mask.as_ref().unwrap().path,
                            &reopened.doc.nodes[0].vector_mask.as_ref().unwrap().path,
                        ));
                    }
                }
            }
        }
    }
}

#[test]
fn vector_fields_require_v11_and_future_versions_fail_before_opening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("version-rejection.ora");
    let doc = fixture();
    let graph = Graph::new(doc.clone(), "Vector");
    for version in [10, ora::FORMAT_VERSION + 1] {
        for target in [MANIFEST, HISTORY] {
            ora::write_full(&doc, Some(&graph), &path).unwrap();
            rewrite(&path, |name, bytes| {
                if name != target {
                    return Some(bytes);
                }
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = json!(version);
                Some(serde_json::to_vec(&value).unwrap())
            });
            if version > ora::FORMAT_VERSION {
                // Unknown future history can carry features absent from the
                // live stack, so both native readers must reject the archive.
                assert!(
                    matches!(ora::read_full(&path), Err(emulsion_io::IoError::TooNew(v)) if v == version)
                );
                assert!(
                    matches!(ora::read(&path), Err(emulsion_io::IoError::TooNew(v)) if v == version)
                );
            } else if target == MANIFEST {
                assert!(ora::read_full(&path).is_err());
            } else {
                let reopened = ora::read_full(&path).unwrap();
                assert!(reopened.history_error.is_some());
                assert_state(&reopened.doc, &doc);
            }
        }
    }
}

#[test]
fn invalid_vector_geometry_and_parameters_are_rejected_before_serialization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.ora");
    let valid = default_document();
    let mut invalid = Vec::new();
    for coordinate in [f64::NAN, f64::INFINITY, 1e10] {
        invalid.push(VectorMask {
            path: Arc::new(VectorPath {
                subpaths: vec![SubPath {
                    closed: false,
                    anchors: vec![Anchor::corner((coordinate, 0.0))],
                }],
            }),
            ..Default::default()
        });
    }
    invalid.push(VectorMask {
        path: Arc::new(VectorPath {
            subpaths: vec![
                SubPath {
                    closed: false,
                    anchors: vec![]
                };
                emulsion_raster::vector::MAX_ANCHORS + 1
            ],
        }),
        ..Default::default()
    });
    for transform in [
        [0.0; 6],
        [1.0, 0.0, 0.0, 1.0, f64::NAN, 0.0],
        [1e300, 0.0, 0.0, 1.0, 0.0, 0.0],
    ] {
        invalid.push(VectorMask {
            transform,
            ..Default::default()
        });
    }
    for properties in [
        MaskProperties {
            density: f32::NAN,
            feather: 0.0,
        },
        MaskProperties {
            density: 1.0,
            feather: f32::INFINITY,
        },
        MaskProperties {
            density: 1.0,
            feather: emulsion_core::MAX_MASK_FEATHER + 1.0,
        },
    ] {
        invalid.push(VectorMask {
            properties,
            ..Default::default()
        });
    }
    for mut mask in invalid {
        mask.enabled = false;
        let mut doc = valid.clone();
        doc.nodes[0].vector_mask = Some(mask);
        assert!(ora::write(&doc, &path).is_err());
        let graph = Graph::new(doc, "Invalid disabled vector");
        assert!(ora::write_full(&valid, Some(&graph), &path).is_err());
    }
}

#[test]
fn vector_masks_standard_ora_previews_png_tiff_psd_and_svg_use_combined_appearance() {
    let dir = tempfile::tempdir().unwrap();
    let ora_path = dir.path().join("appearance.ora");
    for raster_enabled in [false, true] {
        for vector_enabled in [false, true] {
            let mut doc = fixture();
            doc.nodes[0].mask_enabled = raster_enabled;
            doc.nodes[0].vector_mask.as_mut().unwrap().enabled = vector_enabled;
            let expected = flatten(&doc.composite_tree(), 0);
            ora::write(&doc, &ora_path).unwrap();
            assert!(
                String::from_utf8(entry(&ora_path, "stack.xml"))
                    .unwrap()
                    .contains("Appearance (editable layers in Emulsion)")
            );
            for name in ["mergedimage.png", "Thumbnails/thumbnail.png"] {
                assert_eq!(
                    image::load_from_memory(&entry(&ora_path, name))
                        .unwrap()
                        .to_rgba8()
                        .into_raw(),
                    expected.to_srgba8()
                );
            }
            rewrite(
                &ora_path,
                |name, bytes| if name == MANIFEST { None } else { Some(bytes) },
            );
            assert_eq!(
                flatten(&ora::read(&ora_path).unwrap().composite_tree(), 0).to_srgba8(),
                expected.to_srgba8()
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
            let reopened = psd::read(&psd_path).unwrap();
            assert_eq!(reopened.nodes.len(), 1);
            assert!(reopened.nodes[0].name.contains("appearance"));
            assert!(
                !reopened.nodes[0].has_mask(),
                "PSD fallback promises appearance, not editable native masks"
            );
            assert_eq!(
                flatten(&reopened.composite_tree(), 0).to_srgba8(),
                expected.to_srgba8()
            );
            let (svg, fallback) = project_export::svg(&doc).unwrap();
            assert_eq!(fallback, raster_enabled || vector_enabled);
            if fallback {
                let svg = String::from_utf8(svg).unwrap();
                let encoded = svg
                    .split("data:image/png;base64,")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap();
                assert_eq!(
                    image::load_from_memory(&bytes)
                        .unwrap()
                        .to_rgba16()
                        .into_raw(),
                    expected.to_srgba16()
                );
            }
        }
    }
}

#[test]
fn cropped_and_smart_converted_vectors_reopen_with_geometry_and_history_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("converted.ora");
    for linked in [false, true] {
        let mut doc = fixture();
        doc.nodes[0].vector_mask.as_mut().unwrap().linked = linked;
        let original_path = doc.nodes[0].vector_mask.as_ref().unwrap().path.clone();
        let id = doc.nodes[0].id;
        let mut graph = Graph::new(doc.clone(), "Original components");
        for command in [
            Command::Crop {
                rect: emulsion_raster::IRect::new(2, 1, 11, 9),
                rotation: 0.0,
            },
            Command::ImageSize {
                width: 22,
                height: 18,
            },
            Command::ConvertToSmart { id },
            Command::Rasterize { id },
        ] {
            command.apply(&mut doc).unwrap();
            graph
                .record(&doc, "Geometry or source change", false)
                .unwrap();
            assert!(Arc::ptr_eq(
                &original_path,
                &doc.nodes[0].vector_mask.as_ref().unwrap().path
            ));
            ora::write_full(&doc, Some(&graph), &path).unwrap();
            let reopened = ora::read_full(&path).unwrap();
            assert!(reopened.history_error.is_none());
            assert_state(&reopened.doc, &doc);
            for (actual, expected) in reopened.graph.unwrap().commits().zip(graph.commits()) {
                assert_state(&actual.doc, &expected.doc);
            }
        }
    }
}

#[test]
fn group_fill_and_adjustment_vector_components_roundtrip_native_appearance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node-kinds.ora");
    for kind in 0..3 {
        let mut doc = fixture();
        let mask = doc.nodes[0].vector_mask.take();
        let mut node = match kind {
            0 => Node::group(0, "Masked group"),
            1 => Node::new(
                0,
                "Masked fill",
                NodeKind::Fill {
                    rgba: [80, 140, 220, 255],
                },
            ),
            _ => Node::adjust(0, emulsion_raster::Adjustment::Invert),
        };
        node.vector_mask = mask;
        node.mask = Some(Arc::new(Mask::from_fn(16, 12, 0, |x, _| {
            if x > 5 { 255 } else { 0 }
        })));
        add(&mut doc, node);
        if kind == 0 {
            let group_id = doc.nodes.last().unwrap().id;
            let child_id = doc.nodes[0].id;
            Command::MoveNode {
                id: child_id,
                slot: Slot::top_of(Some(group_id)),
            }
            .apply(&mut doc)
            .unwrap();
        }
        for enabled in [false, true] {
            let masked = doc
                .nodes
                .iter_mut()
                .find(|n| n.vector_mask.is_some())
                .unwrap();
            masked.vector_mask.as_mut().unwrap().enabled = enabled;
            let graph = Graph::new(doc.clone(), "Document-space vector");
            ora::write_full(&doc, Some(&graph), &path).unwrap();
            let reopened = ora::read_full(&path).unwrap();
            assert!(reopened.history_error.is_none());
            assert_state(&reopened.doc, &doc);
            assert!(psd::needs_appearance_fallback(&doc));
        }
    }
}

#[test]
fn vector_only_mask_cannot_take_unmasked_ora_preview_fast_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vector-only.ora");
    for enabled in [false, true] {
        let mut doc = fixture();
        doc.nodes[0].mask = None;
        doc.nodes[0].mask_properties = Default::default();
        doc.nodes[0].vector_mask.as_mut().unwrap().enabled = enabled;
        let expected = flatten(&doc.composite_tree(), 0).to_srgba8();
        ora::write(&doc, &path).unwrap();
        assert!(
            String::from_utf8(entry(&path, "stack.xml"))
                .unwrap()
                .contains("mergedimage.png")
        );
        for name in ["mergedimage.png", "Thumbnails/thumbnail.png"] {
            assert_eq!(
                image::load_from_memory(&entry(&path, name))
                    .unwrap()
                    .to_rgba8()
                    .into_raw(),
                expected
            );
        }
        rewrite(
            &path,
            |name, bytes| if name == MANIFEST { None } else { Some(bytes) },
        );
        assert_eq!(
            flatten(&ora::read(&path).unwrap().composite_tree(), 0).to_srgba8(),
            expected
        );
    }
}

#[test]
fn opaque_smart_sources_keep_independent_versions_exact_bytes_and_saved_appearance() {
    use emulsion_core::{Editor, node::SmartEditable};
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("nested-source.ora");
    let host_path = dir.path().join("host.ora");
    let source = fixture();
    let saved_appearance = Arc::new(flatten(&source.composite_tree(), 0));
    for nested_version in [11, ora::FORMAT_VERSION + 1] {
        ora::write(&source, &source_path).unwrap();
        if nested_version > ora::FORMAT_VERSION {
            rewrite(&source_path, |name, bytes| {
                if name != MANIFEST {
                    return Some(bytes);
                }
                let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = json!(nested_version);
                Some(serde_json::to_vec(&value).unwrap())
            });
        }
        let archive = Arc::new(std::fs::read(&source_path).unwrap());
        let mut host = Document::new(source.width, source.height);
        host.source_depth = 16;
        let mut node = Node::smart(
            0,
            "Opaque nested source",
            saved_appearance.clone(),
            vec![],
            Placement::default(),
        );
        let NodeKind::Smart { editable, .. } = &mut node.kind else {
            unreachable!()
        };
        *editable = Some(SmartEditable::Document {
            archive: archive.clone(),
            external: None,
        });
        add(&mut host, node);
        let id = host.nodes[0].id;
        let mut graph = Graph::new(host.clone(), "Nested source");
        let mut earlier = host.clone();
        earlier.nodes[0].opacity = 0.75;
        graph.record(&earlier, "Earlier opacity", false).unwrap();
        ora::write_full(&host, Some(&graph), &host_path).unwrap();
        assert_eq!(manifest(&host_path, MANIFEST)["version"], 9);
        assert_eq!(manifest(&host_path, HISTORY)["version"], 9);
        assert_eq!(manifest(&source_path, MANIFEST)["version"], nested_version);
        let reopened = ora::read_full(&host_path).unwrap();
        assert!(reopened.history_error.is_none());
        assert_eq!(
            flatten(&reopened.doc.composite_tree(), 0).to_srgba8(),
            saved_appearance.to_srgba8()
        );
        let nested_bytes = |doc: &Document| -> Arc<Vec<u8>> {
            let NodeKind::Smart {
                editable: Some(SmartEditable::Document { archive, .. }),
                ..
            } = &doc.nodes[0].kind
            else {
                panic!("embedded source")
            };
            archive.clone()
        };
        assert_eq!(nested_bytes(&reopened.doc).as_slice(), archive.as_slice());
        for commit in reopened.graph.as_ref().unwrap().commits() {
            assert_eq!(nested_bytes(&commit.doc).as_slice(), archive.as_slice());
        }
        let mut editor = Editor::with_graph(reopened.doc.clone(), None, reopened.graph.unwrap());
        let before = editor.doc.clone();
        let history_len = editor.graph.len();
        if nested_version == 11 {
            assert_state(
                &emulsion_io::smart_source::open(&editor.doc, id).unwrap(),
                &source,
            );
        } else {
            assert!(matches!(
                emulsion_io::smart_source::open(&editor.doc, id),
                Err(emulsion_io::IoError::TooNew(version)) if version == nested_version
            ));
            assert!(matches!(
                emulsion_io::smart_source::relink(&mut editor, id, &source_path, false),
                Err(emulsion_io::IoError::TooNew(version)) if version == nested_version
            ));
        }
        assert_eq!(editor.doc, before);
        assert_eq!(editor.graph.len(), history_len);
        assert_eq!(nested_bytes(&editor.doc).as_slice(), archive.as_slice());
    }
}

fn budget_mask_document() -> Document {
    let mut doc = Document::new(32, 24);
    let mut path = VectorPath::default();
    for i in 0..32 {
        path.subpaths.extend(
            emulsion_raster::vector_geometry::rectangle(
                f64::from(i) * 100_000.,
                0.,
                50_000.,
                2_400_000.,
            )
            .subpaths,
        );
    }
    let mut node = Node::raster(
        0,
        "Budgeted vector",
        Arc::new(Raster::solid(32, 24, [1., 0., 0., 1.])),
        Placement::default(),
    );
    node.vector_mask = Some(VectorMask {
        path: Arc::new(path),
        transform: [1e-5, 0., 0., 1e-5, 0., 0.],
        ..Default::default()
    });
    add(&mut doc, node);
    doc.validate().unwrap();
    doc
}

#[test]
fn overbudget_vector_native_load_and_save_fail_without_fallback_or_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bounded-vector.ora");
    let valid = budget_mask_document();
    let original = valid.clone();
    ora::write(&valid, &path).unwrap();
    assert_eq!(manifest(&path, MANIFEST)["version"], 11);
    let saved_bytes = std::fs::read(&path).unwrap();
    let mut overbudget = valid.clone();
    overbudget.nodes[0]
        .vector_mask
        .as_mut()
        .unwrap()
        .properties
        .feather = 1000.;
    assert!(
        overbudget.nodes[0].vector_mask.as_ref().unwrap().valid(),
        "Geometry and parameter ranges are valid; cumulative render work is not"
    );
    let before = overbudget.clone();
    let error = ora::write(&overbudget, &path).unwrap_err();
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), saved_bytes);
    assert_eq!(overbudget, before);
    let graph = Graph::new(overbudget, "Overbudget snapshot");
    let error = ora::write_full(&valid, Some(&graph), &path).unwrap_err();
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), saved_bytes);
    rewrite(&path, |name, bytes| {
        if name != MANIFEST {
            return Some(bytes);
        }
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["nodes"][0]["vector_mask"]["properties"]["feather"] = json!(1000.);
        Some(serde_json::to_vec(&value).unwrap())
    });
    let malformed_bytes = std::fs::read(&path).unwrap();
    let error = ora::read(&path).expect_err("Native input must reject unsafe work");
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    let error = ora::read_full(&path)
        .err()
        .expect("Opening must not substitute unmasked appearance");
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), malformed_bytes);
    assert_eq!(valid, original);
}

#[test]
fn overbudget_vector_export_and_development_reject_before_rendering_or_replacing_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.png");
    let mut doc = budget_mask_document();
    doc.nodes[0]
        .vector_mask
        .as_mut()
        .unwrap()
        .properties
        .feather = 1000.;
    let original = doc.clone();
    std::fs::write(&path, b"unchanged destination").unwrap();
    let error =
        export::develop_document(&doc).expect_err("Even no-RAW development must preflight work");
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    let error = export::export(&doc, &path, export::ExportOptions::for_doc(&doc)).unwrap_err();
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    let error = export::export_with_workflow(
        &doc,
        &path,
        export::ExportOptions::for_doc(&doc),
        Default::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"unchanged destination");
    assert_eq!(doc, original);
}

#[path = "common/raw_fixture.rs"]
mod vector_budget_raw_fixture;

#[test]
fn raw_redevelopment_and_wide_gamut_export_revalidate_larger_mask_grid_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.dng");
    vector_budget_raw_fixture::write_dng(&source);
    let raw_bytes = std::fs::read(&source).unwrap();
    let mut doc = emulsion_io::open(&source).unwrap();
    let id = doc.raw.as_ref().unwrap().node_id;
    let node = doc.node_mut(id).unwrap();
    let NodeKind::Raster { raster, .. } = &mut node.kind else {
        panic!("RAW raster")
    };
    // A tiny saved proxy is safe because its only output sample is well clear
    // of this off-proxy edge. Developing the original exposes many edge samples.
    *raster = Arc::new(Raster::solid(1, 1, [1., 0., 0., 1.]));
    node.vector_mask = Some(VectorMask {
        path: Arc::new(emulsion_raster::vector_geometry::rectangle(
            150_000., 0., 50_000., 2_400_000.,
        )),
        transform: [1e-5, 0., 0., 1e-5, 0., 0.],
        properties: MaskProperties {
            density: 1.,
            feather: 1000.,
        },
        ..Default::default()
    });
    doc.raw.as_mut().unwrap().params.wide_gamut = true;
    doc.validate().unwrap();
    let original = doc.clone();
    let error =
        export::develop_document(&doc).expect_err("Developed full source must be revalidated");
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    let destination = dir.path().join("existing.png");
    std::fs::write(&destination, b"unchanged destination").unwrap();
    let error = export::export_with_workflow(
        &doc,
        &destination,
        export::ExportOptions::for_doc(&doc),
        export::ExportWorkflow {
            color_space: export::ExportColorSpace::ProPhoto,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("native rendering work budget"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"unchanged destination"
    );
    assert_eq!(std::fs::read(&source).unwrap(), raw_bytes);
    assert_eq!(doc, original);
}
