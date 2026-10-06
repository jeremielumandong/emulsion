//! Original source bytes must survive native saves without an RGBA8 re-encode.
use crate::original_image_data::{self as originals, OriginalImagePool, OriginalImageRef};
use emulsion_core::{Document, Node, NodeKind, graph::Graph, node::OriginalImage};
use emulsion_raster::{Placement, Raster};
use image::ImageEncoder;
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn png(samples: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            samples,
            (samples.len() / 4) as u32,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    bytes
}
fn document(samples: &[u8]) -> Document {
    let bytes = Arc::new(png(samples));
    let source = crate::original_image_png::png_source(&bytes).unwrap();
    let original = originals::capture(bytes, &source);
    let mut doc = Document::new(source.width(), source.height());
    let mut node = Node::smart(1, "Original PNG", source, Vec::new(), Placement::default());
    if let NodeKind::Smart { original_image, .. } = &mut node.kind {
        *original_image = Some(original);
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}
fn sample() -> Document {
    document(&[
        255, 255, 255, 0, 1, 27, 85, 1, 41, 125, 248, 2, 252, 97, 3, 3, 123, 47, 211, 4, 11, 231,
        5, 5,
    ])
}
fn original(doc: &Document) -> &Arc<OriginalImage> {
    let NodeKind::Smart {
        original_image: Some(original),
        ..
    } = &doc.nodes[0].kind
    else {
        panic!("missing original")
    };
    original
}
fn source(doc: &Document) -> &Arc<Raster> {
    let NodeKind::Smart { source, .. } = &doc.nodes[0].kind else {
        panic!("missing Smart")
    };
    source
}
fn encoded(doc: &Document, graph: Option<&Graph>) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    crate::ora::write_to(doc, graph, &mut bytes).unwrap();
    bytes.into_inner()
}
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            (file.name().to_owned(), bytes)
        })
        .collect()
}
fn archive(entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}
fn mutate(bytes: &[u8], name: &str, edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut all = entries(bytes);
    let entry = all.iter_mut().find(|entry| entry.0 == name).unwrap();
    let mut value = serde_json::from_slice(&entry.1).unwrap();
    edit(&mut value);
    entry.1 = serde_json::to_vec(&value).unwrap();
    archive(all)
}
fn manifest(bytes: &[u8], name: &str) -> serde_json::Value {
    serde_json::from_slice(
        &entries(bytes)
            .into_iter()
            .find(|entry| entry.0 == name)
            .unwrap()
            .1,
    )
    .unwrap()
}
fn read(bytes: Vec<u8>) -> crate::Result<crate::ora::Opened> {
    crate::ora::read_from(Cursor::new(bytes))
}

#[test]
fn native_original_live_and_history_reopen_exact_pixels_and_bytes() {
    let doc = sample();
    let graph = Graph::new(doc.clone(), "Imported source");
    assert_ne!(
        source(&doc).to_srgba8(),
        image::load_from_memory(original(&doc).bytes())
            .unwrap()
            .into_rgba8()
            .into_raw()
    );
    for history in [None, Some(&graph)] {
        let bytes = encoded(&doc, history);
        let data = entries(&bytes);
        let image_entries: Vec<_> = data
            .iter()
            .filter(|entry| entry.0.starts_with("original-images/"))
            .collect();
        assert_eq!(image_entries.len(), 1);
        assert_eq!(
            image_entries[0].1.as_slice(),
            original(&doc).bytes().as_slice()
        );
        let m = manifest(&bytes, "emulsion.json");
        assert_eq!(m["version"], 13);
        assert_eq!(m["nodes"][0]["kind"]["src"], image_entries[0].0);
        assert!(
            !data
                .iter()
                .any(|entry| entry.0.starts_with("emulsion/src/"))
        );
        let restored = read(bytes).unwrap();
        assert!(restored.history_error.is_none());
        assert_eq!(
            original(&restored.doc).bytes().as_slice(),
            original(&doc).bytes().as_slice()
        );
        assert_eq!(
            originals::source_digest(source(&restored.doc)),
            originals::source_digest(source(&doc))
        );
        if let Some(graph) = restored.graph {
            for commit in graph.commits() {
                assert!(Arc::ptr_eq(original(&restored.doc), original(&commit.doc)));
            }
        }
    }
}

