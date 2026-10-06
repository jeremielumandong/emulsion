//! Editable PSD paths, independent channels, and deliberately unsupported input.
use super::*;
use emulsion_core::{Editor, EmptyVectorCoverage, VectorMask, graph::Graph};
use emulsion_raster::vector::Path as VectorPath;

fn fixture(group: bool, raster_mask: bool) -> Document {
    let mut doc = Document::new(64, 32);
    let mut node = if group {
        Node::group(0, "Editable vector")
    } else {
        Node::raster(
            0,
            "Editable vector",
            Arc::new(Raster::from_srgba8(
                48,
                24,
                &[218, 124, 89, 255].repeat(48 * 24),
            )),
            Placement::at(8.0, 4.0),
        )
    };
    node.vector_mask = Some(VectorMask {
        path: Arc::new(VectorPath::from_svg("M -2 2 C 8 -4 28 0 36 8 L 30 22 L 2 18 Z").unwrap()),
        transform: [1.0, 0.0, 0.0, 1.0, 2.0, 1.0],
        empty_coverage: EmptyVectorCoverage::HideAll,
        ..VectorMask::default()
    });
    if raster_mask {
        node.mask = Some(Arc::new(Mask::from_fn(72, 40, 255, |x, y| {
            ((x * 7 + y * 11) % 256) as u8
        })));
        node.mask_transform = emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.0, 0.0, 1.0, -3.0, -2.0,
        ]));
        node.mask_linked = false;
    }
    let id = add(&mut doc, node, None).unwrap();
    if group {
        add(
            &mut doc,
            Node::raster(
                0,
                "Source",
                Arc::new(Raster::from_srgba8(
                    64,
                    32,
                    &[218, 124, 89, 255].repeat(64 * 32),
                )),
                Placement::default(),
            ),
            Some(id),
        )
        .unwrap();
    }
    doc
}
fn masked(doc: &Document) -> &Node {
    doc.nodes.iter().find(|n| n.vector_mask.is_some()).unwrap()
}
fn world_points(doc: &Document) -> Vec<[f64; 6]> {
    let node = masked(doc);
    let mask = node.vector_mask.as_ref().unwrap();
    let at = match &node.kind {
        NodeKind::Raster { placement, .. } => (placement.x, placement.y),
        _ => (0.0, 0.0),
    };
    let [a, b, c, d, tx, ty] = mask.transform;
    mask.path
        .subpaths
        .iter()
        .flat_map(|s| &s.anchors)
        .map(|anchor| {
            let p = [anchor.h_in, anchor.p, anchor.h_out];
            std::array::from_fn(|i| {
                let p = p[i / 2];
                if i % 2 == 0 {
                    a * p.0 + c * p.1 + tx + at.0
                } else {
                    b * p.0 + d * p.1 + ty + at.1
                }
            })
        })
        .collect()
}
fn assert_state(actual: &Document, expected: &Document) {
    let a = masked(actual);
    let b = masked(expected);
    let x = a.vector_mask.as_ref().unwrap();
    let y = b.vector_mask.as_ref().unwrap();
    assert_eq!(
        (
            x.enabled,
            x.linked,
            x.inverted,
            x.empty_coverage,
            x.properties
        ),
        (
            y.enabled,
            y.linked,
            y.inverted,
            y.empty_coverage,
            y.properties
        )
    );
    assert_eq!(x.path.subpaths.len(), y.path.subpaths.len());
    for (s, t) in x.path.subpaths.iter().zip(&y.path.subpaths) {
        assert_eq!(s.closed, t.closed);
        assert_eq!(
            s.anchors.iter().map(|a| a.smooth).collect::<Vec<_>>(),
            t.anchors.iter().map(|a| a.smooth).collect::<Vec<_>>()
        );
    }
    for (a, b) in world_points(actual)
        .iter()
        .flatten()
        .zip(world_points(expected).iter().flatten())
    {
        assert!((a - b).abs() <= 64.0 / (1u32 << 24) as f64, "{a} != {b}");
    }
    assert_eq!(
        a.mask.as_ref().map(|m| m.to_gray8()),
        b.mask.as_ref().map(|m| m.to_gray8())
    );
    assert_eq!(
        (a.mask_enabled, a.mask_linked, a.mask_properties),
        (b.mask_enabled, b.mask_linked, b.mask_properties)
    );
    if a.mask.is_some() {
        assert_eq!(a.mask_transform, b.mask_transform);
    }
    if let (NodeKind::Raster { raster: a, .. }, NodeKind::Raster { raster: b, .. }) =
        (&a.kind, &b.kind)
    {
        assert_eq!(a.to_srgba8(), b.to_srgba8());
    }
}

