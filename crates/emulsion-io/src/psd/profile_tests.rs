//! Generated files test bounded selection/metadata transport, not third-party
//! parity. The separate immutable numeric fixture gates supply that evidence.
use super::*;
use emulsion_raster::blend::BlendSpace;

fn pixels(doc: &mut Document, name: &str, rgba: [u8; 4], parent: Option<NodeId>) -> NodeId {
    add(
        doc,
        Node::raster(
            0,
            name,
            Arc::new(Raster::from_srgba8(
                doc.width,
                doc.height,
                &rgba.repeat((doc.width * doc.height) as usize),
            )),
            Placement::default(),
        ),
        parent,
    )
    .unwrap()
}

fn alpha_scene() -> Document {
    let mut doc = Document::new(4, 3);
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    pixels(&mut doc, "red", [255, 0, 0, 255], None);
    let top = pixels(&mut doc, "blue", [0, 0, 255, 255], None);
    doc.node_mut(top).unwrap().blending.fill_opacity = 128.0 / 255.0;
    doc
}

fn psd_for(doc: &Document) -> Psd {
    let mut children: Vec<_> = doc
        .children(None)
        .into_iter()
        .map(|id| layer_for(doc, doc.node(id).unwrap()))
        .collect();
    profile::prepare_layers(doc, &mut children).unwrap();
    Psd {
        width: f64::from(doc.width),
        height: f64::from(doc.height),
        bits_per_channel: Some(8.0),
        color_mode: Some(ColorMode::Rgb),
        children: Some(children),
        image_data: Some(PixelData {
            width: doc.width,
            height: doc.height,
            data: profile::render_cpu(doc),
        }),
        image_resources: Some(ag_psd::psd::ImageResources {
            version_info: Some(ag_psd::psd::VersionInfo {
                has_real_merged_data: true,
                writer_name: "Selection test producer".into(),
                reader_name: "Selection test reader".into(),
                file_version: 1.0,
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn encode(psd: &Psd) -> Vec<u8> {
    ag_psd::write_psd(
        psd,
        &WriteOptions {
            no_background: Some(true),
            trim_image_data: Some(false),
            compress: Some(false),
            ..Default::default()
        },
    )
}

#[test]
fn opaque_normal_alpha_unique_match_selects_new_profile_without_writer_name_trust() {
    let original = alpha_scene();
    let psd = psd_for(&original);
    let (doc, report) = read_bytes_with_report(&encode(&psd)).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::UniquePhotoshopSrgbV1
    );
    assert_eq!(doc.blend_space, BlendSpace::PhotoshopSrgbV1);
    assert_eq!(doc.nodes.len(), 2);
    assert_eq!(profile::render_cpu(&doc), [127, 0, 128, 255].repeat(12));
}

#[test]
fn ambiguity_prefers_legacy_and_reports_only_current_pixel_equality() {
    let mut original = alpha_scene();
    original.nodes.last_mut().unwrap().blending.fill_opacity = 1.0;
    let (doc, report) = read_bytes_with_report(&encode(&psd_for(&original))).unwrap();
    assert_eq!(doc.blend_space, BlendSpace::Srgb);
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::LegacyMatch { ambiguous: true }
    );
    assert_eq!(doc.nodes.len(), 2);
}

#[test]
fn no_match_uses_the_original_saved_appearance() {
    let mut psd = psd_for(&alpha_scene());
    psd.image_data.as_mut().unwrap().data = [30, 40, 50, 255].repeat(12);
    let (doc, report) = read_bytes_with_report(&encode(&psd)).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::SavedAppearance
    );
    assert_eq!(doc.nodes.len(), 1);
    assert_eq!(profile::render_cpu(&doc), [30, 40, 50, 255].repeat(12));
}

#[test]
fn linear_only_match_does_not_infer_an_adobe_gamma_preference() {
    let mut original = alpha_scene();
    original.blend_space = BlendSpace::Linear;
    original.nodes[0].kind = NodeKind::Raster {
        raster: Arc::new(Raster::from_srgba8(4, 3, &[77, 151, 201, 255].repeat(12))),
        placement: Placement::default(),
    };
    original.nodes[1].kind = NodeKind::Raster {
        raster: Arc::new(Raster::from_srgba8(4, 3, &[179, 71, 40, 255].repeat(12))),
        placement: Placement::default(),
    };
    original.nodes[1].blend = BlendMode::Screen;
    original.nodes[1].blending.fill_opacity = 1.0;
    let (doc, report) = read_bytes_with_report(&encode(&psd_for(&original))).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::SavedAppearance
    );
    assert_eq!(doc.nodes.len(), 1);
}

#[test]
fn missing_or_false_merged_declaration_never_selects_new_profile() {
    for declaration in [None, Some(false)] {
        let mut psd = psd_for(&alpha_scene());
        if let Some(real) = declaration {
            psd.image_resources
                .as_mut()
                .unwrap()
                .version_info
                .as_mut()
                .unwrap()
                .has_real_merged_data = real;
        } else {
            psd.image_resources = None;
        }
        let (doc, report) = read_bytes_with_report(&encode(&psd)).unwrap();
        assert_eq!(doc.blend_space, BlendSpace::Srgb);
        assert_eq!(report.profile_decision, ImportProfileDecision::NotCompared);
    }
}

#[test]
fn transparent_multicolour_candidates_do_not_use_white_matte_output_as_an_oracle() {
    let mut doc = alpha_scene();
    let NodeKind::Raster { raster, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *raster = Arc::new(Raster::from_srgba8(4, 3, &[255, 0, 0, 128].repeat(12)));
    let (back, report) = read_bytes_with_report(&encode(&psd_for(&doc))).unwrap();
    assert_ne!(
        report.profile_decision,
        ImportProfileDecision::UniquePhotoshopSrgbV1
    );
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::SavedAppearance
    );
    assert_eq!(back.nodes.len(), 1);
}

#[test]
fn single_source_transparent_mask_keeps_editable_state_when_candidates_agree() {
    let mut doc = Document::new(4, 3);
    let id = pixels(&mut doc, "masked", [130, 91, 210, 255], None);
    doc.node_mut(id).unwrap().mask = Some(Arc::new(Mask::from_fn(4, 3, 0, |x, _| (x * 60) as u8)));
    let (back, report) = read_bytes_with_report(&encode(&psd_for(&doc))).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::SameCurrentAppearance
    );
    assert!(back.nodes[0].mask.is_some());
}

#[test]
fn raw_ids_and_group_framing_are_required_for_typed_knockout_association() {
    let doc = alpha_scene();
    let mut psd = psd_for(&doc);
    let duplicate = psd.children.as_ref().unwrap()[0].additional_info.id;
    psd.children.as_mut().unwrap()[1].additional_info.id = duplicate;
    // The encoder deduplicates IDs, so mutate the second structurally found
    // raw lyid after encoding, rather than assuming an arbitrary byte offset.
    let mut bytes = encode(&psd);
    let raw = mask_guard::raw_metadata(&bytes).unwrap();
    let first = raw.records[0].one(b"lyid").unwrap().to_vec();
    let offset = raw.records[1].one(b"lyid").unwrap().as_ptr() as usize - bytes.as_ptr() as usize;
    bytes[offset..offset + 4].copy_from_slice(&first);
    let decoded = ag_psd::read_psd(&bytes, &ReadOptions::default()).unwrap();
    assert!(profile::associate(&mask_guard::raw_metadata(&bytes).unwrap(), &decoded).is_none());
}

#[test]
fn raw_rgb_restriction_ignored_by_dependency_cannot_select_a_profile() {
    let doc = alpha_scene();
    let mut psd = psd_for(&doc);
    psd.children.as_mut().unwrap()[1]
        .additional_info
        .channel_blending_restrictions = Some(vec![0.0]);
    let bytes = encode(&psd);
    let decoded = ag_psd::read_psd(&bytes, &ReadOptions::default()).unwrap();
    let association =
        profile::associate(&mask_guard::raw_metadata(&bytes).unwrap(), &decoded).unwrap();
    let native =
        from_psd_with_metadata(&decoded, false, &Default::default(), Some(&association)).unwrap();
    assert_eq!(
        native.nodes[1].blending.channels, [true; 3],
        "dependency drops the sole word"
    );
    assert!(!profile::selectable(
        &mask_guard::raw_metadata(&bytes).unwrap(),
        &native
    ));
}

#[test]
fn unsupported_hidden_blend_if_remains_an_appearance_import() {
    let mut psd = psd_for(&alpha_scene());
    let layer = &mut psd.children.as_mut().unwrap()[1];
    layer.hidden = Some(true);
    layer.additional_info.blending_ranges =
        blend_metadata::export(emulsion_raster::composite::BlendIf {
            source: BlendRange {
                black: 0.2,
                black_fade: 0.4,
                ..Default::default()
            },
            ..Default::default()
        });
    psd.image_data.as_mut().unwrap().data = [255, 0, 0, 255].repeat(12);
    let (doc, report) = read_bytes_with_report(&encode(&psd)).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::SavedAppearance
    );
    assert_eq!(doc.nodes.len(), 1);
}

