//! Source-only v16 regressions. Execution belongs to the integrated coordinator gate.
use crate::{
    native_relation::{LiveRelation, history_matches},
    ora,
};
use emulsion_core::{
    Document, Node, NodeKind, SmartFilterMask,
    graph::Graph,
    mapping::{Mapping2, SmartPlacement},
};
use emulsion_raster::{Mask, Placement, Raster, composite::flatten, projective::Projective2};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn plain() -> Document {
    let mut doc = Document::new(16, 12);
    doc.source_depth = 16;
    doc.nodes.push(Node::smart(
        1,
        "Retained source",
        Arc::new(Raster::from_srgba8(4, 4, &[255, 0, 0, 255].repeat(16))),
        Vec::new(),
        Placement::at(2.0, 2.0),
    ));
    doc.next_id = 2;
    doc
}
fn perspective() -> Projective2 {
    Projective2::from_row_major([1., 0., 2., 0., 1., 2., 1. / 64., 0., 1.]).unwrap()
}
fn projected() -> Document {
    let mut doc = plain();
    let NodeKind::Smart { placement, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *placement = SmartPlacement::Projective(perspective());
    doc
}
fn latent() -> Document {
    let mut doc = plain();
    doc.nodes[0].mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    doc.nodes[0].mask_enabled = false;
    doc.nodes[0].mask_linked = false;
    doc
}
fn encode(doc: &Document, graph: Option<&Graph>) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    ora::write_to(doc, graph, &mut out).unwrap();
    out.into_inner()
}
fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}
fn json_entry(bytes: &[u8], name: &str) -> Value {
    serde_json::from_slice(&entry(bytes, name)).unwrap()
}
fn rewrite(bytes: &[u8], mut change: impl FnMut(&str, &mut Vec<u8>) -> bool) -> Vec<u8> {
    let mut input = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..input.len() {
        let mut file = input.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if change(&name, &mut data) {
            output
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            output.write_all(&data).unwrap();
        }
    }
    output.finish().unwrap().into_inner()
}
fn json_edit(bytes: &[u8], mut change: impl FnMut(&str, &mut Value)) -> Vec<u8> {
    rewrite(bytes, |name, data| {
        if name == "emulsion.json" || name == crate::history::GRAPH {
            let mut root = serde_json::from_slice(data).unwrap();
            change(name, &mut root);
            *data = serde_json::to_vec(&root).unwrap();
        }
        true
    })
}
fn assert_both_readers_reject(bytes: &[u8], expected: &str) {
    let error = ora::read_from(Cursor::new(bytes))
        .err()
        .expect("full reader must reject");
    assert!(error.to_string().contains(expected), "{error}");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsupported.ora");
    std::fs::write(&path, bytes).unwrap();
    let error = ora::read(&path).expect_err("document reader must reject");
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn projective_live_identity_mixed_and_latent_variants_roundtrip_without_demotion() {
    let mut identity = plain();
    if let NodeKind::Smart { placement, .. } = &mut identity.nodes[0].kind {
        *placement = SmartPlacement::Projective(Projective2::IDENTITY);
    }
    let mut filter_only = plain();
    if let NodeKind::Smart {
        filter_mask,
        filters_enabled,
        ..
    } = &mut filter_only.nodes[0].kind
    {
        let mut mask = SmartFilterMask::new(Arc::new(Mask::empty(4, 4, 255)));
        mask.enabled = false;
        mask.transform = Mapping2::Projective(Projective2::IDENTITY);
        *filter_mask = Some(mask);
        *filters_enabled = false;
    }
    let mut mixed = filter_only.clone();
    mixed.nodes[0].mask_transform = Mapping2::Projective(Projective2::IDENTITY);
    mixed.nodes[0].mask = Some(Arc::new(Mask::from_fn(4, 4, 255, |x, _| {
        if x == 0 { 32 } else { 255 }
    })));
    for doc in [projected(), identity, latent(), filter_only, mixed] {
        assert_eq!(ora::required_version(&doc), 16);
        for history in [false, true] {
            let graph = Graph::try_new(doc.clone(), "Projected state").unwrap();
            let bytes = encode(&doc, history.then_some(&graph));
            assert_eq!(json_entry(&bytes, "emulsion.json")["version"], 16);
            if history {
                assert_eq!(json_entry(&bytes, crate::history::GRAPH)["version"], 16);
            }
            let opened = ora::read_from(Cursor::new(&bytes)).unwrap();
            assert!(opened.history_error.is_none());
            assert_eq!(history_matches(&doc, &opened.doc), LiveRelation::Consistent);
            assert_eq!(
                emulsion_core::smart_support::MappingKey::from(doc.nodes[0].mask_transform),
                emulsion_core::smart_support::MappingKey::from(opened.doc.nodes[0].mask_transform)
            );
            let (
                NodeKind::Smart {
                    placement: expected,
                    filter_mask: expected_mask,
                    ..
                },
                NodeKind::Smart {
                    placement: actual,
                    filter_mask: actual_mask,
                    ..
                },
            ) = (&doc.nodes[0].kind, &opened.doc.nodes[0].kind)
            else {
                unreachable!()
            };
            assert_eq!(
                emulsion_core::smart_support::SmartPlacementKey::from(*expected),
                emulsion_core::smart_support::SmartPlacementKey::from(*actual)
            );
            assert_eq!(
                expected_mask
                    .as_ref()
                    .map(|m| emulsion_core::smart_support::MappingKey::from(m.transform)),
                actual_mask
                    .as_ref()
                    .map(|m| emulsion_core::smart_support::MappingKey::from(m.transform))
            );
            assert_eq!(
                flatten(&doc.try_composite_tree().unwrap(), 0).to_srgba16(),
                flatten(&opened.doc.try_composite_tree().unwrap(), 0).to_srgba16()
            );
            let again = encode(&opened.doc, opened.graph.as_ref());
            assert_eq!(
                json_entry(&bytes, "emulsion.json")["nodes"][0]["kind"]["placement"],
                json_entry(&again, "emulsion.json")["nodes"][0]["kind"]["placement"]
            );
            assert_eq!(
                json_entry(&bytes, "emulsion.json")["nodes"][0]["mask_transform"],
                json_entry(&again, "emulsion.json")["nodes"][0]["mask_transform"]
            );
        }
    }
}

#[test]
fn v16_covers_working_commit_and_other_branch_snapshots_and_preserves_graph() {
    let base = plain();
    let changed = projected();
    let working_graph = Graph::try_new(base.clone(), "Legacy head").unwrap();
    let mut commit_graph = Graph::try_new(changed.clone(), "Projected predecessor").unwrap();
    commit_graph
        .try_record(&base, "Legacy live", false)
        .unwrap()
        .unwrap();
    let mut branch_graph = Graph::try_new(base.clone(), "Legacy head").unwrap();
    let at = branch_graph.head_branch().tip;
    branch_graph.create_branch("Projected", at).unwrap();
    branch_graph.set_head("Projected").unwrap();
    branch_graph
        .try_record(&changed, "Projected branch", false)
        .unwrap()
        .unwrap();
    branch_graph.set_head("main").unwrap();
    for (doc, graph, working) in [
        (&changed, working_graph, true),
        (&base, commit_graph, false),
        (&base, branch_graph, false),
    ] {
        let bytes = encode(doc, Some(&graph));
        assert_eq!(json_entry(&bytes, "emulsion.json")["version"], 16);
        let history = json_entry(&bytes, crate::history::GRAPH);
        assert_eq!(history["version"], 16);
        assert_eq!(history.get("working").is_some(), working);
        let opened = ora::read_from(Cursor::new(&bytes)).unwrap();
        assert!(opened.history_error.is_none());
        assert_eq!(history_matches(doc, &opened.doc), LiveRelation::Consistent);
        let restored = opened.graph.unwrap();
        assert_eq!(restored.branches(), graph.branches());
        assert_eq!(restored.head(), graph.head());
        assert_eq!(restored.len(), graph.len());
        for (before, after) in graph.commits().zip(restored.commits()) {
            assert_eq!(
                (
                    before.id,
                    &before.parents,
                    &before.name,
                    before.time,
                    before.auto,
                    &before.branch
                ),
                (
                    after.id,
                    &after.parents,
                    &after.name,
                    after.time,
                    after.auto,
                    &after.branch
                )
            );
            assert_eq!(
                history_matches(&before.doc, &after.doc),
                LiveRelation::Consistent
            );
        }
        for header in ["emulsion.json", crate::history::GRAPH] {
            let old = json_edit(&bytes, |name, root| {
                if name == header {
                    root["version"] = 15.into();
                }
            });
            assert_both_readers_reject(&old, "16");
        }
    }
}

#[test]
fn original_encoded_source_and_opaque_nested_archive_bytes_survive_projection_and_dedup() {
    // OriginalImage admits this explicit sRGB declaration. The ordinary export
    // helper embeds an iCCP profile, which the retained-source guard rejects.
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, 4, 4);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[10, 50, 90, 77].repeat(16))
            .unwrap();
        writer.finish().unwrap();
    }
    let encoded = Arc::new(encoded);
    let source = crate::original_image_png::png_source(&encoded).unwrap();
    let original = crate::original_image_data::capture(encoded.clone(), &source);
    let mut doc = projected();
    if let NodeKind::Smart {
        source: current,
        cache,
        original_image,
        ..
    } = &mut doc.nodes[0].kind
    {
        *current = source.clone();
        *cache = source;
        *original_image = Some(original.clone());
    }
    let mut copy = doc.nodes[0].clone();
    copy.id = 2;
    doc.nodes.push(copy);
    doc.next_id = 3;
    let mut opaque = latent().nodes.remove(0);
    opaque.id = 3;
    let nested = Arc::new(b"opaque source from a future native version".to_vec());
    if let NodeKind::Smart { editable, .. } = &mut opaque.kind {
        *editable = Some(emulsion_core::node::SmartEditable::Document {
            archive: nested.clone(),
            external: None,
        });
    }
    doc.nodes.push(opaque);
    doc.next_id = 4;
    let graph = Graph::try_new(doc.clone(), "Retained bytes").unwrap();
    let bytes = encode(&doc, Some(&graph));
    let zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(
        zip.file_names()
            .filter(|name| name.starts_with("original-images/"))
            .count(),
        1
    );
    let opened = ora::read_from(Cursor::new(bytes)).unwrap();
    for node in &opened.doc.nodes[..2] {
        let NodeKind::Smart {
            original_image: Some(actual),
            ..
        } = &node.kind
        else {
            panic!("retained PNG");
        };
        assert_eq!(actual.bytes().as_ref(), encoded.as_ref());
        assert_eq!(actual.encoded_sha256(), original.encoded_sha256());
        assert_eq!(actual.source_sha256(), original.source_sha256());
    }
    let NodeKind::Smart {
        editable: Some(emulsion_core::node::SmartEditable::Document { archive, .. }),
        ..
    } = &opened.doc.nodes[2].kind
    else {
        panic!("retained opaque source");
    };
    assert_eq!(archive.as_ref(), nested.as_ref());
}