#[test]
fn editable_vector_paths_roundtrip_psd_psb_with_independent_raster_channels() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
        for group in [false, true] {
            for raster in [false, true] {
                for flags in 0..8 {
                    let mut doc = fixture(group, raster);
                    let mask = doc
                        .nodes
                        .iter_mut()
                        .find_map(|node| node.vector_mask.as_mut())
                        .unwrap();
                    mask.enabled = flags & 1 == 0;
                    mask.linked = flags & 2 == 0;
                    mask.inverted = flags & 4 != 0;
                    let before = doc.clone();
                    assert!(!needs_appearance_fallback(&doc));
                    let path = dir.path().join(format!("{group}-{raster}-{flags}.{ext}"));
                    write(&doc, &path).unwrap();
                    let raw = std::fs::read(&path).unwrap();
                    assert_eq!(mask_guard::unsupported_mask_reason(&raw).unwrap(), None);
                    assert!(smart_objects::inspect(&raw).is_ok());
                    let copy = mask_guard::vector_decoder_copy(&raw).unwrap();
                    let parsed =
                        ag_psd::read_psd(copy.as_deref().unwrap_or(&raw), &ReadOptions::default())
                            .unwrap();
                    assert!(
                        parsed
                            .children
                            .as_ref()
                            .unwrap()
                            .iter()
                            .any(|l| l.additional_info.vector_mask.is_some()),
                        "a valid raw path must not silently disappear in the dependency"
                    );
                    assert!(
                        !layers_need_composite(
                            parsed.children.as_ref().unwrap(),
                            true,
                            &Default::default()
                        ),
                        "decoded masks: {:?}",
                        parsed
                            .children
                            .as_ref()
                            .unwrap()
                            .iter()
                            .map(|l| (&l.additional_info.vector_mask, &l.additional_info.mask))
                            .collect::<Vec<_>>()
                    );
                    let back = read(&path).unwrap();
                    assert_state(&back, &doc);
                    assert_eq!(
                        flatten(&back.composite_tree(), 0).to_srgba8(),
                        flatten(&doc.composite_tree(), 0).to_srgba8()
                    );
                    assert_eq!(doc, before, "PSD export never changes native state");
                }
            }
        }
    }
}

#[test]
fn open_paths_empty_fill_states_and_affines_retain_editability() {
    let dir = tempfile::tempdir().unwrap();
    for empty in [false, true] {
        for reveal in [false, true] {
            for inverted in [false, true] {
                let mut doc = fixture(false, false);
                let mask = doc.nodes[0].vector_mask.as_mut().unwrap();
                if empty {
                    mask.path = Arc::new(VectorPath::default());
                } else {
                    Arc::make_mut(&mut mask.path).subpaths[0].closed = false;
                }
                mask.empty_coverage = if reveal {
                    EmptyVectorCoverage::RevealAll
                } else {
                    EmptyVectorCoverage::HideAll
                };
                mask.inverted = inverted;
                mask.transform = [0.75, 0.125, -0.25, 1.125, 3.5, -1.25];
                assert!(!needs_appearance_fallback(&doc));
                let path = dir.path().join("state.psb");
                write(&doc, &path).unwrap();
                let back = read(&path).unwrap();
                assert_state(&back, &doc);
                assert_eq!(
                    flatten(&back.composite_tree(), 0).to_srgba8(),
                    flatten(&doc.composite_tree(), 0).to_srgba8()
                );
            }
        }
    }
}