#[test]
fn truncated_original_merged_samples_cannot_authorize_selection() {
    let mut bytes = encode(&psd_for(&alpha_scene()));
    bytes.truncate(bytes.len() - 1);
    assert!(read_bytes_with_report(&bytes).is_err());
}

fn clean_knockout() -> Document {
    let mut doc = Document::new(8, 6);
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    let background = pixels(&mut doc, "Renamed role", [255, 255, 255, 255], None);
    doc.psd_background = Some(background);
    pixels(&mut doc, "Background", [255, 0, 0, 255], None);
    let outer = add(&mut doc, Node::group(0, "Outer"), None).unwrap();
    doc.node_mut(outer).unwrap().blend = BlendMode::PassThrough;
    pixels(&mut doc, "Green", [0, 255, 0, 255], Some(outer));
    let inner = add(&mut doc, Node::group(0, "Inner"), Some(outer)).unwrap();
    let group = doc.node_mut(inner).unwrap();
    group.blend = BlendMode::Normal;
    group.blending.knockout = Knockout::Deep;
    group.blending.fill_opacity = 128.0 / 255.0;
    pixels(&mut doc, "Blue", [0, 0, 255, 255], Some(inner));
    doc
}

#[test]
fn explicit_background_and_typed_deep_roundtrip_without_bool_loss() {
    let dir = tempfile::tempdir().unwrap();
    for extension in ["psd", "psb"] {
        let doc = clean_knockout();
        let original = doc.clone();
        let path = dir.path().join(format!("deep.{extension}"));
        assert_eq!(
            write_with_report(&doc, &path).unwrap().appearance_fallback,
            None
        );
        let bytes = std::fs::read(&path).unwrap();
        let raw = mask_guard::raw_metadata(&bytes).unwrap();
        assert_eq!(raw.records[0].channels, [0, 1, 2]);
        assert_eq!(raw.records[0].flags, 0x09);
        assert_eq!(raw.records[0].number(b"lspf"), Some(0x0d));
        assert_eq!(raw.records[0].one(b"lnsr"), Some(b"bgnd".as_slice()));
        assert_eq!(
            raw.records
                .iter()
                .filter(|r| r.one(b"knko") == Some(&[2, 0, 0, 0]))
                .count(),
            1
        );
        assert!(
            raw.records[1].channels.contains(&-1),
            "ordinary opaque BG retains alpha"
        );
        let (back, report) = read_with_report(&path).unwrap();
        assert_eq!(
            report.profile_decision,
            ImportProfileDecision::UniquePhotoshopSrgbV1
        );
        assert!(report.background_preserved);
        assert_eq!(
            back.node(back.psd_background.unwrap()).unwrap().name,
            "Renamed role"
        );
        assert_eq!(
            back.nodes
                .iter()
                .filter(|n| n.blending.knockout == Knockout::Deep)
                .count(),
            1
        );
        assert_eq!(profile::render_cpu(&back), [127, 127, 255, 255].repeat(48));
        assert_eq!(
            doc, original,
            "export never changes native source or history identity"
        );
    }
}