#[test]
fn dormant_map_relation_mismatch_cannot_drop_history_or_hide_behind_identical_pixels() {
    let doc = latent();
    let graph = Graph::try_new(doc.clone(), "Dormant map").unwrap();
    let bytes = encode(&doc, Some(&graph));
    let damaged = json_edit(&bytes, |name, root| {
        if name == crate::history::GRAPH {
            root["commits"][0]["doc"]["nodes"][0]["mask_transform"] = json!([1, 0, 0, 1, 0, 0]);
        }
    });
    let opened = ora::read_from(Cursor::new(damaged)).unwrap();
    assert!(opened.history_error.is_none());
    assert_eq!(history_matches(&doc, &opened.doc), LiveRelation::Consistent);
    let retained = opened.graph.unwrap();
    assert!(matches!(
        history_matches(
            &doc,
            &retained.commit(retained.head_branch().tip).unwrap().doc
        ),
        LiveRelation::Mismatch(_)
    ));
    let retained_tip = retained
        .commit(retained.head_branch().tip)
        .unwrap()
        .doc
        .clone();
    assert!(matches!(
        retained_tip.nodes[0].mask_transform,
        Mapping2::Affine(_)
    ));
    assert!(matches!(
        opened.doc.nodes[0].mask_transform,
        Mapping2::Projective(_)
    ));
    let NodeKind::Smart {
        source: live_source,
        ..
    } = &opened.doc.nodes[0].kind
    else {
        unreachable!()
    };
    let live_source = live_source.clone();
    let mut editor = emulsion_core::Editor::try_with_graph(opened.doc, None, retained).unwrap();
    assert!(!editor.undo());
    editor
        .execute(emulsion_core::Command::SetOpacity {
            id: 1,
            opacity: 0.5,
        })
        .unwrap();
    assert!(editor.undo());
    assert_eq!(history_matches(&doc, &editor.doc), LiveRelation::Consistent);
    let NodeKind::Smart {
        source: after_undo, ..
    } = &editor.doc.nodes[0].kind
    else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(&live_source, after_undo));
    let preserved = encode(&editor.doc, Some(&editor.graph));
    assert!(
        json_entry(&preserved, crate::history::GRAPH)
            .get("working")
            .is_some()
    );
    let reopened = ora::read_from(Cursor::new(preserved)).unwrap();
    assert_eq!(
        history_matches(&doc, &reopened.doc),
        LiveRelation::Consistent
    );
    let restored_graph = reopened.graph.unwrap();
    assert_eq!(
        history_matches(
            &retained_tip,
            &restored_graph
                .commit(restored_graph.head_branch().tip)
                .unwrap()
                .doc
        ),
        LiveRelation::Consistent
    );
    assert!(matches!(
        reopened.doc.nodes[0].mask_transform,
        Mapping2::Projective(_)
    ));
    let broken = rewrite(&bytes, |name, data| {
        if name == crate::history::GRAPH {
            *data =
                br#"{"format":"emulsion-history","version":16,"broken":],"projec\u0074ive":null}"#
                    .to_vec();
        }
        true
    });
    assert!(ora::read_from(Cursor::new(broken)).is_err());
}

