use super::*;
use ag_psd::psd::{ColorMode, Psd, ReadOptions, WriteOptions};

const REAL_SOURCE: &[u8] = include_bytes!("../../tests/fixtures/psd/smartobject-layer.psd");

fn source() -> Arc<Raster> {
    Arc::new(Raster::from_srgba8(
        3,
        2,
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 51, 102, 153, 255, 204, 153, 102, 128,
            0, 0, 0, 0,
        ],
    ))
}
fn document() -> Document {
    let mut doc = Document::new(8, 8);
    doc.nodes.push(Node::smart(
        7,
        "Original source",
        source(),
        Vec::new(),
        Placement::at(2.0, 3.0),
    ));
    doc
}
fn raw_file(layers: Vec<Layer>, psb: bool) -> Vec<u8> {
    ag_psd::write_psd(
        &Psd {
            width: 8.0,
            height: 8.0,
            channels: Some(4.0),
            bits_per_channel: Some(8.0),
            color_mode: Some(ColorMode::Rgb),
            children: Some(layers),
            image_data: Some(PixelData {
                width: 8,
                height: 8,
                data: vec![0; 8 * 8 * 4],
            }),
            ..Default::default()
        },
        &WriteOptions {
            psb: Some(psb),
            compress: Some(false),
            trim_image_data: Some(false),
            generate_thumbnail: Some(false),
            ..Default::default()
        },
    )
}
fn exported(doc: &Document, psb: bool, change_preview: bool) -> Vec<u8> {
    let sources = prepare_export(doc).unwrap();
    let layers = doc
        .nodes
        .iter()
        .map(|node| {
            let mut l = Layer::default();
            l.additional_info.name = Some(node.name.clone());
            assert!(sources.apply_layer(node, &mut l));
            if change_preview {
                l.image_data.as_mut().unwrap().data.fill(0);
            }
            l
        })
        .collect();
    sources.insert(raw_file(layers, psb)).unwrap()
}
fn decoded(bytes: &[u8]) -> Psd {
    ag_psd::read_psd(
        bytes,
        &ReadOptions {
            use_image_data: Some(true),
            skip_thumbnail: Some(true),
            ..Default::default()
        },
    )
    .unwrap()
}
fn smart_fields(kind: &NodeKind) -> (&Arc<Raster>, Placement) {
    let NodeKind::Smart {
        source,
        cache,
        placement,
        filters,
        filter_styles,
        filter_mask,
        editable,
        offset,
        ..
    } = kind
    else {
        panic!("expected editable Smart source")
    };
    assert!(filters.is_empty());
    assert!(filter_styles.is_empty());
    assert!(filter_mask.is_none());
    assert!(editable.is_none());
    assert_eq!(*offset, (0, 0));
    assert!(Arc::ptr_eq(source, cache));
    (source, placement.legacy().unwrap())
}
fn placed_body(layer: &Layer) -> Vec<u8> {
    let options = WriteOptions::default();
    let mut context = ag_psd::additional_info::WriteCtx::new(&options, false);
    let mut writer = ag_psd::writer::create_writer(1024);
    ag_psd::additional_info::smart_object_keys::write(
        "SoLd",
        &mut writer,
        &layer.additional_info,
        &mut context,
    )
    .unwrap()
    .unwrap();
    ag_psd::writer::get_writer_buffer(&writer)
}
fn descriptor_body(d: &Descriptor) -> Vec<u8> {
    let mut writer = ag_psd::writer::create_writer(1024);
    ag_psd::writer::write_signature(&mut writer, "soLD");
    ag_psd::writer::write_int32(&mut writer, 4);
    ag_psd::descriptor::write_version_and_descriptor(&mut writer, d);
    ag_psd::writer::get_writer_buffer(&writer)
}
fn exported_descriptor() -> Descriptor {
    let doc = document();
    let sources = prepare_export(&doc).unwrap();
    let mut layer = Layer::default();
    sources.apply_layer(&doc.nodes[0], &mut layer);
    let body = placed_body(&layer);
    let mut c = Cursor::new(&body);
    c.take(8).unwrap();
    descriptor(&mut c).unwrap()
}
fn encoded_png() -> Vec<u8> {
    let src = source();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&src.to_srgba8(), 3, 2, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

#[test]
fn independent_mit_fixture_preserves_exact_original_png_across_native_round_trip() {
    let imported = inspect(REAL_SOURCE).unwrap();
    assert_eq!(imported.instances.len(), 1);
    let instance = imported.instances.values().next().unwrap();
    assert_eq!((instance.width, instance.height), (32, 32));
    let (source, original) = imported.sources.values().next().unwrap();
    let decoded_png = image::load_from_memory(original.bytes())
        .unwrap()
        .into_rgba8();
    assert_ne!(
        source.to_srgba8(),
        *decoded_png.as_raw(),
        "fixture contains hidden RGB/low-alpha source samples"
    );
    let parsed = decoded(REAL_SOURCE);
    let mut doc = Document::new(32, 32);
    doc.nodes.push(Node::new(
        1,
        "External original",
        imported
            .layer_kind(&parsed.children.as_ref().unwrap()[0])
            .unwrap()
            .unwrap(),
    ));
    doc.next_id = 2;
    let mut archive = std::io::Cursor::new(Vec::new());
    crate::ora::write_to(&doc, None, &mut archive).unwrap();
    let reopened = crate::ora::read_from(std::io::Cursor::new(archive.into_inner())).unwrap();
    let output = inspect(&exported(&reopened.doc, false, false)).unwrap();
    let (_, retained) = output.sources.values().next().unwrap();
    assert_eq!(retained.bytes().as_slice(), original.bytes().as_slice());
    assert_eq!(retained.source_sha256(), original.source_sha256());
}

#[test]
fn psd_and_psb_source_bytes_override_unrelated_layer_preview() {
    for psb in [false, true] {
        let doc = document();
        let bytes = exported(&doc, psb, true);
        let sources = inspect(&bytes).unwrap();
        let psd = decoded(&bytes);
        let layer = &psd.children.as_ref().unwrap()[0];
        assert!(
            layer
                .image_data
                .as_ref()
                .unwrap()
                .data
                .iter()
                .all(|b| *b == 0)
        );
        let kind = sources.layer_kind(layer).unwrap().unwrap();
        let (actual, placement) = smart_fields(&kind);
        assert_eq!(actual.to_srgba8(), source().to_srgba8());
        assert_eq!((placement.x, placement.y), (2.0, 3.0));
        let NodeKind::Raster { raster, .. } =
            emulsion_core::smart::restore_source(&Node::new(1, "source", kind), 8, 8).unwrap()
        else {
            panic!("source must be recoverable for pixel editing")
        };
        assert_eq!(raster.to_srgba8(), source().to_srgba8());
    }
}

#[test]
fn ordinary_layer_mask_remains_independent_of_smart_source() {
    let doc = document();
    let sources = prepare_export(&doc).unwrap();
    let mut layer = Layer::default();
    assert!(sources.apply_layer(&doc.nodes[0], &mut layer));
    layer.additional_info.mask = Some(ag_psd::psd::LayerMaskData {
        left: Some(3.0),
        top: Some(2.0),
        right: Some(5.0),
        bottom: Some(4.0),
        default_color: Some(255.0),
        disabled: Some(true),
        position_relative_to_layer: Some(true),
        image_data: Some(PixelData {
            width: 2,
            height: 2,
            data: vec![
                0, 0, 0, 255, 85, 85, 85, 255, 170, 170, 170, 255, 255, 255, 255, 255,
            ],
        }),
        ..Default::default()
    });
    let bytes = sources.insert(raw_file(vec![layer], false)).unwrap();
    let sources = inspect(&bytes).unwrap();
    let psd = decoded(&bytes);
    let layer = &psd.children.as_ref().unwrap()[0];
    let kind = sources.layer_kind(layer).unwrap().unwrap();
    let mut node = Node::new(1, "masked source", kind);
    super::super::mask_in(
        &mut node,
        layer.additional_info.mask.as_ref().unwrap(),
        2.0,
        3.0,
    )
    .unwrap();
    assert_eq!(
        node.mask_transform.affine().unwrap().to_cols_array(),
        [1.0, 0.0, 0.0, 1.0, 1.0, -1.0]
    );
    assert!(!node.mask_enabled);
    assert!(!node.mask_linked);
    assert_eq!(node.mask.as_ref().unwrap().to_gray8(), [0, 85, 170, 255]);
    assert_eq!(smart_fields(&node.kind).0.to_srgba8(), source().to_srgba8());
}

#[test]
fn source_and_instance_uuids_are_v4_unique_across_nodes_and_exports() {
    let mut doc = document();
    let mut other = doc.nodes[0].clone();
    other.id = 8;
    doc.nodes.push(other);
    let before = doc.clone();
    let mut all_ids = BTreeSet::new();
    for psb in [false, true] {
        let prepared = prepare_export(&doc).unwrap();
        for source in prepared.layers.values() {
            assert_eq!(source.placed.id, source.source_id);
            assert_eq!(source.placed.placed.as_ref(), Some(&source.instance_id));
            for uuid in [&source.source_id, &source.instance_id] {
                assert_eq!(&guid(uuid).unwrap(), uuid);
                assert_eq!(uuid.as_bytes()[14], b'4');
                assert!(matches!(uuid.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
                assert!(
                    all_ids.insert(uuid.clone()),
                    "UUID reused across roles/nodes/exports"
                );
            }
        }
        let layers = doc
            .nodes
            .iter()
            .map(|node| {
                let mut layer = Layer::default();
                assert!(prepared.apply_layer(node, &mut layer));
                layer
            })
            .collect();
        let bytes = prepared.insert(raw_file(layers, psb)).unwrap();
        let restored = inspect(&bytes).unwrap();
        assert_eq!((restored.instances.len(), restored.sources.len()), (2, 2));
        for source in prepared.layers.values() {
            assert_eq!(
                restored.instances[&source.instance_id].source_id,
                source.source_id
            );
            assert!(restored.sources.contains_key(&source.source_id));
        }
    }
    assert_eq!(all_ids.len(), 8);
    assert_eq!(doc, before);
}

#[test]
fn uuid_collision_retry_checks_final_version_and_variant_bits() {
    let doc = document();
    let mut calls = 0;
    let prepared = prepare_export_with_entropy(&doc, |bytes| {
        calls += 1;
        bytes.fill(0);
        match calls {
            1 => {}
            2 => {
                // These differ from the first candidate only in bits that
                // UUIDv4 must overwrite, so they are still a collision.
                bytes[6] = 0xf0;
                bytes[8] = 0xc0;
            }
            3 => bytes[15] = 1,
            _ => panic!("unexpected entropy retry"),
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 3);
    let source = &prepared.layers[&7];
    assert_eq!(source.source_id, "00000000-0000-4000-8000-000000000000");
    assert_eq!(source.instance_id, "00000000-0000-4000-8000-000000000001");
}

#[test]
fn uuid_entropy_failure_and_collision_exhaustion_abort_preparation() {
    let mut doc = document();
    let original_bytes = Arc::new(encoded_png());
    if let NodeKind::Smart {
        original_image,
        source,
        ..
    } = &mut doc.nodes[0].kind
    {
        *original_image = Some(crate::original_image_data::capture(
            original_bytes.clone(),
            source,
        ));
    }
    let bytes_before = original_bytes.as_ref().clone();
    let before = doc.clone();
    for fail_on in [1, 2] {
        let mut calls = 0;
        let result = prepare_export_with_entropy(&doc, |bytes| {
            calls += 1;
            bytes.fill(0xab); // Partial output must not become an ID on failure.
            if calls == fail_on {
                Err(SmartError::Unavailable("injected entropy failure"))
            } else {
                Ok(())
            }
        });
        assert!(matches!(
            result,
            Err(SmartError::Unavailable("injected entropy failure"))
        ));
        assert_eq!(
            calls, fail_on,
            "entropy errors are not retried or flattened"
        );
    }
    let mut calls = 0;
    let result = prepare_export_with_entropy(&doc, |bytes| {
        calls += 1;
        bytes.fill(0);
        Ok(())
    });
    assert!(matches!(
        result,
        Err(SmartError::Unavailable(
            "could not generate distinct Smart Object UUIDs"
        ))
    ));
    assert_eq!(calls, 1 + MAX_UUID_ATTEMPTS);
    assert_eq!(doc, before);
    assert_eq!(original_bytes.as_ref(), &bytes_before);

    // A document with no Smart source must not depend on the random service.
    assert!(
        prepare_export_with_entropy(&Document::new(1, 1), |_| {
            panic!("ordinary documents require no UUID entropy")
        })
        .is_ok()
    );
}

#[test]
fn insertion_rejects_ids_from_an_independent_export_of_the_same_nodes() {
    let doc = document();
    let first = prepare_export(&doc).unwrap();
    let second = prepare_export(&doc).unwrap();
    let mut layer = Layer::default();
    assert!(first.apply_layer(&doc.nodes[0], &mut layer));
    assert!(matches!(
        second.insert(raw_file(vec![layer], false)),
        Err(SmartError::Malformed(
            "emitted placed layers do not match embedded sources"
        ))
    ));
}

#[test]
fn filters_masks_source_archives_and_nontranslation_do_not_claim_support() {
    let doc = document();
    assert!(can_export(&doc.nodes[0]));
    let mut node = doc.nodes[0].clone();
    if let NodeKind::Smart { filters, .. } = &mut node.kind {
        filters.push(emulsion_filters::Filter::GaussianBlur { radius: 1.0 });
    }
    assert!(!can_export(&node));
    let mut node = doc.nodes[0].clone();
    if let NodeKind::Smart { filter_mask, .. } = &mut node.kind {
        *filter_mask = Some(emulsion_core::SmartFilterMask::new(Arc::new(
            emulsion_raster::Mask::white(3, 2),
        )));
    }
    assert!(!can_export(&node));
    for placement in [
        Placement::at(0.5, 0.0),
        Placement {
            scale_x: 2.0,
            ..Placement::default()
        },
        Placement {
            rotation: 90.0,
            ..Placement::default()
        },
        Placement {
            flip_x: true,
            ..Placement::default()
        },
    ] {
        let mut node = doc.nodes[0].clone();
        if let NodeKind::Smart { placement: p, .. } = &mut node.kind {
            *p = emulsion_core::SmartPlacement::Legacy(placement);
        }
        assert!(!can_export(&node));
    }
    let mut node = doc.nodes[0].clone();
    if let NodeKind::Smart { editable, .. } = &mut node.kind {
        *editable = Some(emulsion_core::node::SmartEditable::Svg {
            xml: Arc::from("<svg/>"),
        });
    }
    assert!(!can_export(&node));
}

#[test]
fn raw_filterfx_unknown_fields_and_duplicate_descriptor_keys_are_rejected() {
    let mut d = exported_descriptor();
    d.set(
        "filterFX",
        DescriptorValue::Descriptor(Descriptor::new("", "filterFX")),
    );
    assert!(matches!(
        placed(&descriptor_body(&d)),
        Err(SmartError::Unsupported(_))
    ));
    let mut d = exported_descriptor();
    d.set(
        "quiltWarp",
        DescriptorValue::Descriptor(Descriptor::new("", "warp")),
    );
    assert!(placed(&descriptor_body(&d)).is_err());
    let mut d = exported_descriptor();
    let id = d.get("Idnt").unwrap().clone();
    d.set("Idnt", id);
    assert!(matches!(
        placed(&descriptor_body(&d)),
        Err(SmartError::Malformed(_))
    ));
}

#[test]
fn descriptor_depth_and_list_counts_are_bounded_before_general_parser() {
    let mut nested = Descriptor::new("", "null");
    for _ in 0..12 {
        let mut parent = Descriptor::new("", "null");
        parent.set("child", DescriptorValue::Descriptor(nested));
        nested = parent;
    }
    assert!(placed(&descriptor_body(&nested)).is_err());
    let mut d = exported_descriptor();
    d.set(
        "huge",
        DescriptorValue::List(vec![DescriptorValue::Double(1.0); 257]),
    );
    assert!(placed(&descriptor_body(&d)).is_err());
}

#[test]
fn duplicate_sources_external_aliases_and_oversized_png_are_rejected() {
    let png = encoded_png();
    let record = embedded_record("00000001-0000-4000-8000-000000000001", &png).unwrap();
    let mut duplicate = record.clone();
    duplicate.extend_from_slice(&record);
    assert!(
        linked_sources(
            &duplicate,
            &mut BTreeMap::new(),
            &mut SourceBudget::default()
        )
        .is_err()
    );
    for kind in [b"liFE", b"liFA"] {
        let mut bytes = record.clone();
        bytes[8..12].copy_from_slice(kind);
        assert!(matches!(
            linked_sources(&bytes, &mut BTreeMap::new(), &mut SourceBudget::default()),
            Err(SmartError::Unsupported(_))
        ));
    }
    let mut huge = png.clone();
    huge[16..20].copy_from_slice(&30_001u32.to_be_bytes());
    let checksum = png_crc(b"IHDR", &huge[16..29]);
    huge[29..33].copy_from_slice(&checksum.to_be_bytes());
    assert!(matches!(png_source(&huge), Err(SmartError::Unsupported(_))));
    let mut wide_depth = png.clone();
    wide_depth[24] = 16;
    let checksum = png_crc(b"IHDR", &wide_depth[16..29]);
    wide_depth[29..33].copy_from_slice(&checksum.to_be_bytes());
    assert!(matches!(
        png_source(&wide_depth),
        Err(SmartError::Unsupported(_))
    ));
    let mut bad = png;
    let n = bad.len();
    bad[n - 1] ^= 1;
    assert!(png_source(&bad).is_err());
}

#[test]
fn source_uuid_and_instance_uuid_mismatches_never_use_preview() {
    let bytes = exported(&document(), false, false);
    let sources = inspect(&bytes).unwrap();
    let mut psd = decoded(&bytes);
    let layer = &mut psd.children.as_mut().unwrap()[0];
    layer.additional_info.placed_layer.as_mut().unwrap().id =
        "00000001-0000-4000-8000-000000000099".into();
    assert!(sources.layer_kind(layer).is_err());
    layer.additional_info.placed_layer.as_mut().unwrap().placed =
        Some("00000002-0000-4000-8000-000000000099".into());
    assert!(sources.layer_kind(layer).is_err());
    assert!(ImportSources::default().layer_kind(layer).is_err());
}

#[test]
fn legacy_and_modern_placement_disagreement_is_not_ignored() {
    let mut bytes = exported(&document(), false, false);
    let start = bytes.windows(4).position(|b| b == b"plcL").unwrap();
    let uuid_len = usize::from(bytes[start + 8]);
    let transform = start + 9 + uuid_len + 16;
    bytes[transform..transform + 8].copy_from_slice(&999.0f64.to_be_bytes());
    assert!(matches!(inspect(&bytes), Err(SmartError::Unsupported(_))));
}

#[test]
fn insertion_cannot_expose_source_inside_flattened_or_mismatched_output() {
    let doc = document();
    let sources = prepare_export(&doc).unwrap();
    assert!(
        sources
            .insert(raw_file(vec![Layer::default()], false))
            .is_err()
    );
    let bytes = exported(&doc, false, false);
    assert!(sources.insert(bytes).is_err());
}

#[test]
fn stale_empty_stack_cache_and_duplicate_node_ids_fail_preparation() {
    let mut doc = document();
    if let NodeKind::Smart { cache, .. } = &mut doc.nodes[0].kind {
        *cache = Arc::new(Raster::transparent(3, 2));
    }
    assert!(prepare_export(&doc).is_err());
    let mut doc = document();
    doc.nodes.push(doc.nodes[0].clone());
    assert!(prepare_export(&doc).is_err());
}

#[test]
fn source_framing_truncations_and_invalid_profiles_are_errors() {
    let bytes = exported(&document(), false, false);
    let file = file_sections(&bytes).unwrap();
    for cut in [
        0,
        4,
        12,
        25,
        file.length_at,
        file.length_at + file.wide + file.body.len() - 1,
    ] {
        assert!(inspect(&bytes[..cut]).is_err(), "cut {cut}");
    }
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&1039u16.to_be_bytes());
    resource.extend_from_slice(&[0, 0]);
    resource.extend_from_slice(&4u32.to_be_bytes());
    resource.extend_from_slice(b"fake");
    assert!(matches!(
        document_profile(&resource),
        Err(SmartError::Unsupported(_))
    ));
    assert!(document_profile(&[]).is_ok());
}

#[test]
fn document_tag_alignment_keeps_layer_tags_and_malformed_records_strict() {
    let tag = b"8BIMabcd\0\0\0\x03\x09\x08\x07\0";
    let mut padded = vec![0, 0];
    padded.extend_from_slice(tag);
    padded.extend_from_slice(&[0; 4]);
    let mut found = Vec::new();
    tags(
        &mut Cursor::new(&padded),
        4,
        TagLocation::Document,
        |key, body| {
            found.push((key.to_owned(), body.to_vec()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(found, vec![("abcd".to_owned(), vec![9, 8, 7])]);
    assert!(matches!(
        tags(&mut Cursor::new(&padded), 4, TagLocation::Layer, |_, _| Ok(
            ()
        )),
        Err(SmartError::Malformed(_))
    ));
    for bad in [
        [b"\0\0\x01".as_slice(), tag].concat(),
        [b"\0\0".as_slice(), &tag[..tag.len() - 1]].concat(),
        b"\0\0\x38\x42\x49".to_vec(),
        b"\0\0\x38\x42\x49\x4d\x61\x62\x63\x64\xff\xff\xff\xff".to_vec(),
    ] {
        assert!(matches!(
            tags(&mut Cursor::new(&bad), 4, TagLocation::Document, |_, _| Ok(
                ()
            )),
            Err(SmartError::Malformed(_))
        ));
    }
}

#[test]
fn padded_global_high_depth_tags_pass_the_full_read_path_in_psd_and_psb() {
    // Independently framed empty layer tables isolate document alignment and
    // PSD/PSB tag widths from the dependency's own writer conventions.
    let dir = tempfile::tempdir().unwrap();
    for psb in [false, true] {
        for (key, depth) in [(b"Lr16", 16u16), (b"Lr32", 32u16)] {
            for signature in [b"8BIM", b"8B64"] {
                let wide = if psb { 8 } else { 4 };
                let mut body = vec![0; wide + 4]; // Base layer info and global mask.
                body.extend_from_slice(b"8BIMabcd\0\0\0\x03\x09\x08\x07\0");
                body.extend_from_slice(&[0, 0]); // Additional document alignment.
                body.extend_from_slice(signature);
                body.extend_from_slice(key);
                put_length(
                    &mut body,
                    2,
                    if psb || signature == b"8B64" { 8 } else { 4 },
                )
                .unwrap();
                body.extend_from_slice(&[0; 4]); // Zero layer count and alignment.
                let mut bytes = b"8BPS".to_vec();
                bytes.extend_from_slice(&(if psb { 2u16 } else { 1u16 }).to_be_bytes());
                bytes.extend_from_slice(&[0; 6]);
                bytes.extend_from_slice(&3u16.to_be_bytes());
                bytes.extend_from_slice(&1u32.to_be_bytes());
                bytes.extend_from_slice(&1u32.to_be_bytes());
                bytes.extend_from_slice(&depth.to_be_bytes());
                bytes.extend_from_slice(&3u16.to_be_bytes());
                bytes.extend_from_slice(&[0; 8]); // Color-mode data and resources.
                put_length(&mut bytes, body.len(), wide).unwrap();
                bytes.extend_from_slice(&body);
                bytes.extend_from_slice(&[0; 2]); // Raw composite compression.
                bytes.resize(bytes.len() + 3 * usize::from(depth / 8), 0);
                assert!(inspect(&bytes).unwrap().instances.is_empty());
                assert_eq!(
                    super::super::mask_guard::unsupported_mask_reason(&bytes),
                    Ok(None)
                );
                assert!(
                    super::super::mask_guard::vector_decoder_copy(&bytes)
                        .unwrap()
                        .is_none()
                );
                let path = dir.path().join("padded.psd");
                std::fs::write(&path, &bytes).unwrap();
                let doc = super::super::read(&path).unwrap();
                assert_eq!((doc.width, doc.height, doc.nodes.len()), (1, 1, 1));
            }
        }
    }
}

#[test]
fn source_insertion_and_inspection_preserve_document_alignment() {
    for psb in [false, true] {
        let doc = document();
        let sources = prepare_export(&doc).unwrap();
        let mut layer = Layer::default();
        assert!(sources.apply_layer(&doc.nodes[0], &mut layer));
        let raw = raw_file(vec![layer], psb);
        let file = file_sections(&raw).unwrap();
        let extra = b"8BIMabcd\0\0\0\x03\x09\x08\x07\0\0\0";
        let mut padded = raw[..file.length_at].to_vec();
        put_length(&mut padded, file.body.len() + extra.len(), file.wide).unwrap();
        padded.extend_from_slice(file.body);
        padded.extend_from_slice(extra);
        padded.extend_from_slice(file.after);
        let inserted = sources.insert(padded).unwrap();
        let after = file_sections(&inserted).unwrap();
        assert!(after.body.starts_with(file.body));
        assert_eq!(
            &after.body[file.body.len()..file.body.len() + extra.len()],
            extra
        );
        let imported = inspect(&inserted).unwrap();
        assert_eq!((imported.instances.len(), imported.sources.len()), (1, 1));
    }
}

#[test]
fn shared_photoshop_source_instances_require_appearance_fallback() {
    let mut doc = document();
    let mut other = doc.nodes[0].clone();
    other.id = 8;
    doc.nodes.push(other);
    let sources = prepare_export(&doc).unwrap();
    let shared_id = sources.layers[&7].source_id.clone();
    let layers = doc
        .nodes
        .iter()
        .map(|node| {
            let mut layer = Layer::default();
            sources.apply_layer(node, &mut layer);
            // Two distinct placed UUIDs deliberately point at one embedded source.
            layer.additional_info.placed_layer.as_mut().unwrap().id = shared_id.clone();
            layer
        })
        .collect();
    let bytes = raw_file(layers, false);
    let file = file_sections(&bytes).unwrap();
    let linked = embedded_record(&shared_id, &encoded_png()).unwrap();
    let mut tag = b"8BIMlnk2".to_vec();
    put_length(&mut tag, linked.len(), 4).unwrap();
    tag.extend_from_slice(&linked);
    let mut external = bytes[..file.length_at].to_vec();
    put_length(&mut external, file.body.len() + tag.len(), file.wide).unwrap();
    external.extend_from_slice(file.body);
    external.extend_from_slice(&tag);
    external.extend_from_slice(file.after);
    assert!(matches!(
        inspect(&external),
        Err(SmartError::Unsupported(
            "shared editable Smart source instances"
        ))
    ));
    // Independent native nodes, even when they initially share pixel storage,
    // deliberately keep distinct embedded source UUIDs on ordinary export.
    assert_eq!(
        inspect(&exported(&doc, false, false))
            .unwrap()
            .sources
            .len(),
        2
    );
}

#[test]
fn higher_precision_native_source_is_not_silently_quantized_to_rgba8() {
    let raster = Arc::new(Raster::from_srgba16(1, 1, &[12345, 23456, 34567, 65535]));
    let reconstructed = Raster::from_srgba8(1, 1, &raster.to_srgba8());
    assert!(!same_pixels(&raster, &reconstructed));
    let mut doc = Document::new(1, 1);
    doc.nodes.push(Node::smart(
        1,
        "16-bit source",
        raster,
        Vec::new(),
        Placement::default(),
    ));
    assert!(matches!(
        prepare_export(&doc),
        Err(SmartError::Unsupported(
            "Smart source requires more than RGBA8 precision"
        ))
    ));
}

#[test]
fn differing_source_and_cache_inside_one_rgba8_bin_are_rejected() {
    let mut doc = document();
    let NodeKind::Smart { source, cache, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    let changed = Arc::new(Raster::from_fn(
        source.width(),
        source.height(),
        [0; 4],
        |x, y| {
            let mut pixel = source.get(x, y);
            if (x, y) == (0, 0) {
                pixel[0] -= 1;
            }
            pixel
        },
    ));
    assert_eq!(
        source.to_srgba8(),
        changed.to_srgba8(),
        "the preview intentionally cannot distinguish the samples"
    );
    assert!(!same_pixels(source, &changed));
    *cache = changed;
    assert!(matches!(
        prepare_export(&doc),
        Err(SmartError::Unsupported(
            "empty filter stack cache differs from source"
        ))
    ));
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&png_crc(kind, data).to_be_bytes());
    out
}

#[test]
fn every_png_chunk_crc_and_supported_metadata_order_are_checked() {
    assert_eq!(png_crc(b"IEND", &[]), 0xae42_6082);
    let png = encoded_png();
    let mut c = Cursor::new(&png);
    c.take(8).unwrap();
    let mut checksums = Vec::new();
    while !c.remaining().is_empty() {
        let n = c.length(4).unwrap();
        c.take(4 + n).unwrap();
        checksums.push(c.at);
        c.take(4).unwrap();
    }
    for offset in checksums {
        let mut bad = png.clone();
        bad[offset] ^= 1;
        assert!(matches!(
            png_source(&bad),
            Err(SmartError::Malformed("PNG chunk checksum"))
        ));
    }
    let srgb = png_chunk(b"sRGB", &[0]);
    let mut valid = png[..33].to_vec();
    valid.extend_from_slice(&srgb);
    valid.extend_from_slice(&png[33..]);
    assert!(png_source(&valid).is_ok());
    let mut duplicate = png[..33].to_vec();
    duplicate.extend_from_slice(&srgb);
    duplicate.extend_from_slice(&srgb);
    duplicate.extend_from_slice(&png[33..]);
    assert!(matches!(
        png_source(&duplicate),
        Err(SmartError::Malformed("PNG metadata order or duplicate"))
    ));
    let end = png.len() - 12;
    let mut late = png[..end].to_vec();
    late.extend_from_slice(&srgb);
    late.extend_from_slice(&png[end..]);
    assert!(matches!(
        png_source(&late),
        Err(SmartError::Malformed("PNG metadata order or duplicate"))
    ));
}

fn rgba_png(samples: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            samples,
            (samples.len() / 4) as u32,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    out
}

#[test]
fn hidden_rgb_and_alpha_one_through_five_keep_exact_original_png_bytes() {
    let samples = [
        255, 255, 255, 0, 1, 27, 85, 1, 41, 125, 248, 2, 252, 97, 3, 3, 123, 47, 211, 4, 11, 231,
        5, 5,
    ];
    let bytes = rgba_png(&samples);
    let source = png_source(&bytes).unwrap();
    assert_ne!(source.to_srgba8(), samples);
    let original = crate::original_image_data::capture(Arc::new(bytes.clone()), &source);
    let mut doc = Document::new(8, 8);
    let mut node = Node::smart(1, "Retained", source, Vec::new(), Placement::default());
    if let NodeKind::Smart { original_image, .. } = &mut node.kind {
        *original_image = Some(original);
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    for psb in [false, true] {
        let first = exported(&doc, psb, false);
        let imported = inspect(&first).unwrap();
        assert_eq!(
            imported
                .sources
                .values()
                .next()
                .unwrap()
                .1
                .bytes()
                .as_slice(),
            bytes
        );
        let parsed = decoded(&first);
        doc.nodes[0].kind = imported
            .layer_kind(&parsed.children.as_ref().unwrap()[0])
            .unwrap()
            .unwrap();
        let mut native = std::io::Cursor::new(Vec::new());
        crate::ora::write_to(&doc, None, &mut native).unwrap();
        let restored = crate::ora::read_from(std::io::Cursor::new(native.into_inner())).unwrap();
        let second = inspect(&exported(&restored.doc, psb, false)).unwrap();
        assert_eq!(
            second.sources.values().next().unwrap().1.bytes().as_slice(),
            bytes
        );
    }
}

#[test]
fn padded_tile_budget_rejects_skinny_sources_before_any_pixel_allocation() {
    assert_eq!(native_storage(1, 30_000).unwrap(), 118 * 256 * 256 * 8);
    let mut budget = SourceBudget::default();
    for _ in 0..4 {
        budget.charge(1, 30_000, 100).unwrap();
    }
    assert!(matches!(
        budget.charge(1, 30_000, 100),
        Err(SmartError::Unsupported(
            "total padded native tile byte limit"
        ))
    ));
    assert_eq!(
        budget.pixels, 120_000,
        "failed charge must not mutate budget"
    );
    assert!(matches!(
        native_storage(30_000, 533),
        Err(SmartError::Unsupported(
            "source padded native tile byte limit"
        ))
    ));

    // The PNG is structurally framed and checksummed but its IDAT is invalid.
    // Resource refusal must happen before the pixel decoder reaches that data.
    let mut png = encoded_png();
    let mut c = Cursor::new(&png);
    c.take(8).unwrap();
    let (body_at, length, crc_at) = loop {
        let length = c.length(4).unwrap();
        let kind = c.take(4).unwrap();
        let at = c.at;
        c.take(length).unwrap();
        let crc = c.at;
        c.take(4).unwrap();
        if kind == b"IDAT" {
            break (at, length, crc);
        }
    };
    png[body_at..body_at + length].fill(0);
    let crc = png_crc(b"IDAT", &png[body_at..body_at + length]);
    png[crc_at..crc_at + 4].copy_from_slice(&crc.to_be_bytes());
    let mut exhausted = SourceBudget {
        native: MAX_TOTAL_SOURCE_NATIVE_BYTES,
        ..Default::default()
    };
    assert!(matches!(
        png_source_with_budget(&png, &mut exhausted),
        Err(SmartError::Unsupported(
            "total padded native tile byte limit"
        ))
    ));
}

#[test]
fn export_checks_all_padded_source_storage_before_reconstruction() {
    let source = Arc::new(Raster::transparent(1, 30_000));
    let mut doc = Document::new(1, 30_000);
    for id in 1..=5 {
        doc.nodes.push(Node::smart(
            id,
            "skinny",
            source.clone(),
            Vec::new(),
            Placement::default(),
        ));
    }
    assert!(matches!(
        prepare_export(&doc),
        Err(SmartError::Unsupported(
            "total padded native tile byte limit"
        ))
    ));
}

#[test]
fn insertion_rejects_changed_source_placement_even_when_ids_and_dimensions_match() {
    let doc = document();
    let sources = prepare_export(&doc).unwrap();
    let mut layer = Layer::default();
    sources.apply_layer(&doc.nodes[0], &mut layer);
    let placed = layer.additional_info.placed_layer.as_mut().unwrap();
    for x in placed.transform.iter_mut().step_by(2) {
        *x += 5.0;
    }
    assert!(matches!(
        sources.insert(raw_file(vec![layer], false)),
        Err(SmartError::Malformed(
            "emitted placed layers do not match embedded sources"
        ))
    ));
}

#[test]
fn png_idat_requires_adler_stream_end_exact_output_and_no_trailing_data() {
    use std::io::Write;
    let scanline = [0, 17, 34, 51, 255];
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&scanline).unwrap();
    let encoded = encoder.finish().unwrap();
    assert!(strict_idat(&[&encoded], scanline.len()).is_ok());
    let pieces: Vec<_> = encoded.chunks(1).collect();
    assert!(
        strict_idat(&pieces, scanline.len()).is_ok(),
        "IDAT boundaries may split any zlib byte"
    );
    let mut checksum = encoded.clone();
    *checksum.last_mut().unwrap() ^= 1;
    let mut trailing = encoded.clone();
    trailing.push(0);
    let mut concatenated = encoded.clone();
    concatenated.extend_from_slice(&encoded);
    for bad in [
        &checksum[..],
        &encoded[..encoded.len() - 1],
        &encoded[..encoded.len() - 4],
        &trailing[..],
        &concatenated[..],
    ] {
        assert!(strict_idat(&[bad], scanline.len()).is_err());
    }
    assert!(strict_idat(&[&encoded], scanline.len() - 1).is_err());
    assert!(strict_idat(&[&encoded], scanline.len() + 1).is_err());

    let png = rgba_png(&scanline[1..]);
    for bad in [&checksum[..], &encoded[..encoded.len() - 1], &trailing[..]] {
        let mut forged = png[..33].to_vec();
        forged.extend_from_slice(&png_chunk(b"IDAT", bad));
        forged.extend_from_slice(&png_chunk(b"IEND", &[]));
        assert!(
            matches!(png_source(&forged), Err(SmartError::Malformed(_))),
            "valid chunk CRCs cannot hide a bad zlib stream"
        );
    }
}

#[test]
fn strict_idat_drains_scratch_boundaries_and_multiple_png_chunks() {
    use std::io::Write;
    for length in [16 * 1024, 32 * 1024 + 1] {
        let bytes: Vec<u8> = (0..length).map(|i| ((i * 13) % 251) as u8).collect();
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&bytes).unwrap();
        let encoded = encoder.finish().unwrap();
        for chunk_size in [1, 7, encoded.len()] {
            let chunks: Vec<_> = encoded.chunks(chunk_size).collect();
            assert!(
                strict_idat(&chunks, length).is_ok(),
                "output={length}, chunk={chunk_size}"
            );
        }
    }
    let pixels: Vec<u8> = [17, 34, 51, 255].repeat(128 * 64);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, 128, 64, image::ExtendedColorType::Rgba8)
        .unwrap();
    let mut c = Cursor::new(&png);
    c.take(8).unwrap();
    let mut compressed = Vec::new();
    while !c.remaining().is_empty() {
        let n = c.length(4).unwrap();
        let key = c.take(4).unwrap();
        let body = c.take(n).unwrap();
        c.take(4).unwrap();
        if key == b"IDAT" {
            compressed.extend_from_slice(body);
        }
    }
    let mut split = png[..33].to_vec();
    for body in compressed.chunks(7) {
        split.extend_from_slice(&png_chunk(b"IDAT", body));
    }
    split.extend_from_slice(&png_chunk(b"IEND", &[]));
    assert_eq!(png_source(&split).unwrap().to_srgba8(), pixels);
}

#[test]
fn stale_original_rejects_source_export_and_fallback_contains_no_hidden_payload() {
    let png = rgba_png(&[255, 255, 255, 0, 1, 2, 3, 1]);
    let source = png_source(&png).unwrap();
    let original = crate::original_image_data::capture(Arc::new(png.clone()), &source);
    let mut doc = Document::new(2, 1);
    let mut node = Node::smart(
        1,
        "Invalid provenance",
        Arc::new(Raster::solid(2, 1, [1., 0., 0., 1.])),
        Vec::new(),
        Placement::default(),
    );
    if let NodeKind::Smart { original_image, .. } = &mut node.kind {
        *original_image = Some(original);
    }
    doc.nodes.push(node);
    doc.next_id = 2;
    assert!(prepare_export(&doc).is_err());
    let path = std::env::temp_dir().join(format!(
        "emulsion-original-fallback-{}.psd",
        std::process::id()
    ));
    let report = super::super::write_with_report(&doc, &path).unwrap();
    assert!(report.appearance_fallback.is_some());
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(path);
    assert!(inspect(&bytes).unwrap().sources.is_empty());
    assert!(
        !bytes
            .windows(png.len())
            .any(|window| window == png.as_slice())
    );
    assert!(
        decoded(&bytes)
            .children
            .unwrap()
            .iter()
            .all(|layer| layer.additional_info.placed_layer.is_none())
    );
}