#[test]
fn ordinary_opaque_background_name_never_creates_the_special_role() {
    let mut doc = Document::new(4, 3);
    pixels(&mut doc, "Background", [255; 4], None);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ordinary.psd");
    write(&doc, &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let raw = mask_guard::raw_metadata(&bytes).unwrap();
    assert!(raw.records[0].channels.contains(&-1));
    assert_eq!(raw.records[0].one(b"lnsr"), None);
    assert_eq!(read(&path).unwrap().psd_background, None);
}

#[test]
fn emitted_deep_patch_refuses_mismatched_ids_before_any_destination_write() {
    let doc = clean_knockout();
    let mut psd = psd_for(&doc);
    let expected = profile::prepare_layers(&doc, psd.children.as_mut().unwrap()).unwrap();
    psd.children.as_mut().unwrap()[0].additional_info.id = Some(99.0);
    assert!(profile::patch_export(encode(&psd), &psd, &expected).is_err());
}

#[test]
fn blank_document_exports_without_an_unassociated_dependency_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blank.psd");
    let doc = Document::new(4, 3);
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        None
    );
    assert_eq!(profile::render_cpu(&read(&path).unwrap()), vec![0; 48]);
}

#[test]
fn multicolour_masked_new_profile_roundtrip_preserves_paths_and_reports_legacy_difference() {
    let mut doc = alpha_scene();
    let layer = &mut doc.nodes[1];
    layer.blending.fill_opacity = 1.0;
    layer.mask = Some(Arc::new(Mask::from_fn(4, 3, 0, |x, _| {
        if x == 1 { 128 } else { 255 }
    })));
    layer.vector_mask = Some(emulsion_core::VectorMask {
        path: Arc::new(
            emulsion_raster::vector::Path::from_svg("M 0 0 L 3 0 L 3 3 L 0 3 Z").unwrap(),
        ),
        ..Default::default()
    });
    let native = doc.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multicolour-mask.psd");
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        None
    );
    let (back, report) = read_with_report(&path).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::UniquePhotoshopSrgbV1
    );
    assert_eq!(back.nodes.len(), 2);
    assert!(
        back.nodes
            .iter()
            .any(|node| node.mask.is_some() && node.vector_mask.is_some())
    );
    assert_eq!(profile::render_cpu(&back), profile::render_cpu(&doc));
    assert_eq!(doc, native);
    doc.blend_space = BlendSpace::Linear;
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        Some(AppearanceFallback::BlendSpaceDifference)
    );
    assert_eq!(
        profile::render_cpu(&read(&path).unwrap()),
        profile::render_cpu(&doc)
    );
    assert!(
        doc.nodes[1].vector_mask.is_some(),
        "native editable geometry is never changed by export"
    );
}

