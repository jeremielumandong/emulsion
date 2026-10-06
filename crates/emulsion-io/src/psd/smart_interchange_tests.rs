//! Public PSD → native → PSD/PSB integration, independent of adapter-only tests.
use super::*;
use emulsion_core::{EmptyVectorCoverage, VectorMask};
use emulsion_raster::vector::Path as VectorPath;

#[test]
fn disabled_empty_smart_stack_is_disclosed_instead_of_exported_as_source_only() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd/smartobject-layer.psd");
    let mut editor = emulsion_core::Editor::new(read(&fixture).unwrap(), None);
    let id = editor.doc.nodes[0].id;
    let NodeKind::Smart {
        filters,
        filter_styles,
        filters_enabled,
        original_image: Some(original),
        ..
    } = &editor.doc.nodes[0].kind
    else {
        panic!("the source-only fixture must remain editable");
    };
    assert!(*filters_enabled);
    assert!(filters.is_empty() && filter_styles.is_empty());
    let original = original.clone();
    assert!(!needs_appearance_fallback(&editor.doc));
    editor
        .execute(emulsion_core::Command::SetFiltersEnabled { id, enabled: false })
        .unwrap();
    let disabled = editor.doc.clone();
    let revision = editor.revision;
    let dir = tempfile::tempdir().unwrap();
    for visible in [false, true] {
        let mut doc = disabled.clone();
        doc.nodes[0].visible = visible;
        let before = doc.clone();
        assert!(!smart_objects::can_export(&doc.nodes[0]));
        assert!(needs_appearance_fallback(&doc));
        let expected = profile::render_cpu(&doc);
        for ext in ["psd", "psb"] {
            let path = dir.path().join(format!("disabled-empty-{visible}.{ext}"));
            let report = write_with_source_preparer(&doc, &path, |_| {
                panic!("unsupported stack state must not prepare embedded source records")
            })
            .unwrap();
            assert_eq!(
                report.appearance_fallback,
                Some(AppearanceFallback::UnsupportedFeatures)
            );
            assert!(!report.baked_raster_masks);
            let bytes = std::fs::read(&path).unwrap();
            let psd = ag_psd::read_psd(
                &bytes,
                &ReadOptions {
                    use_image_data: Some(true),
                    skip_composite_image_data: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
            let layers = psd.children.unwrap();
            assert_eq!(layers.len(), 1);
            assert!(layers[0].additional_info.placed_layer.is_none());
            assert_eq!(layers[0].image_data.as_ref().unwrap().data, expected);
            assert!(matches!(
                read(&path).unwrap().nodes[0].kind,
                NodeKind::Raster { .. }
            ));
            assert_eq!(doc, before);
        }
    }
    assert_eq!(editor.revision, revision);
    assert_eq!(editor.doc, disabled);
    let NodeKind::Smart {
        original_image: Some(retained),
        filters_enabled: false,
        ..
    } = &editor.doc.nodes[0].kind
    else {
        panic!("export must not erase native source or authored enabled state");
    };
    assert!(Arc::ptr_eq(retained, &original));
    assert_eq!(retained.bytes(), original.bytes());
    assert!(editor.undo());
    assert!(smart_objects::can_export(&editor.doc.nodes[0]));
}

#[test]
fn unavailable_smart_identifiers_abort_without_replacing_or_creating_output() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd/smartobject-layer.psd");
    let doc = read(&fixture).unwrap();
    let before = doc.clone();
    assert!(!needs_appearance_fallback(&doc));
    let dir = tempfile::tempdir().unwrap();
    for existed in [false, true] {
        let path = dir.path().join(format!("protected-{existed}.psd"));
        if existed {
            std::fs::write(&path, b"existing destination bytes").unwrap();
        }
        let result = write_with_source_preparer(&doc, &path, |_| {
            Err(smart_objects::SmartError::Unavailable(
                "injected entropy failure",
            ))
        });
        assert!(matches!(result, Err(IoError::Io(_))));
        if existed {
            assert_eq!(std::fs::read(&path).unwrap(), b"existing destination bytes");
        } else {
            assert!(!path.exists());
        }
        assert_eq!(doc, before);
    }
}