#[test]
fn vector_density_feather_survive_supported_raster_parameter_carriers() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
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
            for feather in [0.0, 2.25] {
                let mut doc = fixture(false, true);
                doc.nodes[0].vector_mask.as_mut().unwrap().properties =
                    MaskProperties { density, feather };
                let path = dir.path().join(format!("parameters.{ext}"));
                assert!(!needs_appearance_fallback(&doc));
                let before = doc.clone();
                let graph = Graph::new(doc.clone(), "Unrounded vector mask");
                let report = write_with_report(&doc, &path).unwrap();
                assert_eq!(report.appearance_fallback, None);
                let rounded = ((f64::from(density) * 255.0).round() / 255.0) as f32;
                assert_eq!(
                    report.rounded_mask_densities,
                    usize::from(rounded != density)
                );
                let mut represented = doc.clone();
                represented.nodes[0]
                    .vector_mask
                    .as_mut()
                    .unwrap()
                    .properties
                    .density = rounded;
                let back = read(&path).unwrap();
                assert_state(&back, &represented);
                assert_eq!(
                    flatten(&back.composite_tree(), 0).to_srgba8(),
                    flatten(&represented.composite_tree(), 0).to_srgba8()
                );
                assert_eq!(doc, before);
                assert!(graph.commits().all(|commit| commit.doc == before));
                assert!(Arc::ptr_eq(
                    &masked(&doc).vector_mask.as_ref().unwrap().path,
                    &masked(&before).vector_mask.as_ref().unwrap().path,
                ));
                assert_eq!(
                    write_with_report(&back, &path)
                        .unwrap()
                        .rounded_mask_densities,
                    0
                );
            }
        }
    }
}

#[test]
fn every_carried_vector_density_byte_is_idempotent() {
    let mut doc = fixture(false, true);
    for byte in 0..=255u8 {
        doc.nodes[0].mask_properties.density = f32::from(byte) / 255.0;
        doc.nodes[0]
            .vector_mask
            .as_mut()
            .unwrap()
            .properties
            .density = f32::from(byte) / 255.0;
        assert!(
            mask_density_export_reference(&doc).is_none(),
            "carried raster/vector byte {byte} must not be rounded again"
        );
    }
}

#[test]
fn independent_raster_and_vector_density_rounding_counts_both_carried_fields() {
    let dir = tempfile::tempdir().unwrap();
    for enabled in [false, true] {
        let mut doc = fixture(false, true);
        doc.nodes[0].mask_properties.density = 0.1;
        doc.nodes[0].mask_enabled = enabled;
        let vector = doc.nodes[0].vector_mask.as_mut().unwrap();
        vector.properties.density = 0.5;
        vector.enabled = enabled;
        let before = doc.clone();
        let mut represented = doc.clone();
        represented.nodes[0].mask_properties.density = 26.0 / 255.0;
        represented.nodes[0]
            .vector_mask
            .as_mut()
            .unwrap()
            .properties
            .density = 128.0 / 255.0;
        let path = dir.path().join("both-densities.psd");
        let report = write_with_report(&doc, &path).unwrap();
        assert_eq!(report.appearance_fallback, None);
        assert_eq!(report.rounded_mask_densities, 2);
        let back = read(&path).unwrap();
        assert_state(&back, &represented);
        assert_eq!(
            profile::render_cpu(&back),
            profile::render_cpu(&represented)
        );
        assert_eq!(doc, before);
    }
}

#[test]
fn density_rounding_cannot_admit_a_missing_vector_parameter_carrier() {
    // 0.999 rounds to the default 255/255 byte. Admission must still see that
    // the original authored parameter cannot be written without a -2 carrier.
    let mut doc = fixture(false, false);
    doc.nodes[0]
        .vector_mask
        .as_mut()
        .unwrap()
        .properties
        .density = 0.999;
    let before = doc.clone();
    assert!(needs_appearance_fallback(&doc));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing-carrier.psd");
    let report = write_with_report(&doc, &path).unwrap();
    assert_eq!(
        report.appearance_fallback,
        Some(AppearanceFallback::UnsupportedFeatures)
    );
    assert_eq!(report.rounded_mask_densities, 0);
    assert!(read(&path).unwrap().nodes[0].vector_mask.is_none());
    assert_eq!(doc, before);
}