#[test]
fn source_only_masked_pass_through_survives_but_any_preceding_root_is_conservative_fallback() {
    let mut doc = Document::new(4, 3);
    let mut group = Node::group(0, "source-only mask");
    group.vector_mask = Some(emulsion_core::VectorMask {
        path: Arc::new(
            emulsion_raster::vector::Path::from_svg("M 0 0 L 3 0 L 3 3 L 0 3 Z").unwrap(),
        ),
        ..Default::default()
    });
    let group = add(&mut doc, group, None).unwrap();
    pixels(&mut doc, "source", [153, 68, 221, 255], Some(group));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source-only.psd");
    assert!(!needs_appearance_fallback(&doc));
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        None
    );
    let back = read(&path).unwrap();
    assert_eq!(back.nodes.len(), 2);
    assert!(back.nodes.iter().any(|node| node.vector_mask.is_some()));
    assert_eq!(profile::render_cpu(&back), profile::render_cpu(&doc));
    let mut earlier = Node::raster(
        0,
        "even a hidden preceding root",
        Arc::new(Raster::transparent(4, 3)),
        Placement::default(),
    );
    earlier.visible = false;
    Command::AddNode {
        node: Box::new(earlier),
        slot: Slot {
            parent: None,
            index: 0,
        },
    }
    .apply(&mut doc)
    .unwrap();
    assert!(needs_appearance_fallback(&doc));
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        Some(AppearanceFallback::UnsupportedFeatures)
    );
}

#[test]
fn actual_quantized_opacity_is_checked_before_reporting_layered_success() {
    let mut doc = alpha_scene();
    doc.nodes[1].blending.fill_opacity = 1.0;
    doc.nodes[1].opacity = 0.5;
    let before = doc.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("half-opacity.psd");
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        Some(AppearanceFallback::BlendSpaceDifference)
    );
    assert_eq!(
        profile::render_cpu(&read(&path).unwrap()),
        profile::render_cpu(&doc)
    );
    assert_eq!(doc, before);
}

fn append_resource(bytes: &[u8], resource: &[u8]) -> Vec<u8> {
    let color_len = u32::from_be_bytes(bytes[26..30].try_into().unwrap()) as usize;
    let at = 30 + color_len;
    let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut output = bytes[..at].to_vec();
    output.extend_from_slice(
        &u32::try_from(length + resource.len())
            .unwrap()
            .to_be_bytes(),
    );
    output.extend_from_slice(&bytes[at + 4..at + 4 + length]);
    output.extend_from_slice(resource);
    output.extend_from_slice(&bytes[at + 4 + length..]);
    output
}