#[test]
fn unsafe_history_support_precedes_missing_source_png_and_history_tile_errors() {
    let doc = projected();
    let graph = Graph::try_new(doc.clone(), "Projected support").unwrap();
    let bytes = encode(&doc, Some(&graph));
    let bytes = json_edit(&bytes, |name, root| {
        if name == crate::history::GRAPH {
            root["commits"][0]["doc"]["nodes"][0]["kind"]["placement"] =
                json!({"projective":[1,0,0,0,1,0,-0.25,0,1]});
        }
    });
    let missing = rewrite(&bytes, |name, _| {
        !name.starts_with("history/tiles/") && !name.starts_with("emulsion/src/")
    });
    assert_both_readers_reject(&missing, "projective pixel support");
}

#[test]
fn pending_mask_metadata_never_becomes_a_completed_resource_certificate() {
    use emulsion_core::smart_support::{
        metadata_for_node, preflight_pending_mask_resources, preflight_stack_support,
    };
    for fill in [0, 127, 255] {
        let mut doc = projected();
        if let NodeKind::Smart { placement, .. } = &mut doc.nodes[0].kind {
            *placement = SmartPlacement::Projective(Projective2::IDENTITY);
        }
        doc.nodes[0].mask = Some(Arc::new(Mask::empty(30_000, 1, fill)));
        doc.nodes[0].mask_properties.feather = 1.0;
        preflight_pending_mask_resources(metadata_for_node(&doc.nodes[0]).unwrap()).unwrap();
        preflight_stack_support(metadata_for_node(&doc.nodes[0]).unwrap()).unwrap();
        let graph = Graph::try_new(doc.clone(), "Uniform mask remains valid").unwrap();
        let bytes = encode(&doc, Some(&graph));
        assert!(ora::read_from(Cursor::new(&bytes)).is_ok());
        let mut values = vec![fill; 30_000];
        values[0] = 255 - fill;
        let unsupported = rewrite(&bytes, |name, data| {
            if name == "emulsion/mask-1.png" {
                *data = crate::export::png_gray(30_000, 1, &values).unwrap();
            }
            true
        });
        assert_both_readers_reject(&unsupported, "ProcessedMask");
    }
}