#[test]
fn native_original_project_emu_preserves_page_source_and_outer_version_one() {
    use emulsion_core::project::{PageMeta, Project, ProjectKind, ProjectPage};
    let doc = sample();
    let project = Project {
        kind: ProjectKind::Design,
        pages: vec![ProjectPage {
            meta: PageMeta {
                id: 1,
                name: "Original".into(),
                bleed_mm: 0.,
            },
            graph: Graph::new(doc.clone(), "Imported"),
            doc: doc.clone(),
        }],
        active: 1,
        next_page_id: 2,
        storyboard: None,
    };
    let mut bytes = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut bytes).unwrap();
    assert_eq!(manifest(bytes.get_ref(), "project.json")["version"], 1);
    let restored = crate::project::read_from(Cursor::new(bytes.into_inner())).unwrap();
    assert_eq!(
        original(&restored.pages[0].doc).bytes().as_slice(),
        original(&doc).bytes().as_slice()
    );
}

#[test]
fn native_original_history_only_requires_v13_and_old_documents_stay_v9() {
    let original_doc = sample();
    let graph = Graph::new(original_doc.clone(), "Retained import");
    let mut live = original_doc.clone();
    if let NodeKind::Smart { original_image, .. } = &mut live.nodes[0].kind {
        *original_image = None;
    }
    assert_eq!(crate::ora::required_version(&live), 9);
    assert_eq!(
        manifest(&encoded(&live, None), "emulsion.json")["version"],
        9
    );
    let bytes = encoded(&live, Some(&graph));
    assert_eq!(manifest(&bytes, "emulsion.json")["version"], 13);
    assert_eq!(manifest(&bytes, crate::history::GRAPH)["version"], 13);
    let restored = read(bytes.clone()).unwrap();
    assert!(matches!(
        restored.doc.nodes[0].kind,
        NodeKind::Smart {
            original_image: None,
            ..
        }
    ));
    assert_eq!(
        original(&restored.graph.unwrap().commits().next().unwrap().doc)
            .bytes()
            .as_slice(),
        original(&original_doc).bytes().as_slice()
    );
    for file in ["emulsion.json", crate::history::GRAPH] {
        assert!(read(mutate(&bytes, file, |value| value["version"] = 12.into())).is_err());
    }
}

#[test]
fn native_original_rejects_missing_changed_digest_dimensions_and_false_versions() {
    let doc = sample();
    let bytes = encoded(&doc, None);
    for version in [9, 12] {
        assert!(read(mutate(&bytes, "emulsion.json", |v| v["version"] = version.into())).is_err());
    }
    let future_version = crate::ora::FORMAT_VERSION + 1;
    assert!(matches!(
        read(mutate(&bytes, "emulsion.json", |v| v["version"] = future_version.into())),
        Err(crate::IoError::TooNew(version)) if version == future_version
    ));
    for key in ["encoded_sha256", "source_sha256"] {
        assert!(
            read(mutate(&bytes, "emulsion.json", |v| v["nodes"][0]["kind"]
                ["original_image"][key] =
                "00".repeat(32).into()))
            .is_err()
        );
    }
    assert!(
        read(mutate(&bytes, "emulsion.json", |v| v["nodes"][0]["kind"]
            ["original_image"]["width"] =
            1.into()))
        .is_err()
    );
    assert!(
        read(mutate(&bytes, "emulsion.json", |v| v["nodes"][0]["kind"]
            ["width"] =
            1.into()))
        .is_err()
    );
    assert!(
        read(mutate(&bytes, "emulsion.json", |v| v["nodes"][0]["kind"]
            ["src"] =
            "data/node-1.png".into()))
        .is_err()
    );
    let mut missing = entries(&bytes);
    missing.retain(|entry| !entry.0.starts_with("original-images/"));
    assert!(read(archive(missing)).is_err());
    let mut corrupted = entries(&bytes);
    corrupted
        .iter_mut()
        .find(|entry| entry.0.starts_with("original-images/"))
        .unwrap()
        .1[0] ^= 1;
    assert!(read(archive(corrupted)).is_err());
}