#[test]
fn original_smart_pixels_paths_and_masks_survive_public_interchange() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd/smartobject-layer.psd");
    let mut doc = read(&fixture).unwrap();
    assert_eq!(doc.nodes.len(), 1);
    let NodeKind::Smart {
        source,
        original_image: Some(original),
        ..
    } = &doc.nodes[0].kind
    else {
        panic!("supported original fixture must import as a Smart source, not its preview")
    };
    let source = source.clone();
    let original = original.clone();
    let decoded = image::load_from_memory(original.bytes())
        .unwrap()
        .into_rgba8();
    assert_ne!(
        source.to_srgba8(),
        *decoded.as_raw(),
        "hidden/low-alpha RGB exercises byte retention"
    );
    doc.width = 64;
    doc.height = 48;
    let dir = tempfile::tempdir().unwrap();
    for variant in 0..3 {
        let node = &mut doc.nodes[0];
        if let NodeKind::Smart { placement, .. } = &mut node.kind {
            *placement = emulsion_core::SmartPlacement::Legacy(Placement::at(11.0, 7.0));
        }
        node.vector_mask = Some(VectorMask {
            path: Arc::new(VectorPath::from_svg("M 2 3 L 29 3 L 29 28 L 2 28 Z").unwrap()),
            empty_coverage: EmptyVectorCoverage::HideAll,
            enabled: variant != 1,
            linked: false,
            inverted: variant == 2,
            ..VectorMask::default()
        });
        node.mask = (variant == 2).then(|| {
            Arc::new(Mask::from_fn(36, 38, 255, |x, y| {
                ((x * 7 + y * 11) % 256) as u8
            }))
        });
        node.mask_transform = emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.0, 0.0, 1.0, -2.0, -3.0,
        ]));
        node.mask_linked = false;
        let before = doc.clone();
        let native = dir.path().join("original.ora");
        crate::ora::write(&doc, &native).unwrap();
        let native = crate::ora::read(&native).unwrap();
        for ext in ["psd", "psb"] {
            let path = dir.path().join(format!("source-{variant}.{ext}"));
            let report = write_with_report(&native, &path).unwrap();
            assert_eq!(report.appearance_fallback, None);
            assert!(!report.baked_raster_masks);
            let back = read(&path).unwrap();
            assert_eq!(back.nodes.len(), 1);
            let actual = &back.nodes[0];
            let NodeKind::Smart {
                source: actual_source,
                original_image: Some(actual_original),
                placement,
                ..
            } = &actual.kind
            else {
                panic!("exported source must retain editable identity")
            };
            assert_eq!(
                crate::original_image_data::source_digest(actual_source),
                crate::original_image_data::source_digest(&source)
            );
            assert_eq!(actual_original.bytes(), original.bytes());
            assert_eq!(placement.legacy().unwrap(), Placement::at(11.0, 7.0));
            let expected = &doc.nodes[0];
            let vector = actual.vector_mask.as_ref().unwrap();
            assert_eq!(
                (vector.enabled, vector.linked, vector.inverted),
                (variant != 1, false, variant == 2)
            );
            // Imported anchors are document-space with an explicit inverse
            // source translation; they must not use the cropped preview bounds.
            let anchor = vector.path.subpaths[0].anchors[0].p;
            let [a, b, c, d, tx, ty] = vector.transform;
            assert!((a * anchor.0 + c * anchor.1 + tx - 2.0).abs() <= 64.0 / (1u32 << 24) as f64);
            assert!((b * anchor.0 + d * anchor.1 + ty - 3.0).abs() <= 48.0 / (1u32 << 24) as f64);
            assert_eq!(
                actual.mask.as_ref().map(|m| m.to_gray8()),
                expected.mask.as_ref().map(|m| m.to_gray8())
            );
            if actual.mask.is_some() {
                assert_eq!(actual.mask_transform, expected.mask_transform);
                assert_eq!(actual.mask_linked, expected.mask_linked);
            }
        }
        assert_eq!(
            doc, before,
            "exports leave the original native document intact"
        );
    }
}