#[test]
fn unsafe_live_or_retained_projection_cannot_replace_existing_destination() {
    let mut invalid = projected();
    if let NodeKind::Smart { placement, .. } = &mut invalid.nodes[0].kind {
        *placement = SmartPlacement::Projective(
            Projective2::from_row_major([1., 0., 0., 0., 1., 0., -0.25, 0., 1.]).unwrap(),
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preserve.ora");
    std::fs::write(&path, b"previous destination").unwrap();
    assert!(ora::write(&invalid, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous destination");
    assert!(Graph::try_new(invalid, "Unsafe retained projective support").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous destination");
}

#[test]
fn affine_wire_minimum_and_signed_zero_bits_remain_legacy() {
    let mut doc = plain();
    if let NodeKind::Smart { placement, .. } = &mut doc.nodes[0].kind {
        *placement = SmartPlacement::Legacy(Placement {
            x: -0.0,
            y: -0.0,
            rotation: -0.0,
            ..Placement::default()
        });
    }
    doc.nodes[0].mask_transform = Mapping2::Affine(glam::DAffine2::from_cols_array(&[
        1., -0., -0., 1., -0., -0.,
    ]));
    assert_eq!(ora::required_version(&doc), 9);
    let bytes = encode(&doc, None);
    let root = json_entry(&bytes, "emulsion.json");
    assert_eq!(root["version"], 9);
    assert!(root["nodes"][0]["mask_transform"].is_array());
    assert_eq!(
        root["nodes"][0]["kind"]["placement"]
            .as_object()
            .unwrap()
            .len(),
        7
    );
    let reopened = ora::read_from(Cursor::new(bytes)).unwrap();
    let NodeKind::Smart { placement, .. } = &reopened.doc.nodes[0].kind else {
        unreachable!()
    };
    let p = placement.require_legacy("legacy roundtrip").unwrap();
    assert_eq!(
        [p.x, p.y, p.rotation].map(f64::to_bits),
        [-0.0f64; 3].map(f64::to_bits)
    );
    assert_eq!(
        reopened.doc.nodes[0]
            .mask_transform
            .require_affine("legacy roundtrip")
            .unwrap()
            .to_cols_array()
            .map(f64::to_bits),
        [1., -0., -0., 1., -0., -0.].map(f64::to_bits)
    );
}

#[test]
fn source_pool_cache_footprint_and_live_expansion_fail_before_pixel_reads() {
    let doc = projected();
    let graph = Graph::try_new(doc.clone(), "Bounded support").unwrap();
    let bytes = encode(&doc, Some(&graph));
    for (case, expected) in [
        ("pool", "missing Smart source plane"),
        ("cache", "actual Smart cache"),
    ] {
        let edited = json_edit(&bytes, |name, root| {
            if name == crate::history::GRAPH {
                let kind = &mut root["commits"][0]["doc"]["nodes"][0]["kind"];
                if case == "pool" {
                    kind["source"] = 999999.into();
                } else {
                    kind["filters"] = json!([{"kind":"invert"}]);
                    kind["offset"] = json!([1, 0]);
                }
            }
        });
        let missing = rewrite(&edited, |name, _| {
            !name.starts_with("history/tiles/") && !name.starts_with("emulsion/src/")
        });
        assert_both_readers_reject(&missing, expected);
    }
    let bytes = encode(&doc, None);
    let unsafe_blur = json_edit(&bytes, |name, root| {
        if name == "emulsion.json" {
            root["nodes"][0]["kind"]["filters"] = json!([{"kind":"gaussian-blur","radius":32.0}]);
        }
    });
    let missing = rewrite(&unsafe_blur, |name, _| !name.starts_with("emulsion/src/"));
    assert_both_readers_reject(&missing, "projective pixel support");
}

#[test]
fn callable_projective_command_native_reopen_and_one_step_undo_redo_keep_sources() {
    let baseline = plain();
    let mut editor = emulsion_core::Editor::try_new(baseline.clone(), None).unwrap();
    let NodeKind::Smart { source, cache, .. } = &editor.doc.nodes[0].kind else {
        unreachable!()
    };
    let (source, cache) = (source.clone(), cache.clone());
    let delta = Projective2::from_row_major([1., 0., 0., 0., 1., 0., 1. / 128., 0., 1.]).unwrap();
    editor
        .execute(emulsion_core::Command::TransformSmartProjective { id: 1, delta })
        .unwrap();
    let projected = editor.doc.clone();
    let NodeKind::Smart {
        source: after,
        cache: after_cache,
        ..
    } = &projected.nodes[0].kind
    else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(&source, after));
    assert!(Arc::ptr_eq(&cache, after_cache));
    let rendered = flatten(&projected.try_composite_tree().unwrap(), 0).to_srgba16();
    let opened = ora::read_from(Cursor::new(encode(&projected, Some(&editor.graph)))).unwrap();
    assert_eq!(
        history_matches(&projected, &opened.doc),
        LiveRelation::Consistent
    );
    assert_eq!(
        flatten(&opened.doc.try_composite_tree().unwrap(), 0).to_srgba16(),
        rendered
    );
    assert!(editor.undo());
    assert_eq!(
        history_matches(&baseline, &editor.doc),
        LiveRelation::Consistent
    );
    assert!(editor.redo());
    assert_eq!(
        history_matches(&projected, &editor.doc),
        LiveRelation::Consistent
    );
    let mut restored =
        emulsion_core::Editor::try_with_graph(opened.doc, None, opened.graph.unwrap()).unwrap();
    restored
        .execute(emulsion_core::Command::TransformSmartProjective { id: 1, delta })
        .unwrap();
    assert!(restored.undo());
    assert_eq!(
        history_matches(&projected, &restored.doc),
        LiveRelation::Consistent
    );
    assert!(restored.redo());
}

#[test]
fn legacy_authored_mask_fill_is_the_same_final_plane_before_and_after_preflight() {
    // The pre-v16 reader's final construction is an independent control for
    // moving authored-fill normalization before exact detail certification.
    for fill in [0, 127, 255] {
        for detailed in [false, true] {
            let mut doc = plain();
            let raw = Arc::new(Mask::from_fn(9, 3, fill, |x, y| {
                if detailed && x == 1 && y == 1 {
                    255 - fill
                } else {
                    fill
                }
            }));
            doc.nodes[0].mask = Some(raw.clone());
            doc.nodes[0].mask_properties.feather = 1.25;
            doc.nodes[0].mask_linked = false;
            let old_temporary = Mask::from_gray8(raw.width(), raw.height(), &raw.to_gray8());
            let old_final = Mask::from_pixels(
                old_temporary.width(),
                old_temporary.height(),
                fill,
                &old_temporary.to_gray8(),
            );
            let appearance = flatten(&doc.try_composite_tree().unwrap(), 0).to_srgba16();
            let graph = Graph::new(doc.clone(), "Legacy authored mask fill");
            assert_eq!(ora::required_version(&doc), 10);
            for saved_history in [false, true] {
                let bytes = encode(&doc, saved_history.then_some(&graph));
                assert_eq!(json_entry(&bytes, "emulsion.json")["version"], 10);
                let opened = ora::read_from(Cursor::new(bytes)).unwrap();
                assert!(opened.history_error.is_none());
                let actual = opened.doc.nodes[0].mask.as_ref().unwrap();
                assert_eq!(actual.fill(), old_final.fill());
                assert_eq!(actual.to_gray8(), old_final.to_gray8());
                assert_eq!(actual.tile_count(), old_final.tile_count());
                assert_eq!(actual.tile_bounds(), old_final.tile_bounds());
                assert_eq!(history_matches(&doc, &opened.doc), LiveRelation::Consistent);
                assert_eq!(
                    flatten(&opened.doc.try_composite_tree().unwrap(), 0).to_srgba16(),
                    appearance
                );
                if let Some(graph) = &opened.graph {
                    let tip = &graph.commit(graph.head_branch().tip).unwrap().doc;
                    assert!(Arc::ptr_eq(actual, tip.nodes[0].mask.as_ref().unwrap()));
                    assert_eq!(history_matches(&doc, tip), LiveRelation::Consistent);
                }
                let again = ora::read_from(Cursor::new(encode(&opened.doc, opened.graph.as_ref())))
                    .unwrap();
                assert!(again.history_error.is_none());
                assert_eq!(history_matches(&doc, &again.doc), LiveRelation::Consistent);
                assert_eq!(
                    flatten(&again.doc.try_composite_tree().unwrap(), 0).to_srgba16(),
                    appearance
                );
            }
        }
    }
}

#[test]
fn native_cancellation_maps_retain_off_grid_mask_detail_without_forward_mask_bounds() {
    let h = Projective2::from_row_major([1., 0., 0., 0., 1., 0., 1. / 8., 0., 1.]).unwrap();
    let c = Projective2::from_row_major([1., 0., 0., 0., 1., 0., -1. / 8., 0., 1.]).unwrap();
    for fill in [0, 255] {
        let mut doc = plain();
        let mask = Arc::new(Mask::from_fn(16, 4, fill, |x, _| {
            if x == 1 || x == 12 { 255 - fill } else { fill }
        }));
        doc.nodes[0].mask = Some(mask.clone());
        doc.nodes[0].mask_transform = Mapping2::Projective(c);
        doc.nodes[0].mask_properties.feather = 1.0;
        doc.nodes[0].mask_properties.density = 0.65;
        if let NodeKind::Smart {
            placement,
            filter_mask,
            ..
        } = &mut doc.nodes[0].kind
        {
            *placement = SmartPlacement::Projective(h);
            let mut filter = SmartFilterMask::new(mask.clone());
            filter.enabled = false;
            filter.linked = false;
            filter.transform = Mapping2::Projective(c);
            *filter_mask = Some(filter);
        }
        let graph = Graph::try_new(doc.clone(), "Horizon cancellation").unwrap();
        let before = flatten(&doc.try_composite_tree().unwrap(), 0).to_srgba16();
        let opened = ora::read_from(Cursor::new(encode(&doc, Some(&graph)))).unwrap();
        assert!(opened.history_error.is_none());
        assert_eq!(history_matches(&doc, &opened.doc), LiveRelation::Consistent);
        assert_eq!(
            flatten(&opened.doc.try_composite_tree().unwrap(), 0).to_srgba16(),
            before
        );
        let restored = opened.doc.nodes[0].mask.as_ref().unwrap();
        assert_eq!(
            (restored.width(), restored.height(), restored.fill()),
            (16, 4, fill)
        );
        assert_eq!(restored.get(12, 2), 255 - fill);
        assert_eq!(restored.to_gray8(), mask.to_gray8());
        assert!(matches!(
            opened.doc.nodes[0].mask_transform,
            Mapping2::Projective(_)
        ));
    }
}