#[test]
fn incompatible_vector_states_are_explicit_appearance_exports() {
    let dir = tempfile::tempdir().unwrap();
    for case in 0..9 {
        let mut doc = fixture(false, true);
        let node = &mut doc.nodes[0];
        let mask = node.vector_mask.as_mut().unwrap();
        match case {
            0 => {
                mask.path = Arc::new(VectorPath::from_svg("M 0 0 L 24 24 L 0 24 L 24 0 Z").unwrap())
            }
            1 => {
                mask.path = Arc::new(
                    VectorPath::from_svg("M 0 0 L 24 0 L 24 24 Z M 4 4 L 8 4 L 8 8 Z").unwrap(),
                )
            }
            2 => {
                node.mask = None;
                mask.properties.density = 0.5;
            }
            3 => {
                node.mask_properties.feather = 2.0;
                mask.properties.feather = 3.0;
            }
            4 => {
                mask.transform[0] = 1.5;
                mask.properties.feather = 2.0;
            }
            5 => {
                mask.transform[4] = 0.5;
                mask.properties.feather = 2.0;
            }
            6 => mask.transform[4] = 1024.0,
            7 => mask.path = Arc::new(VectorPath::from_svg("M 2 2").unwrap()),
            8 => {
                if let NodeKind::Raster { placement, .. } = &mut node.kind {
                    placement.rotation = 0.1;
                }
            }
            _ => unreachable!(),
        }
        assert!(needs_appearance_fallback(&doc), "case {case}");
        let before = doc.clone();
        let path = dir.path().join("fallback.psd");
        write(&doc, &path).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.nodes.len(), 1);
        assert!(back.nodes[0].vector_mask.is_none());
        assert!(back.nodes[0].name.contains("appearance"));
        let encoded = flatten(&doc.composite_tree(), 0).to_srgba8();
        let decoded = Raster::from_srgba8(doc.width, doc.height, &encoded);
        assert_eq!(
            flatten(&back.composite_tree(), 0).to_srgba8(),
            decoded.to_srgba8(),
            "appearance matches the exact native decode of the serialized RGBA8; case {case}"
        );
        assert_eq!(doc, before);
    }
}

fn vector_block(bytes: &[u8]) -> (usize, usize) {
    let at = bytes
        .windows(8)
        .position(|b| b == b"8BIMvmsk" || b == b"8BIMvsms")
        .unwrap();
    let length = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    (at + 12, length)
}
#[test]
fn writer_path_bytes_match_independent_document_normalized_golden() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["psd", "psb"] {
        let mut doc = fixture(false, false);
        let mask = doc.nodes[0].vector_mask.as_mut().unwrap();
        mask.path = Arc::new(VectorPath::from_svg("M -8 -4 L 24 -4 L 24 12 L -8 12 Z").unwrap());
        mask.transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        mask.enabled = false;
        mask.linked = false;
        mask.inverted = true;
        let path = dir.path().join(format!("golden.{ext}"));
        write(&doc, &path).unwrap();
        let bytes = std::fs::read(path).unwrap();
        let (at, length) = vector_block(&bytes);
        let raw = &bytes[at..at + length];
        let mut expected = Vec::new();
        expected.extend_from_slice(&3u32.to_be_bytes());
        expected.extend_from_slice(&7u32.to_be_bytes());
        let mut record = [0u8; 26];
        record[..2].copy_from_slice(&6u16.to_be_bytes());
        expected.extend_from_slice(&record);
        record = [0; 26];
        record[..2].copy_from_slice(&8u16.to_be_bytes());
        expected.extend_from_slice(&record);
        record = [0; 26];
        record[2..4].copy_from_slice(&4u16.to_be_bytes());
        record[4..6].copy_from_slice(&1u16.to_be_bytes());
        record[6..8].copy_from_slice(&1u16.to_be_bytes());
        expected.extend_from_slice(&record);
        // Document size 64×32 and layer origin (8,4): anchors become
        // normalized (0,0),(.5,0),(.5,.5),(0,.5); all corner handles match.
        for (x, y) in [(0i32, 0i32), (1 << 23, 0), (1 << 23, 1 << 23), (0, 1 << 23)] {
            record = [0; 26];
            record[..2].copy_from_slice(&2u16.to_be_bytes());
            for offset in [2, 10, 18] {
                record[offset..offset + 4].copy_from_slice(&y.to_be_bytes());
                record[offset + 4..offset + 8].copy_from_slice(&x.to_be_bytes());
            }
            expected.extend_from_slice(&record);
        }
        expected.resize(expected.len().next_multiple_of(4), 0);
        assert_eq!(raw, expected);
        assert_eq!(mask_guard::unsupported_mask_reason(&bytes).unwrap(), None);
    }
}