#[test]
fn native_original_history_source_tiles_are_digest_bound() {
    let doc = sample();
    let graph = Graph::new(doc.clone(), "Original");
    let bytes = encoded(&doc, Some(&graph));
    let damaged = mutate(&bytes, crate::history::GRAPH, |v| {
        let source = v["commits"][0]["doc"]["nodes"][0]["kind"]["source"]
            .as_u64()
            .unwrap() as usize;
        v["rasters"][source]["width"] = 7.into();
    });
    assert!(read(damaged).is_err());
    let damaged = mutate(&bytes, crate::history::GRAPH, |v| {
        v["commits"][0]["doc"]["nodes"][0]["kind"]["original_image"]["source_sha256"] =
            "00".repeat(32).into()
    });
    assert!(read(damaged).is_err());
}

#[test]
fn native_original_write_refuses_stale_source_provenance_and_digests() {
    let doc = sample();
    for case in 0..4 {
        let mut changed = doc.clone();
        if let NodeKind::Smart {
            source,
            cache,
            original_image,
            ..
        } = &mut changed.nodes[0].kind
        {
            let original = original_image.as_ref().unwrap();
            match case {
                0 => {
                    *source = Arc::new(Raster::solid(
                        source.width(),
                        source.height(),
                        [1., 0., 0., 1.],
                    ));
                    *cache = source.clone();
                }
                1 => {
                    *original_image = Some(Arc::new(OriginalImage::new(
                        original.bytes().clone(),
                        [0; 32],
                        *original.source_sha256(),
                    )))
                }
                2 => {
                    *original_image = Some(Arc::new(OriginalImage::new(
                        original.bytes().clone(),
                        *original.encoded_sha256(),
                        [0; 32],
                    )))
                }
                _ => {
                    *source = Arc::new(Raster::transparent(1, 6));
                    *cache = source.clone();
                }
            }
        }
        assert!(
            crate::ora::write_to(&changed, None, Cursor::new(Vec::new())).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn native_original_dedup_uses_encoded_bytes_not_decoded_pixels() {
    let mut doc = document(&[255, 255, 255, 0]);
    let same = doc.nodes[0].clone();
    let other = document(&[0, 0, 0, 0]);
    assert_eq!(
        originals::source_digest(source(&doc)),
        originals::source_digest(source(&other))
    );
    assert_ne!(
        original(&doc).encoded_sha256(),
        original(&other).encoded_sha256()
    );
    doc.nodes.push(same);
    doc.nodes[1].id = 2;
    doc.nodes.push(other.nodes[0].clone());
    doc.nodes[2].id = 3;
    doc.next_id = 4;
    let graph = Graph::new(doc.clone(), "Dedup");
    let bytes = encoded(&doc, Some(&graph));
    assert_eq!(
        entries(&bytes)
            .iter()
            .filter(|entry| entry.0.starts_with("original-images/"))
            .count(),
        2
    );
    let restored = read(bytes).unwrap();
    let get = |index: usize| -> &Arc<OriginalImage> {
        let NodeKind::Smart {
            original_image: Some(original),
            ..
        } = &restored.doc.nodes[index].kind
        else {
            panic!()
        };
        original
    };
    assert!(Arc::ptr_eq(get(0), get(1)));
    assert!(!Arc::ptr_eq(get(0), get(2)));
}

fn reference(index: usize, width: u32, height: u32) -> OriginalImageRef {
    OriginalImageRef {
        encoded_sha256: format!("{index:064x}"),
        source_sha256: "11".repeat(32),
        width,
        height,
    }
}
fn forged_resource_sizes(references: &[OriginalImageRef], size: u32) -> Vec<u8> {
    let mut bytes = archive(
        references
            .iter()
            .map(|reference| (reference.path(), vec![0]))
            .collect(),
    );
    let mut index = 0;
    while index + 46 <= bytes.len() {
        if &bytes[index..index + 4] == b"PK\x01\x02" {
            bytes[index + 24..index + 28].copy_from_slice(&size.to_le_bytes());
            index += 46;
        } else {
            index += 1;
        }
    }
    bytes
}
#[test]
fn native_original_resource_byte_and_padded_budgets_precede_decode() {
    let limit = crate::original_image_png::MAX_SOURCE_BYTES as u32;
    for (refs, size, message) in [
        (vec![reference(1, 1, 1)], limit + 1, "byte"),
        ((1..=3).map(|i| reference(i, 1, 1)).collect(), limit, "byte"),
        (
            (1..=5).map(|i| reference(i, 1, 30_000)).collect(),
            1,
            "padded",
        ),
        (vec![reference(1, 30_000, 533)], 1, "padded"),
    ] {
        let mut zip = ZipArchive::new(Cursor::new(forged_resource_sizes(&refs, size))).unwrap();
        let error = OriginalImagePool::default()
            .prepare(refs, &mut zip)
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn native_original_canonical_digest_ignores_storage_padding_but_binds_dimensions() {
    let a = Raster::from_fn(1, 1, [0; 4], |_, _| [1, 2, 3, 4]);
    let b = Raster::from_tiles(
        1,
        1,
        [0; 4],
        vec![(
            emulsion_raster::TileCoord::new(0, 0),
            std::iter::once([1, 2, 3, 4])
                .chain(std::iter::repeat_n([9; 4], emulsion_raster::TILE_PX - 1))
                .collect(),
        )],
    )
    .unwrap();
    assert_eq!(originals::source_digest(&a), originals::source_digest(&b));
    assert_ne!(
        originals::source_digest(&a),
        originals::source_digest(&Raster::transparent(1, 2))
    );
}

#[test]
fn native_original_rejects_malformed_history_metadata_even_with_lower_declared_version() {
    let doc = sample();
    let graph = Graph::new(doc.clone(), "Original");
    let bytes = encoded(&doc, Some(&graph));
    for version in [12, 13] {
        let damaged = mutate(&bytes, crate::history::GRAPH, |v| {
            v["version"] = version.into();
            v["commits"][0]["doc"]["nodes"][0]["kind"]["original_image"]["source_sha256"] =
                serde_json::json!([]);
        });
        assert!(read(damaged).is_err());
    }
    let future_version = crate::history::HISTORY_VERSION + 1;
    let mut all = entries(&bytes);
    all.iter_mut()
        .find(|entry| entry.0 == crate::history::GRAPH)
        .unwrap()
        .1 = serde_json::to_vec(&serde_json::json!({ "version": future_version })).unwrap();
    assert!(matches!(
        read(archive(all)),
        Err(crate::IoError::TooNew(version)) if version == future_version
    ));
}

#[test]
fn native_original_metadata_cannot_be_hidden_on_a_different_node_kind() {
    let doc = sample();
    let bytes = encoded(&doc, None);
    assert!(
        read(mutate(&bytes, "emulsion.json", |v| {
            v["nodes"][0]["kind"]["type"] = "raster".into();
        }))
        .is_err()
    );
}

#[test]
fn native_original_history_only_rejects_syntactically_damaged_graph() {
    let original_doc = sample();
    let graph = Graph::new(original_doc.clone(), "Retained import");
    let mut live = original_doc;
    if let NodeKind::Smart { original_image, .. } = &mut live.nodes[0].kind {
        *original_image = None;
    }
    let bytes = encoded(&live, Some(&graph));
    for damage in [
        b"{\"version\":13,\"commits\":[".as_slice(),
        b"not JSON".as_slice(),
    ] {
        let mut all = entries(&bytes);
        all.iter_mut()
            .find(|entry| entry.0 == crate::history::GRAPH)
            .unwrap()
            .1 = damage.to_vec();
        assert!(read(archive(all)).is_err());
    }
}

#[test]
fn native_original_pool_caches_shared_source_work_and_bounds_distinct_allocations() {
    let doc = sample();
    let mut pool = OriginalImagePool::default();
    let original = Some(original(&doc).clone());
    let reference = pool
        .reference(&original, source(&doc), false)
        .unwrap()
        .unwrap();
    // A large history sharing one immutable source must pay its digest cost once.
    for _ in 0..2_000 {
        assert_eq!(
            pool.reference(&original, source(&doc), false)
                .unwrap()
                .as_ref(),
            Some(&reference)
        );
        pool.restore(&reference, Some(source(&doc)), false).unwrap();
    }
    // One 256x256 native tile is charged per distinct source allocation, even
    // when those allocations have identical samples and share encoded bytes.
    for _ in 1..512 {
        let distinct = Arc::new(source(&doc).as_ref().clone());
        pool.reference(&original, &distinct, false).unwrap();
    }
    let distinct = Arc::new(source(&doc).as_ref().clone());
    assert!(
        pool.reference(&original, &distinct, false)
            .unwrap_err()
            .to_string()
            .contains("total padded native tile byte limit")
    );
}

#[test]
fn native_original_history_source_work_budget_precedes_tile_loading() {
    let doc = sample();
    let graph = Graph::new(doc.clone(), "Original");
    let bytes = mutate(&encoded(&doc, Some(&graph)), crate::history::GRAPH, |v| {
        let node = v["commits"][0]["doc"]["nodes"][0].clone();
        let source = node["kind"]["source"].as_u64().unwrap() as usize;
        let plane = v["rasters"][source].clone();
        v["rasters"] = serde_json::Value::Array(vec![plane; 513]);
        let nodes = (0..513)
            .map(|index| {
                let mut node = node.clone();
                node["id"] = (index + 1).into();
                node["kind"]["source"] = index.into();
                node
            })
            .collect();
        v["commits"][0]["doc"]["nodes"] = serde_json::Value::Array(nodes);
    });
    let mut all = entries(&bytes);
    all.retain(|entry| !entry.0.starts_with("history/tiles/"));
    let error = read(archive(all))
        .err()
        .expect("over-budget planes must fail")
        .to_string();
    assert!(
        error.contains("total padded native tile byte limit"),
        "{error}"
    );
}

#[test]
fn disabled_filters_preserve_original_png_bytes_digests_and_history_source_identity() {
    use emulsion_core::{Command, Editor};
    use emulsion_filters::{Filter, FilterStyle};
    let mut doc = sample();
    Command::SetFilterStack {
        id: 1,
        filters: vec![Filter::Invert, Filter::GaussianBlur { radius: 2. }],
        styles: vec![
            FilterStyle::default(),
            FilterStyle {
                enabled: false,
                opacity: 0.25,
                ..Default::default()
            },
        ],
    }
    .apply(&mut doc)
    .unwrap();
    let before = original(&doc).clone();
    let pixels = source(&doc).clone();
    let mut editor = Editor::new(doc.clone(), None);
    editor
        .execute(Command::SetFiltersEnabled {
            id: 1,
            enabled: false,
        })
        .unwrap();
    assert!(Arc::ptr_eq(original(&editor.doc), &before));
    assert!(Arc::ptr_eq(source(&editor.doc), &pixels));
    let saved = read(encoded(&editor.doc, Some(&editor.graph))).unwrap();
    let actual = original(&saved.doc);
    assert_eq!(actual.bytes().as_slice(), before.bytes().as_slice());
    assert_eq!(actual.encoded_sha256(), before.encoded_sha256());
    assert_eq!(actual.source_sha256(), before.source_sha256());
    let NodeKind::Smart {
        source,
        cache,
        filters_enabled,
        filter_styles,
        offset,
        ..
    } = &saved.doc.nodes[0].kind
    else {
        panic!()
    };
    assert!(!filters_enabled && !filter_styles[1].enabled);
    assert!(Arc::ptr_eq(source, cache));
    assert_eq!(*offset, (0, 0));
    assert!(editor.undo());
    assert!(Arc::ptr_eq(original(&editor.doc), &before));
    assert!(editor.redo());
    assert!(Arc::ptr_eq(original(&editor.doc), &before));
}
