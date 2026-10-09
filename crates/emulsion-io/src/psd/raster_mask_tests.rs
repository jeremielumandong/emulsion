//! Byte-layout/own-roundtrip tests. These do not assert third-party rendering parity.
use super::*;
use emulsion_core::{Editor, graph::Graph};

fn fixture(group: bool, enabled: bool, linked: bool) -> Document {
    let mut doc = Document::new(13, 11);
    let mut node = if group {
        Node::group(0, "Independent raster mask")
    } else {
        Node::raster(
            0,
            "Independent raster mask",
            Arc::new(Raster::solid(7, 6, [0.7, 0.2, 0.1, 1.0])),
            Placement::at(3.0, 2.0),
        )
    };
    node.mask = Some(Arc::new(Mask::from_fn(17, 15, 255, |x, y| {
        ((x * 19 + y * 37) % 256) as u8
    })));
    node.mask_transform = emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
        1.0, 0.0, 0.0, 1.0, -5.0, -4.0,
    ]));
    node.mask_enabled = enabled;
    node.mask_linked = linked;
    node.mask_properties = MaskProperties {
        density: 153.0 / 255.0,
        feather: 2.25,
    };
    let id = add(&mut doc, node, None).unwrap();
    if group {
        add(
            &mut doc,
            Node::raster(
                0,
                "Child",
                Arc::new(Raster::solid(13, 11, [0.7, 0.2, 0.1, 1.0])),
                Placement::default(),
            ),
            Some(id),
        )
        .unwrap();
    }
    doc
}

fn masked(doc: &Document) -> &Node {
    doc.nodes.iter().find(|n| n.mask.is_some()).unwrap()
}

fn assert_mask_state(actual: &Node, expected: &Node) {
    assert_eq!(actual.mask_enabled, expected.mask_enabled);
    assert_eq!(actual.mask_linked, expected.mask_linked);
    assert_eq!(actual.mask_transform, expected.mask_transform);
    assert_eq!(actual.mask_properties, expected.mask_properties);
    let (a, b) = (
        actual.mask.as_ref().unwrap(),
        expected.mask.as_ref().unwrap(),
    );
    assert_eq!(
        (a.width(), a.height(), a.fill()),
        (b.width(), b.height(), b.fill())
    );
    assert_eq!(a.to_gray8(), b.to_gray8());
}