#[test]
fn unknown_vector_flags_operations_and_fill_do_not_silently_import_editably() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guard.psd");
    let mut doc = fixture(false, false);
    // This oracle exercises unsupported path-record routing. With a backdrop,
    // antialiased mask edges require the PSD-compatible compositing convention;
    // a native Linear document correctly takes the appearance-only route.
    doc.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
    // A saved PSD composite can be white-matted at 8-bit alpha. Keep this
    // record-routing oracle opaque so exact RGB equality tests routing, not
    // the inherent low-alpha precision of that separate preview encoding.
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Opaque backdrop",
            Arc::new(Raster::from_srgba8(
                64,
                32,
                &[30, 60, 90, 255].repeat(64 * 32),
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
    assert_eq!(
        write_with_report(&doc, &path).unwrap().appearance_fallback,
        None,
        "the supported source must remain layered before its records are mutated"
    );
    assert_state(&read(&path).unwrap(), &doc);
    let original = std::fs::read(&path).unwrap();
    let (at, _) = vector_block(&original);
    for (offset, value) in [
        (at + 7, 0x80),
        (at + 8 + 52 + 5, 0),
        (at + 8 + 52 + 7, 9),
        (at + 8 + 52 + 8, 1),
    ] {
        let mut bytes = original.clone();
        bytes[offset] = value;
        std::fs::write(&path, bytes).unwrap();
        let back = read(&path).unwrap();
        assert!(back.nodes[0].vector_mask.is_none());
        assert!(back.nodes[0].name.contains("appearance"));
        assert_eq!(
            flatten(&back.composite_tree(), 0).to_srgba8(),
            flatten(&doc.composite_tree(), 0).to_srgba8()
        );
    }
    for (offset, value) in [(at + 8 + 52 + 3, 20), (at + 8 + 78 + 1, 5)] {
        let mut bytes = original.clone();
        bytes[offset] = value;
        std::fs::write(&path, bytes).unwrap();
        assert!(read(&path).is_err(), "malformed count/closure must fail");
    }
}

#[test]
fn imported_vector_geometry_is_editable_undoable_and_native_persistent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native-source.psd");
    write(&fixture(false, true), &path).unwrap();
    let imported = read(&path).unwrap();
    let id = masked(&imported).id;
    let mut editor = Editor::new(imported.clone(), None);
    let path = Arc::new(VectorPath::from_svg("M 10 2 C 22 -2 38 4 30 20 L 12 24 Z").unwrap());
    editor
        .execute(Command::SetVectorMaskPath { id, path })
        .unwrap();
    let edited = editor.doc.clone();
    assert!(editor.undo());
    assert_eq!(editor.doc, imported);
    assert!(editor.redo());
    assert_eq!(editor.doc, edited);
    let native = dir.path().join("edited.ora");
    crate::ora::write_full(
        &editor.doc,
        Some(&Graph::new(imported, "PSD source")),
        &native,
    )
    .unwrap();
    let back = crate::ora::read(&native).unwrap();
    assert_eq!(masked(&back).vector_mask, masked(&edited).vector_mask);
    assert_eq!(
        masked(&back).mask.as_ref().unwrap().to_gray8(),
        masked(&edited).mask.as_ref().unwrap().to_gray8()
    );
}