fn resource(id: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = b"8BIM".to_vec();
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.extend_from_slice(&[0, 0]); // Empty padded Pascal name.
    bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
    bytes.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        bytes.push(0);
    }
    bytes
}

#[test]
fn unknown_icc_and_duplicate_merged_declarations_do_not_select_a_profile() {
    let original = encode(&psd_for(&alpha_scene()));
    let unknown = append_resource(&original, &resource(1039, &[0; 128]));
    assert!(!mask_guard::raw_metadata(&unknown).unwrap().srgb);
    assert_ne!(
        read_bytes_with_report(&unknown).unwrap().1.profile_decision,
        ImportProfileDecision::UniquePhotoshopSrgbV1
    );
    // Independently encoded minimal VersionInfo 1 with empty writer/reader.
    let mut version = vec![0, 0, 0, 1, 1];
    version.extend_from_slice(&[0; 8]);
    version.extend_from_slice(&1u32.to_be_bytes());
    let duplicate = append_resource(&original, &resource(1057, &version));
    assert!(!mask_guard::raw_metadata(&duplicate).unwrap().real_merged);
    assert_eq!(
        read_bytes_with_report(&duplicate)
            .unwrap()
            .1
            .profile_decision,
        ImportProfileDecision::NotCompared
    );
}

#[test]
fn hidden_background_role_is_retained_without_name_inference() {
    let mut doc = Document::new(4, 3);
    let id = pixels(&mut doc, "Completely different name", [255; 4], None);
    doc.psd_background = Some(id);
    doc.node_mut(id).unwrap().visible = false;
    pixels(&mut doc, "ordinary", [0, 255, 0, 255], None);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hidden-background.psd");
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        None
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        mask_guard::raw_metadata(&bytes).unwrap().records[0].flags,
        0x0b
    );
    let (back, report) = read_with_report(&path).unwrap();
    assert!(report.background_preserved);
    assert!(!back.node(back.psd_background.unwrap()).unwrap().visible);
}

#[test]
fn unknown_resource_and_unidentified_extra_composite_channel_are_not_evidence() {
    let doc = alpha_scene();
    let bytes = encode(&psd_for(&doc));
    let unknown = append_resource(&bytes, &resource(1999, &[0; 4]));
    let raw = mask_guard::raw_metadata(&unknown).unwrap();
    assert!(!raw.known_document_metadata);
    assert!(!profile::admissible(&raw, &doc));
    assert_eq!(
        read_bytes_with_report(&unknown).unwrap().1.profile_decision,
        ImportProfileDecision::NotCompared
    );
    let mut spot = bytes;
    spot[12..14].copy_from_slice(&4u16.to_be_bytes());
    let raw = mask_guard::raw_metadata(&spot).unwrap();
    assert!(
        !raw.rgb8,
        "a fourth header plane without negative layer count is not merged alpha"
    );
    assert!(!profile::admissible(&raw, &doc));
}

#[test]
fn bounded_depth_export_falls_back_instead_of_failing_an_association() {
    let mut doc = Document::new(1, 1);
    let mut parent = None;
    for _ in 0..34 {
        parent = Some(add(&mut doc, Node::group(0, "group"), parent).unwrap());
    }
    assert!(!profile::within_budget(&doc));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep-hierarchy.psd");
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        Some(AppearanceFallback::UnsupportedFeatures)
    );
    assert_eq!(profile::render_cpu(&read(&path).unwrap()), [0; 4]);
}

#[test]
fn populated_disabled_styles_are_never_admitted_by_their_current_pixels() {
    let mut psd = psd_for(&alpha_scene());
    // A populated disabled master still contains authored state even when
    // there is no currently active effect family to change saved pixels.
    psd.children.as_mut().unwrap()[1].additional_info.effects =
        Some(ag_psd::psd::LayerEffectsInfo {
            disabled: Some(true),
            scale: Some(2.0),
            ..Default::default()
        });
    let (doc, report) = read_bytes_with_report(&encode(&psd)).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::SavedAppearance
    );
    assert_eq!(doc.nodes.len(), 1);
}