#[test]
fn independent_raster_masks_roundtrip_psd_psb_without_baking_properties() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
        for group in [false, true] {
            for enabled in [false, true] {
                for linked in [false, true] {
                    let doc = fixture(group, enabled, linked);
                    let source = masked(&doc).mask.clone().unwrap();
                    let before = doc.clone();
                    let path = dir.path().join(format!("{group}-{enabled}-{linked}.{ext}"));
                    write(&doc, &path).unwrap();
                    let back = read(&path).unwrap();
                    assert_eq!(back.nodes.len(), doc.nodes.len());
                    assert_mask_state(masked(&back), masked(&doc));
                    assert_eq!(
                        flatten(&back.composite_tree(), 0).to_srgba8(),
                        flatten(&doc.composite_tree(), 0).to_srgba8()
                    );
                    assert_eq!(doc, before, "export never changes source state");
                    assert!(Arc::ptr_eq(&source, masked(&doc).mask.as_ref().unwrap()));
                    let layer = layer_for(&doc, masked(&doc));
                    let mask = layer.additional_info.mask.unwrap();
                    assert_eq!(mask.user_mask_feather, Some(2.25));
                    assert_eq!(mask.user_mask_density, Some(f64::from(153.0f32 / 255.0)));
                    assert_eq!(mask.position_relative_to_layer, Some(!linked));
                    assert_eq!(
                        mask_bytes(mask.image_data.as_ref().unwrap()),
                        source.to_gray8()
                    );
                    assert_eq!(
                        (mask.left, mask.top),
                        if group {
                            (Some(-5.0), Some(-4.0))
                        } else {
                            (Some(-2.0), Some(-2.0))
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn raster_mask_density_quantizes_only_to_the_standard_byte() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
        for group in [false, true] {
            for density in [
                0.0,
                0.1,
                0.5,
                153.0 / 255.0,
                0.9,
                f32::from_bits(0.9f32.to_bits() + 1),
                0.999,
                1.0,
            ] {
                let mut doc = fixture(group, true, true);
                // Children precede their group in the flat node list. Target
                // the actual mask, rather than the unmasked child at index 0.
                let masked_id = masked(&doc).id;
                assert_eq!(doc.node(masked_id).unwrap().kind.is_group(), group);
                doc.node_mut(masked_id).unwrap().mask_properties.density = density;
                let before = doc.clone();
                let graph = Graph::new(doc.clone(), "Unrounded mask");
                let path = dir.path().join(format!("density.{ext}"));
                let report = write_with_report(&doc, &path).unwrap();
                assert_eq!(
                    report.appearance_fallback, None,
                    "standard byte-density quantization must retain the editable mask: {density}"
                );
                let back = read(&path).unwrap();
                let expected = ((f64::from(density) * 255.0).round() / 255.0) as f32;
                assert_eq!(
                    report.rounded_mask_densities,
                    usize::from(expected != density),
                    "{ext}, group={group}, density={density}"
                );
                assert!(!report.baked_raster_masks);
                assert!((expected - density).abs() <= 0.5 / 255.0 + f32::EPSILON);
                let mut represented = doc.clone();
                represented
                    .node_mut(masked_id)
                    .unwrap()
                    .mask_properties
                    .density = expected;
                assert_eq!(back.nodes.len(), doc.nodes.len());
                assert_eq!(masked(&back).kind.is_group(), group);
                assert_mask_state(masked(&back), masked(&represented));
                assert_eq!(
                    profile::render_cpu(&back),
                    profile::render_cpu(&represented)
                );
                assert_eq!(doc, before, "PSD export must not round native state");
                assert!(graph.commits().all(|commit| commit.doc == before));
                assert!(Arc::ptr_eq(
                    masked(&doc).mask.as_ref().unwrap(),
                    masked(&before).mask.as_ref().unwrap()
                ));
                assert_eq!(
                    write_with_report(&back, &path)
                        .unwrap()
                        .rounded_mask_densities,
                    0,
                    "the emitted byte-grid value must be idempotent"
                );
            }
        }
    }
}

#[test]
fn density_byte_reference_uses_writer_precision_and_every_byte_grid_is_idempotent() {
    let mut doc = fixture(false, true, true);
    // The native f32 below 0.9 and its immediate successor straddle the exact
    // byte midpoint. Multiplying in f32 first would invent a tie for 0.9.
    for (density, byte) in [(0.9f32, 229u8), (f32::from_bits(0.9f32.to_bits() + 1), 230)] {
        doc.nodes[0].mask_properties.density = density;
        let (reference, count) = mask_density_export_reference(&doc).unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            reference.nodes[0].mask_properties.density,
            f32::from(byte) / 255.0
        );
        assert_eq!(doc.nodes[0].mask_properties.density, density);
    }
    for byte in 0..=255u8 {
        doc.nodes[0].mask_properties.density = f32::from(byte) / 255.0;
        assert!(
            mask_density_export_reference(&doc).is_none(),
            "byte {byte} must neither change nor be reported as rounded"
        );
    }
}

#[test]
fn density_rounding_counts_hidden_and_disabled_parameters_without_baking() {
    let dir = tempfile::tempdir().unwrap();
    for (enabled, visible) in [(false, true), (true, false), (false, false)] {
        let mut doc = fixture(false, enabled, false);
        doc.nodes[0].visible = visible;
        doc.nodes[0].mask_properties.density = 0.1;
        let before = doc.clone();
        let path = dir.path().join("dormant-density.psd");
        let report = write_with_report(&doc, &path).unwrap();
        assert_eq!(report.appearance_fallback, None);
        assert_eq!(report.rounded_mask_densities, 1);
        let back = read(&path).unwrap();
        assert_eq!(masked(&back).mask_properties.density, 26.0 / 255.0);
        assert_eq!(masked(&back).mask_enabled, enabled);
        assert_eq!(masked(&back).visible, visible);
        assert_eq!(profile::render_cpu(&back), profile::render_cpu(&doc));
        assert_eq!(doc, before);
    }
}

#[test]
fn density_rounding_merged_preview_matches_the_actual_layered_reference() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = fixture(false, true, true);
    doc.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
    doc.nodes[0].mask_properties.density = 0.1;
    if let NodeKind::Raster { raster, .. } = &mut doc.nodes[0].kind {
        *raster = Arc::new(Raster::from_srgba8(7, 6, &raster.to_srgba8()));
    }
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Opaque preview reference",
            Arc::new(Raster::from_srgba8(
                13,
                11,
                &[30, 60, 90, 255].repeat(13 * 11),
            )),
            Placement::default(),
        )),
        slot: Slot {
            parent: None,
            index: 0,
        },
    }
    .apply(&mut doc)
    .unwrap();
    let before = doc.clone();
    let mut represented = doc.clone();
    let id = masked(&represented).id;
    represented.node_mut(id).unwrap().mask_properties.density = 26.0 / 255.0;
    let expected = profile::render_cpu(&represented);
    assert_ne!(
        expected,
        profile::render_cpu(&doc),
        "fixture must expose rounding"
    );
    for ext in ["psd", "psb"] {
        let path = dir.path().join(format!("preview.{ext}"));
        let report = write_with_report(&doc, &path).unwrap();
        assert_eq!(report.appearance_fallback, None);
        assert_eq!(report.rounded_mask_densities, 1);
        let bytes = std::fs::read(&path).unwrap();
        let decoded = ag_psd::read_psd(
            &bytes,
            &ReadOptions {
                use_image_data: Some(true),
                skip_composite_image_data: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        let merged = decoded
            .image_data
            .expect("the layered PSD must save its preview");
        assert_eq!((merged.width, merged.height), (doc.width, doc.height));
        assert_eq!(merged.data, expected);
        let (back, report) = read_with_report(&path).unwrap();
        assert_ne!(
            report.profile_decision,
            ImportProfileDecision::SavedAppearance
        );
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(profile::render_cpu(&back), expected);
        assert_eq!(doc, before);
    }
}

#[test]
fn density_rounding_does_not_change_baked_coverage_or_unsupported_fallbacks() {
    let dir = tempfile::tempdir().unwrap();
    let mut baked = fixture(false, false, false);
    baked.nodes[0].mask_properties.density = 0.1;
    {
        let node = &mut baked.nodes[0];
        let mut affine = node
            .mask_transform
            .affine()
            .expect("affine fixture mapping");
        affine.translation.x += 0.5;
        node.mask_transform = emulsion_core::Mapping2::Affine(affine);
    }
    let before = baked.clone();
    let report = write_with_report(&baked, &dir.path().join("baked.psd")).unwrap();
    assert_eq!(report.appearance_fallback, None);
    assert!(report.baked_raster_masks);
    assert_eq!(report.rounded_mask_densities, 0);
    assert_eq!(baked, before);

    let mut unsupported = fixture(false, true, true);
    unsupported.nodes[0].mask_properties.density = 0.1;
    unsupported.nodes[0].mask = Some(Arc::new(Mask::from_fn(17, 15, 127, |x, y| {
        ((x * 19 + y * 37) % 256) as u8
    })));
    let before = unsupported.clone();
    let path = dir.path().join("unsupported.psd");
    let report = write_with_report(&unsupported, &path).unwrap();
    assert_eq!(
        report.appearance_fallback,
        Some(AppearanceFallback::UnsupportedFeatures)
    );
    assert_eq!(report.rounded_mask_densities, 0);
    assert_eq!(
        profile::render_cpu(&read(&path).unwrap()),
        profile::render_cpu(&unsupported)
    );
    assert_eq!(unsupported, before);
}

#[test]
fn density_rounding_never_excuses_quantized_layer_opacity() {
    let mut doc = Document::new(4, 3);
    doc.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
    for (name, rgba) in [("red", [255, 0, 0, 255]), ("blue", [0, 0, 255, 255])] {
        add(
            &mut doc,
            Node::raster(
                0,
                name,
                Arc::new(Raster::from_srgba8(4, 3, &rgba.repeat(12))),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
    }
    let top = doc.nodes.last_mut().unwrap();
    top.opacity = 0.5;
    top.mask = Some(Arc::new(Mask::empty(4, 3, 0)));
    top.mask_enabled = false;
    top.mask_properties.density = 0.1;
    let before = doc.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("strict-opacity.psd");
    let report = write_with_report(&doc, &path).unwrap();
    assert_eq!(
        report.appearance_fallback,
        Some(AppearanceFallback::BlendSpaceDifference)
    );
    assert_eq!(report.rounded_mask_densities, 0);
    assert_eq!(
        profile::render_cpu(&read(&path).unwrap()),
        profile::render_cpu(&doc)
    );
    assert_eq!(doc, before);
}

#[test]
fn later_emitted_fallback_uses_native_pixels_instead_of_rounded_density_preview() {
    let mut doc = Document::new(256, 1);
    doc.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
    for (name, rgba) in [("red", [255, 0, 0, 255]), ("blue", [0, 0, 255, 255])] {
        add(
            &mut doc,
            Node::raster(
                0,
                name,
                Arc::new(Raster::from_srgba8(256, 1, &rgba.repeat(256))),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
    }
    let top = doc.nodes.last_mut().unwrap();
    top.opacity = 0.5;
    // The full byte ramp exposes Density rounding at enabled coverage values.
    // Its white endpoint separately exposes the strict 0.5 Opacity mismatch.
    top.mask = Some(Arc::new(Mask::from_fn(256, 1, 0, |x, _| x as u8)));
    top.mask_enabled = true;
    top.mask_properties.density = 0.1;
    assert!(!needs_appearance_fallback(&doc));
    assert!(profile::within_budget(&doc));
    let before = doc.clone();
    let native = profile::render_cpu(&doc);
    let mut represented = doc.clone();
    represented
        .nodes
        .last_mut()
        .unwrap()
        .mask_properties
        .density = 26.0 / 255.0;
    let rounded = profile::render_cpu(&represented);
    assert_ne!(
        native, rounded,
        "the two possible fallback previews must differ"
    );

    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
        let path = dir.path().join(format!("late-fallback.{ext}"));
        let candidate =
            encode_document(&represented, &path, &rounded, None, &Default::default()).unwrap();
        assert!(
            !profile::emitted_matches(&candidate, &rounded).unwrap(),
            "the actual emitted-input gate must trigger the later fallback"
        );
        let report = write_with_report(&doc, &path).unwrap();
        assert_eq!(
            report.appearance_fallback,
            Some(AppearanceFallback::BlendSpaceDifference)
        );
        assert_eq!(report.rounded_mask_densities, 0);
        let bytes = std::fs::read(&path).unwrap();
        let decoded = ag_psd::read_psd(
            &bytes,
            &ReadOptions {
                use_image_data: Some(true),
                skip_composite_image_data: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        let merged = decoded
            .image_data
            .expect("the fallback PSD must save its preview");
        assert_eq!((merged.width, merged.height), (doc.width, doc.height));
        assert_eq!(merged.data, native);
        let back = read(&path).unwrap();
        assert_eq!(back.nodes.len(), 1);
        assert!(back.nodes[0].mask.is_none());
        assert_eq!(profile::render_cpu(&back), native);
        assert_ne!(profile::render_cpu(&back), rounded);
        assert_eq!(doc, before);
    }
}

#[test]
fn off_canvas_mask_pixels_survive_crop_export_and_later_mask_move() {
    let mut doc = fixture(true, true, false);
    let raw = masked(&doc).mask.clone().unwrap();
    Command::Crop {
        rect: emulsion_raster::IRect::new(2, 1, 7, 6),
        rotation: 0.0,
    }
    .apply(&mut doc)
    .unwrap();
    assert!(Arc::ptr_eq(&raw, masked(&doc).mask.as_ref().unwrap()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cropped.psd");
    write(&doc, &path).unwrap();
    let mut back = read(&path).unwrap();
    assert_mask_state(masked(&back), masked(&doc));
    let back_id = masked(&back).id;
    let source_id = masked(&doc).id;
    // Moving the retained plane brings previously off-canvas pixels into view.
    {
        let node = doc.node_mut(source_id).unwrap();
        let mut affine = node
            .mask_transform
            .affine()
            .expect("affine fixture mapping");
        affine.translation.x += 6.0;
        node.mask_transform = emulsion_core::Mapping2::Affine(affine);
    }
    {
        let node = back.node_mut(back_id).unwrap();
        let mut affine = node
            .mask_transform
            .affine()
            .expect("affine fixture mapping");
        affine.translation.x += 6.0;
        node.mask_transform = emulsion_core::Mapping2::Affine(affine);
    }
    assert_eq!(
        flatten(&back.composite_tree(), 0).to_srgba8(),
        flatten(&doc.composite_tree(), 0).to_srgba8()
    );
}

#[test]
fn imported_mask_properties_remain_editable_undoable_and_native_persistent() {
    let dir = tempfile::tempdir().unwrap();
    let psd = dir.path().join("editable.psd");
    let native = dir.path().join("editable.ora");
    write(&fixture(false, false, false), &psd).unwrap();
    let imported = read(&psd).unwrap();
    let id = masked(&imported).id;
    let mut editor = Editor::new(imported.clone(), None);
    editor
        .execute(Command::SetMaskProperties {
            id,
            properties: MaskProperties {
                density: 0.25,
                feather: 4.5,
            },
        })
        .unwrap();
    assert_eq!(masked(&editor.doc).mask_properties.feather, 4.5);
    assert!(editor.undo());
    assert_mask_state(masked(&editor.doc), masked(&imported));
    assert!(editor.redo());
    crate::ora::write_full(
        &editor.doc,
        Some(&Graph::new(editor.doc.clone(), "Imported PSD")),
        &native,
    )
    .unwrap();
    let back = crate::ora::read(&native).unwrap();
    assert_mask_state(masked(&back), masked(&editor.doc));
    assert_eq!(std::fs::read(&psd).unwrap()[..4], *b"8BPS");
}

#[test]
fn arbitrary_mask_affines_keep_the_existing_baked_coverage_route() {
    for affine in [
        [1.0, 0.0, 0.0, 1.0, 0.5, -0.25],
        [1.25, 0.0, 0.0, 0.75, 2.0, 1.0],
        [1.0, 0.2, -0.1, 1.0, 2.0, 1.0],
    ] {
        let mut doc = fixture(false, false, false);
        doc.nodes[0].mask_transform =
            emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&affine));
        let source = masked(&doc).mask.clone().unwrap();
        let layer = layer_for(&doc, masked(&doc));
        let mask = layer.additional_info.mask.unwrap();
        assert_eq!(mask.user_mask_density, None);
        assert_eq!(mask.user_mask_feather, None);
        assert_eq!(mask.disabled, Some(true));
        assert_eq!(mask.position_relative_to_layer, Some(true));
        assert_eq!(
            mask_bytes(mask.image_data.as_ref().unwrap()),
            doc.mask_for_inspection(masked(&doc))
                .unwrap()
                .unwrap()
                .to_gray8()
        );
        assert!(Arc::ptr_eq(&source, masked(&doc).mask.as_ref().unwrap()));
    }
}

#[test]
fn editable_mask_bounds_reject_subpixel_and_signed_integer_overflow() {
    let mut doc = fixture(false, true, true);
    let node = &mut doc.nodes[0];
    assert_eq!(editable_mask_origin(node, 3.0, 2.0), Some((-2.0, -2.0)));
    for offset in [
        f64::NAN,
        f64::INFINITY,
        -f64::INFINITY,
        i32::MIN as f64 - 1.0,
        i32::MAX as f64,
        0.5,
    ] {
        {
            let node = &mut *node;
            let mut affine = node
                .mask_transform
                .affine()
                .expect("affine fixture mapping");
            affine.translation.x = offset;
            node.mask_transform = emulsion_core::Mapping2::Affine(affine);
        }
        assert!(editable_mask_origin(node, 0.0, 0.0).is_none());
    }
}

#[test]
fn unsupported_mask_parameters_are_not_silently_clamped() {
    for (density, feather) in [
        (f64::NAN, 0.0),
        (1.00000000001, 0.0),
        (-0.01, 0.0),
        (1.0, f64::INFINITY),
        (1.0, -0.001),
        (1.0, 1000.00000001),
    ] {
        let mask = LayerMaskData {
            user_mask_density: Some(density),
            user_mask_feather: Some(feather),
            ..Default::default()
        };
        assert!(mask_properties_in(&mask).is_none());
    }
}

#[test]
fn truncated_mask_pixels_cannot_turn_into_a_reveal_all_mask() {
    let mut node = fixture(false, true, true).nodes.remove(0);
    for len in [0, 1, 5, 7, 23] {
        let mask = LayerMaskData {
            image_data: Some(PixelData {
                width: 3,
                height: 2,
                data: vec![255; len],
            }),
            ..Default::default()
        };
        assert!(mask_in(&mut node, &mask, 0.0, 0.0).is_err());
    }
}

#[test]
fn photoshop_authored_parameter_layout_recovers_its_saved_appearance() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd");
    let path = root.join("mask-parameters-no-real-channel.psd");
    let source = std::fs::read(&path).unwrap();
    assert!(
        mask_guard::unsupported_mask_reason(&source)
            .unwrap()
            .is_some()
    );
    let doc = read(&path).unwrap();
    assert_eq!(doc.nodes.len(), 1);
    assert!(doc.nodes[0].name.contains("flattened"));
    assert!(doc.nodes[0].mask.is_none());
    assert!(doc.nodes[0].vector_mask.is_none());
    let expected = image::open(root.join("mask-parameters-no-real-channel.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(
        flatten(&doc.composite_tree(), 0).to_srgba8(),
        expected.into_raw()
    );
    assert_eq!(
        std::fs::read(path).unwrap(),
        source,
        "input PSD is never rewritten"
    );
}

#[test]
fn fallback_rejects_a_file_declaring_no_real_merged_image() {
    let mut psd = Psd {
        width: 2.0,
        height: 2.0,
        image_data: Some(PixelData {
            width: 2,
            height: 2,
            data: vec![255; 16],
        }),
        ..Default::default()
    };
    psd.image_resources = Some(ag_psd::psd::ImageResources {
        version_info: Some(ag_psd::psd::VersionInfo {
            has_real_merged_data: false,
            ..Default::default()
        }),
        ..Default::default()
    });
    let error = from_psd_with_fallback(&psd, true).unwrap_err().to_string();
    assert!(error.contains("Maximize Compatibility"), "{error}");
}

#[test]
fn gray_raw_outside_fill_uses_an_explicit_appearance_fallback() {
    let mut doc = fixture(false, true, true);
    doc.nodes[0].mask = Some(Arc::new(Mask::from_fn(17, 15, 127, |x, y| {
        ((x * 19 + y * 37) % 256) as u8
    })));
    assert!(needs_appearance_fallback(&doc));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gray-fill.psd");
    write(&doc, &path).unwrap();
    let back = read(&path).unwrap();
    assert_eq!(back.nodes.len(), 1);
    assert!(back.nodes[0].name.contains("flattened"));
    assert!(back.nodes[0].mask.is_none());
    assert_eq!(
        flatten(&back.composite_tree(), 0).to_srgba8(),
        flatten(&doc.composite_tree(), 0).to_srgba8()
    );
}

#[test]
fn export_warning_distinguishes_editable_masks_from_baked_affines() {
    for group in [false, true] {
        let mut doc = fixture(group, false, false);
        assert!(!has_baked_raster_masks(&doc));
        let id = masked(&doc).id;
        {
            let node = doc.node_mut(id).unwrap();
            let mut affine = node
                .mask_transform
                .affine()
                .expect("affine fixture mapping");
            affine.translation.x += 0.5;
            node.mask_transform = emulsion_core::Mapping2::Affine(affine);
        }
        assert!(has_baked_raster_masks(&doc));
        assert!(!needs_appearance_fallback(&doc));
    }
    let mut doc = fixture(false, true, true);
    let NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    placement.rotation = 0.2;
    assert!(has_baked_raster_masks(&doc));
}

#[test]
fn imported_link_state_controls_later_layer_motion_and_undo() {
    for linked in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linked.psd");
        write(&fixture(false, true, linked), &path).unwrap();
        let imported = read(&path).unwrap();
        let id = masked(&imported).id;
        let before = emulsion_core::transform::mask_to_document(masked(&imported))
            .unwrap()
            .affine()
            .unwrap();
        let mut editor = Editor::new(imported.clone(), None);
        editor
            .execute(Command::SetPlacement {
                id,
                placement: Placement::at(8.0, 5.0),
            })
            .unwrap();
        let after = emulsion_core::transform::mask_to_document(masked(&editor.doc))
            .unwrap()
            .affine()
            .unwrap();
        assert_eq!(
            after.translation - before.translation,
            if linked {
                glam::DVec2::new(5.0, 3.0)
            } else {
                glam::DVec2::ZERO
            }
        );
        assert!(editor.undo());
        assert_mask_state(masked(&editor.doc), masked(&imported));
        assert!(editor.redo());
        assert_eq!(
            emulsion_core::transform::mask_to_document(masked(&editor.doc))
                .unwrap()
                .affine()
                .unwrap(),
            after
        );
    }
}

// Independent one-pixel RGB record, not generated by the dependency whose
// permissive PackBits behavior is under test. The layer pixels are valid.
fn inverted_mask_psd(composite: &[u8]) -> Vec<u8> {
    let mut extra = Vec::new();
    extra.extend_from_slice(&20u32.to_be_bytes());
    for coordinate in [0i32, 0, 1, 1] {
        extra.extend_from_slice(&coordinate.to_be_bytes());
    }
    extra.extend_from_slice(&[255, 4, 0, 0]); // inverted mask, no parameters
    extra.extend_from_slice(&0u32.to_be_bytes()); // no blending ranges
    extra.extend_from_slice(&[1, b'A', 0, 0]); // Pascal name padded to four
    let mut layer = Vec::new();
    for coordinate in [0i32, 0, 1, 1] {
        layer.extend_from_slice(&coordinate.to_be_bytes());
    }
    layer.extend_from_slice(&4u16.to_be_bytes());
    for id in [0i16, 1, 2, -2] {
        layer.extend_from_slice(&id.to_be_bytes());
        layer.extend_from_slice(&3u32.to_be_bytes());
    }
    layer.extend_from_slice(b"8BIMnorm\xff\0\0\0");
    layer.extend_from_slice(&(extra.len() as u32).to_be_bytes());
    layer.extend_from_slice(&extra);
    let mut info = 1i16.to_be_bytes().to_vec();
    info.extend_from_slice(&layer);
    for sample in [20, 40, 80, 0] {
        info.extend_from_slice(&[0, 0, sample]);
    }
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut section = (info.len() as u32).to_be_bytes().to_vec();
    section.extend_from_slice(&info);
    section.extend_from_slice(&0u32.to_be_bytes()); // no global mask
    let mut file = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    file.extend_from_slice(&3u16.to_be_bytes());
    file.extend_from_slice(&1u32.to_be_bytes());
    file.extend_from_slice(&1u32.to_be_bytes());
    file.extend_from_slice(&8u16.to_be_bytes());
    file.extend_from_slice(&3u16.to_be_bytes());
    file.extend_from_slice(&[0; 8]); // empty color data and resources
    file.extend_from_slice(&(section.len() as u32).to_be_bytes());
    file.extend_from_slice(&section);
    file.extend_from_slice(composite);
    file
}

#[test]
fn successful_decoder_cannot_publish_an_underfilled_saved_composite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inverted-mask.psd");
    let valid = inverted_mask_psd(&[0, 0, 20, 40, 80]);
    std::fs::write(&path, &valid).unwrap();
    let doc = read(&path).unwrap();
    assert!(doc.nodes[0].name.contains("flattened"));
    assert_eq!(
        flatten(&doc.composite_tree(), 0).to_srgba8(),
        [20, 40, 80, 255]
    );
    // Each declared RLE row is only a no-op. ag-psd currently returns black
    // pixels successfully, but those pixels were never present in the file.
    let malformed = inverted_mask_psd(&[0, 1, 0, 1, 0, 1, 0, 1, 128, 128, 128]);
    assert!(ag_psd::read_psd(&malformed, &ReadOptions::default()).is_ok());
    std::fs::write(&path, malformed).unwrap();
    let error = read(&path).unwrap_err().to_string();
    assert!(error.contains("RLE row is truncated"), "{error}");
}