#[test]
fn background_identity_requires_every_scoped_marker_and_rgb_only_channels() {
    let mut doc = Document::new(4, 3);
    let id = pixels(&mut doc, "arbitrary label", [255; 4], None);
    doc.psd_background = Some(id);
    for missing in 0..4 {
        let mut psd = psd_for(&doc);
        let layer = &mut psd.children.as_mut().unwrap()[0];
        match missing {
            0 => layer.transparency_protected = Some(false),
            1 => layer.additional_info.protected_info = None,
            2 => layer.additional_info.name_source = None,
            _ => {}
        }
        let bytes = ag_psd::write_psd(
            &psd,
            &WriteOptions {
                no_background: Some(missing == 3),
                trim_image_data: Some(false),
                ..Default::default()
            },
        );
        let parsed = ag_psd::read_psd(&bytes, &ReadOptions::default()).unwrap();
        let raw = mask_guard::raw_metadata(&bytes).unwrap();
        assert_eq!(
            profile::associate(&raw, &parsed).unwrap().background,
            None,
            "missing marker {missing}"
        );
    }
}

#[test]
fn hidden_supported_screen_survives_public_new_profile_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    for hidden_ancestor in [false, true] {
        let mut doc = alpha_scene();
        let parent = hidden_ancestor.then(|| {
            let group = add(&mut doc, Node::group(0, "Hidden ancestor"), None).unwrap();
            doc.node_mut(group).unwrap().visible = false;
            group
        });
        let id = pixels(&mut doc, "Retained Screen", [54, 128, 210, 255], parent);
        let node = doc.node_mut(id).unwrap();
        node.blend = BlendMode::Screen;
        node.visible = hidden_ancestor;
        let original = doc.clone();
        assert!(profile::selector_eligible(&doc));
        let path = dir
            .path()
            .join(format!("hidden-screen-{hidden_ancestor}.psd"));
        assert_eq!(
            write_with_report(&doc, &path).unwrap().appearance_fallback,
            None
        );
        let (back, report) = read_with_report(&path).unwrap();
        assert_eq!(
            report.profile_decision,
            ImportProfileDecision::UniquePhotoshopSrgbV1
        );
        assert_eq!(back.nodes.len(), doc.nodes.len());
        let retained = back
            .nodes
            .iter()
            .find(|node| node.name == "Retained Screen")
            .unwrap();
        assert_eq!(retained.blend, BlendMode::Screen);
        assert_eq!(retained.visible, hidden_ancestor);
        if hidden_ancestor {
            assert!(!back.node(retained.parent.unwrap()).unwrap().visible);
        }
        assert_eq!(profile::render_cpu(&back), profile::render_cpu(&doc));
        assert_eq!(doc, original);
    }
}

#[test]
fn hidden_unsupported_authored_families_do_not_gain_selector_eligibility() {
    let dir = tempfile::tempdir().unwrap();
    for family in [
        "blend-if",
        "special-fill",
        "disabled-style",
        "root-knockout",
    ] {
        let mut doc = alpha_scene();
        let id = pixels(
            &mut doc,
            "Hidden unsupported state",
            [60, 120, 180, 255],
            None,
        );
        let node = doc.node_mut(id).unwrap();
        node.visible = false;
        match family {
            "blend-if" => {
                node.blending.blend_if.source = BlendRange {
                    black: 0.2,
                    black_fade: 0.4,
                    ..Default::default()
                }
            }
            "special-fill" => {
                node.blend = BlendMode::LinearDodge;
                node.blending.fill_opacity = 128.0 / 255.0;
            }
            "disabled-style" => {
                node.styles = vec![emulsion_core::styles::LayerStyle::ColorOverlay {
                    color: [255, 100, 20],
                    opacity: 50.0,
                }];
                node.effects_enabled = false;
            }
            "root-knockout" => node.blending.knockout = Knockout::Shallow,
            _ => unreachable!(),
        }
        assert!(!profile::selector_eligible(&doc), "{family}");
        let before = doc.clone();
        let path = dir.path().join(format!("hidden-{family}.psd"));
        assert_eq!(
            write_with_report(&doc, &path).unwrap().appearance_fallback,
            Some(AppearanceFallback::UnsupportedFeatures),
            "{family}"
        );
        assert_eq!(read(&path).unwrap().nodes.len(), 1, "{family}");
        assert_eq!(doc, before);

        // Public import independently refuses the same hidden metadata. Native
        // styles are normally rasterized, so supply their actual PSD container.
        let mut psd = psd_for(&doc);
        if family == "disabled-style" {
            psd.children
                .as_mut()
                .unwrap()
                .last_mut()
                .unwrap()
                .additional_info
                .effects = Some(ag_psd::psd::LayerEffectsInfo {
                disabled: Some(true),
                ..Default::default()
            });
        }
        let (back, report) = read_bytes_with_report(&encode(&psd)).unwrap();
        assert_eq!(
            report.profile_decision,
            ImportProfileDecision::SavedAppearance,
            "{family}"
        );
        assert_eq!(back.nodes.len(), 1, "{family}");
    }
}

#[test]
fn visible_non_normal_modes_require_an_exact_legacy_match_at_export() {
    let dir = tempfile::tempdir().unwrap();
    for noncontribution in ["identity-pixels", "zero-opacity", "hidden-clip-base"] {
        let mut doc = alpha_scene();
        let base = if noncontribution == "hidden-clip-base" {
            let id = pixels(&mut doc, "Hidden clip base", [255; 4], None);
            doc.node_mut(id).unwrap().visible = false;
            Some(id)
        } else {
            None
        };
        let id = pixels(&mut doc, "Visible Screen", [0, 0, 0, 255], None);
        let node = doc.node_mut(id).unwrap();
        node.blend = BlendMode::Screen;
        if noncontribution == "zero-opacity" {
            node.opacity = 0.0;
        }
        node.clip_to = base;
        assert!(!profile::selector_eligible(&doc), "{noncontribution}");
        let original = doc.clone();
        let path = dir
            .path()
            .join(format!("visible-screen-{noncontribution}.psd"));
        assert_eq!(
            write_with_report(&doc, &path).unwrap().appearance_fallback,
            Some(AppearanceFallback::BlendSpaceDifference),
            "{noncontribution}"
        );
        let back = read(&path).unwrap();
        assert_eq!(back.nodes.len(), 1);
        assert_eq!(profile::render_cpu(&back), profile::render_cpu(&doc));
        assert_eq!(doc, original);
    }

    // A visible Screen layer remains editable when the legacy importer really
    // can reproduce the saved image, rather than merely ignoring its mode.
    let mut doc = alpha_scene();
    doc.nodes[1].blending.fill_opacity = 1.0;
    let id = pixels(&mut doc, "Visible Screen", [54, 128, 210, 255], None);
    doc.node_mut(id).unwrap().blend = BlendMode::Screen;
    assert!(!profile::selector_eligible(&doc));
    let path = dir.path().join("legacy-screen-match.psd");
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        None
    );
    let (back, report) = read_with_report(&path).unwrap();
    assert_eq!(
        report.profile_decision,
        ImportProfileDecision::LegacyMatch { ambiguous: true }
    );
    assert_eq!(back.nodes.len(), doc.nodes.len());
    assert!(
        back.nodes
            .iter()
            .any(|node| node.blend == BlendMode::Screen)
    );
    assert_eq!(profile::render_cpu(&back), profile::render_cpu(&doc));
}

#[test]
fn ambiguous_hidden_deep_is_reported_as_export_fallback_before_reopening() {
    let mut doc = clean_knockout();
    let id = doc
        .nodes
        .iter()
        .find(|node| node.blending.knockout == Knockout::Deep)
        .unwrap()
        .id;
    doc.node_mut(id).unwrap().visible = false;
    let before = doc.clone();
    let expected = profile::render_cpu(&doc);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ambiguous-hidden-deep.psd");
    assert!(
        profile::selector_eligible(&doc),
        "mode eligibility alone is insufficient"
    );

    // Pin the import uniqueness rule independently of the public export guard.
    let undecided = encode_document(&doc, &path, &expected, None, &Default::default()).unwrap();
    let (appearance, import) = read_bytes_with_report(&undecided).unwrap();
    assert_eq!(
        import.profile_decision,
        ImportProfileDecision::SavedAppearance
    );
    assert_eq!(appearance.nodes.len(), 1);

    let export = write_with_report(&doc, &path).unwrap();
    assert_eq!(
        export.appearance_fallback,
        Some(AppearanceFallback::BlendSpaceDifference)
    );
    let back = read(&path).unwrap();
    assert_eq!(back.nodes.len(), 1);
    assert_eq!(profile::render_cpu(&back), expected);
    assert_eq!(
        doc, before,
        "ambiguous profile evidence never rewrites native Deep metadata"
    );
}
